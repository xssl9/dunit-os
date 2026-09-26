#![no_std]

extern crate alloc;

use core::alloc::{GlobalAlloc, Layout};
use core::ptr::{self, null_mut};
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

use alloc::string::String;
use alloc::vec::Vec;

pub const SYSCALL_EXIT: usize = 0;
pub const SYSCALL_READ: usize = 3;
pub const SYSCALL_WRITE: usize = 4;
pub const SYSCALL_OPEN: usize = 5;
pub const SYSCALL_CLOSE: usize = 6;
pub const SYSCALL_MMAP: usize = 7;
pub const SYSCALL_SEND_MESSAGE: usize = 8;
pub const SYSCALL_RECEIVE_MESSAGE: usize = 9;
pub const SYSCALL_GET_FRAMEBUFFER: usize = 10;
pub const SYSCALL_DRAW_PIXEL: usize = 11;
pub const SYSCALL_DRAW_RECT: usize = 12;
pub const SYSCALL_GET_KEY: usize = 13;
pub const SYSCALL_GET_MOUSE_POS: usize = 14;
pub const SYSCALL_SPAWN_PROCESS: usize = 15;
pub const SYSCALL_WAIT_PROCESS: usize = 16;
pub const SYSCALL_GET_PID: usize = 17;
pub const SYSCALL_KILL_PROCESS: usize = 18;
pub const SYSCALL_SLEEP: usize = 19;
pub const SYSCALL_WAIT_EVENT: usize = 33;
pub const SYSCALL_DEBUG_LOG: usize = 20;
pub const SYSCALL_GETCWD: usize = 22;
pub const SYSCALL_CHDIR: usize = 23;
pub const SYSCALL_YIELD: usize = 24;
pub const SYSCALL_GET_TERMINAL_CURSOR: usize = 25;
pub const SYSCALL_GET_SYSTEM_STATS: usize = 26;
pub const SYSCALL_READDIR: usize = 27;
pub const SYSCALL_STAT: usize = 28;
pub const SYSCALL_THREAD_CREATE: usize = 29;
pub const SYSCALL_THREAD_JOIN: usize = 30;
pub const SYSCALL_THREAD_EXIT: usize = 31;
pub const SYSCALL_GET_TID: usize = 32;
pub const SYSCALL_MUNMAP: usize = 34;
pub const SYSCALL_MPROTECT: usize = 35;
pub const SYSCALL_SHARED_VM_CREATE: usize = 36;
pub const SYSCALL_SHARED_VM_MAP: usize = 37;
pub const SYSCALL_SHARED_VM_CLOSE: usize = 38;
pub const SYSCALL_SET_THREAD_POINTER: usize = 39;
pub const SYSCALL_GET_THREAD_POINTER: usize = 40;
pub const SYSCALL_FUTEX_WAIT: usize = 41;
pub const SYSCALL_FUTEX_WAKE: usize = 42;
pub const SYSCALL_HANDLE_CREATE_MEMORY: usize = 43;
pub const SYSCALL_HANDLE_CREATE_ENDPOINT: usize = 44;
pub const SYSCALL_HANDLE_READ: usize = 45;
pub const SYSCALL_HANDLE_WRITE: usize = 46;
pub const SYSCALL_HANDLE_MAP: usize = 47;
pub const SYSCALL_HANDLE_DUP: usize = 48;
pub const SYSCALL_HANDLE_RIGHTS: usize = 49;
pub const SYSCALL_HANDLE_CLOSE: usize = 50;
pub const SYSCALL_HANDLE_SIGNAL: usize = 51;
pub const SYSCALL_HANDLE_TAKE_SIGNALS: usize = 52;
pub const SYSCALL_HANDLE_DISPLAY_ACQUIRE: usize = 53;
pub const SYSCALL_HANDLE_TRANSFER: usize = 54;
pub const SYSCALL_HANDLE_INPUT_ACQUIRE: usize = 55;
pub const SYSCALL_HANDLE_CREATE_SHARED: usize = 56;
pub const SYSCALL_FB_PRESENT: usize = 57;
pub const SYSCALL_HANDLE_SHARED_LEN: usize = 58;
pub const SYSCALL_RECEIVE_MESSAGE_FROM: usize = 59;
pub const SYSCALL_GET_MOUSE_STATE: usize = 60;
pub const SYSCALL_PTY_CREATE: usize = 61;
pub const SYSCALL_PTY_SPAWN: usize = 62;
pub const SYSCALL_PTY_READ: usize = 63;
pub const SYSCALL_PTY_WRITE: usize = 64;
pub const SYSCALL_PTY_CLOSE: usize = 65;
pub const SYSCALL_GET_CHAR: usize = 66;
pub const SYSCALL_GET_KEY_EVENT: usize = 67;

/// Права хэндлов (capabilities). Совпадают с битами в ядре (kernel/src/handle.rs).
pub const RIGHT_READ: u32 = 1 << 0;
pub const RIGHT_WRITE: u32 = 1 << 1;
pub const RIGHT_MAP: u32 = 1 << 2;
pub const RIGHT_SIGNAL: u32 = 1 << 3;
pub const RIGHT_TRANSFER: u32 = 1 << 4;
pub const RIGHT_DISPLAY_MASTER: u32 = 1 << 5;
pub const RIGHT_INPUT_MASTER: u32 = 1 << 6;

pub const VM_PROT_READ: usize = 1;
pub const VM_PROT_WRITE: usize = 2;
pub const VM_PROT_EXEC: usize = 4;
pub const VM_MAP_PRIVATE: usize = 1 << 1;
pub const VM_MAP_ANONYMOUS: usize = 1 << 5;
/// Add one inaccessible page before and after the returned usable range.
pub const VM_MAP_GUARD: usize = 1 << 6;

pub const EPERM: isize = -1;
pub const EAGAIN: isize = -11;
pub const ENOMEM: isize = -12;
pub const EBUSY: isize = -16;
pub const EINTR: isize = -4;
pub const EIO: isize = -5;
pub const EBADF: isize = -9;
pub const ECHILD: isize = -10;
pub const EACCES: isize = -13;
pub const EFAULT: isize = -14;
pub const EEXIST: isize = -17;
pub const ENOTDIR: isize = -20;
pub const EISDIR: isize = -21;
pub const EINVAL: isize = -22;
pub const ENFILE: isize = -23;
pub const ENAMETOOLONG: isize = -36;
pub const ENOSYS: isize = -38;
pub const EMSGSIZE: isize = -90;
pub const EOPNOTSUPP: isize = -95;
pub const ENOBUFS: isize = -105;
pub const EPIPE: isize = -32;

const PAGE_SIZE: usize = 4096;
const HEAP_GROW_CHUNK: usize = 64 * 1024;
const PROT_READ: usize = 1 << 0;
const PROT_WRITE: usize = 1 << 1;
const MAP_PRIVATE: usize = 1 << 1;
const MAP_ANONYMOUS: usize = 1 << 5;

