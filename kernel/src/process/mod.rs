pub mod scheduler;

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering};
use core::cell::UnsafeCell;

use crate::handle::{
    Handle, HandleError, HandleObject, RIGHT_DISPLAY_MASTER, RIGHT_INPUT_MASTER, RIGHT_MAP,
    RIGHT_READ, RIGHT_SIGNAL, RIGHT_TRANSFER, RIGHT_WRITE,
};
use crate::memory::pmm::{get_pmm, PhysicalAddress};
use crate::memory::vmm::{ActiveAddressSpace, AddressSpace, PageFlags, VirtualAddress};
use crate::sync::{InterruptGuard, IrqSafeSpinLock};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProcessId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ThreadId(pub u64);

pub type ProcessFd = u32;

pub const FIRST_PROCESS_FD: ProcessFd = 3;
pub const MAX_PROCESS_FD: ProcessFd = 1024;
pub const DEFAULT_KERNEL_STACK_SIZE: usize = 0x40000;

static NEXT_PID: AtomicU64 = AtomicU64::new(1);
static NEXT_SHARED_VM_ID: AtomicU64 = AtomicU64::new(1);
static SHARED_VM_OBJECTS: IrqSafeSpinLock<BTreeMap<u64, SharedVmObject>> =
    IrqSafeSpinLock::new(BTreeMap::new());

struct SharedVmObject {
    frames: Vec<PhysicalAddress>,
    refs: usize,
}

fn shared_vm_release(id: u64, count: usize) {
    let mut objects = SHARED_VM_OBJECTS.lock();
    let Some(object) = objects.get_mut(&id) else { return; };
    object.refs -= count;
    if object.refs == 0 {
        let object = objects.remove(&id).unwrap();
        if let Some(pmm) = get_pmm() {
            for frame in object.frames { pmm.free_frame(frame); }
        }
    }
}

/// Аллоцирует набор обнулённых физических фреймов под разделяемый объект и
/// регистрирует его в глобальном реестре с `refs = 1`. Возвращает id объекта.
/// В отличие от [`Process::create_shared_vm`] НЕ добавляет id в `shared_owned` —
/// эту единственную ссылку держит вызывающий (например, capability-хэндл).
fn alloc_shared_vm_object(length: usize) -> Result<u64, ProcessError> {
    let length = length.checked_add(USER_PAGE_SIZE - 1)
        .map(|value| value & !(USER_PAGE_SIZE - 1))
        .ok_or(ProcessError::InvalidMemoryRange)?;
    if length == 0 || length > 16 * 1024 * 1024 {
        return Err(ProcessError::InvalidMemoryRange);
    }
    let pmm = get_pmm().ok_or(ProcessError::OutOfMemory)?;
    let mut frames = Vec::new();
    for _ in 0..length / USER_PAGE_SIZE {
        let Some(frame) = pmm.alloc_frame() else {
            for frame in frames { pmm.free_frame(frame); }
            return Err(ProcessError::OutOfMemory);
        };
        unsafe {
            core::ptr::write_bytes(
                crate::memory::vmm::phys_to_virt(frame.as_usize()) as *mut u8,
                0, USER_PAGE_SIZE,
            );
        }
        frames.push(frame);
    }
    let id = NEXT_SHARED_VM_ID.fetch_add(1, Ordering::SeqCst);
    SHARED_VM_OBJECTS.lock().insert(id, SharedVmObject { frames, refs: 1 });
    Ok(id)
}
/// Таблица процессов. Раньше `static mut Option<Vec<..>>`; теперь `UnsafeCell`-
/// newtype без `static mut`. Единственная точка доступа — `process_table_mut`.
struct ProcessTableCell(UnsafeCell<Option<Vec<ProcessRecord>>>);
unsafe impl Sync for ProcessTableCell {}
static PROCESS_TABLE: ProcessTableCell = ProcessTableCell(UnsafeCell::new(None));
static CURRENT_PID: AtomicU64 = AtomicU64::new(0);
static PROCESS_EXIT_REQUESTED: AtomicBool = AtomicBool::new(false);
static PROCESS_YIELD_REQUESTED: AtomicBool = AtomicBool::new(false);
static PROCESS_SCHEDULE_HINT: AtomicU64 = AtomicU64::new(0);
static PROCESS_EXIT_CODE: AtomicI32 = AtomicI32::new(0);
static PROCESS_EXIT_KIND: AtomicI32 = AtomicI32::new(0);
static TERMINAL_FOREGROUND_PID: AtomicU64 = AtomicU64::new(0);
static FOREGROUND_OUTPUT_SINK: AtomicU64 = AtomicU64::new(ProcessOutputSink::SerialOnly as u64);
static TERMINAL_STDIN_WAITING_PID: AtomicU64 = AtomicU64::new(0);
static PREEMPT_SWITCH_REQUESTED: AtomicBool = AtomicBool::new(false);
/// Count of committed timer-driven context switches. Only advanced when
/// `timer_preempt_save_and_schedule` actually hands the CPU to another process,
/// so a non-zero value is proof that preemption fired (see M1 [PREEMPT-TEST]).
static PREEMPTION_COUNT: AtomicU64 = AtomicU64::new(0);
// UP round-robin is the default. A child may finish before its parent's first wait.
static PREEMPTION_ENABLED: AtomicBool = AtomicBool::new(true);
/// IRQ-safe wait queue; check/block is covered by the caller's IRQ guard.
static WAIT_QUEUE: IrqSafeSpinLock<Vec<WaitEntry>> = IrqSafeSpinLock::new(Vec::new());

#[derive(Clone, Copy, PartialEq, Eq)]
enum WaitKey {
    Timer,
    Ipc(ProcessId),
    /// Dunit-native futex key: threads share their owner's address space, so
    /// (owner, user virtual address of the word) uniquely names a futex word.
    Word { owner: ProcessId, addr: u64 },
}

#[derive(Clone, Copy)]
struct WaitEntry {
    entity: ProcessId,
    key: WaitKey,
    deadline: Option<u64>,
}
/// Буфер строки stdin терминала для процесса, ожидающего ввод. Раньше три
/// `static mut`; теперь одна структура за `UnsafeCell`-newtype. Доступ идёт
/// кооперативно (подача из оболочки и чтение из syscall на одном CPU); какой
/// именно PID вправе читать, определяет отдельный атомик TERMINAL_STDIN_WAITING_PID.
struct TerminalStdin {
    buffer: [u8; 256],
    len: usize,
    ready: bool,
}

struct TerminalStdinCell(UnsafeCell<TerminalStdin>);
unsafe impl Sync for TerminalStdinCell {}
static TERMINAL_STDIN: TerminalStdinCell = TerminalStdinCell(UnsafeCell::new(TerminalStdin {
    buffer: [0; 256],
    len: 0,
    ready: false,
}));

pub const WAIT_KIND_EMPTY: i32 = -1;
pub const WAIT_KIND_SPAWN_PREPARED: i32 = -2;

/// Process lifecycle for the non-preemptive foundation:
///
/// Prepared: process object exists and owns its address space, kernel stack,
/// fd table, cwd and pid, but no executable image has run yet.
/// Ready: executable context has been prepared and a future scheduler may run it.
/// Running: CURRENT_PID points at the process table record while the CPU is in
/// user mode. The process object still lives inside PROCESS_TABLE.
/// Dead: execution finished or faulted; wait observes the real status only when
/// has_run is true. Prepared/not-started children keep an explicit wait status.
/// Reaped: terminal exec or a future wait path consumed the heavyweight
/// ownership. Metadata stays behind briefly for diagnostics, but it is not
/// runnable or waitable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessState {
    Prepared,
    Ready,
    Running,
    Blocked,
    Dead,
    Reaped,
}

pub struct ProcessRecord {
    pub pid: ProcessId,
    /// None for a process/main thread; Some(pid) for an additional thread.
    pub owner: Option<ProcessId>,
    pub parent: Option<ProcessId>,
    pub state: ProcessState,
    pub status: Option<ProcessExitStatus>,
    pub has_run: bool,
    pub waitable: bool,
    pub path: String,
    /// Тяжёлый объект процесса живёт в `Box`, а не прямо в `Vec`. Это важно:
    /// `current_process()`/`with_process_mut()` раздают `&'static Process`,
    /// указывающие внутрь этого объекта. Если бы `Process` лежал прямо в
    /// элементе `Vec`, то `push` с реаллокацией буфера сделал бы такие ссылки
    /// висячими (UB). Указатель в `Box` при реаллокации `Vec` перемещается, а
    /// сам объект на куче остаётся на месте — ссылки остаются валидными.
    process: Option<Box<Process>>,
}

