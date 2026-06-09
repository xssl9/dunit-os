pub mod scheduler;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering};

use crate::memory::vmm::{ActiveAddressSpace, AddressSpace, PageFlags, VirtualAddress};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProcessId(pub u64);

pub type ProcessFd = u32;

pub const FIRST_PROCESS_FD: ProcessFd = 3;
pub const MAX_PROCESS_FD: ProcessFd = 1024;
pub const DEFAULT_KERNEL_STACK_SIZE: usize = 0x40000;

static NEXT_PID: AtomicU64 = AtomicU64::new(1);
static mut CURRENT_PROCESS: Option<Process> = None;
static PROCESS_EXIT_REQUESTED: AtomicBool = AtomicBool::new(false);
static PROCESS_EXIT_CODE: AtomicI32 = AtomicI32::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessState {
    Ready,
    Running,
    Blocked,
    Dead,
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

pub struct Process {
    pub pid: ProcessId,
    pub state: ProcessState,
    pub context: CpuContext,
    pub is_kernel: bool,
    pub cwd: String,
    pub exit_code: Option<i32>,
    address_space: Option<AddressSpace>,
    kernel_stack: Option<Vec<u8>>,
    pub kernel_stack_top: usize,
    fd_table: BTreeMap<ProcessFd, FdEntry>,
    next_fd: ProcessFd,
}

impl Process {
    pub fn new(pid: ProcessId) -> Self {
        match Self::new_user(pid) {
            Ok(process) => process,
            Err(_) => Self::new_without_address_space(pid, false),
        }
    }

    fn new_without_address_space(pid: ProcessId, is_kernel: bool) -> Self {
        let mut process = Self {
            pid,
            state: ProcessState::Ready,
            context: CpuContext::new(),
            is_kernel,
            cwd: String::from("/"),
            exit_code: None,
            address_space: None,
            kernel_stack: None,
            kernel_stack_top: 0,
            fd_table: BTreeMap::new(),
            next_fd: FIRST_PROCESS_FD,
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
            is_kernel: false,
            cwd: String::from("/"),
            exit_code: None,
            address_space: Some(AddressSpace::new().map_err(|_| ProcessError::AddressSpaceCreateFailed)?),
            kernel_stack: Some(kernel_stack),
            kernel_stack_top,
            fd_table: BTreeMap::new(),
            next_fd: FIRST_PROCESS_FD,
        };
        process.reserve_stdio();
        Ok(process)
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

    pub fn terminate(&mut self) {
        self.state = ProcessState::Dead;
    }

    pub fn exit(&mut self, code: i32) {
        self.exit_code = Some(code);
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

    pub fn fd_count(&self) -> usize {
        self.fd_table.len()
    }

    fn reserve_stdio(&mut self) {
        self.fd_table.insert(0, FdEntry::stdin());
        self.fd_table.insert(1, FdEntry::stdout());
        self.fd_table.insert(2, FdEntry::stderr());
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessError {
    NoCurrentProcess,
    InvalidFd,
    FdTableFull,
    NoAddressSpace,
    AddressSpaceCreateFailed,
    NoKernelStack,
    InvalidUserContext,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessExit {
    pub pid: ProcessId,
    pub exit_code: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FdTarget {
    Stdin,
    Stdout,
    Stderr,
    Vfs(crate::fs::vfs::FileDescriptor),
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

pub fn init_current_kernel_process() {
    unsafe {
        if CURRENT_PROCESS.is_none() {
            let pid = allocate_pid();
            let mut process = Process::new_kernel(pid);
            process.state = ProcessState::Running;
            CURRENT_PROCESS = Some(process);
        }
    }
}

pub fn current_process() -> Option<&'static Process> {
    unsafe { CURRENT_PROCESS.as_ref() }
}

pub fn current_process_mut() -> Option<&'static mut Process> {
    unsafe { CURRENT_PROCESS.as_mut() }
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
    PROCESS_EXIT_REQUESTED.store(true, Ordering::SeqCst);
    Some(process.pid)
}

fn take_process_exit_request() -> Option<i32> {
    if PROCESS_EXIT_REQUESTED.swap(false, Ordering::SeqCst) {
        Some(PROCESS_EXIT_CODE.load(Ordering::SeqCst))
    } else {
        None
    }
}

pub fn enter_user_process(mut process: Process) -> Result<ProcessExit, ProcessError> {
    if process.is_kernel {
        return Err(ProcessError::InvalidUserContext);
    }
    if process.context.rip == 0 || process.context.rsp == 0 {
        return Err(ProcessError::InvalidUserContext);
    }
    if process.address_space().is_none() {
        return Err(ProcessError::NoAddressSpace);
    }
    if process.kernel_stack_top().is_none() {
        return Err(ProcessError::NoKernelStack);
    }

    process.state = ProcessState::Running;
    let pid = process.pid;
    let entry = process.context.rip;
    let user_stack = process.context.rsp;

    crate::memory::serial_write("[PROCESS-RUN] starting pid=");
    serial_write_u64(pid.0);
    crate::memory::serial_write("\r\n");

    let previous = unsafe { CURRENT_PROCESS.take() };
    unsafe {
        CURRENT_PROCESS = Some(process);
    }

    let run_result = unsafe {
        match CURRENT_PROCESS.as_ref() {
            Some(current) => {
                match current.install_syscall_stack() {
                    Ok(()) => {
                        match current.activate_address_space() {
                            Ok(active) => {
                                crate::memory::serial_write("[PROCESS-RUN] entered user mode\r\n");
                                crate::hal::run_user_syscall_smoke(entry, user_stack);
                                drop(active);
                                Ok(())
                            }
                            Err(error) => Err(error),
                        }
                    }
                    Err(error) => Err(error),
                }
            }
            None => Err(ProcessError::NoCurrentProcess),
        }
    };

    unsafe {
        Process::reset_syscall_stack_policy();
    }

    let mut finished = unsafe { CURRENT_PROCESS.take() }.ok_or(ProcessError::NoCurrentProcess)?;
    if let Some(code) = take_process_exit_request() {
        finished.exit(code);
    } else if finished.state != ProcessState::Dead {
        finished.exit(-1);
    }
    let exit_code = finished.exit_code.unwrap_or(-1);
    let closed = finished.cleanup_fds();
    if closed > 0 {
        crate::memory::serial_write("[PROCESS-RUN] cleaned fds=");
        serial_write_usize(closed);
        crate::memory::serial_write("\r\n");
    }

    unsafe {
        CURRENT_PROCESS = previous;
    }

    run_result.map(|_| ProcessExit { pid, exit_code })
}

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