#[repr(C)]
struct FreeBlock {
    size: usize,
    next: *mut FreeBlock,
}

#[repr(C)]
struct AllocationHeader {
    block_start: usize,
    block_size: usize,
}

struct RuntimeAllocator {
    free_list: AtomicUsize,
    locked: AtomicBool,
}

impl RuntimeAllocator {
    const fn new() -> Self {
        Self {
            free_list: AtomicUsize::new(0),
            locked: AtomicBool::new(false),
        }
    }

    fn lock(&self) {
        while self
            .locked
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            core::hint::spin_loop();
        }
    }

    fn unlock(&self) {
        self.locked.store(false, Ordering::Release);
    }

    unsafe fn allocate_from_free_list(&self, layout: Layout) -> *mut u8 {
        let payload_size = layout.size().max(1);
        let header_size = core::mem::size_of::<AllocationHeader>();
        let user_align = layout
            .align()
            .max(core::mem::align_of::<AllocationHeader>());
        let minimum_free = core::mem::size_of::<FreeBlock>();
        let mut current_ptr = self.free_list.load(Ordering::Relaxed) as *mut FreeBlock;
        let mut previous: *mut FreeBlock = ptr::null_mut();

        while !current_ptr.is_null() {
            let block_start = current_ptr as usize;
            let block_end = block_start.saturating_add((*current_ptr).size);
            let user_addr = align_up(block_start + header_size, user_align);
            let requested_end = align_up(
                user_addr.saturating_add(payload_size),
                core::mem::align_of::<FreeBlock>(),
            );

            if requested_end <= block_end {
                let remaining = block_end - requested_end;
                let (allocated_end, replacement) = if remaining >= minimum_free {
                    let next = requested_end as *mut FreeBlock;
                    (*next).size = remaining;
                    (*next).next = (*current_ptr).next;
                    (requested_end, next)
                } else {
                    (block_end, (*current_ptr).next)
                };

                if previous.is_null() {
                    self.free_list
                        .store(replacement as usize, Ordering::Relaxed);
                } else {
                    (*previous).next = replacement;
                }

                let header = (user_addr - header_size) as *mut AllocationHeader;
                (*header).block_start = block_start;
                (*header).block_size = allocated_end - block_start;
                return user_addr as *mut u8;
            }

            previous = current_ptr;
            current_ptr = (*current_ptr).next;
        }

        null_mut()
    }

    unsafe fn add_free_region(&self, start: usize, size: usize) {
        let block = start as *mut FreeBlock;
        (*block).size = size;

        let mut current = self.free_list.load(Ordering::Relaxed) as *mut FreeBlock;
        let mut previous: *mut FreeBlock = ptr::null_mut();
        while !current.is_null() && (current as usize) < start {
            previous = current;
            current = (*current).next;
        }

        (*block).next = current;
        if previous.is_null() {
            self.free_list.store(block as usize, Ordering::Relaxed);
        } else {
            (*previous).next = block;
        }
        coalesce_free_neighbors(self, previous, block);
    }

    unsafe fn grow(&self, minimum: usize) -> bool {
        let requested = align_up(minimum.max(HEAP_GROW_CHUNK), PAGE_SIZE);
        let mapped = syscall5(
            SYSCALL_MMAP,
            0,
            requested,
            PROT_READ | PROT_WRITE,
            MAP_PRIVATE | MAP_ANONYMOUS,
            0,
        );
        if mapped < 0 {
            return false;
        }
        self.add_free_region(mapped as usize, requested);
        true
    }
}

unsafe fn coalesce_free_neighbors(
    allocator: &RuntimeAllocator,
    previous: *mut FreeBlock,
    mut block: *mut FreeBlock,
) {
    if !previous.is_null() && previous as usize + (*previous).size == block as usize {
        (*previous).size += (*block).size;
        (*previous).next = (*block).next;
        block = previous;
    }

    let next = (*block).next;
    if !next.is_null() && block as usize + (*block).size == next as usize {
        (*block).size += (*next).size;
        (*block).next = (*next).next;
    }

    if previous.is_null() {
        allocator.free_list.store(block as usize, Ordering::Relaxed);
    }
}

unsafe impl GlobalAlloc for RuntimeAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        self.lock();
        let mut result = self.allocate_from_free_list(layout);
        if result.is_null() {
            let required = layout
                .size()
                .saturating_add(layout.align())
                .saturating_add(core::mem::size_of::<AllocationHeader>());
            if self.grow(required) {
                result = self.allocate_from_free_list(layout);
            }
        }
        self.unlock();
        result
    }

    unsafe fn dealloc(&self, pointer: *mut u8, _layout: Layout) {
        if pointer.is_null() {
            return;
        }
        self.lock();
        let header = (pointer as usize - core::mem::size_of::<AllocationHeader>())
            as *const AllocationHeader;
        self.add_free_region((*header).block_start, (*header).block_size);
        self.unlock();
    }
}

#[global_allocator]
static ALLOCATOR: RuntimeAllocator = RuntimeAllocator::new();

fn align_up(value: usize, align: usize) -> usize {
    value.saturating_add(align - 1) & !(align - 1)
}

static mut RUNTIME_ARGC: usize = 0;
static mut RUNTIME_ARGV: RawArgv = core::ptr::null();
static mut RUNTIME_ENVP: RawEnvp = core::ptr::null();

pub const GUI_SHELL_PID: u32 = 1;
pub const GUI_MSG_MAGIC: u32 = 0x3149_5547; // GUI1
pub const GUI_MSG_VERSION: u16 = 1;
pub const GUI_MSG_CREATE_WINDOW: u16 = 1;
pub const GUI_MSG_DRAW_TEXT: u16 = 2;
pub const GUI_MSG_SET_STATUS: u16 = 3;
pub const GUI_MSG_EXIT: u16 = 4;
pub const GUI_MSG_COMMAND: u16 = 5;
pub const GUI_MSG_CLEAR: u16 = 6;
pub const GUI_MSG_SET_TITLE: u16 = 7;
pub const GUI_MSG_DRAW_RECT: u16 = 8;
pub const GUI_MSG_KEY_EVENT: u16 = 101;
pub const GUI_MSG_CLOSE_EVENT: u16 = 102;
pub const GUI_MSG_POINTER_EVENT: u16 = 103;
pub const GUI_MSG_DATA_CAP: usize = 160;

pub const OPEN_READ: usize = 1 << 0;
pub const OPEN_WRITE: usize = 1 << 1;
pub const OPEN_CREATE: usize = 1 << 2;
pub const OPEN_TRUNC: usize = 1 << 3;
pub const OPEN_APPEND: usize = 1 << 4;
pub const OPEN_READ_WRITE: usize = OPEN_READ | OPEN_WRITE;