#[derive(Debug, Clone)]
pub struct ProcessSnapshot {
    pub pid: ProcessId,
    pub parent: Option<ProcessId>,
    pub state: ProcessState,
    pub status: Option<ProcessExitStatus>,
    pub has_run: bool,
    pub waitable: bool,
    pub path: String,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct ProcessStats {
    pub total: u64,
    pub prepared: u64,
    pub ready: u64,
    pub running: u64,
    pub blocked: u64,
    pub dead: u64,
    pub reaped: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WaitRecord {
    pub pid: ProcessId,
    pub kind: i32,
    pub code: i32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct CpuContext {
    pub rax: u64,
    pub rbx: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub rbp: u64,
    pub rsp: u64,
    pub r8: u64,
    pub r9: u64,
    pub r10: u64,
    pub r11: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
    pub rip: u64,
    pub rflags: u64,
}

impl CpuContext {
    pub const fn new() -> Self {
        Self {
            rax: 0,
            rbx: 0,
            rcx: 0,
            rdx: 0,
            rsi: 0,
            rdi: 0,
            rbp: 0,
            rsp: 0,
            r8: 0,
            r9: 0,
            r10: 0,
            r11: 0,
            r12: 0,
            r13: 0,
            r14: 0,
            r15: 0,
            rip: 0,
            rflags: 0,
        }
    }
}

#[repr(C, align(16))]
pub struct FpuState(UnsafeCell<[u8; 512]>);

// On this UP kernel, the selected process alone owns its FPU buffer while
// user mode runs. IRQs disable nesting before saving to the same buffer.
unsafe impl Sync for FpuState {}

impl FpuState {
    pub const fn new() -> Self {
        let mut bytes = [0; 512];
        // FINIT defaults: x87 control word and MXCSR. The remaining registers
        // start cleared, so each new process receives an independent state.
        bytes[0] = 0x7f;
        bytes[1] = 0x03;
        bytes[24] = 0x80;
        bytes[25] = 0x1f;
        Self(UnsafeCell::new(bytes))
    }
}

pub struct Process {
    pub pid: ProcessId,
    pub state: ProcessState,
    pub context: CpuContext,
    /// 16-byte aligned legacy x87/MMX/SSE state (FXSAVE format).
    pub fpu_state: FpuState,
    /// x86_64 userspace thread pointer installed in IA32_FS_BASE.
    pub fs_base: u64,
    pub is_kernel: bool,
    pub cwd: String,
    pub status: Option<ProcessExitStatus>,
    address_space: Option<AddressSpace>,
    kernel_stack: Option<Vec<u8>>,
    pub kernel_stack_top: usize,
    pub entry_argc: usize,
    pub entry_argv: usize,
    pub entry_envp: usize,
    fd_table: BTreeMap<ProcessFd, FdEntry>,
    next_fd: ProcessFd,
    next_mmap_addr: usize,
    shared_owned: Vec<u64>,
    shared_pages: BTreeMap<usize, u64>,
    guard_regions: BTreeMap<usize, usize>,
    tls_image: Vec<u8>,
    tls_mem_size: usize,
    tls_align: usize,
    tls_allocation: Option<(usize, usize)>,
    /// Таблица хэндлов процесса (capabilities с правами).
    handle_table: crate::handle::HandleTable,
    /// Счётчик доставленных, но ещё не считанных сигналов (право SIGNAL).
    pending_signals: u64,
}

const USER_MMAP_BASE: usize = 0x0000_0010_0000_0000;
const USER_MMAP_END: usize = 0x0000_7000_0000_0000;
// Exclusive top of the user half: first address past the canonical lower half.
// Checks use `base >= USER_ADDRESS_END`, equivalent to `base > 0x7FFF_FFFF_FFFF`
// (see `syscall::USER_SPACE_END`, the inclusive form of the same boundary).
const USER_ADDRESS_END: u64 = 0x0000_8000_0000_0000;
const USER_PAGE_SIZE: usize = 4096;

fn vm_range_end(start: usize, length: usize) -> Result<usize, ProcessError> {
    if start < USER_MMAP_BASE || start & (USER_PAGE_SIZE - 1) != 0 || length == 0 {
        return Err(ProcessError::InvalidMemoryRange);
    }
    let length = length.checked_add(USER_PAGE_SIZE - 1)
        .map(|value| value & !(USER_PAGE_SIZE - 1))
        .ok_or(ProcessError::InvalidMemoryRange)?;
    let end = start.checked_add(length).ok_or(ProcessError::InvalidMemoryRange)?;
    if end > USER_MMAP_END { return Err(ProcessError::InvalidMemoryRange); }
    Ok(end)
}

impl Process {
    /// Creates an isolated userspace process.
    ///
    /// Address-space allocation failure is part of the API contract: callers
    /// must handle it instead of receiving a process that only looks usable.
    pub fn new(pid: ProcessId) -> Result<Self, ProcessError> {
        Self::new_user(pid)
    }

    fn new_without_address_space(pid: ProcessId, is_kernel: bool) -> Self {
        let mut process = Self {
            pid,
            state: ProcessState::Ready,
            context: CpuContext::new(),
            fpu_state: FpuState::new(),
            fs_base: 0,
            is_kernel,
            cwd: String::from("/"),
            status: None,
            address_space: None,
            kernel_stack: None,
            kernel_stack_top: 0,
            entry_argc: 0,
            entry_argv: 0,
            entry_envp: 0,
            fd_table: BTreeMap::new(),
            next_fd: FIRST_PROCESS_FD,
            next_mmap_addr: USER_MMAP_BASE,
            shared_owned: Vec::new(),
            shared_pages: BTreeMap::new(),
            guard_regions: BTreeMap::new(),
            tls_image: Vec::new(),
            tls_mem_size: 0,
            tls_align: 1,
            tls_allocation: None,
            handle_table: crate::handle::HandleTable::new(),
            pending_signals: 0,
        };
        process.reserve_stdio();
        process
    }

    pub fn new_kernel(pid: ProcessId) -> Self {
        Self::new_without_address_space(pid, true)
    }

    pub fn new_user(pid: ProcessId) -> Result<Self, ProcessError> {
        let mut kernel_stack = vec![0u8; DEFAULT_KERNEL_STACK_SIZE];
        let kernel_stack_top = (kernel_stack.as_mut_ptr() as usize + kernel_stack.len()) & !0xF;
        let mut process = Self {
            pid,
            state: ProcessState::Ready,
            context: CpuContext::new(),
            fpu_state: FpuState::new(),
            fs_base: 0,
            is_kernel: false,
            cwd: String::from("/"),
            status: None,
            address_space: Some(
                AddressSpace::new().map_err(|_| ProcessError::AddressSpaceCreateFailed)?,
            ),
            kernel_stack: Some(kernel_stack),
            kernel_stack_top,
            entry_argc: 0,
            entry_argv: 0,
            entry_envp: 0,
            fd_table: BTreeMap::new(),
            next_fd: FIRST_PROCESS_FD,
            next_mmap_addr: USER_MMAP_BASE,
            shared_owned: Vec::new(),
            shared_pages: BTreeMap::new(),
            guard_regions: BTreeMap::new(),
            tls_image: Vec::new(),
            tls_mem_size: 0,
            tls_align: 1,
            tls_allocation: None,
            handle_table: crate::handle::HandleTable::new(),
            pending_signals: 0,
        };
        process.reserve_stdio();
        Ok(process)
    }

    fn new_user_thread(tid: ThreadId, entry: u64, stack_top: u64, arg: u64) -> Self {
        let mut thread = Self::new_without_address_space(ProcessId(tid.0), false);
        let mut kernel_stack = vec![0u8; DEFAULT_KERNEL_STACK_SIZE];
        thread.kernel_stack_top = (kernel_stack.as_mut_ptr() as usize + kernel_stack.len()) & !0xF;
        thread.kernel_stack = Some(kernel_stack);
        thread.fd_table.clear(); // Owned by the process, shared by its threads.
        thread.context.rip = entry;
        thread.context.rsp = stack_top;
        thread.context.rdi = arg;
        thread.context.rflags = 0x202;
        thread
    }

    pub fn address_space(&self) -> Option<&AddressSpace> {
        self.address_space.as_ref()
    }

    pub fn address_space_mut(&mut self) -> Option<&mut AddressSpace> {
        self.address_space.as_mut()
    }

    pub fn has_kernel_stack(&self) -> bool {
        self.kernel_stack.is_some() && self.kernel_stack_top != 0
    }

    pub fn kernel_stack_top(&self) -> Option<usize> {
        if self.has_kernel_stack() {
            Some(self.kernel_stack_top)
        } else {
            None
        }
    }

    pub unsafe fn install_syscall_stack(&self) -> Result<(), ProcessError> {
        let stack_top = self.kernel_stack_top().ok_or(ProcessError::NoKernelStack)?;
        crate::hal::syscall_set_kernel_stack_top(stack_top as u64);
        Ok(())
    }

    pub unsafe fn reset_syscall_stack_policy() {
        crate::hal::syscall_reset_kernel_stack();
    }

    pub unsafe fn activate_address_space(&self) -> Result<ActiveAddressSpace, ProcessError> {
        self.address_space()
            .ok_or(ProcessError::NoAddressSpace)
            .map(|address_space| address_space.activate())
    }

    pub unsafe fn switch_to_address_space(&self) -> Result<(), ProcessError> {
        let root_frame = self
            .address_space()
            .ok_or(ProcessError::NoAddressSpace)?
            .root_frame()
            .as_usize();
        crate::memory::vmm::switch_to_root_frame(root_frame);
        Ok(())
    }

    pub fn terminate(&mut self) {
        self.state = ProcessState::Dead;
    }

    pub fn exit(&mut self, code: i32) {
        self.status = Some(ProcessExitStatus::Exited(code));
        self.terminate();
    }

    pub fn fault(&mut self, fault: ProcessFault) {
        self.status = Some(ProcessExitStatus::Fault(fault));
        self.terminate();
    }

    pub fn cleanup_fds(&mut self) -> usize {
        let fd_table = core::mem::take(&mut self.fd_table);
        let mut closed = 0;

        for (_, entry) in fd_table {
            if let FdTarget::Vfs(vfs_fd) = entry.target {
                if let Some(vfs) = crate::fs::vfs::get_vfs() {
                    let _ = vfs.close(vfs_fd);
                    closed += 1;
                }
            }
        }

        closed
    }

    pub fn allocate_fd(&mut self, entry: FdEntry) -> Result<ProcessFd, ProcessError> {
        for _ in FIRST_PROCESS_FD..MAX_PROCESS_FD {
            let fd = self.next_fd;
            self.next_fd += 1;
            if self.next_fd >= MAX_PROCESS_FD {
                self.next_fd = FIRST_PROCESS_FD;
            }

            if !self.fd_table.contains_key(&fd) {
                self.fd_table.insert(fd, entry);
                return Ok(fd);
            }
        }

        Err(ProcessError::FdTableFull)
    }

    pub fn get_fd(&self, fd: ProcessFd) -> Option<&FdEntry> {
        self.fd_table.get(&fd)
    }

    pub fn close_fd(&mut self, fd: ProcessFd) -> Result<FdEntry, ProcessError> {
        self.fd_table.remove(&fd).ok_or(ProcessError::InvalidFd)
    }

    /// Maps zero-filled private anonymous memory into this process.
    /// A zero address chooses a monotonically increasing, page-aligned region;
    /// a non-zero address is treated as an exact request and must be free.
    pub fn map_anonymous(
        &mut self,
        requested_addr: usize,
        length: usize,
        writable: bool,
        executable: bool,
        guarded: bool,
        accessible: bool,
    ) -> Result<usize, ProcessError> {
        if self.is_kernel || length == 0 {
            return Err(ProcessError::InvalidMemoryRange);
        }
        let map_length = length
            .checked_add(USER_PAGE_SIZE - 1)
            .map(|value| value & !(USER_PAGE_SIZE - 1))
            .ok_or(ProcessError::InvalidMemoryRange)?;
        let total_length = if guarded {
            map_length.checked_add(2 * USER_PAGE_SIZE)
        } else {
            Some(map_length)
        }.ok_or(ProcessError::InvalidMemoryRange)?;
        let start = if requested_addr == 0 {
            self.next_mmap_addr
        } else {
            if requested_addr & (USER_PAGE_SIZE - 1) != 0 {
                return Err(ProcessError::InvalidMemoryRange);
            }
            requested_addr
        };
        let end = start
            .checked_add(total_length)
            .ok_or(ProcessError::InvalidMemoryRange)?;
        if start < USER_MMAP_BASE || end > USER_MMAP_END {
            return Err(ProcessError::InvalidMemoryRange);
        }

        let address_space = self
            .address_space_mut()
            .ok_or(ProcessError::NoAddressSpace)?;
        let mut page = start;
        while page < end {
            match address_space
                .translate_user_page(crate::memory::vmm::VirtualAddress::from_usize(page))
            {
                Ok(None) => {}
                Ok(Some(_)) => return Err(ProcessError::AddressInUse),
                Err(_) => return Err(ProcessError::InvalidMemoryRange),
            }
            page += USER_PAGE_SIZE;
        }

        let mut page_flags = crate::memory::vmm::PageFlags::empty();
        if writable {
            page_flags |= crate::memory::vmm::PageFlags::WRITABLE;
        }
        if !executable {
            page_flags |= crate::memory::vmm::PageFlags::NO_EXECUTE;
        }

        let mut mapped_end = start;
        while mapped_end < end {
            if address_space
                .map_user_page(
                    crate::memory::vmm::VirtualAddress::from_usize(mapped_end),
                    page_flags,
                )
                .is_err()
            {
                let mut rollback = start;
                while rollback < mapped_end {
                    let _ = address_space
                        .unmap_user_page(crate::memory::vmm::VirtualAddress::from_usize(rollback));
                    rollback += USER_PAGE_SIZE;
                }
                return Err(ProcessError::OutOfMemory);
            }
            mapped_end += USER_PAGE_SIZE;
        }

        if requested_addr == 0 {
            self.next_mmap_addr = end;
        }
        if !accessible {
            let accessible_start = if guarded { start + USER_PAGE_SIZE } else { start };
            let accessible_end = if guarded { end - USER_PAGE_SIZE } else { end };
            let address_space = self.address_space_mut().unwrap();
            for page in (accessible_start..accessible_end).step_by(USER_PAGE_SIZE) {
                address_space.protect_user_page(
                    VirtualAddress(page), PageFlags::NO_EXECUTE, false,
                ).map_err(|_| ProcessError::InvalidMemoryRange)?;
            }
        }
        if guarded {
            let address_space = self.address_space_mut().unwrap();
            let guard_flags = PageFlags::NO_EXECUTE;
            address_space.protect_user_page(VirtualAddress(start), guard_flags, false)
                .map_err(|_| ProcessError::InvalidMemoryRange)?;
            address_space.protect_user_page(VirtualAddress(end - USER_PAGE_SIZE), guard_flags, false)
                .map_err(|_| ProcessError::InvalidMemoryRange)?;
            self.guard_regions.insert(start + USER_PAGE_SIZE, map_length);
            Ok(start + USER_PAGE_SIZE)
        } else {
            Ok(start)
        }
    }

    pub fn create_shared_vm(&mut self, length: usize) -> Result<u64, ProcessError> {
        if self.is_kernel || length == 0 { return Err(ProcessError::InvalidMemoryRange); }
        let id = alloc_shared_vm_object(length)?;
        self.shared_owned.push(id);
        Ok(id)
    }

    pub fn close_shared_vm(&mut self, id: u64) -> Result<(), ProcessError> {
        let index = self.shared_owned.iter().position(|owned| *owned == id)
            .ok_or(ProcessError::InvalidMemoryRange)?;
        self.shared_owned.swap_remove(index);
        shared_vm_release(id, 1);
        Ok(())
    }

    pub fn map_shared_vm(
        &mut self, id: u64, requested_addr: usize, writable: bool,
    ) -> Result<usize, ProcessError> {
        if self.is_kernel { return Err(ProcessError::InvalidUserContext); }
        let frames = {
            let mut objects = SHARED_VM_OBJECTS.lock();
            let object = objects.get_mut(&id).ok_or(ProcessError::InvalidMemoryRange)?;
            object.refs += object.frames.len();
            object.frames.clone()
        };
        let length = frames.len() * USER_PAGE_SIZE;
        let start = if requested_addr == 0 { self.next_mmap_addr } else { requested_addr };
        let end = start.checked_add(length).ok_or(ProcessError::InvalidMemoryRange);
        let valid = start >= USER_MMAP_BASE && start & (USER_PAGE_SIZE - 1) == 0
            && end.as_ref().map(|end| *end <= USER_MMAP_END).unwrap_or(false);
        if !valid {
            shared_vm_release(id, frames.len());
            return Err(ProcessError::InvalidMemoryRange);
        }
        let end = end.unwrap();
        let Some(address_space) = self.address_space_mut() else {
            shared_vm_release(id, frames.len());
            return Err(ProcessError::NoAddressSpace);
        };
        let mut mapped = 0;
        for (index, frame) in frames.iter().copied().enumerate() {
            let page = start + index * USER_PAGE_SIZE;
            if address_space.translate_user_page(VirtualAddress(page)).ok().flatten().is_some()
                || address_space.map_user_frame(
                    VirtualAddress(page), frame,
                    PageFlags::NO_EXECUTE | if writable { PageFlags::WRITABLE } else { PageFlags::empty() },
                ).is_err()
            {
                for rollback in 0..mapped {
                    let _ = address_space.unmap_user_page(VirtualAddress(start + rollback * USER_PAGE_SIZE));
                }
                shared_vm_release(id, frames.len());
                return Err(ProcessError::AddressInUse);
            }
            mapped += 1;
        }
        for index in 0..frames.len() {
            self.shared_pages.insert(start + index * USER_PAGE_SIZE, id);
        }
        if requested_addr == 0 { self.next_mmap_addr = end; }
        Ok(start)
    }

    pub fn unmap_range(&mut self, start: usize, length: usize) -> Result<(), ProcessError> {
        let end = vm_range_end(start, length)?;
        let mut guard = None;
        for (&usable, &len) in &self.guard_regions {
            let reservation_start = usable - USER_PAGE_SIZE;
            let reservation_end = usable + len + USER_PAGE_SIZE;
            if start < reservation_end && end > reservation_start {
                if start != usable || end != usable + len {
                    return Err(ProcessError::InvalidMemoryRange);
                }
                guard = Some((usable, len));
            }
        }
        {
            let address_space = self.address_space_mut().ok_or(ProcessError::NoAddressSpace)?;
            for page in (start..end).step_by(USER_PAGE_SIZE) {
                if address_space.translate_user_page(VirtualAddress(page)).ok().flatten().is_none() {
                    return Err(ProcessError::InvalidMemoryRange);
                }
            }
        }
        for page in (start..end).step_by(USER_PAGE_SIZE) {
            self.address_space_mut().unwrap().unmap_user_page(VirtualAddress(page))
                .map_err(|_| ProcessError::InvalidMemoryRange)?;
            if let Some(id) = self.shared_pages.remove(&page) { shared_vm_release(id, 1); }
        }
        if let Some((usable, len)) = guard {
            let address_space = self.address_space_mut().unwrap();
            address_space.unmap_user_page(VirtualAddress(usable - USER_PAGE_SIZE)).unwrap();
            address_space.unmap_user_page(VirtualAddress(usable + len)).unwrap();
            self.guard_regions.remove(&usable);
        }
        Ok(())
    }

    pub fn protect_range(
        &mut self, start: usize, length: usize, writable: bool,
        executable: bool, present: bool,
    ) -> Result<(), ProcessError> {
        let end = vm_range_end(start, length)?;
        for (usable, len) in &self.guard_regions {
            if start <= *usable - USER_PAGE_SIZE && end > *usable - USER_PAGE_SIZE
                || start <= *usable + *len && end > *usable + *len {
                return Err(ProcessError::InvalidMemoryRange);
            }
        }
        let address_space = self.address_space_mut().ok_or(ProcessError::NoAddressSpace)?;
        for page in (start..end).step_by(USER_PAGE_SIZE) {
            if address_space.translate_user_page(VirtualAddress(page)).ok().flatten().is_none() {
                return Err(ProcessError::InvalidMemoryRange);
            }
        }
        let flags = (if writable { PageFlags::WRITABLE } else { PageFlags::empty() })
            | (if executable { PageFlags::empty() } else { PageFlags::NO_EXECUTE });
        for page in (start..end).step_by(USER_PAGE_SIZE) {
            address_space.protect_user_page(VirtualAddress(page), flags, present)
                .map_err(|_| ProcessError::InvalidMemoryRange)?;
        }
        Ok(())
    }

    pub fn install_tls_template(
        &mut self,
        image: &[u8],
        mem_size: usize,
        align: usize,
    ) -> Result<(), ProcessError> {
        if image.len() > mem_size || mem_size == 0 || !align.is_power_of_two() {
            return Err(ProcessError::InvalidMemoryRange);
        }
        self.tls_image.clear();
        self.tls_image.extend_from_slice(image);
        self.tls_mem_size = mem_size;
        self.tls_align = align.max(1);
        let (base, length, thread_pointer) = self.allocate_tls_instance()?;
        self.fs_base = thread_pointer as u64;
        self.tls_allocation = Some((base, length));
        Ok(())
    }

    fn allocate_tls_instance(&mut self) -> Result<(usize, usize, usize), ProcessError> {
        if self.tls_mem_size == 0 {
            return Ok((0, 0, 0));
        }
        let tls_size = self.tls_mem_size.checked_add(self.tls_align - 1)
            .map(|value| value & !(self.tls_align - 1))
            .ok_or(ProcessError::InvalidMemoryRange)?;
        // Dunit static TLS follows x86_64 variant II: TLS data is directly
        // below the thread pointer and FS:0 contains the thread pointer itself.
        let mapped_length = tls_size.checked_add(core::mem::size_of::<u64>())
            .and_then(|value| value.checked_add(USER_PAGE_SIZE - 1))
            .map(|value| value & !(USER_PAGE_SIZE - 1))
            .ok_or(ProcessError::InvalidMemoryRange)?;
        let base = self.map_anonymous(0, mapped_length, true, false, false, true)?;
        let address_space = self.address_space().ok_or(ProcessError::NoAddressSpace)?;
        let mut copied = 0;
        while copied < self.tls_image.len() {
            let addr = base + copied;
            let chunk = (USER_PAGE_SIZE - (addr & (USER_PAGE_SIZE - 1)))
                .min(self.tls_image.len() - copied);
            let phys = address_space.translate_user_page(VirtualAddress(addr))
                .map_err(|_| ProcessError::InvalidMemoryRange)?
                .ok_or(ProcessError::InvalidMemoryRange)?;
            unsafe {
                core::ptr::copy_nonoverlapping(
                    self.tls_image.as_ptr().add(copied),
                    crate::memory::vmm::phys_to_virt(phys.as_usize()) as *mut u8,
                    chunk,
                );
            }
            copied += chunk;
        }
        let thread_pointer = base + tls_size;
        let tp_phys = address_space.translate_user_page(VirtualAddress(thread_pointer))
            .map_err(|_| ProcessError::InvalidMemoryRange)?
            .ok_or(ProcessError::InvalidMemoryRange)?;
        unsafe {
            (crate::memory::vmm::phys_to_virt(tp_phys.as_usize()) as *mut u64)
                .write_unaligned(thread_pointer as u64);
        }
        Ok((base, mapped_length, thread_pointer))
    }

    pub fn set_thread_pointer(&mut self, base: u64) -> Result<(), ProcessError> {
        if base >= USER_ADDRESS_END {
            return Err(ProcessError::InvalidMemoryRange);
        }
        self.fs_base = base;
        unsafe { crate::hal::set_fs_base(base); }
        Ok(())
    }

    /// Release heavyweight VM ownership at exit, independently of wait/reap.
    /// The caller must have switched away from this address space first.
    fn release_vm_resources(&mut self) {
        self.address_space = None;
        for (_, id) in core::mem::take(&mut self.shared_pages) {
            shared_vm_release(id, 1);
        }
        for id in core::mem::take(&mut self.shared_owned) {
            shared_vm_release(id, 1);
        }
        self.guard_regions.clear();
        self.tls_allocation = None;
        // Освобождаем capability-ресурсы: если процесс держал мастер-право на
        // дисплей, вернуть его системе; затем очистить таблицу хэндлов.
        for object in self.handle_table.drain_objects() {
            if matches!(object, crate::handle::HandleObject::Display) {
                crate::handle::release_display(self.pid);
            }
            if matches!(object, crate::handle::HandleObject::Input) {
                crate::handle::release_input(self.pid);
            }
            if let crate::handle::HandleObject::SharedFrames { id } = object {
                shared_vm_release(id, 1);
            }
        }
    }

    pub fn fd_count(&self) -> usize {
        self.fd_table.len()
    }

    fn reserve_stdio(&mut self) {
        self.fd_table.insert(0, FdEntry::stdin());
        self.fd_table.insert(1, FdEntry::stdout());
        self.fd_table.insert(2, FdEntry::stderr());
    }

    // --- Таблица хэндлов (capabilities с правами) ------------------------------

    /// Создаёт объект памяти нужного размера и возвращает хэндл с полным набором
    /// прав для памяти: READ|WRITE|MAP|TRANSFER.
    pub fn handle_create_memory(&mut self, len: usize) -> Result<Handle, HandleError> {
        if len == 0 || len > crate::handle::MAX_MEMORY_OBJECT {
            return Err(HandleError::TooLarge);
        }
        let data = alloc::vec![0u8; len];
        Ok(self.handle_table.insert(
            HandleObject::Memory(data),
            RIGHT_READ | RIGHT_WRITE | RIGHT_MAP | RIGHT_TRANSFER,
        ))
    }

    /// Создаёт конечную точку для сигналов процессу `target`. Право SIGNAL|TRANSFER.
    pub fn handle_create_endpoint(&mut self, target: ProcessId) -> Handle {
        self.handle_table
            .insert(HandleObject::Endpoint(target), RIGHT_SIGNAL | RIGHT_TRANSFER)
    }

    pub fn handle_rights(&self, handle: Handle) -> Result<u32, HandleError> {
        self.handle_table.rights(handle)
    }

    pub fn handle_dup(&mut self, handle: Handle, new_rights: u32) -> Result<Handle, HandleError> {
        // Display/Input — эксклюзивные системные singleton'ы с единым глобальным
        // учётом владельца. Дубликат создал бы второй хэндл при одном owner-токене:
        // закрытие первой копии обнулило бы владельца (release_display/input), пока
        // процесс ещё держит вторую и пишет в ресурс, а другой процесс смог бы
        // повторно захватить его. Поэтому дублирование singleton'ов запрещено
        // (перенос уже блокируется в handle_take_for_transfer).
        if let Ok(entry) = self.handle_table.get(handle) {
            if matches!(entry.object, HandleObject::Display | HandleObject::Input) {
                return Err(HandleError::WrongType);
            }
        }
        let new = self.handle_table.duplicate(handle, new_rights)?;
        // Дубликат разделяемого буфера — ещё одна ссылка на тот же объект фреймов;
        // учитываем её, чтобы буфер не освободился раньше времени.
        if let Ok(entry) = self.handle_table.get(new) {
            if let HandleObject::SharedFrames { id } = entry.object {
                if let Some(object) = SHARED_VM_OBJECTS.lock().get_mut(&id) {
                    object.refs += 1;
                }
            }
        }
        Ok(new)
    }

    /// Создаёт разделяемый буфер (zero-copy физические фреймы) и возвращает
    /// capability-хэндл с правами READ|WRITE|MAP|TRANSFER. Хэндл держит одну
    /// ссылку на объект; close/teardown/dup/transfer корректно её учитывают.
    pub fn handle_create_shared(&mut self, len: usize) -> Result<Handle, HandleError> {
        if self.is_kernel {
            return Err(HandleError::WrongType);
        }
        let id = alloc_shared_vm_object(len).map_err(|error| match error {
            ProcessError::OutOfMemory => HandleError::MapFailed,
            _ => HandleError::TooLarge,
        })?;
        Ok(self.handle_table.insert(
            HandleObject::SharedFrames { id },
            RIGHT_READ | RIGHT_WRITE | RIGHT_MAP | RIGHT_TRANSFER,
        ))
    }

    /// Закрывает хэндл; если это было мастер-право на дисплей — возвращает его.
    pub fn handle_close(&mut self, handle: Handle) -> Result<(), HandleError> {
        let entry = self.handle_table.remove(handle)?;
        if matches!(entry.object, HandleObject::Display) {
            crate::handle::release_display(self.pid);
        }
        if matches!(entry.object, HandleObject::Input) {
            crate::handle::release_input(self.pid);
        }
        if let HandleObject::SharedFrames { id } = entry.object {
            shared_vm_release(id, 1);
        }
        Ok(())
    }

    /// Читает из объекта памяти (нужно право READ). Возвращает число байт.
    pub fn handle_memory_read(&self, handle: Handle, out: &mut [u8]) -> Result<usize, HandleError> {
        let entry = self.handle_table.require(handle, RIGHT_READ)?;
        match &entry.object {
            HandleObject::Memory(data) => {
                let n = out.len().min(data.len());
                out[..n].copy_from_slice(&data[..n]);
                Ok(n)
            }
            _ => Err(HandleError::WrongType),
        }
    }

    /// Пишет в объект памяти (нужно право WRITE). Возвращает число байт.
    pub fn handle_memory_write(&mut self, handle: Handle, src: &[u8]) -> Result<usize, HandleError> {
        let entry = self.handle_table.require_mut(handle, RIGHT_WRITE)?;
        match &mut entry.object {
            HandleObject::Memory(data) => {
                let n = src.len().min(data.len());
                data[..n].copy_from_slice(&src[..n]);
                Ok(n)
            }
            _ => Err(HandleError::WrongType),
        }
    }

    /// Отображает содержимое объекта памяти в адресное пространство процесса
    /// (нужно право MAP). Возвращает виртуальный адрес отображения.
    pub fn handle_map(&mut self, handle: Handle, addr: usize, len: usize) -> Result<usize, HandleError> {
        // Проверяем право и достаём байты до вызова map_anonymous, чтобы не
        // держать заимствование таблицы хэндлов через &mut self.
        let shared = {
            let entry = self.handle_table.require(handle, RIGHT_MAP)?;
            match &entry.object {
                HandleObject::Memory(_) => None,
                HandleObject::SharedFrames { id } => {
                    Some((*id, entry.rights & RIGHT_WRITE != 0))
                }
                _ => return Err(HandleError::WrongType),
            }
        };
        if let Some((id, writable)) = shared {
            // Zero-copy: алиасим реальные физические фреймы объекта в адресное
            // пространство через уже проверенный путь map_shared_vm. Право WRITE
            // в маске хэндла определяет writable-маппинг — так capability
            // разграничивает доступ (напр. сервер RO, клиент RW).
            return self
                .map_shared_vm(id, addr, writable)
                .map_err(|_| HandleError::MapFailed);
        }
        let map_len = {
            let entry = self.handle_table.get(handle)?;
            match &entry.object {
                HandleObject::Memory(data) => len.min(data.len()),
                _ => return Err(HandleError::WrongType),
            }
        };
        let mapped = self
            .map_anonymous(addr, len.max(1), true, false, false, true)
            .map_err(|_| HandleError::MapFailed)?;
        // Адресное пространство вызывающего процесса активно во время системного
        // вызова, поэтому пишем прямо в только что отображённый пользовательский
        // регион.
        let entry = self.handle_table.get(handle)?;
        if let HandleObject::Memory(data) = &entry.object {
            unsafe {
                core::ptr::copy_nonoverlapping(data.as_ptr(), mapped as *mut u8, map_len);
            }
        }
        Ok(mapped)
    }

    /// Возвращает истинный размер (в байтах) backing-объекта разделяемого буфера
    /// по хэндлу. Нужен доверенному получателю (напр. компоситору), чтобы
    /// валидировать геометрию присланного буфера против фактически выделенных
    /// фреймов, а не против размера, который называет недоверенный отправитель.
    pub fn handle_shared_len(&self, handle: Handle) -> Result<usize, HandleError> {
        let entry = self.handle_table.get(handle)?;
        match &entry.object {
            HandleObject::SharedFrames { id } => {
                let objects = SHARED_VM_OBJECTS.lock();
                let object = objects.get(id).ok_or(HandleError::BadHandle)?;
                Ok(object.frames.len() * USER_PAGE_SIZE)
            }
            _ => Err(HandleError::WrongType),
        }
    }


    pub fn handle_signal(&mut self, handle: Handle, value: u64) -> Result<(), HandleError> {
        let entry = self.handle_table.require(handle, RIGHT_SIGNAL)?;
        let target = match &entry.object {
            HandleObject::Endpoint(pid) => *pid,
            _ => return Err(HandleError::WrongType),
        };
        if target == self.pid {
            // Доставка самому себе: мутируем напрямую, без повторного взятия
            // &mut на этот же процесс через таблицу процессов (иначе алиасинг).
            self.add_pending_signal(value);
            return Ok(());
        }
        with_process_mut(target, |process| {
            process.add_pending_signal(value);
            Ok(())
        })
        .map_err(|_| HandleError::NoSuchTarget)
    }

    /// Забирает и обнуляет счётчик доставленных сигналов процесса.
    pub fn handle_take_signals(&mut self) -> u64 {
        core::mem::take(&mut self.pending_signals)
    }

    pub fn add_pending_signal(&mut self, value: u64) {
        self.pending_signals = self.pending_signals.saturating_add(value);
    }

    /// Захватывает мастер-право на дисплей (эксклюзивно). Возвращает хэндл с
    /// правом DISPLAY_MASTER (без TRANSFER: singleton не переносится и не
    /// дублируется — см. handle_dup/handle_take_for_transfer).
    pub fn handle_display_acquire(&mut self) -> Result<Handle, HandleError> {
        if !crate::handle::try_acquire_display(self.pid) {
            return Err(HandleError::DisplayBusy);
        }
        Ok(self
            .handle_table
            .insert(HandleObject::Display, RIGHT_DISPLAY_MASTER))
    }

    /// Захватывает мастер-право на источник ввода (эксклюзивно). Возвращает хэндл
    /// с правом INPUT_MASTER (без TRANSFER) либо `DisplayBusy` (ресурс занят).
    pub fn handle_input_acquire(&mut self) -> Result<Handle, HandleError> {
        if !crate::handle::try_acquire_input(self.pid) {
            return Err(HandleError::DisplayBusy);
        }
        Ok(self
            .handle_table
            .insert(HandleObject::Input, RIGHT_INPUT_MASTER))
    }

    /// Забирает хэндл из таблицы для передачи; проверяет право TRANSFER.
    /// Мастер-право на дисплей не переносится (эксклюзивный системный ресурс с
    /// отдельным учётом владельца) — попытка её передать отклоняется.
    pub fn handle_take_for_transfer(
        &mut self,
        handle: Handle,
    ) -> Result<(HandleObject, u32), HandleError> {
        let entry = self.handle_table.require(handle, RIGHT_TRANSFER)?;
        if matches!(entry.object, HandleObject::Display | HandleObject::Input) {
            return Err(HandleError::WrongType);
        }
        self.handle_table.take_for_transfer(handle)
    }

    /// Принимает переданный объект в свою таблицу, сохраняя (сужённые) права.
    pub fn handle_receive(&mut self, object: HandleObject, rights: u32) -> Handle {
        self.handle_table.insert(object, rights)
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        self.release_vm_resources();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessError {
    NoCurrentProcess,
    NoSuchProcess,
    NotChild,
    InvalidFd,
    FdTableFull,
    NoAddressSpace,
    AddressSpaceCreateFailed,
    NoKernelStack,
    InvalidUserContext,
    ProcessNotPrepared,
    ProcessAlreadyExists,
    NotRunnable,
    SchedulerUnavailable,
    InvalidMemoryRange,
    AddressInUse,
    OutOfMemory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessExit {
    pub pid: ProcessId,
    pub status: ProcessExitStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessExitStatus {
    Exited(i32),
    Fault(ProcessFault),
}

impl ProcessExitStatus {
    pub const fn exit_code(self) -> i32 {
        match self {
            ProcessExitStatus::Exited(code) => code,
            ProcessExitStatus::Fault(fault) => fault.exit_code(),
        }
    }

    pub const fn kind_code(self) -> i32 {
        match self {
            ProcessExitStatus::Exited(_) => 0,
            ProcessExitStatus::Fault(fault) => fault_kind_to_i32(fault),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessFault {
    PageFault,
    GeneralProtection,
    InvalidOpcode,
    DivideByZero,
    Unknown,
}

impl ProcessFault {
    pub const fn exit_code(self) -> i32 {
        match self {
            ProcessFault::PageFault => -14,
            ProcessFault::GeneralProtection => -11,
            ProcessFault::InvalidOpcode => -4,
            ProcessFault::DivideByZero => -8,
            ProcessFault::Unknown => -1,
        }
    }

    pub const fn reason(self) -> &'static str {
        match self {
            ProcessFault::PageFault => "page fault",
            ProcessFault::GeneralProtection => "general protection fault",
            ProcessFault::InvalidOpcode => "invalid opcode",
            ProcessFault::DivideByZero => "divide by zero",
            ProcessFault::Unknown => "user fault",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FdTarget {
    Stdin,
    Stdout,
    Stderr,
    Vfs(crate::fs::vfs::FileDescriptor),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u64)]
pub enum ProcessOutputSink {
    SerialOnly = 0,
    Terminal = 1,
    GuiTerminal = 2,
}

impl ProcessOutputSink {
    fn from_u64(value: u64) -> Self {
        match value {
            1 => Self::Terminal,
            2 => Self::GuiTerminal,
            _ => Self::SerialOnly,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FdEntry {
    pub target: FdTarget,
}

impl FdEntry {
    pub const fn stdin() -> Self {
        Self {
            target: FdTarget::Stdin,
        }
    }

    pub const fn stdout() -> Self {
        Self {
            target: FdTarget::Stdout,
        }
    }

    pub const fn stderr() -> Self {
        Self {
            target: FdTarget::Stderr,
        }
    }

    pub const fn vfs(fd: crate::fs::vfs::FileDescriptor) -> Self {
        Self {
            target: FdTarget::Vfs(fd),
        }
    }
}

pub fn allocate_pid() -> ProcessId {
    ProcessId(NEXT_PID.fetch_add(1, Ordering::SeqCst))
}

fn process_table_mut() -> &'static mut Vec<ProcessRecord> {
    unsafe {
        let slot = PROCESS_TABLE.0.get();
        if (*slot).is_none() {
            *slot = Some(Vec::new());
        }
        (*slot).as_mut().unwrap()
    }
}

fn process_record_index(table: &[ProcessRecord], pid: ProcessId) -> Option<usize> {
    table.iter().position(|record| record.pid == pid)
}

fn log_process_transition(pid: ProcessId, from: ProcessState, to: ProcessState, reason: &str) {
    if reason == "yield" || reason == "enter-user" {
        return;
    }
    if !crate::serial::debug_trace_enabled() {
        return;
    }
    crate::memory::serial_write("[PROCESS] pid=");
    serial_write_u64(pid.0);
    crate::memory::serial_write(" state=");
    serial_write_state(from);
    crate::memory::serial_write("->");
    serial_write_state(to);
    if !reason.is_empty() {
        crate::memory::serial_write(" reason=");
        crate::memory::serial_write(reason);
    }
    crate::memory::serial_write("\r\n");
}

fn current_entity_id() -> Option<ProcessId> {
    let pid = CURRENT_PID.load(Ordering::SeqCst);
    if pid == 0 {
        None
    } else {
        Some(ProcessId(pid))
    }
}

fn owner_of(entity: ProcessId) -> Option<ProcessId> {
    // Короткое чтение таблицы на горячем пути планировщика: маскируем IRQ, чтобы
    // таймер (wake_expired_waiters) не взял &mut на тот же Vec во время нашего
    // разделяемого заимствования.
    let _irq = InterruptGuard::new();
    let table = process_table_mut();
    let record = table.get(process_record_index(table, entity)?)?;
    Some(record.owner.unwrap_or(entity))
}

fn current_pid() -> Option<ProcessId> {
    owner_of(current_entity_id()?)
}

pub fn current_tid() -> Option<ThreadId> {
    current_entity_id().map(|id| ThreadId(id.0))
}

fn current_thread() -> Option<&'static Process> {
    let entity = current_entity_id()?;
    let table = process_table_mut();
    let index = process_record_index(table, entity)?;
    table[index].process.as_ref().map(|boxed| &**boxed)
}

pub fn current_thread_mut() -> Option<&'static mut Process> {
    let entity = current_entity_id()?;
    let table = process_table_mut();
    let index = process_record_index(table, entity)?;
    table[index].process.as_mut().map(|boxed| &mut **boxed)
}

pub fn current_thread_pointer() -> Option<u64> {
    current_thread().map(|thread| thread.fs_base)
}

pub fn set_current_thread_pointer(base: u64) -> Result<(), ProcessError> {
    current_thread_mut()
        .ok_or(ProcessError::NoCurrentProcess)?
        .set_thread_pointer(base)
}

pub fn set_terminal_foreground_process(pid: Option<ProcessId>) {
    set_foreground_process(pid, ProcessOutputSink::Terminal);
}

pub fn set_foreground_process(pid: Option<ProcessId>, sink: ProcessOutputSink) {
    TERMINAL_FOREGROUND_PID.store(pid.map(|pid| pid.0).unwrap_or(0), Ordering::SeqCst);
    FOREGROUND_OUTPUT_SINK.store(
        if pid.is_some() {
            sink as u64
        } else {
            ProcessOutputSink::SerialOnly as u64
        },
        Ordering::SeqCst,
    );
    if pid.is_none() {
        TERMINAL_STDIN_WAITING_PID.store(0, Ordering::SeqCst);
        unsafe {
            let stdin = &mut *TERMINAL_STDIN.0.get();
            stdin.len = 0;
            stdin.ready = false;
        }
    }
}

pub fn current_process_output_sink() -> Option<ProcessOutputSink> {
    let foreground = TERMINAL_FOREGROUND_PID.load(Ordering::SeqCst);
    if foreground == 0 {
        return None;
    }

    let mut pid = current_pid().map(|id| id.0).unwrap_or(0);
    while pid != 0 {
        if pid == foreground {
            return Some(ProcessOutputSink::from_u64(
                FOREGROUND_OUTPUT_SINK.load(Ordering::SeqCst),
            ));
        }
        let table = process_table_mut();
        let Some(index) = process_record_index(table, ProcessId(pid)) else {
            return None;
        };
        pid = table[index].parent.map(|parent| parent.0).unwrap_or(0);
    }

    None
}

pub fn current_process_is_terminal_foreground() -> bool {
    current_process_output_sink() == Some(ProcessOutputSink::Terminal)
}

pub fn request_terminal_stdin_for_current() -> Result<(), ProcessError> {
    let pid = current_pid().ok_or(ProcessError::NoCurrentProcess)?;
    if !current_process_is_terminal_foreground() {
        return Err(ProcessError::NoCurrentProcess);
    }
    TERMINAL_STDIN_WAITING_PID.store(pid.0, Ordering::SeqCst);
    Ok(())
}

pub fn terminal_stdin_waiting_pid() -> Option<ProcessId> {
    let pid = TERMINAL_STDIN_WAITING_PID.load(Ordering::SeqCst);
    if pid == 0 {
        None
    } else {
        Some(ProcessId(pid))
    }
}

pub fn provide_terminal_stdin(pid: ProcessId, data: &[u8]) -> Result<(), ProcessError> {
    if TERMINAL_STDIN_WAITING_PID.load(Ordering::SeqCst) != pid.0 {
        return Err(ProcessError::NoSuchProcess);
    }
    unsafe {
        let stdin = &mut *TERMINAL_STDIN.0.get();
        let len = data.len().min(stdin.buffer.len());
        stdin.buffer[..len].copy_from_slice(&data[..len]);
        stdin.len = len;
        stdin.ready = true;
    }
    Ok(())
}

pub fn take_terminal_stdin_for_current(out: &mut [u8]) -> Result<Option<usize>, ProcessError> {
    let pid = current_pid().ok_or(ProcessError::NoCurrentProcess)?;
    if TERMINAL_STDIN_WAITING_PID.load(Ordering::SeqCst) != pid.0 {
        return Ok(None);
    }
    unsafe {
        let stdin = &mut *TERMINAL_STDIN.0.get();
        if !stdin.ready {
            return Ok(None);
        }
        let len = stdin.len.min(out.len());
        out[..len].copy_from_slice(&stdin.buffer[..len]);
        stdin.len = 0;
        stdin.ready = false;
        TERMINAL_STDIN_WAITING_PID.store(0, Ordering::SeqCst);
        Ok(Some(len))
    }
}

pub fn get_process_snapshots() -> Vec<ProcessSnapshot> {
    let mut snapshots = Vec::new();
    let table = process_table_mut();
    for record in table.iter() {
        if record.owner.is_some() {
            continue;
        }
        snapshots.push(ProcessSnapshot {
            pid: record.pid,
            parent: record.parent,
            state: effective_process_state(table, record),
            status: record.status,
            has_run: record.has_run,
            waitable: record.waitable,
            path: record.path.clone(),
        });
    }
    snapshots
}

fn effective_process_state(table: &[ProcessRecord], process: &ProcessRecord) -> ProcessState {
    if process.state == ProcessState::Ready
        && table.iter().any(|record| {
            record.owner == Some(process.pid) && record.state == ProcessState::Running
        })
    {
        ProcessState::Running
    } else {
        process.state
    }
}

pub fn process_stats() -> ProcessStats {
    let table = process_table_mut();
    let mut stats = ProcessStats::default();
    for record in table.iter() {
        if record.owner.is_some() {
            continue;
        }
        stats.total += 1;
        match effective_process_state(table, record) {
            ProcessState::Prepared => stats.prepared += 1,
            ProcessState::Ready => stats.ready += 1,
            ProcessState::Running => stats.running += 1,
            ProcessState::Blocked => stats.blocked += 1,
            ProcessState::Dead => stats.dead += 1,
            ProcessState::Reaped => stats.reaped += 1,
        }
    }
    stats
}

pub fn insert_process_record(
    pid: ProcessId,
    parent: Option<ProcessId>,
    path: String,
    state: ProcessState,
    waitable: bool,
    has_run: bool,
    process: Option<Process>,
) {
    // Тяжёлый объект процесса переносится на кучу: ссылки, розданные из таблицы,
    // должны переживать реаллокацию буфера `Vec` при `push`.
    let process = process.map(Box::new);
    // Прерывания запрещены на время структурной мутации таблицы: реаллокация
    // буфера `Vec` не должна перекрыться с чтением таблицы из IRQ таймера
    // (путь преемпшна).
    let _irq = InterruptGuard::new();
    let table = process_table_mut();
    if let Some(index) = process_record_index(table, pid) {
        let record = &mut table[index];
        record.parent = parent;
        record.owner = None;
        record.path = path;
        record.state = state;
        record.waitable = waitable;
        record.has_run = has_run;
        record.status = None;
        record.process = process;
        return;
    }

    table.push(ProcessRecord {
        pid,
        owner: None,
        parent,
        state,
        status: None,
        has_run,
        waitable,
        path,
        process,
    });
}

pub fn create_user_process_record(path: String, waitable: bool) -> Result<ProcessId, ProcessError> {
    let pid = allocate_pid();
    let parent = current_pid();
    let mut process = Process::new_user(pid)?;
    process.state = ProcessState::Prepared;
    if let Some(current) = current_process() {
        process.cwd = current.cwd.clone();
    }

    insert_process_record(
        pid,
        parent,
        path.clone(),
        ProcessState::Prepared,
        waitable,
        false,
        Some(process),
    );

    if crate::serial::debug_trace_enabled() {
        crate::memory::serial_write("[PROCESS] pid=");
        serial_write_u64(pid.0);
        crate::memory::serial_write(" parent=");
        match parent {
            Some(parent_pid) => serial_write_u64(parent_pid.0),
            None => crate::memory::serial_write("none"),
        }
        crate::memory::serial_write(" state=prepared path=");
        crate::memory::serial_write(&path);
        crate::memory::serial_write("\r\n");
    }

    Ok(pid)
}

/// Create a schedulable thread in the caller's address space. The caller
/// supplies an already mapped user stack; the kernel owns a separate syscall
/// stack, GPR context and FXSAVE area for each TID.
pub fn create_user_thread(entry: u64, stack_top: u64, arg: u64) -> Result<ThreadId, ProcessError> {
    let owner = current_pid().ok_or(ProcessError::NoCurrentProcess)?;
    if current_process().map(|p| p.is_kernel).unwrap_or(true) {
        return Err(ProcessError::InvalidUserContext);
    }
    let tid = ThreadId(allocate_pid().0);
    let (tls_base, tls_length, thread_pointer) = current_process_mut()
        .ok_or(ProcessError::NoCurrentProcess)?
        .allocate_tls_instance()?;
    let mut thread = Box::new(Process::new_user_thread(tid, entry, stack_top, arg));
    thread.fs_base = thread_pointer as u64;
    if tls_length != 0 {
        thread.tls_allocation = Some((tls_base, tls_length));
    }
    {
        let _irq = InterruptGuard::new();
        let table = process_table_mut();
        let owner_index = process_record_index(table, owner).ok_or(ProcessError::NoSuchProcess)?;
        if matches!(table[owner_index].state, ProcessState::Dead | ProcessState::Reaped) {
            return Err(ProcessError::NotRunnable);
        }
        table.push(ProcessRecord {
            pid: ProcessId(tid.0),
            owner: Some(owner),
            parent: None,
            state: ProcessState::Ready,
            status: None,
            has_run: false,
            waitable: false,
            path: String::from("[thread]"),
            process: Some(thread),
        });
    }
    if let Err(error) = crate::process::scheduler::enqueue_ready(ProcessId(tid.0)) {
        let _irq = InterruptGuard::new();
        let table = process_table_mut();
        if let Some(index) = process_record_index(table, ProcessId(tid.0)) {
            table.remove(index);
        }
        if tls_length != 0 {
            let _ = current_process_mut().map(|owner| owner.unmap_range(tls_base, tls_length));
        }
        return Err(error);
    }
    crate::memory::serial_write("[THREAD] ready tid=");
    serial_write_u64(tid.0);
    crate::memory::serial_write(" pid=");
    serial_write_u64(owner.0);
    crate::memory::serial_write("\r\n");
    Ok(tid)
}

pub fn wait_thread(tid: ThreadId) -> Result<ProcessExitStatus, ProcessError> {
    let owner = current_pid().ok_or(ProcessError::NoCurrentProcess)?;
    if current_tid() == Some(tid) {
        return Err(ProcessError::NotChild);
    }
    let table = process_table_mut();
    let index = process_record_index(table, ProcessId(tid.0)).ok_or(ProcessError::NoSuchProcess)?;
    let record = &table[index];
    if record.owner != Some(owner) {
        return Err(ProcessError::NotChild);
    }
    if record.state != ProcessState::Dead {
        return Err(ProcessError::NotRunnable);
    }
    let status = record.status.ok_or(ProcessError::ProcessNotPrepared)?;
    let tls = record.process.as_ref().and_then(|thread| thread.tls_allocation);
    let _irq = InterruptGuard::new();
    crate::process::scheduler::remove(ProcessId(tid.0));
    table.remove(index);
    if let Some((base, length)) = tls {
        if let Some(owner) = current_process_mut() {
            owner.unmap_range(base, length)?;
        }
    }
    Ok(status)
}

fn cleanup_owned_threads(owner: ProcessId) {
    let _irq = InterruptGuard::new();
    let table = process_table_mut();
    let mut index = 0;
    while index < table.len() {
        if table[index].owner == Some(owner) {
            remove_waiter(table[index].pid);
            crate::process::scheduler::remove(table[index].pid);
            table.remove(index);
        } else {
            index += 1;
        }
    }
}

pub fn with_process_mut<R>(
    pid: ProcessId,
    f: impl FnOnce(&mut Process) -> Result<R, ProcessError>,
) -> Result<R, ProcessError> {
    // Держим &mut в таблицу процессов, поэтому маскируем прерывания на всё время
    // заимствования: иначе таймерный IRQ (wake_expired_waiters → wake_entry)
    // возьмёт второй &mut на тот же Vec, пока этот жив, что является UB
    // (нарушение noalias). Scheduler-loop вызывает нас при разрешённых
    // прерываниях, так что guard здесь обязателен. Замыкания короткие (мутация
    // записи), в userspace они не входят, так что рост latency несущественен.
    let _irq = InterruptGuard::new();
    let table = process_table_mut();
    let index = process_record_index(table, pid).ok_or(ProcessError::NoSuchProcess)?;
    let record = &mut table[index];
    let process = record
        .process
        .as_mut()
        .ok_or(ProcessError::ProcessNotPrepared)?;
    f(&mut **process)
}

pub fn mark_process_prepared_as_ready(pid: ProcessId) -> Result<(), ProcessError> {
    {
        let table = process_table_mut();
        let index = process_record_index(table, pid).ok_or(ProcessError::NoSuchProcess)?;
        let record = &mut table[index];
        if record.state == ProcessState::Dead || record.state == ProcessState::Reaped {
            return Err(ProcessError::ProcessNotPrepared);
        }
        if record.process.is_none() {
            return Err(ProcessError::ProcessNotPrepared);
        }
        let from = record.state;
        record.state = ProcessState::Ready;
        if let Some(process) = record.process.as_mut() {
            process.state = ProcessState::Ready;
        }
        if from != ProcessState::Ready {
            log_process_transition(pid, from, ProcessState::Ready, "prepare-ready");
        }
    }
    if crate::process::scheduler::enqueue_ready(pid).is_err() {
        crate::memory::serial_write("[SCHED] enqueue rejected pid=");
        serial_write_u64(pid.0);
        crate::memory::serial_write("\r\n");
        return Err(ProcessError::NotRunnable);
    }
    Ok(())
}

pub fn mark_process_ready(pid: ProcessId) {
    let table = process_table_mut();
    if let Some(index) = process_record_index(table, pid) {
        let record = &mut table[index];
        if record.state != ProcessState::Dead && record.state != ProcessState::Reaped {
            let from = record.state;
            record.state = ProcessState::Ready;
            if let Some(process) = record.process.as_mut() {
                process.state = ProcessState::Ready;
            }
            if from != ProcessState::Ready {
                log_process_transition(pid, from, ProcessState::Ready, "elf-ready");
            }
            if crate::process::scheduler::enqueue_ready(pid).is_err() {
                crate::memory::serial_write("[SCHED] enqueue rejected pid=");
                serial_write_u64(pid.0);
                crate::memory::serial_write("\r\n");
            }
        }
    }
}

pub fn mark_process_started(pid: ProcessId) {
    let table = process_table_mut();
    if let Some(index) = process_record_index(table, pid) {
        let record = &mut table[index];
        let from = record.state;
        record.state = ProcessState::Running;
        record.has_run = true;
        crate::process::scheduler::remove(pid);
        if from != ProcessState::Running {
            log_process_transition(pid, from, ProcessState::Running, "enter-user");
        }
    }
}

pub fn save_current_user_context_for_yield(next_pid: ProcessId) -> Result<ProcessId, ProcessError> {
    let pid = current_entity_id().ok_or(ProcessError::NoCurrentProcess)?;
    let table = process_table_mut();
    let index = process_record_index(table, pid).ok_or(ProcessError::NoSuchProcess)?;
    let record = &mut table[index];
    if record.state != ProcessState::Running {
        return Err(ProcessError::NotRunnable);
    }
    let process = record
        .process
        .as_mut()
        .ok_or(ProcessError::ProcessNotPrepared)?;
    if process.is_kernel {
        return Err(ProcessError::InvalidUserContext);
    }

    unsafe {
        crate::hal::syscall_capture_user_context(&mut process.context as *mut CpuContext, 0);
    }
    let from = record.state;
    record.state = ProcessState::Ready;
    process.state = ProcessState::Ready;
    PROCESS_YIELD_REQUESTED.store(true, Ordering::SeqCst);
    PROCESS_SCHEDULE_HINT.store(next_pid.0, Ordering::SeqCst);
    if from != ProcessState::Ready {
        log_process_transition(pid, from, ProcessState::Ready, "yield");
    }
    crate::process::scheduler::enqueue_ready(pid)?;
    Ok(pid)
}

pub fn block_current(key_ipc: Option<ProcessId>, deadline: Option<u64>) -> Result<(), ProcessError> {
    let _irq = InterruptGuard::new();
    let entity = current_entity_id().ok_or(ProcessError::NoCurrentProcess)?;
    let table = process_table_mut();
    let index = process_record_index(table, entity).ok_or(ProcessError::NoSuchProcess)?;
    let record = &mut table[index];
    if record.state != ProcessState::Running {
        return Err(ProcessError::NotRunnable);
    }
    let process = record.process.as_mut().ok_or(ProcessError::ProcessNotPrepared)?;
    if process.is_kernel {
        return Err(ProcessError::InvalidUserContext);
    }
    unsafe {
        crate::hal::syscall_capture_user_context(&mut process.context, 0);
    }
    record.state = ProcessState::Blocked;
    process.state = ProcessState::Blocked;
    WAIT_QUEUE.lock().push(WaitEntry {
        entity,
        key: key_ipc.map(WaitKey::Ipc).unwrap_or(WaitKey::Timer),
        deadline,
    });
    PROCESS_YIELD_REQUESTED.store(true, Ordering::SeqCst);
    PROCESS_SCHEDULE_HINT.store(0, Ordering::SeqCst);
    Ok(())
}

fn wake_entry(entry: WaitEntry) {
    let table = process_table_mut();
    if let Some(index) = process_record_index(table, entry.entity) {
        let record = &mut table[index];
        if record.state == ProcessState::Blocked {
            record.state = ProcessState::Ready;
            if let Some(process) = record.process.as_mut() {
                process.state = ProcessState::Ready;
            }
            let _ = scheduler::enqueue_ready(entry.entity);
        }
    }
}

pub fn wake_ipc_waiters(pid: ProcessId) {
    let _irq = InterruptGuard::new();
    let mut queue = WAIT_QUEUE.lock();
    let mut index = 0;
    while index < queue.len() {
        if queue[index].key == WaitKey::Ipc(pid) {
            wake_entry(queue.remove(index));
        } else {
            index += 1;
        }
    }
}

pub fn wake_expired_waiters() {
    let _irq = InterruptGuard::new();
    let mut queue = WAIT_QUEUE.lock();
    let now = crate::clock::monotonic_ticks();
    let mut index = 0;
    while index < queue.len() {
        if queue[index].deadline.map(|deadline| now >= deadline).unwrap_or(false) {
            wake_entry(queue.remove(index));
        } else {
            index += 1;
        }
    }
}

/// Outcome of a futex-style compare-and-block attempt.
pub enum FutexWaitOutcome {
    /// The word matched `expected`; the caller was parked and must return the
    /// user-context-return sentinel (it will resume on wake).
    Parked,
    /// The word did not match `expected`; the caller should report EAGAIN.
    ValueMismatch,
}

/// Read a `u32` at `addr` from `owner`'s address space, or None if the page is
/// not a present, user-accessible mapping. Checking PRESENT|USER (not just a
/// non-zero page-table entry) keeps this sound once COW/lazy/swapped pages
/// exist: a non-present entry must never be dereferenced through phys_to_virt.
fn read_user_u32(owner: ProcessId, addr: u64) -> Option<u32> {
    use crate::memory::vmm::PageFlags;
    let table = process_table_mut();
    let index = process_record_index(table, owner)?;
    let process = table[index].process.as_ref()?;
    let address_space = process.address_space()?;
    let (phys, flags) = address_space
        .user_page_mapping(VirtualAddress(addr as usize))
        .ok()??;
    if !flags.contains(PageFlags::PRESENT | PageFlags::USER) {
        return None;
    }
    // addr is 4-byte aligned and 4 bytes never cross a page boundary.
    let virt = crate::memory::vmm::phys_to_virt(phys.as_usize()) as *const u32;
    Some(unsafe { virt.read_unaligned() })
}

/// Compare-and-block on a futex word. Under a single IRQ guard + WAIT_QUEUE
/// lock (same ordering as `block_current` / the wake path) so the compare is
/// atomic w.r.t. `futex_wake`. Returns Parked when the caller was blocked,
/// ValueMismatch when the word differs from `expected` (map to EAGAIN), or an
/// error (an unmapped word yields InvalidMemoryRange -> EFAULT at the syscall).
pub fn futex_wait(
    addr: u64,
    expected: u32,
    deadline: Option<u64>,
) -> Result<FutexWaitOutcome, ProcessError> {
    let _irq = InterruptGuard::new();
    let entity = current_entity_id().ok_or(ProcessError::NoCurrentProcess)?;
    let owner = owner_of(entity).ok_or(ProcessError::NoSuchProcess)?;
    // Hold the wait queue lock across the compare so a concurrent wake cannot
    // slip in between the value check and the enqueue.
    let mut queue = WAIT_QUEUE.lock();
    let current = read_user_u32(owner, addr).ok_or(ProcessError::InvalidMemoryRange)?;
    if current != expected {
        return Ok(FutexWaitOutcome::ValueMismatch);
    }
    let table = process_table_mut();
    let index = process_record_index(table, entity).ok_or(ProcessError::NoSuchProcess)?;
    let record = &mut table[index];
    if record.state != ProcessState::Running {
        return Err(ProcessError::NotRunnable);
    }
    let process = record.process.as_mut().ok_or(ProcessError::ProcessNotPrepared)?;
    if process.is_kernel {
        return Err(ProcessError::InvalidUserContext);
    }
    unsafe {
        crate::hal::syscall_capture_user_context(&mut process.context, 0);
    }
    record.state = ProcessState::Blocked;
    process.state = ProcessState::Blocked;
    queue.push(WaitEntry {
        entity,
        key: WaitKey::Word { owner, addr },
        deadline,
    });
    PROCESS_YIELD_REQUESTED.store(true, Ordering::SeqCst);
    PROCESS_SCHEDULE_HINT.store(0, Ordering::SeqCst);
    Ok(FutexWaitOutcome::Parked)
}

/// Wake up to `max` waiters parked on the futex word at `addr` in the current
/// address space. `max == 0` means wake all waiters. Returns the count woken.
pub fn futex_wake(addr: u64, max: usize) -> usize {
    let _irq = InterruptGuard::new();
    let Some(owner) = current_pid() else { return 0; };
    let target = WaitKey::Word { owner, addr };
    let mut queue = WAIT_QUEUE.lock();
    let wake_all = max == 0;
    let mut woken = 0;
    let mut index = 0;
    while index < queue.len() {
        if queue[index].key == target {
            if !wake_all && woken >= max {
                break;
            }
            wake_entry(queue.remove(index));
            woken += 1;
        } else {
            index += 1;
        }
    }
    woken
}

pub fn is_pid_blocked(pid: ProcessId) -> bool {
    // Короткое чтение таблицы: маскируем IRQ, чтобы таймер не мутировал Vec под нами.
    let _irq = InterruptGuard::new();
    process_table_mut()
        .iter()
        .any(|record| record.pid == pid && record.state == ProcessState::Blocked)
}

fn remove_waiter(entity: ProcessId) {
    let _irq = InterruptGuard::new();
    WAIT_QUEUE.lock().retain(|entry| entry.entity != entity);
}

pub fn mark_process_finished(exit: ProcessExit) {
    remove_waiter(exit.pid);
    let table = process_table_mut();
    if let Some(index) = process_record_index(table, exit.pid) {
        let record = &mut table[index];
        let from = record.state;
        record.state = ProcessState::Dead;
        record.status = Some(exit.status);
        record.has_run = true;
        if let Some(process) = record.process.as_mut() {
            process.state = ProcessState::Dead;
            process.status = Some(exit.status);
        }
        crate::process::scheduler::remove(exit.pid);
        if from != ProcessState::Dead {
            log_process_transition(exit.pid, from, ProcessState::Dead, "finished");
        }
    }
}

pub fn reap_process(pid: ProcessId) -> Result<(), ProcessError> {
    remove_waiter(pid);
    let table = process_table_mut();
    let index = process_record_index(table, pid).ok_or(ProcessError::NoSuchProcess)?;
    let record = &mut table[index];
    if record.state != ProcessState::Dead {
        return Err(ProcessError::NotRunnable);
    }
    let from = record.state;
    record.process = None;
    record.state = ProcessState::Reaped;
    record.waitable = false;
    crate::process::scheduler::remove(pid);
    if let Some(ipc) = crate::ipc::get_ipc_manager() {
        ipc.clear_messages(pid);
    }
    log_process_transition(pid, from, ProcessState::Reaped, "reap");
    Ok(())
}

pub fn process_exists(pid: ProcessId) -> bool {
    let table = process_table_mut();
    process_record_index(table, pid)
        .map(|index| table[index].owner.is_none())
        .unwrap_or(false)
}

pub fn thread_count_for_pid(owner: ProcessId) -> usize {
    process_table_mut().iter().filter(|record| record.owner == Some(owner)).count()
}

/// Terminates a non-kernel process and leaves a waitable exit status for its
/// parent. The current process uses the normal syscall escape path; a prepared
/// or ready child can be marked dead without ever entering userspace.
pub fn kill_process(pid: ProcessId, code: i32) -> Result<bool, ProcessError> {
    if current_pid() == Some(pid) {
        // A secondary thread cannot synchronously destroy the owner address
        // space while still executing on it. Process-wide self-exit is added
        // with the later lifecycle/signal contract.
        if current_entity_id() != Some(pid) {
            return Err(ProcessError::InvalidUserContext);
        }
        let process = current_process().ok_or(ProcessError::NoCurrentProcess)?;
        if process.is_kernel {
            return Err(ProcessError::InvalidUserContext);
        }
        PROCESS_EXIT_CODE.store(code, Ordering::SeqCst);
        PROCESS_EXIT_KIND.store(0, Ordering::SeqCst);
        PROCESS_EXIT_REQUESTED.store(true, Ordering::SeqCst);
        return Ok(true);
    }

    {
        let table = process_table_mut();
        let index = process_record_index(table, pid).ok_or(ProcessError::NoSuchProcess)?;
        let record = &mut table[index];
        if record.owner.is_some() {
            return Err(ProcessError::NoSuchProcess);
        }
        let process = record
            .process
            .as_mut()
            .ok_or(ProcessError::ProcessNotPrepared)?;
        if process.is_kernel {
            return Err(ProcessError::InvalidUserContext);
        }
        if matches!(record.state, ProcessState::Dead | ProcessState::Reaped) {
            return Err(ProcessError::NotRunnable);
        }

        let status = ProcessExitStatus::Exited(code);
        let from = record.state;
        process.exit(code);
        process.release_vm_resources();
        record.state = ProcessState::Dead;
        record.status = Some(status);
        record.has_run = true;
        crate::process::scheduler::remove(pid);
        if let Some(ipc) = crate::ipc::get_ipc_manager() {
            ipc.clear_messages(pid);
        }
        log_process_transition(pid, from, ProcessState::Dead, "kill");
    }
    remove_waiter(pid);
    cleanup_owned_threads(pid);
    Ok(false)
}

pub fn is_pid_runnable(pid: ProcessId) -> bool {
    let table = process_table_mut();
    let Some(index) = process_record_index(table, pid) else {
        return false;
    };
    let record = &table[index];
    if record.state != ProcessState::Ready {
        return false;
    }
    let Some(process) = record.process.as_ref() else {
        return false;
    };
    let owner = record.owner.unwrap_or(pid);
    let owner_address_space = process_record_index(table, owner)
        .and_then(|owner_index| table[owner_index].process.as_ref())
        .and_then(|owner_process| owner_process.address_space());
    !process.is_kernel
        && process.context.rip != 0
        && process.context.rsp != 0
        && owner_address_space.is_some()
        && process.kernel_stack_top().is_some()
}

pub fn wait_for_child(requested_pid: ProcessId) -> Result<WaitRecord, ProcessError> {
    let parent_pid = current_process().ok_or(ProcessError::NoCurrentProcess)?.pid;
    let table = process_table_mut();
    let has_requested_process =
        requested_pid.0 == 0 || table.iter().any(|record| record.pid == requested_pid);

    let mut child_index = None;
    for (index, record) in table.iter().enumerate() {
        if record.parent != Some(parent_pid) || !record.waitable {
            continue;
        }
        if requested_pid.0 != 0 && record.pid != requested_pid {
            continue;
        }
        child_index = Some(index);
        break;
    }

    let index = match child_index {
        Some(index) => index,
        None if has_requested_process => return Err(ProcessError::NotChild),
        None => return Err(ProcessError::NoSuchProcess),
    };

    let record = &table[index];
    let (kind, code) = match record.status {
        Some(status) if record.has_run => (status.kind_code(), status.exit_code()),
        _ if record.state == ProcessState::Prepared && !record.has_run => {
            (WAIT_KIND_SPAWN_PREPARED, 0)
        }
        _ if matches!(
            record.state,
            ProcessState::Ready | ProcessState::Running | ProcessState::Blocked
        ) =>
        {
            return Err(ProcessError::NotRunnable);
        }
        _ => return Err(ProcessError::ProcessNotPrepared),
    };

    let pid = record.pid;
    let from = record.state;
    log_process_transition(pid, from, ProcessState::Reaped, "wait");
    crate::process::scheduler::remove(pid);
    if let Some(ipc) = crate::ipc::get_ipc_manager() {
        ipc.clear_messages(pid);
    }
    // Удаление сдвигает буфер `Vec`: запрещаем прерывания, чтобы скан таблицы
    // из IRQ таймера не увидел таблицу в промежуточном состоянии.
    let irq = InterruptGuard::new();
    let mut record = table.remove(index);
    drop(irq);
    if let Some(mut process) = record.process.take() {
        let closed = process.cleanup_fds();
        if closed > 0 {
            crate::memory::serial_write("[PROCESS] reaped pid=");
            serial_write_u64(pid.0);
            crate::memory::serial_write(" cleaned-fds=");
            serial_write_usize(closed);
            crate::memory::serial_write("\r\n");
        }
    }

    Ok(WaitRecord { pid, kind, code })
}

pub fn cleanup_prepared_children(parent_pid: ProcessId) -> usize {
    let table = process_table_mut();
    let mut removed = 0;
    let mut index = 0;
    while index < table.len() {
        let should_remove = table[index].parent == Some(parent_pid)
            && !table[index].has_run
            && table[index].process.is_some();
        if should_remove {
            let pid = table[index].pid;
            let from = table[index].state;
            log_process_transition(pid, from, ProcessState::Reaped, "parent-exit");
            let irq = InterruptGuard::new();
            let mut record = table.remove(index);
            drop(irq);
            if let Some(mut process) = record.process.take() {
                let _ = process.cleanup_fds();
            }
            crate::process::scheduler::remove(pid);
            if let Some(ipc) = crate::ipc::get_ipc_manager() {
                ipc.clear_messages(pid);
            }
            removed += 1;
        } else {
            index += 1;
        }
    }
    removed
}

pub fn autoreap_process(pid: ProcessId, reason: &str) -> Result<(), ProcessError> {
    let table = process_table_mut();
    let index = process_record_index(table, pid).ok_or(ProcessError::NoSuchProcess)?;
    let from = table[index].state;
    log_process_transition(pid, from, ProcessState::Reaped, reason);
    crate::process::scheduler::remove(pid);
    if let Some(ipc) = crate::ipc::get_ipc_manager() {
        ipc.clear_messages(pid);
    }
    let irq = InterruptGuard::new();
    let mut record = table.remove(index);
    drop(irq);
    if let Some(mut process) = record.process.take() {
        let _ = process.cleanup_fds();
    }
    Ok(())
}

pub fn snapshot_processes(out: &mut Vec<ProcessSnapshot>) {
    out.clear();
    let table = process_table_mut();
    for record in table.iter() {
        if record.owner.is_some() {
            continue;
        }
        out.push(ProcessSnapshot {
            pid: record.pid,
            parent: record.parent,
            state: effective_process_state(table, record),
            status: record.status,
            has_run: record.has_run,
            waitable: record.waitable,
            path: record.path.clone(),
        });
    }
}

pub fn init_current_kernel_process() {
    if CURRENT_PID.load(Ordering::SeqCst) != 0 {
        return;
    }

    let pid = allocate_pid();
    let mut process = Process::new_kernel(pid);
    process.state = ProcessState::Running;
    insert_process_record(
        pid,
        None,
        String::from("kernel"),
        ProcessState::Running,
        false,
        true,
        Some(process),
    );
    CURRENT_PID.store(pid.0, Ordering::SeqCst);
}

pub fn current_process() -> Option<&'static Process> {
    let pid = current_pid()?;
    let table = process_table_mut();
    let index = process_record_index(table, pid)?;
    table[index].process.as_ref().map(|boxed| &**boxed)
}

pub fn current_process_mut() -> Option<&'static mut Process> {
    let pid = current_pid()?;
    let table = process_table_mut();
    let index = process_record_index(table, pid)?;
    table[index].process.as_mut().map(|boxed| &mut **boxed)
}

pub fn allocate_fd(entry: FdEntry) -> Result<ProcessFd, ProcessError> {
    current_process_mut()
        .ok_or(ProcessError::NoCurrentProcess)?
        .allocate_fd(entry)
}

pub fn get_fd(fd: ProcessFd) -> Option<&'static FdEntry> {
    current_process()?.get_fd(fd)
}

pub fn close_fd(fd: ProcessFd) -> Result<FdEntry, ProcessError> {
    current_process_mut()
        .ok_or(ProcessError::NoCurrentProcess)?
        .close_fd(fd)
}

pub fn exit_current_process(code: i32) -> Option<ProcessId> {
    let process = current_process_mut()?;
    process.exit(code);
    Some(process.pid)
}

pub fn request_current_user_exit(code: i32) -> Option<ProcessId> {
    let process = current_process()?;
    if process.is_kernel {
        return None;
    }
    PROCESS_EXIT_CODE.store(code, Ordering::SeqCst);
    PROCESS_EXIT_KIND.store(0, Ordering::SeqCst);
    PROCESS_EXIT_REQUESTED.store(true, Ordering::SeqCst);
    Some(process.pid)
}

pub fn request_current_user_fault(fault: ProcessFault) -> Option<ProcessId> {
    let process = current_process()?;
    if process.is_kernel {
        return None;
    }
    PROCESS_EXIT_CODE.store(fault.exit_code(), Ordering::SeqCst);
    PROCESS_EXIT_KIND.store(fault_kind_to_i32(fault), Ordering::SeqCst);
    PROCESS_EXIT_REQUESTED.store(true, Ordering::SeqCst);
    Some(process.pid)
}

pub fn user_fault_escape_requested() -> bool {
    PROCESS_EXIT_REQUESTED.load(Ordering::SeqCst) && PROCESS_EXIT_KIND.load(Ordering::SeqCst) != 0
}

pub fn preempt_switch_requested() -> bool {
    PREEMPT_SWITCH_REQUESTED.load(Ordering::SeqCst)
}

pub fn clear_preempt_switch() {
    PREEMPT_SWITCH_REQUESTED.store(false, Ordering::SeqCst);
}

/// Runtime switch for diagnostics; normal operation is preemptive.
pub fn set_preemption_enabled(enabled: bool) {
    PREEMPTION_ENABLED.store(enabled, Ordering::SeqCst);
}

pub fn preemption_enabled() -> bool {
    PREEMPTION_ENABLED.load(Ordering::SeqCst)
}

pub fn timer_preempt_save_and_schedule(frame: &crate::interrupts::InterruptFrame) {
    if !PREEMPTION_ENABLED.load(Ordering::SeqCst) {
        return;
    }

    let current = match current_entity_id() {
        Some(pid) => pid,
        None => return,
    };

    {
        let table = process_table_mut();
        let Some(index) = process_record_index(table, current) else {
            return;
        };
        let record = &table[index];
        if record.state != ProcessState::Running {
            return;
        }
        if record.process.as_ref().map(|p| p.is_kernel).unwrap_or(true) {
            return;
        }
    }

    let next = match crate::process::scheduler::pick_next_candidate_excluding(current) {
        Some(pid) => pid,
        None => return,
    };

    {
        let table = process_table_mut();
        let Some(index) = process_record_index(table, current) else {
            return;
        };
        let record = &mut table[index];
        if let Some(process) = record.process.as_mut() {
            if !process.is_kernel {
                let ctx = &mut process.context;
                ctx.rax = frame.rax;
                ctx.rbx = frame.rbx;
                ctx.rcx = frame.rcx;
                ctx.rdx = frame.rdx;
                ctx.rsi = frame.rsi;
                ctx.rdi = frame.rdi;
                ctx.rbp = frame.rbp;
                ctx.rsp = frame.rsp;
                ctx.r8 = frame.r8;
                ctx.r9 = frame.r9;
                ctx.r10 = frame.r10;
                ctx.r11 = frame.r11;
                ctx.r12 = frame.r12;
                ctx.r13 = frame.r13;
                ctx.r14 = frame.r14;
                ctx.r15 = frame.r15;
                ctx.rip = frame.rip;
                ctx.rflags = frame.rflags;
            }
        }
        let from = record.state;
        record.state = ProcessState::Ready;
        if let Some(process) = record.process.as_mut() {
            process.state = ProcessState::Ready;
        }
        if from != ProcessState::Ready {
            log_process_transition(current, from, ProcessState::Ready, "preempt");
        }
    }

    let _ = crate::process::scheduler::enqueue_ready(current);

    PREEMPTION_COUNT.fetch_add(1, Ordering::SeqCst);
    PROCESS_YIELD_REQUESTED.store(true, Ordering::SeqCst);
    PROCESS_SCHEDULE_HINT.store(next.0, Ordering::SeqCst);
    PREEMPT_SWITCH_REQUESTED.store(true, Ordering::SeqCst);
}

/// Number of timer-driven context switches committed so far.
pub fn preemption_count() -> u64 {
    PREEMPTION_COUNT.load(Ordering::SeqCst)
}

/// Reset the preemption counter (used to bracket a smoke-test window).
pub fn reset_preemption_count() {
    PREEMPTION_COUNT.store(0, Ordering::SeqCst);
}

fn take_process_exit_request() -> Option<(i32, ProcessExitStatus)> {
    if PROCESS_EXIT_REQUESTED.swap(false, Ordering::SeqCst) {
        let code = PROCESS_EXIT_CODE.load(Ordering::SeqCst);
        let kind = PROCESS_EXIT_KIND.swap(0, Ordering::SeqCst);
        let status = match kind {
            0 => ProcessExitStatus::Exited(code),
            value => ProcessExitStatus::Fault(fault_kind_from_i32(value)),
        };
        Some((code, status))
    } else {
        None
    }
}

fn take_process_yield_request() -> bool {
    PROCESS_YIELD_REQUESTED.swap(false, Ordering::SeqCst)
}

fn take_schedule_hint() -> Option<ProcessId> {
    let pid = PROCESS_SCHEDULE_HINT.swap(0, Ordering::SeqCst);
    if pid == 0 {
        None
    } else {
        Some(ProcessId(pid))
    }
}

pub fn enter_user_process(pid: ProcessId) -> Result<ProcessExit, ProcessError> {
    let previous_pid = CURRENT_PID.load(Ordering::SeqCst);
    let previous_root = unsafe { crate::memory::vmm::active_root_frame() };
    let root_pid = pid;
    let mut next_pid = pid;
    loop {
        if is_pid_blocked(next_pid) {
            next_pid = crate::process::scheduler::pick_next_candidate()
                .ok_or(ProcessError::SchedulerUnavailable)?;
        }
        let address_space_ready = {
            // Разделяемое чтение таблицы под маской IRQ; ссылки не покидают блок.
            let _irq = InterruptGuard::new();
            owner_of(next_pid)
                .and_then(|owner| process_table_mut()
                    .iter()
                    .find(|record| record.pid == owner)
                    .and_then(|record| record.process.as_ref())
                    .and_then(|p| p.address_space()))
                .is_some()
        };
        let context = with_process_mut(next_pid, |process| {
            if process.is_kernel {
                return Err(ProcessError::InvalidUserContext);
            }
            if process.context.rip == 0 || process.context.rsp == 0 {
                return Err(ProcessError::InvalidUserContext);
            }
            if !address_space_ready {
                return Err(ProcessError::NoAddressSpace);
            }
            if process.kernel_stack_top().is_none() {
                return Err(ProcessError::NoKernelStack);
            }

            process.state = ProcessState::Running;
            Ok(process.context)
        })?;

        mark_process_started(next_pid);
        CURRENT_PID.store(next_pid.0, Ordering::SeqCst);

        let run_result = unsafe {
            match (current_thread(), current_process()) {
                (Some(thread), Some(owner)) => match thread.install_syscall_stack() {
                    Ok(()) => match owner.switch_to_address_space() {
                        Ok(()) => {
                            crate::hal::set_user_fpu_state(thread.fpu_state.0.get().cast::<u8>());
                            crate::hal::set_fs_base(thread.fs_base);
                            crate::hal::run_user_context(&context as *const CpuContext);
                            crate::hal::set_fs_base(0);
                            crate::hal::set_user_fpu_state(core::ptr::null_mut());
                            Ok(())
                        }
                        Err(error) => Err(error),
                    },
                    Err(error) => Err(error),
                },
                _ => Err(ProcessError::NoCurrentProcess),
            }
        };

        unsafe {
            Process::reset_syscall_stack_policy();
        }

        if take_process_yield_request() {
        } else {
            let mut status = ProcessExitStatus::Fault(ProcessFault::Unknown);
            let is_thread = owner_of(next_pid) != Some(next_pid);
            let closed = with_process_mut(next_pid, |finished| {
                status = if let Some((_code, requested_status)) = take_process_exit_request() {
                    match requested_status {
                        ProcessExitStatus::Exited(code) => finished.exit(code),
                        ProcessExitStatus::Fault(fault) => finished.fault(fault),
                    }
                    requested_status
                } else if finished.state != ProcessState::Dead {
                    finished.fault(ProcessFault::Unknown);
                    ProcessExitStatus::Fault(ProcessFault::Unknown)
                } else {
                    finished
                        .status
                        .unwrap_or(ProcessExitStatus::Fault(ProcessFault::Unknown))
                };
                Ok(if is_thread { 0 } else { finished.cleanup_fds() })
            })?;
            if closed > 0 {
                crate::memory::serial_write("[PROCESS-RUN] cleaned fds=");
                serial_write_usize(closed);
                crate::memory::serial_write("\r\n");
            }
            let exit = ProcessExit {
                pid: next_pid,
                status,
            };
            mark_process_finished(exit);
            if !is_thread {
                cleanup_owned_threads(next_pid);
                unsafe { crate::memory::vmm::switch_to_root_frame(previous_root); }
                with_process_mut(next_pid, |finished| {
                    finished.release_vm_resources();
                    Ok(())
                })?;
            }
            let reaped_children = if is_thread {
                0
            } else {
                cleanup_prepared_children(next_pid)
            };
            if reaped_children > 0 {
                crate::memory::serial_write("[PROCESS-RUN] reaped prepared children=");
                serial_write_usize(reaped_children);
                crate::memory::serial_write("\r\n");
            }
            if next_pid == root_pid {
                CURRENT_PID.store(previous_pid, Ordering::SeqCst);
                unsafe {
                    crate::memory::vmm::switch_to_root_frame(previous_root);
                    match current_process() {
                        Some(current)
                            if !current.is_kernel && current.kernel_stack_top().is_some() =>
                        {
                            let _ = current.install_syscall_stack();
                        }
                        _ => Process::reset_syscall_stack_policy(),
                    }
                }
                return run_result.map(|_| exit);
            }
        }

        if let Err(error) = run_result {
            return Err(error);
        }
        let hinted = take_schedule_hint()
            .filter(|candidate| *candidate != next_pid && is_pid_runnable(*candidate));
        let preferred_root =
            (next_pid != root_pid && is_pid_runnable(root_pid)).then_some(root_pid);
        match hinted
            .or(preferred_root)
            .or_else(|| crate::process::scheduler::pick_next_candidate_excluding(next_pid))
        {
            Some(candidate) => {
                next_pid = candidate;
            }
            None => {
                if next_pid == root_pid || is_pid_blocked(root_pid) {
                    CURRENT_PID.store(previous_pid, Ordering::SeqCst);
                    unsafe {
                        crate::memory::vmm::switch_to_root_frame(previous_root);
                        match current_process() {
                            Some(current)
                                if !current.is_kernel && current.kernel_stack_top().is_some() =>
                            {
                                let _ = current.install_syscall_stack();
                            }
                            _ => Process::reset_syscall_stack_policy(),
                        }
                    }
                    return Err(ProcessError::SchedulerUnavailable);
                }
                next_pid = root_pid;
            }
        }
    }
}

const fn fault_kind_to_i32(fault: ProcessFault) -> i32 {
    match fault {
        ProcessFault::PageFault => 1,
        ProcessFault::GeneralProtection => 2,
        ProcessFault::InvalidOpcode => 3,
        ProcessFault::DivideByZero => 4,
        ProcessFault::Unknown => 5,
    }
}

const fn fault_kind_from_i32(value: i32) -> ProcessFault {
    match value {
        1 => ProcessFault::PageFault,
        2 => ProcessFault::GeneralProtection,
        3 => ProcessFault::InvalidOpcode,
        4 => ProcessFault::DivideByZero,
        _ => ProcessFault::Unknown,
    }
}

#[cfg(feature = "boot-smoke-tests")]
pub fn run_process_address_space_smoke() -> bool {
    crate::memory::serial_write("[PROCESS-ADDRSPACE-TEST] START\r\n");

    let pid = allocate_pid();
    let mut process = match Process::new_user(pid) {
        Ok(process) => process,
        Err(_) => {
            crate::memory::serial_write("[PROCESS-ADDRSPACE-TEST] create failed\r\n");
            return false;
        }
    };

    let address_space = match process.address_space.as_mut() {
        Some(address_space) => address_space,
        None => {
            crate::memory::serial_write("[PROCESS-ADDRSPACE-TEST] no address space\r\n");
            return false;
        }
    };

    let user_page = VirtualAddress::from_usize(0x0050_0000);
    if address_space
        .map_user_page(user_page, PageFlags::WRITABLE)
        .is_err()
    {
        crate::memory::serial_write("[PROCESS-ADDRSPACE-TEST] map failed\r\n");
        return false;
    }

    unsafe {
        let _active = process.activate_address_space().ok();
        if _active.is_none() {
            crate::memory::serial_write("[PROCESS-ADDRSPACE-TEST] activate failed\r\n");
            return false;
        }

        let ptr = user_page.as_usize() as *mut u64;
        core::ptr::write_volatile(ptr, 0x5052_4F43_4153_5043);
        if core::ptr::read_volatile(ptr) != 0x5052_4F43_4153_5043 {
            crate::memory::serial_write("[PROCESS-ADDRSPACE-TEST] user page check failed\r\n");
            return false;
        }
    }

    crate::memory::serial_write("[PROCESS-ADDRSPACE-TEST] OK\r\n");
    true
}

fn serial_write_u64(mut value: u64) {
    let mut buf = [0u8; 20];
    let mut index = buf.len();

    if value == 0 {
        crate::memory::serial_write("0");
        return;
    }

    while value > 0 {
        index -= 1;
        buf[index] = b'0' + (value % 10) as u8;
        value /= 10;
    }

    for byte in &buf[index..] {
        let ch = [*byte];
        let s = unsafe { core::str::from_utf8_unchecked(&ch) };
        crate::memory::serial_write(s);
    }
}

fn serial_write_usize(value: usize) {
    serial_write_u64(value as u64);
}

fn serial_write_state(state: ProcessState) {
    let name = match state {
        ProcessState::Prepared => "Prepared",
        ProcessState::Ready => "Ready",
        ProcessState::Running => "Running",
        ProcessState::Blocked => "Blocked",
        ProcessState::Dead => "Dead",
        ProcessState::Reaped => "Reaped",
    };
    crate::memory::serial_write(name);
}

#[cfg(feature = "boot-smoke-tests")]
pub fn run_process_kernel_stack_smoke() -> bool {
    crate::memory::serial_write("[PROCESS-KSTACK-TEST] START\r\n");

    let pid = allocate_pid();
    let process = match Process::new_user(pid) {
        Ok(process) => process,
        Err(_) => {
            crate::memory::serial_write("[PROCESS-KSTACK-TEST] create failed\r\n");
            return false;
        }
    };

    match process.kernel_stack_top() {
        Some(stack_top) if stack_top & 0xF == 0 => {}
        Some(_) => {
            crate::memory::serial_write("[PROCESS-KSTACK-TEST] unaligned stack top\r\n");
            return false;
        }
        None => {
            crate::memory::serial_write("[PROCESS-KSTACK-TEST] missing kernel stack\r\n");
            return false;
        }
    }

    crate::memory::serial_write("[PROCESS-KSTACK-TEST] stack ready\r\n");

    let ok = unsafe {
        if process.install_syscall_stack().is_err() {
            crate::memory::serial_write("[PROCESS-KSTACK-TEST] install failed\r\n");
            return false;
        }

        let ok = crate::syscall::run_userspace_syscall_smoke();
        Process::reset_syscall_stack_policy();
        ok
    };

    if !ok {
        crate::memory::serial_write("[PROCESS-KSTACK-TEST] syscall smoke failed\r\n");
        return false;
    }

    crate::memory::serial_write("[PROCESS-KSTACK-TEST] OK\r\n");
    true
}