pub const FILE_TYPE_FILE: u32 = 1;
pub const FILE_TYPE_DIRECTORY: u32 = 2;
pub const FILE_TYPE_DEVICE: u32 = 3;

pub type RawArgv = *const *const u8;
pub type RawEnvp = *const *const u8;

pub fn init_runtime(argc: usize, argv: RawArgv, envp: RawEnvp) {
    unsafe {
        RUNTIME_ARGC = argc;
        RUNTIME_ARGV = argv;
        RUNTIME_ENVP = envp;
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct GuiMessage {
    pub magic: u32,
    pub version: u16,
    pub kind: u16,
    pub window_id: u32,
    pub a: i32,
    pub b: i32,
    pub c: u32,
    pub len: u32,
    pub data: [u8; GUI_MSG_DATA_CAP],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct SystemStats {
    pub process_total: u64,
    pub process_prepared: u64,
    pub process_ready: u64,
    pub process_running: u64,
    pub process_blocked: u64,
    pub process_dead: u64,
    pub process_reaped: u64,
    pub pmm_total_bytes: u64,
    pub pmm_free_bytes: u64,
    pub pmm_used_bytes: u64,
    pub heap_total_bytes: u64,
    pub heap_free_bytes: u64,
    pub heap_used_bytes: u64,
    pub heap_free_blocks: u64,
    pub ipc_queue_count: u64,
    pub ipc_queued_messages: u64,
    pub ipc_shared_regions: u64,
    pub ipc_max_queue_messages: u64,
    pub fs_files: u64,
    pub fs_directories: u64,
    pub fs_bytes: u64,
    pub fs_open_handles: u64,
    pub uptime_ticks: u64,
    pub uptime_available: u64,
    pub net_total_nics: u64,
    pub net_supported_nics: u64,
    pub net_mmio_ready_nics: u64,
    pub net_mac_ready_nics: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct DirEntry {
    pub name: [u8; 64],
    pub name_len: usize,
    pub file_type: u32,
}

impl DirEntry {
    pub const fn empty() -> Self {
        Self {
            name: [0; 64],
            name_len: 0,
            file_type: FILE_TYPE_FILE,
        }
    }

    pub fn name(&self) -> &str {
        core::str::from_utf8(&self.name[..self.name_len.min(self.name.len())])
            .unwrap_or("<invalid>")
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct FileStat {
    pub file_type: u32,
    pub size: usize,
}

impl GuiMessage {
    pub const fn new(kind: u16) -> Self {
        Self {
            magic: GUI_MSG_MAGIC,
            version: GUI_MSG_VERSION,
            kind,
            window_id: 0,
            a: 0,
            b: 0,
            c: 0,
            len: 0,
            data: [0; GUI_MSG_DATA_CAP],
        }
    }

    pub fn set_data(&mut self, data: &[u8]) {
        let len = if data.len() > GUI_MSG_DATA_CAP {
            GUI_MSG_DATA_CAP
        } else {
            data.len()
        };
        let mut index = 0usize;
        while index < len {
            self.data[index] = data[index];
            index += 1;
        }
        self.len = len as u32;
    }

    pub fn data(&self) -> &[u8] {
        let len = (self.len as usize).min(GUI_MSG_DATA_CAP);
        &self.data[..len]
    }

    pub fn valid(&self) -> bool {
        self.magic == GUI_MSG_MAGIC
            && self.version == GUI_MSG_VERSION
            && (self.len as usize) <= GUI_MSG_DATA_CAP
    }
}

#[repr(C)]
pub struct FbInfo {
    pub addr: u64,
    pub width: u32,
    pub height: u32,
    pub pitch: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct TerminalCursorInfo {
    pub x: u32,
    pub y: u32,
    pub char_width: u32,
    pub char_height: u32,
}

#[inline(always)]
pub fn syscall0(num: usize) -> isize {
    let ret: isize;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") num,
            lateout("rax") ret,
            lateout("rdi") _,
            lateout("rsi") _,
            lateout("rdx") _,
            lateout("r8") _,
            lateout("r9") _,
            lateout("r10") _,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack)
        );
    }
    ret
}

#[inline(always)]
pub fn syscall1(num: usize, a1: usize) -> isize {
    let ret: isize;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") num,
            inlateout("rdi") a1 => _,
            lateout("rax") ret,
            lateout("rsi") _,
            lateout("rdx") _,
            lateout("r8") _,
            lateout("r9") _,
            lateout("r10") _,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack)
        );
    }
    ret
}

#[inline(always)]
pub fn syscall2(num: usize, a1: usize, a2: usize) -> isize {
    let ret: isize;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") num,
            inlateout("rdi") a1 => _,
            inlateout("rsi") a2 => _,
            lateout("rax") ret,
            lateout("rdx") _,
            lateout("r8") _,
            lateout("r9") _,
            lateout("r10") _,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack)
        );
    }
    ret
}

#[inline(always)]
pub fn syscall3(num: usize, a1: usize, a2: usize, a3: usize) -> isize {
    let ret: isize;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") num,
            inlateout("rdi") a1 => _,
            inlateout("rsi") a2 => _,
            inlateout("rdx") a3 => _,
            lateout("rax") ret,
            lateout("r8") _,
            lateout("r9") _,
            lateout("r10") _,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack)
        );
    }
    ret
}

#[inline(always)]
pub fn syscall5(num: usize, a1: usize, a2: usize, a3: usize, a4: usize, a5: usize) -> isize {
    let ret: isize;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") num,
            inlateout("rdi") a1 => _,
            inlateout("rsi") a2 => _,
            inlateout("rdx") a3 => _,
            inlateout("r10") a4 => _,
            inlateout("r8") a5 => _,
            lateout("rax") ret,
            lateout("r9") _,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack)
        );
    }
    ret
}

#[inline(always)]
pub fn syscall6(
    num: usize,
    a1: usize,
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
) -> isize {
    let ret: isize;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") num,
            inlateout("rdi") a1 => _,
            inlateout("rsi") a2 => _,
            inlateout("rdx") a3 => _,
            inlateout("r10") a4 => _,
            inlateout("r8") a5 => _,
            inlateout("r9") a6 => _,
            lateout("rax") ret,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack)
        );
    }
    ret
}

pub fn exit(code: i32) -> ! {
    syscall1(SYSCALL_EXIT, code as usize);
    loop {}
}

pub fn write(fd: usize, buf: &[u8]) -> isize {
    syscall3(SYSCALL_WRITE, fd, buf.as_ptr() as usize, buf.len())
}

pub fn write_stdout(s: &str) -> isize {
    write(1, s.as_bytes())
}

pub fn write_stderr(s: &str) -> isize {
    write(2, s.as_bytes())
}

pub fn open(path: &str, flags: usize) -> isize {
    syscall3(SYSCALL_OPEN, path.as_ptr() as usize, path.len(), flags)
}

pub fn read(fd: usize, buf: &mut [u8]) -> isize {
    syscall3(SYSCALL_READ, fd, buf.as_mut_ptr() as usize, buf.len())
}

pub fn close(fd: usize) -> isize {
    syscall1(SYSCALL_CLOSE, fd)
}

pub fn readdir(path: &str, entries: &mut [DirEntry]) -> isize {
    syscall5(
        SYSCALL_READDIR,
        path.as_ptr() as usize,
        path.len(),
        entries.as_mut_ptr() as usize,
        entries.len(),
        0,
    )
}

pub fn stat(path: &str, stat: &mut FileStat) -> isize {
    syscall3(
        SYSCALL_STAT,
        path.as_ptr() as usize,
        path.len(),
        stat as *mut FileStat as usize,
    )
}

pub fn print(s: &str) {
    write_stdout(s);
}

pub fn println(s: &str) {
    write_stdout(s);
    write_stdout("\n");
}

pub fn error_name(code: isize) -> &'static str {
    match code {
        EAGAIN => "EAGAIN",
        ENOMEM => "ENOMEM",
        EINTR => "EINTR",
        EIO => "EIO",
        EBADF => "EBADF",
        ECHILD => "ECHILD",
        EACCES => "EACCES",
        EFAULT => "EFAULT",
        EEXIST => "EEXIST",
        ENOTDIR => "ENOTDIR",
        EISDIR => "EISDIR",
        EINVAL => "EINVAL",
        ENFILE => "ENFILE",
        ENAMETOOLONG => "ENAMETOOLONG",
        ENOSYS => "ENOSYS",
        EMSGSIZE => "EMSGSIZE",
        EOPNOTSUPP => "EOPNOTSUPP",
        ENOBUFS => "ENOBUFS",
        _ => "ERR",
    }
}

pub fn read_line() -> Result<String, isize> {
    let mut bytes = Vec::new();
    let mut buf = [0u8; 64];
    loop {
        let read = read(0, &mut buf);
        if read == EAGAIN {
            let yielded = yield_now();
            if yielded < 0 && yielded != EAGAIN {
                return Err(yielded);
            }
            continue;
        }
        if read < 0 {
            return Err(read);
        }
        if read == 0 {
            return String::from_utf8(bytes).map_err(|_| EINVAL);
        }
        for byte in &buf[..read as usize] {
            if *byte == b'\n' {
                return String::from_utf8(bytes).map_err(|_| EINVAL);
            }
            bytes.push(*byte);
        }
    }
}

pub fn read_to_string(path: &str) -> Result<String, isize> {
    let fd = open(path, OPEN_READ);
    if fd < 0 {
        return Err(fd);
    }

    let mut bytes = Vec::new();
    let mut buf = [0u8; 128];
    loop {
        let read = read(fd as usize, &mut buf);
        if read < 0 {
            close(fd as usize);
            return Err(read);
        }
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&buf[..read as usize]);
    }

    let closed = close(fd as usize);
    if closed < 0 {
        return Err(closed);
    }
    String::from_utf8(bytes).map_err(|_| EINVAL)
}

fn write_string_with_flags(path: &str, data: &str, flags: usize) -> Result<(), isize> {
    let fd = open(path, flags);
    if fd < 0 {
        return Err(fd);
    }
    let written = write(fd as usize, data.as_bytes());
    if written != data.len() as isize {
        close(fd as usize);
        return Err(if written < 0 { written } else { EIO });
    }
    let closed = close(fd as usize);
    if closed < 0 {
        return Err(closed);
    }
    Ok(())
}

pub fn write_string(path: &str, data: &str) -> Result<(), isize> {
    write_string_with_flags(path, data, OPEN_CREATE | OPEN_WRITE | OPEN_TRUNC)
}

pub fn append_string(path: &str, data: &str) -> Result<(), isize> {
    write_string_with_flags(path, data, OPEN_CREATE | OPEN_WRITE | OPEN_APPEND)
}

pub fn file_exists(path: &str) -> bool {
    let fd = open(path, OPEN_READ);
    if fd < 0 {
        false
    } else {
        close(fd as usize);
        true
    }
}

pub fn argc() -> usize {
    unsafe { RUNTIME_ARGC }
}

pub fn arg(index: usize) -> Option<&'static str> {
    unsafe { argv_get(RUNTIME_ARGC, RUNTIME_ARGV, index) }
}

pub fn getenv(name: &str) -> Option<&'static str> {
    let envp = unsafe { RUNTIME_ENVP };
    let mut index = 0usize;
    loop {
        let entry = unsafe { env_get(envp, index) }?;
        if let Some(eq_index) = find_byte(entry.as_bytes(), b'=') {
            if &entry.as_bytes()[..eq_index] == name.as_bytes() {
                return core::str::from_utf8(&entry.as_bytes()[eq_index + 1..]).ok();
            }
        }
        index += 1;
    }
}

fn find_byte(bytes: &[u8], needle: u8) -> Option<usize> {
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] == needle {
            return Some(index);
        }
        index += 1;
    }
    None
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct WaitStatus {
    pub kind: i32,
    pub code: i32,
}

pub const WAIT_KIND_EMPTY: i32 = -1;
pub const WAIT_KIND_SPAWN_PREPARED: i32 = -2;
pub const WAIT_KIND_EXITED: i32 = 0;

impl WaitStatus {
    pub const fn empty() -> Self {
        Self {
            kind: WAIT_KIND_EMPTY,
            code: 0,
        }
    }

    pub const fn spawn_prepared(&self) -> bool {
        self.kind == WAIT_KIND_SPAWN_PREPARED
    }

    pub const fn exited(&self) -> bool {
        self.kind == WAIT_KIND_EXITED
    }

    pub const fn faulted(&self) -> bool {
        self.kind > 0
    }
}

pub fn print_usize(mut value: usize) {
    let mut buf = [0u8; 20];
    let mut index = buf.len();

    if value == 0 {
        write_stdout("0");
        return;
    }

    while value > 0 {
        index -= 1;
        buf[index] = b'0' + (value % 10) as u8;
        value /= 10;
    }

    let s = unsafe { core::str::from_utf8_unchecked(&buf[index..]) };
    write_stdout(s);
}

pub unsafe fn argv_get<'a>(argc: usize, argv: RawArgv, index: usize) -> Option<&'a str> {
    if argv.is_null() || index >= argc {
        return None;
    }

    cstr_at(*argv.add(index))
}

pub unsafe fn env_get<'a>(envp: RawEnvp, index: usize) -> Option<&'a str> {
    if envp.is_null() {
        return None;
    }

    let ptr = *envp.add(index);
    if ptr.is_null() {
        return None;
    }

    cstr_at(ptr)
}

unsafe fn cstr_at<'a>(ptr: *const u8) -> Option<&'a str> {
    if ptr.is_null() {
        return None;
    }

    let mut len = 0;
    while *ptr.add(len) != 0 {
        len += 1;
        if len > 4096 {
            return None;
        }
    }

    core::str::from_utf8(core::slice::from_raw_parts(ptr, len)).ok()
}

pub fn get_framebuffer(info: &mut FbInfo) -> bool {
    syscall1(SYSCALL_GET_FRAMEBUFFER, info as *mut FbInfo as usize) == 0
}

pub fn get_terminal_cursor(info: &mut TerminalCursorInfo) -> bool {
    syscall1(
        SYSCALL_GET_TERMINAL_CURSOR,
        info as *mut TerminalCursorInfo as usize,
    ) == 0
}

pub fn draw_pixel(x: u32, y: u32, color: u32) {
    syscall3(SYSCALL_DRAW_PIXEL, x as usize, y as usize, color as usize);
}

pub fn draw_rect(x: u32, y: u32, w: u32, h: u32, color: u32) {
    syscall5(
        SYSCALL_DRAW_RECT,
        x as usize,
        y as usize,
        w as usize,
        h as usize,
        color as usize,
    );
}

pub fn get_key() -> Option<u8> {
    let k = syscall0(SYSCALL_GET_KEY);
    if k < 0 {
        None
    } else {
        Some(k as u8)
    }
}

/// Next typed character (ASCII), with the kernel's scancode keymap applied and
/// key-release/modifier scancodes skipped. `None` once the key ring is drained.
/// The GUI compositor forwards these to the focused window.
pub fn get_char() -> Option<u8> {
    let c = syscall0(SYSCALL_GET_CHAR);
    if c < 0 {
        None
    } else {
        Some(c as u8)
    }
}

/// Биты маски модификаторов в `KeyEvent::mods` (совпадают с `modifier_mask()` в
/// ядре: kernel/src/drivers/keyboard.rs).
pub const KEYMOD_SHIFT: u8 = 1 << 0;
pub const KEYMOD_CTRL: u8 = 1 << 1;
pub const KEYMOD_ALT: u8 = 1 << 2;
pub const KEYMOD_SUPER: u8 = 1 << 3;

/// Одно событие клавиши от `get_key_event`: сырой переход (нажатие ИЛИ отпускание)
/// вместе с активными модификаторами. В отличие от `get_char`, отдаёт все события
/// (нужно компоситору для горячих клавиш Super+…), но при обычном наборе `ascii`
/// уже содержит готовый байт из клавиатурной раскладки ядра.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct KeyEvent {
    /// Cooked ASCII на нажатии обычной клавиши; 0 для отпускания и расширенных.
    pub ascii: u8,
    /// Битовая маска `KEYMOD_*` на момент события.
    pub mods: u8,
    /// Скан-код без бита отпускания (`sc & 0x7F`).
    pub scancode: u8,
    /// true — make (нажатие), false — break (отпускание).
    pub pressed: bool,
}

impl KeyEvent {
    pub fn shift(&self) -> bool {
        self.mods & KEYMOD_SHIFT != 0
    }
    pub fn ctrl(&self) -> bool {
        self.mods & KEYMOD_CTRL != 0
    }
    pub fn alt(&self) -> bool {
        self.mods & KEYMOD_ALT != 0
    }
    pub fn super_key(&self) -> bool {
        self.mods & KEYMOD_SUPER != 0
    }
}

/// Следующее событие клавиши, либо `None` когда кольцо скан-кодов пусто. Распаковка
/// упакованного i64 из `sys_get_key_event`:
/// `[base:31..24][pressed:16][mods:15..8][ascii:7..0]`.
pub fn get_key_event() -> Option<KeyEvent> {
    let packed = syscall0(SYSCALL_GET_KEY_EVENT);
    if packed < 0 {
        return None;
    }
    let packed = packed as u64;
    Some(KeyEvent {
        ascii: (packed & 0xFF) as u8,
        mods: ((packed >> 8) & 0xFF) as u8,
        scancode: ((packed >> 24) & 0xFF) as u8,
        pressed: ((packed >> 16) & 0xFF) != 0,
    })
}

pub fn get_mouse_pos() -> (u32, u32) {
    let mut x: u32 = 0;
    let mut y: u32 = 0;
    syscall3(
        SYSCALL_GET_MOUSE_POS,
        &mut x as *mut u32 as usize,
        &mut y as *mut u32 as usize,
        0,
    );
    (x, y)
}

/// Full pointer snapshot for a userspace compositor. `buttons` is a bitmask
/// (bit0 left, bit1 right, bit2 middle); `wheel` is the scroll delta since the
/// previous `get_mouse_state()` call (the kernel drains its accumulator each
/// read). Diff successive snapshots to derive motion/button/axis transitions.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct MouseState {
    pub x: u32,
    pub y: u32,
    pub buttons: u32,
    pub wheel: i32,
}

impl MouseState {
    pub fn left(&self) -> bool {
        self.buttons & 0b001 != 0
    }
    pub fn right(&self) -> bool {
        self.buttons & 0b010 != 0
    }
    pub fn middle(&self) -> bool {
        self.buttons & 0b100 != 0
    }
}

pub fn get_mouse_state() -> MouseState {
    let mut rec = [0u8; 16];
    syscall1(SYSCALL_GET_MOUSE_STATE, rec.as_mut_ptr() as usize);
    MouseState {
        x: u32::from_le_bytes([rec[0], rec[1], rec[2], rec[3]]),
        y: u32::from_le_bytes([rec[4], rec[5], rec[6], rec[7]]),
        buttons: u32::from_le_bytes([rec[8], rec[9], rec[10], rec[11]]),
        wheel: i32::from_le_bytes([rec[12], rec[13], rec[14], rec[15]]),
    }
}

pub fn sleep_ms(ms: u64) {
    syscall1(SYSCALL_SLEEP, ms as usize);
}

pub fn vm_map(addr: usize, length: usize, prot: usize, flags: usize) -> isize {
    syscall5(SYSCALL_MMAP, addr, length, prot, flags, 0)
}

pub fn vm_unmap(addr: usize, length: usize) -> isize {
    syscall2(SYSCALL_MUNMAP, addr, length)
}

pub fn vm_protect(addr: usize, length: usize, prot: usize) -> isize {
    syscall3(SYSCALL_MPROTECT, addr, length, prot)
}

pub fn shared_vm_create(length: usize) -> isize {
    syscall1(SYSCALL_SHARED_VM_CREATE, length)
}

pub fn shared_vm_map(id: u64, addr: usize, prot: usize) -> isize {
    syscall3(SYSCALL_SHARED_VM_MAP, id as usize, addr, prot)
}

pub fn shared_vm_close(id: u64) -> isize {
    syscall1(SYSCALL_SHARED_VM_CLOSE, id as usize)
}

pub fn set_thread_pointer(base: usize) -> isize {
    syscall1(SYSCALL_SET_THREAD_POINTER, base)
}

pub fn get_thread_pointer() -> usize {
    syscall0(SYSCALL_GET_THREAD_POINTER) as usize
}

/// Park the calling thread while `*word == expected`, keyed by the word's
/// address in this process. `timeout_ms == 0` blocks with no deadline. Returns
/// 0 on a normal wake, EAGAIN if the value already differed, EFAULT if the word
/// is unmapped, EINVAL if unaligned. `word` must be 4-byte aligned.
pub fn futex_wait(word: &AtomicU32, expected: u32, timeout_ms: u64) -> isize {
    syscall3(
        SYSCALL_FUTEX_WAIT,
        word as *const AtomicU32 as usize,
        expected as usize,
        timeout_ms as usize,
    )
}

/// Wake up to `count` threads parked on `word`. `count == 0` wakes all waiters.
/// Returns the number of threads woken, or a negative errno.
pub fn futex_wake(word: &AtomicU32, count: usize) -> isize {
    syscall2(SYSCALL_FUTEX_WAKE, word as *const AtomicU32 as usize, count)
}

// --- Таблица хэндлов (capabilities с правами) --------------------------------

/// Создаёт объект памяти `len` байт. Возвращает хэндл (>0) или отрицательный
/// errno. Начальные права: READ|WRITE|MAP|TRANSFER.
pub fn handle_create_memory(len: usize) -> isize {
    syscall1(SYSCALL_HANDLE_CREATE_MEMORY, len)
}

/// Создаёт конечную точку для сигналов процессу `target_pid`. Права SIGNAL|TRANSFER.
pub fn handle_create_endpoint(target_pid: u32) -> isize {
    syscall1(SYSCALL_HANDLE_CREATE_ENDPOINT, target_pid as usize)
}

/// Читает из объекта памяти за хэндлом (нужно право READ). Возвращает число
/// прочитанных байт или отрицательный errno.
pub fn handle_read(handle: u32, buf: &mut [u8]) -> isize {
    syscall3(
        SYSCALL_HANDLE_READ,
        handle as usize,
        buf.as_mut_ptr() as usize,
        buf.len(),
    )
}

/// Пишет в объект памяти за хэндлом (нужно право WRITE). Возвращает число
/// записанных байт или отрицательный errno.
pub fn handle_write(handle: u32, buf: &[u8]) -> isize {
    syscall3(
        SYSCALL_HANDLE_WRITE,
        handle as usize,
        buf.as_ptr() as usize,
        buf.len(),
    )
}

/// Отображает объект памяти в адресное пространство (нужно право MAP).
/// `addr == 0` — выбрать адрес автоматически. Возвращает виртуальный адрес.
pub fn handle_map(handle: u32, addr: usize, len: usize) -> isize {
    syscall3(SYSCALL_HANDLE_MAP, handle as usize, addr, len)
}

/// Дублирует хэндл, сужая права до `new_rights` (обязано быть подмножеством
/// текущих прав). Возвращает новый хэндл или отрицательный errno.
pub fn handle_dup(handle: u32, new_rights: u32) -> isize {
    syscall2(SYSCALL_HANDLE_DUP, handle as usize, new_rights as usize)
}

/// Возвращает маску прав хэндла (>=0) или отрицательный errno.
pub fn handle_rights(handle: u32) -> isize {
    syscall1(SYSCALL_HANDLE_RIGHTS, handle as usize)
}

/// Закрывает хэндл. Возвращает 0 или отрицательный errno.
pub fn handle_close(handle: u32) -> isize {
    syscall1(SYSCALL_HANDLE_CLOSE, handle as usize)
}

/// Доставляет сигнал через конечную точку (нужно право SIGNAL). Возвращает 0
/// или отрицательный errno.
pub fn handle_signal(handle: u32, value: u64) -> isize {
    syscall2(SYSCALL_HANDLE_SIGNAL, handle as usize, value as usize)
}

/// Забирает и обнуляет счётчик доставленных сигналов процесса.
pub fn handle_take_signals() -> isize {
    syscall0(SYSCALL_HANDLE_TAKE_SIGNALS)
}

/// Захватывает мастер-право на дисплей (эксклюзивно). Возвращает хэндл с правом
/// DISPLAY_MASTER или отрицательный errno (EBUSY, если дисплей занят).
pub fn handle_display_acquire() -> isize {
    syscall0(SYSCALL_HANDLE_DISPLAY_ACQUIRE)
}

/// Захватывает эксклюзивный мастер-источник ввода (клавиатура/мышь). Возвращает
/// хэндл с правом INPUT_MASTER|TRANSFER или отрицательный errno (EBUSY, если
/// вводом уже владеет другой процесс).
pub fn handle_input_acquire() -> isize {
    syscall0(SYSCALL_HANDLE_INPUT_ACQUIRE)
}

/// Создаёт разделяемый буфер (zero-copy физические фреймы) и возвращает
/// capability-хэндл с правами READ|WRITE|MAP|TRANSFER (или отрицательный errno).
/// Хэндл можно отобразить (`handle_map`) и передать (`handle_transfer`); право
/// WRITE в маске определяет, будет ли отображение доступно на запись.
pub fn handle_create_shared(len: usize) -> isize {
    syscall1(SYSCALL_HANDLE_CREATE_SHARED, len)
}

/// Выводит блок пикселей `src` (BGRA/XRGB little-endian, `width`×`height`, 4
/// байта на пиксель) в системный фреймбуфер с левым верхним углом в (`x`, `y`).
/// Разрешено только владельцу дисплея (compositor); иначе EACCES. Ядро отдаёт
/// лишь механизм блита — политика композитинга живёт в userspace.
pub fn fb_present(src: &[u8], width: u32, height: u32, x: u32, y: u32) -> isize {
    syscall5(
        SYSCALL_FB_PRESENT,
        src.as_ptr() as usize,
        width as usize,
        height as usize,
        x as usize,
        y as usize,
    )
}

/// Передаёт хэндл процессу `target_pid` (нужно право TRANSFER). Возвращает новый
/// хэндл в таблице цели или отрицательный errno.
pub fn handle_transfer(handle: u32, target_pid: u32) -> isize {
    syscall2(SYSCALL_HANDLE_TRANSFER, handle as usize, target_pid as usize)
}

pub fn get_pid() -> u32 {
    syscall0(SYSCALL_GET_PID) as u32
}

/// Start a thread in this process. `stack_top` must be the aligned top of a
/// writable user stack; the caller retains its mapping until after join.
pub fn thread_create(entry: extern "C" fn(usize) -> !, stack_top: *mut u8, arg: usize) -> isize {
    syscall3(SYSCALL_THREAD_CREATE, entry as usize, stack_top as usize, arg)
}

pub fn get_tid() -> u32 {
    syscall0(SYSCALL_GET_TID) as u32
}

pub fn thread_exit(code: i32) -> ! {
    syscall1(SYSCALL_THREAD_EXIT, code as usize);
    loop { core::hint::spin_loop(); }
}

/// Nonblocking join: returns EAGAIN while the thread is still runnable.
pub fn thread_join(tid: u32, status: &mut WaitStatus) -> isize {
    syscall2(SYSCALL_THREAD_JOIN, tid as usize, status as *mut WaitStatus as usize)
}

pub fn kill(pid: u32) -> isize {
    syscall1(SYSCALL_KILL_PROCESS, pid as usize)
}

pub fn spawn(path: &str) -> isize {
    syscall2(SYSCALL_SPAWN_PROCESS, path.as_ptr() as usize, path.len())
}

/// Create a PTY endpoint owned by this process (the terminal emulator/master).
/// Returns the pty id (>= 1) or a negative errno.
pub fn pty_create() -> isize {
    syscall0(SYSCALL_PTY_CREATE)
}

/// Spawn `path` attached as the slave of pty `id`: the child's stdin reads drain
/// what we `pty_write`, and its stdout/stderr feed what we `pty_read`. Returns
/// the child pid or a negative errno.
pub fn pty_spawn(path: &str, id: u32) -> isize {
    syscall3(
        SYSCALL_PTY_SPAWN,
        path.as_ptr() as usize,
        path.len(),
        id as usize,
    )
}

/// Master side: read the slave's stdout into `buf`. Returns bytes read (0 means
/// nothing available yet — yield and retry), or `EPIPE` once the slave has
/// exited and its output is drained.
pub fn pty_read(id: u32, buf: &mut [u8]) -> isize {
    syscall3(
        SYSCALL_PTY_READ,
        id as usize,
        buf.as_mut_ptr() as usize,
        buf.len(),
    )
}

/// Master side: write `buf` to the slave's stdin. Returns bytes accepted.
pub fn pty_write(id: u32, buf: &[u8]) -> isize {
    syscall3(
        SYSCALL_PTY_WRITE,
        id as usize,
        buf.as_ptr() as usize,
        buf.len(),
    )
}

/// Master side: destroy pty `id`; the slave then sees EOF on stdin.
pub fn pty_close(id: u32) -> isize {
    syscall1(SYSCALL_PTY_CLOSE, id as usize)
}

pub fn wait(pid: u32, status: &mut WaitStatus) -> isize {
    syscall2(
        SYSCALL_WAIT_PROCESS,
        pid as usize,
        status as *mut WaitStatus as usize,
    )
}

pub fn ipc_send(pid: u32, data: &[u8]) -> isize {
    syscall3(
        SYSCALL_SEND_MESSAGE,
        pid as usize,
        data.as_ptr() as usize,
        data.len(),
    )
}

pub fn ipc_recv(buf: &mut [u8]) -> isize {
    syscall2(
        SYSCALL_RECEIVE_MESSAGE,
        buf.as_mut_ptr() as usize,
        buf.len(),
    )
}

/// Как `ipc_recv`, но записывает аутентифицированный ядром pid отправителя в
/// `sender`. Маршрутизация по этому значению безопасна против подделки: его
/// проставляет ядро, а не отправитель.
pub fn ipc_recv_from(buf: &mut [u8], sender: &mut u32) -> isize {
    syscall3(
        SYSCALL_RECEIVE_MESSAGE_FROM,
        buf.as_mut_ptr() as usize,
        buf.len(),
        sender as *mut u32 as usize,
    )
}

/// Возвращает истинный размер (в байтах) backing-объекта разделяемого буфера по
/// хэндлу, либо отрицательный errno. Позволяет получателю проверять геометрию
/// присланного буфера против реально выделенных фреймов.
pub fn handle_shared_len(handle: u32) -> isize {
    syscall1(SYSCALL_HANDLE_SHARED_LEN, handle as usize)
}

pub fn wait_ipc_event(timeout_ms: u64) -> isize {
    syscall1(SYSCALL_WAIT_EVENT, timeout_ms as usize)
}

pub fn ipc_recv_blocking(buf: &mut [u8], timeout_ms: u64) -> isize {
    loop {
        let received = ipc_recv(buf);
        if received != EAGAIN { return received; }
        let waited = wait_ipc_event(timeout_ms);
        if waited < 0 { return waited; }
        if timeout_ms != 0 { return ipc_recv(buf); }
    }
}

/// Блокирующий `ipc_recv_from`: как `ipc_recv_blocking`, но также отдаёт
/// аутентифицированный pid отправителя.
pub fn ipc_recv_blocking_from(buf: &mut [u8], sender: &mut u32, timeout_ms: u64) -> isize {
    loop {
        let received = ipc_recv_from(buf, sender);
        if received != EAGAIN { return received; }
        let waited = wait_ipc_event(timeout_ms);
        if waited < 0 { return waited; }
        if timeout_ms != 0 { return ipc_recv_from(buf, sender); }
    }
}

pub fn gui_send(message: &GuiMessage) -> isize {
    let bytes = unsafe {
        core::slice::from_raw_parts(
            message as *const GuiMessage as *const u8,
            core::mem::size_of::<GuiMessage>(),
        )
    };
    ipc_send(GUI_SHELL_PID, bytes)
}

pub fn gui_create_window(window_id: u32, title: &str, width: u32, height: u32) -> isize {
    let mut message = GuiMessage::new(GUI_MSG_CREATE_WINDOW);
    message.window_id = window_id;
    message.a = width as i32;
    message.b = height as i32;
    message.set_data(title.as_bytes());
    gui_send(&message)
}

pub fn gui_draw_text(window_id: u32, x: i32, y: i32, text: &str) -> isize {
    let mut message = GuiMessage::new(GUI_MSG_DRAW_TEXT);
    message.window_id = window_id;
    message.a = x;
    message.b = y;
    message.set_data(text.as_bytes());
    gui_send(&message)
}

pub fn gui_draw_rect(window_id: u32, x: i32, y: i32, width: u32, height: u32, color: u32) -> isize {
    let mut message = GuiMessage::new(GUI_MSG_DRAW_RECT);
    message.window_id = window_id;
    message.a = x;
    message.b = y;
    message.c = color;
    message.len = 8;
    let width_bytes = width.to_le_bytes();
    let height_bytes = height.to_le_bytes();
    message.data[0] = width_bytes[0];
    message.data[1] = width_bytes[1];
    message.data[2] = width_bytes[2];
    message.data[3] = width_bytes[3];
    message.data[4] = height_bytes[0];
    message.data[5] = height_bytes[1];
    message.data[6] = height_bytes[2];
    message.data[7] = height_bytes[3];
    gui_send(&message)
}

pub fn gui_set_status(text: &str) -> isize {
    let mut message = GuiMessage::new(GUI_MSG_SET_STATUS);
    message.set_data(text.as_bytes());
    gui_send(&message)
}

pub fn gui_clear(window_id: u32) -> isize {
    let mut message = GuiMessage::new(GUI_MSG_CLEAR);
    message.window_id = window_id;
    gui_send(&message)
}

pub fn gui_set_title(window_id: u32, title: &str) -> isize {
    let mut message = GuiMessage::new(GUI_MSG_SET_TITLE);
    message.window_id = window_id;
    message.set_data(title.as_bytes());
    gui_send(&message)
}

pub fn gui_recv_event(message: &mut GuiMessage) -> isize {
    let bytes = unsafe {
        core::slice::from_raw_parts_mut(
            message as *mut GuiMessage as *mut u8,
            core::mem::size_of::<GuiMessage>(),
        )
    };
    let received = ipc_recv(bytes);
    if received == core::mem::size_of::<GuiMessage>() as isize && message.valid() {
        received
    } else if received < 0 {
        received
    } else {
        EAGAIN
    }
}

pub fn gui_recv_event_wait(message: &mut GuiMessage, timeout_ms: u64) -> isize {
    let bytes = unsafe {
        core::slice::from_raw_parts_mut(
            message as *mut GuiMessage as *mut u8,
            core::mem::size_of::<GuiMessage>(),
        )
    };
    let received = ipc_recv_blocking(bytes, timeout_ms);
    if received == core::mem::size_of::<GuiMessage>() as isize && message.valid() {
        received
    } else if received < 0 {
        received
    } else {
        EAGAIN
    }
}

pub fn yield_now() -> isize {
    syscall0(SYSCALL_YIELD)
}

pub fn getcwd(buf: &mut [u8]) -> isize {
    syscall2(SYSCALL_GETCWD, buf.as_mut_ptr() as usize, buf.len())
}

pub fn get_system_stats(stats: &mut SystemStats) -> isize {
    syscall1(SYSCALL_GET_SYSTEM_STATS, stats as *mut SystemStats as usize)
}

pub fn chdir(path: &str) -> isize {
    syscall2(SYSCALL_CHDIR, path.as_ptr() as usize, path.len())
}

pub fn debug_log(code: usize) -> isize {
    syscall1(SYSCALL_DEBUG_LOG, code)
}

const MOD_LEFT_SHIFT: usize = 1 << 0;
const MOD_RIGHT_SHIFT: usize = 1 << 1;
const MOD_CAPS_LOCK: usize = 1 << 2;
static KEYBOARD_MODIFIERS: AtomicUsize = AtomicUsize::new(0);

/// Decodes PS/2 Set-1 make/break codes while retaining modifier state.
/// Shift affects symbols, and Shift XOR Caps Lock controls letter case.
pub fn scancode_to_char(scancode: u8) -> Option<char> {
    match scancode {
        0x2A => {
            KEYBOARD_MODIFIERS.fetch_or(MOD_LEFT_SHIFT, Ordering::Relaxed);
            return None;
        }
        0x36 => {
            KEYBOARD_MODIFIERS.fetch_or(MOD_RIGHT_SHIFT, Ordering::Relaxed);
            return None;
        }
        0xAA => {
            KEYBOARD_MODIFIERS.fetch_and(!MOD_LEFT_SHIFT, Ordering::Relaxed);
            return None;
        }
        0xB6 => {
            KEYBOARD_MODIFIERS.fetch_and(!MOD_RIGHT_SHIFT, Ordering::Relaxed);
            return None;
        }
        0x3A => {
            KEYBOARD_MODIFIERS.fetch_xor(MOD_CAPS_LOCK, Ordering::Relaxed);
            return None;
        }
        _ if scancode & 0x80 != 0 => return None,
        _ => {}
    }

    let modifiers = KEYBOARD_MODIFIERS.load(Ordering::Relaxed);
    let shifted = modifiers & (MOD_LEFT_SHIFT | MOD_RIGHT_SHIFT) != 0;
    let upper = shifted ^ (modifiers & MOD_CAPS_LOCK != 0);
    let character = match scancode {
        0x02 => {
            if shifted {
                '!'
            } else {
                '1'
            }
        }
        0x03 => {
            if shifted {
                '@'
            } else {
                '2'
            }
        }
        0x04 => {
            if shifted {
                '#'
            } else {
                '3'
            }
        }
        0x05 => {
            if shifted {
                '$'
            } else {
                '4'
            }
        }
        0x06 => {
            if shifted {
                '%'
            } else {
                '5'
            }
        }
        0x07 => {
            if shifted {
                '^'
            } else {
                '6'
            }
        }
        0x08 => {
            if shifted {
                '&'
            } else {
                '7'
            }
        }
        0x09 => {
            if shifted {
                '*'
            } else {
                '8'
            }
        }
        0x0A => {
            if shifted {
                '('
            } else {
                '9'
            }
        }
        0x0B => {
            if shifted {
                ')'
            } else {
                '0'
            }
        }
        0x0C => {
            if shifted {
                '_'
            } else {
                '-'
            }
        }
        0x0D => {
            if shifted {
                '+'
            } else {
                '='
            }
        }
        0x10 => letter('q', upper),
        0x11 => letter('w', upper),
        0x12 => letter('e', upper),
        0x13 => letter('r', upper),
        0x14 => letter('t', upper),
        0x15 => letter('y', upper),
        0x16 => letter('u', upper),
        0x17 => letter('i', upper),
        0x18 => letter('o', upper),
        0x19 => letter('p', upper),
        0x1A => {
            if shifted {
                '{'
            } else {
                '['
            }
        }
        0x1B => {
            if shifted {
                '}'
            } else {
                ']'
            }
        }
        0x1E => letter('a', upper),
        0x1F => letter('s', upper),
        0x20 => letter('d', upper),
        0x21 => letter('f', upper),
        0x22 => letter('g', upper),
        0x23 => letter('h', upper),
        0x24 => letter('j', upper),
        0x25 => letter('k', upper),
        0x26 => letter('l', upper),
        0x27 => {
            if shifted {
                ':'
            } else {
                ';'
            }
        }
        0x28 => {
            if shifted {
                '"'
            } else {
                '\''
            }
        }
        0x2B => {
            if shifted {
                '|'
            } else {
                '\\'
            }
        }
        0x2C => letter('z', upper),
        0x2D => letter('x', upper),
        0x2E => letter('c', upper),
        0x2F => letter('v', upper),
        0x30 => letter('b', upper),
        0x31 => letter('n', upper),
        0x32 => letter('m', upper),
        0x33 => {
            if shifted {
                '<'
            } else {
                ','
            }
        }
        0x34 => {
            if shifted {
                '>'
            } else {
                '.'
            }
        }
        0x35 => {
            if shifted {
                '?'
            } else {
                '/'
            }
        }
        0x39 => ' ',
        0x1C => '\n',
        0x0F => '\t',
        _ => return None,
    };
    Some(character)
}

fn letter(lower: char, upper: bool) -> char {
    if upper {
        ((lower as u8) - b'a' + b'A') as char
    } else {
        lower
    }
}
