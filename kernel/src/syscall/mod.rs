use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write;
#[cfg(feature = "boot-smoke-tests")]
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

#[repr(u64)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Syscall {
    Exit = 0,
    Fork = 1,
    Exec = 2,
    Read = 3,
    Write = 4,
    Open = 5,
    Close = 6,
    Mmap = 7,
    SendMessage = 8,
    ReceiveMessage = 9,
    GetFramebuffer = 10,
    DrawPixel = 11,
    DrawRect = 12,
    GetKey = 13,
    GetMousePos = 14,
    SpawnProcess = 15,
    WaitProcess = 16,
    GetPid = 17,
    KillProcess = 18,
    Sleep = 19,
    DebugLog = 20,
    #[cfg(feature = "boot-smoke-tests")]
    SmokeDone = 21,
    GetCwd = 22,
    Chdir = 23,
    Yield = 24,
    GetTerminalCursor = 25,
    GetSystemStats = 26,
    Readdir = 27,
    Stat = 28,
    ThreadCreate = 29,
    ThreadJoin = 30,
    ThreadExit = 31,
    GetTid = 32,
    WaitEvent = 33,
    Munmap = 34,
    Mprotect = 35,
    SharedVmCreate = 36,
    SharedVmMap = 37,
    SharedVmClose = 38,
    SetThreadPointer = 39,
    GetThreadPointer = 40,
    FutexWait = 41,
    FutexWake = 42,
    HandleCreateMemory = 43,
    HandleCreateEndpoint = 44,
    HandleRead = 45,
    HandleWrite = 46,
    HandleMap = 47,
    HandleDup = 48,
    HandleRights = 49,
    HandleClose = 50,
    HandleSignal = 51,
    HandleTakeSignals = 52,
    HandleDisplayAcquire = 53,
    HandleTransfer = 54,
    HandleInputAcquire = 55,
    HandleCreateShared = 56,
    FbPresent = 57,
}

impl Syscall {
    pub fn from_u64(value: u64) -> Option<Self> {
        match value {
            0 => Some(Syscall::Exit),
            1 => Some(Syscall::Fork),
            2 => Some(Syscall::Exec),
            3 => Some(Syscall::Read),
            4 => Some(Syscall::Write),
            5 => Some(Syscall::Open),
            6 => Some(Syscall::Close),
            7 => Some(Syscall::Mmap),
            8 => Some(Syscall::SendMessage),
            9 => Some(Syscall::ReceiveMessage),
            10 => Some(Syscall::GetFramebuffer),
            11 => Some(Syscall::DrawPixel),
            12 => Some(Syscall::DrawRect),
            13 => Some(Syscall::GetKey),
            14 => Some(Syscall::GetMousePos),
            15 => Some(Syscall::SpawnProcess),
            16 => Some(Syscall::WaitProcess),
            17 => Some(Syscall::GetPid),
            18 => Some(Syscall::KillProcess),
            19 => Some(Syscall::Sleep),
            20 => Some(Syscall::DebugLog),
            #[cfg(feature = "boot-smoke-tests")]
            21 => Some(Syscall::SmokeDone),
            22 => Some(Syscall::GetCwd),
            23 => Some(Syscall::Chdir),
            24 => Some(Syscall::Yield),
            25 => Some(Syscall::GetTerminalCursor),
            26 => Some(Syscall::GetSystemStats),
            27 => Some(Syscall::Readdir),
            28 => Some(Syscall::Stat),
            29 => Some(Syscall::ThreadCreate),
            30 => Some(Syscall::ThreadJoin),
            31 => Some(Syscall::ThreadExit),
            32 => Some(Syscall::GetTid),
            33 => Some(Syscall::WaitEvent),
            34 => Some(Syscall::Munmap),
            35 => Some(Syscall::Mprotect),
            36 => Some(Syscall::SharedVmCreate),
            37 => Some(Syscall::SharedVmMap),
            38 => Some(Syscall::SharedVmClose),
            39 => Some(Syscall::SetThreadPointer),
            40 => Some(Syscall::GetThreadPointer),
            41 => Some(Syscall::FutexWait),
            42 => Some(Syscall::FutexWake),
            43 => Some(Syscall::HandleCreateMemory),
            44 => Some(Syscall::HandleCreateEndpoint),
            45 => Some(Syscall::HandleRead),
            46 => Some(Syscall::HandleWrite),
            47 => Some(Syscall::HandleMap),
            48 => Some(Syscall::HandleDup),
            49 => Some(Syscall::HandleRights),
            50 => Some(Syscall::HandleClose),
            51 => Some(Syscall::HandleSignal),
            52 => Some(Syscall::HandleTakeSignals),
            53 => Some(Syscall::HandleDisplayAcquire),
            54 => Some(Syscall::HandleTransfer),
            55 => Some(Syscall::HandleInputAcquire),
            56 => Some(Syscall::HandleCreateShared),
            57 => Some(Syscall::FbPresent),
            _ => None,
        }
    }
}

pub const EFAULT: i64 = -14;
pub const EINVAL: i64 = -22;
pub const EBADF: i64 = -9;
pub const ENOSYS: i64 = -38;
pub const ENAMETOOLONG: i64 = -36;
pub const ENOENT: i64 = -2;
pub const EACCES: i64 = -13;
pub const EEXIST: i64 = -17;
pub const ENOTDIR: i64 = -20;
pub const EISDIR: i64 = -21;
pub const EIO: i64 = -5;
pub const ENFILE: i64 = -23;
pub const EOPNOTSUPP: i64 = -95;
pub const ECHILD: i64 = -10;
pub const EAGAIN: i64 = -11;
pub const ENOMEM: i64 = -12;
pub const EINTR: i64 = -4;
pub const EMSGSIZE: i64 = -90;
pub const ENOBUFS: i64 = -105;
pub const EPERM: i64 = -1;
pub const EBUSY: i64 = -16;

/// Параметры фреймбуфера ядра. Раньше четыре `pub static mut` скаляра,
/// записываемые один раз из `lib.rs` и читаемые из системных вызовов display;
/// теперь атомики — простые скаляры не нуждаются в локе.
pub static KERNEL_FB_ADDR: AtomicU64 = AtomicU64::new(0);
pub static KERNEL_FB_WIDTH: AtomicU32 = AtomicU32::new(0);
pub static KERNEL_FB_HEIGHT: AtomicU32 = AtomicU32::new(0);
pub static KERNEL_FB_PITCH: AtomicU32 = AtomicU32::new(0);

#[repr(C)]
pub struct FbInfo {
    pub addr: u64,
    pub width: u32,
    pub height: u32,
    pub pitch: u32,
}

#[repr(C)]
pub struct TerminalCursorInfo {
    pub x: u32,
    pub y: u32,
    pub char_width: u32,
    pub char_height: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
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
pub struct UserDirEntry {
    pub name: [u8; 64],
    pub name_len: usize,
    pub file_type: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct UserFileStat {
    pub file_type: u32,
    pub size: usize,
}

const USER_SPACE_START: u64 = 0x0000_0000_0000_0000;
// Inclusive top of the canonical lower half: highest byte a user pointer may
// occupy. Checks use `end <= USER_SPACE_END`. This is numerically equivalent to
// the exclusive bound `process::USER_ADDRESS_END` (0x0000_8000_0000_0000): a
// byte at `base` is rejected iff `base > USER_SPACE_END` iff `base >= 0x8000_...`.
const USER_SPACE_END: u64 = 0x0000_7FFF_FFFF_FFFF;
const MAX_FD: u32 = 1024;
const MAX_USER_COPY: usize = 64 * 1024;
const MAX_USER_PATH: usize = 256;
const MAX_USER_DIRENTS: usize = 64;
const USER_CONTEXT_RETURN_MAGIC: i64 = 0x0051_5953_4341_4C4C;
#[cfg(feature = "boot-smoke-tests")]
const SMOKE_USER_PAGE: usize = 0x0000_0000_0040_0000;
#[cfg(feature = "boot-smoke-tests")]
const SMOKE_USER_PATH: usize = SMOKE_USER_PAGE;
#[cfg(feature = "boot-smoke-tests")]
const SMOKE_USER_WRITE: usize = SMOKE_USER_PAGE + 64;
#[cfg(feature = "boot-smoke-tests")]
const SMOKE_USER_READ: usize = SMOKE_USER_PAGE + 128;
#[cfg(feature = "boot-smoke-tests")]
const SMOKE_USER_APPEND_A: usize = SMOKE_USER_PAGE + 192;
#[cfg(feature = "boot-smoke-tests")]
const SMOKE_USER_APPEND_B: usize = SMOKE_USER_PAGE + 256;
#[cfg(feature = "boot-smoke-tests")]
const SMOKE_USER_STDOUT: usize = SMOKE_USER_PAGE + 320;
#[cfg(feature = "boot-smoke-tests")]
const SMOKE_FS_PATH: &[u8] = b"/tmp/syscall-smoke.txt";
#[cfg(feature = "boot-smoke-tests")]
const SMOKE_FS_DATA: &[u8] = b"hello";
#[cfg(feature = "boot-smoke-tests")]
const SMOKE_APPEND_A: &[u8] = b"A";
#[cfg(feature = "boot-smoke-tests")]
const SMOKE_APPEND_B: &[u8] = b"B";
#[cfg(feature = "boot-smoke-tests")]
const SMOKE_STDOUT_DATA: &[u8] = b"[STDOUT-TEST] hello from userspace\n";
const EXEC_PATH: &str = "/app";

#[cfg(feature = "boot-smoke-tests")]
static SYSCALL_SMOKE_OK: AtomicBool = AtomicBool::new(false);
#[cfg(feature = "boot-smoke-tests")]
static SYSCALL_FS_SMOKE_OK: AtomicBool = AtomicBool::new(false);
#[cfg(feature = "boot-smoke-tests")]
static SYSCALL_FS_SEMANTICS_OK: AtomicBool = AtomicBool::new(false);

#[repr(C)]
#[derive(Clone, Copy)]
pub struct WaitStatus {
    pub kind: i32,
    pub code: i32,
}

#[cfg(feature = "boot-smoke-tests")]
#[repr(align(4096))]
struct UserSmokeStack(UnsafeCell<[u8; 4096]>);
unsafe impl Sync for UserSmokeStack {}

#[cfg(feature = "boot-smoke-tests")]
static USER_SMOKE_STACK: UserSmokeStack = UserSmokeStack(UnsafeCell::new([0; 4096]));

struct SyscallLogWriter;

impl core::fmt::Write for SyscallLogWriter {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        crate::memory::serial_write(s);
        Ok(())
    }
}

fn syscall_log(args: core::fmt::Arguments) {
    let _ = SyscallLogWriter.write_fmt(args);
}

macro_rules! syscall_log {
    ($($arg:tt)*) => {{
        syscall_log(format_args!($($arg)*));
    }};
}

fn is_valid_user_pointer(ptr: u64, size: usize) -> bool {
    if size == 0 {
        return true;
    }

    if ptr == 0 {
        return false;
    }

    let end = ptr.saturating_add(size as u64);

    if end < ptr {
        return false;
    }

    ptr >= USER_SPACE_START && end <= USER_SPACE_END
}

fn validate_user_range(ptr: u64, size: usize) -> Result<(), i64> {
    if size > MAX_USER_COPY {
        syscall_log!("[SYSCALL] user copy too large: len={}\r\n", size);
        return Err(EINVAL);
    }

    if !is_valid_user_pointer(ptr, size) {
        syscall_log!(
            "[SYSCALL] invalid user pointer: ptr={:#x}, len={}\r\n",
            ptr,
            size
        );
        return Err(EFAULT);
    }

    Ok(())
}

fn validate_user_mapping(ptr: u64, size: usize, writable: bool) -> Result<(), i64> {
    validate_user_range(ptr, size)?;
    if size == 0 {
        return Ok(());
    }

    const PAGE_SIZE: u64 = 4096;
    let first_page = ptr & !(PAGE_SIZE - 1);
    let last_byte = ptr.checked_add(size as u64 - 1).ok_or(EFAULT)?;
    let last_page = last_byte & !(PAGE_SIZE - 1);
    let mut page = first_page;

    loop {
        let flags = crate::memory::vmm::active_user_page_flags(
            crate::memory::vmm::VirtualAddress::from_usize(page as usize),
        )
        .map_err(|_| EFAULT)?
        .ok_or(EFAULT)?;
        let required = crate::memory::vmm::PageFlags::PRESENT | crate::memory::vmm::PageFlags::USER;
        if !flags.contains(required)
            || (writable && !flags.contains(crate::memory::vmm::PageFlags::WRITABLE))
        {
            return Err(EFAULT);
        }
        if page == last_page {
            break;
        }
        page = page.checked_add(PAGE_SIZE).ok_or(EFAULT)?;
    }

    Ok(())
}

pub fn user_copy_is_range_checked_only() -> bool {
    false
}

fn is_valid_fd_number(fd: u32) -> bool {
    fd < MAX_FD
}

fn is_valid_string_pointer(ptr: *const u8) -> bool {
    if ptr.is_null() {
        return false;
    }

    let addr = ptr as u64;
    is_valid_user_pointer(addr, 1)
}

pub fn copy_string_from_user(ptr: *const u8, max_len: usize) -> Result<String, i64> {
    if max_len == 0 || max_len > MAX_USER_COPY {
        syscall_log!("[SYSCALL] invalid user string max_len={}\r\n", max_len);
        return Err(EINVAL);
    }

    validate_user_mapping(ptr as u64, max_len, false)?;

    let mut bytes = Vec::new();
    for offset in 0..max_len {
        let byte = unsafe { core::ptr::read_volatile(ptr.add(offset)) };
        if byte == 0 {
            return String::from_utf8(bytes).map_err(|_| EINVAL);
        }
        bytes.push(byte);
    }

    syscall_log!(
        "[SYSCALL] unterminated user string: ptr={:#x}, max_len={}\r\n",
        ptr as u64,
        max_len
    );
    Err(ENAMETOOLONG)
}

pub fn copy_string_from_user_len(
    ptr: *const u8,
    len: usize,
    max_len: usize,
) -> Result<String, i64> {
    if max_len == 0 || max_len > MAX_USER_COPY {
        syscall_log!("[SYSCALL] invalid user string max_len={}\r\n", max_len);
        return Err(EINVAL);
    }

    if len > max_len {
        syscall_log!(
            "[SYSCALL] user string too large: len={}, max={}\r\n",
            len,
            max_len
        );
        return Err(ENAMETOOLONG);
    }

    validate_user_mapping(ptr as u64, len, false)?;

    let mut bytes = Vec::new();
    for offset in 0..len {
        let byte = unsafe { core::ptr::read_volatile(ptr.add(offset)) };
        if byte == 0 {
            return Err(EINVAL);
        }
        bytes.push(byte);
    }

    String::from_utf8(bytes).map_err(|_| EINVAL)
}

pub fn copy_buffer_from_user(ptr: *const u8, len: usize) -> Result<Vec<u8>, i64> {
    if len == 0 {
        return Ok(Vec::new());
    }

    validate_user_mapping(ptr as u64, len, false)?;

    let mut out = Vec::new();
    out.reserve(len);
    for offset in 0..len {
        let byte = unsafe { core::ptr::read_volatile(ptr.add(offset)) };
        out.push(byte);
    }

    Ok(out)
}

pub fn copy_buffer_to_user(ptr: *mut u8, data: &[u8]) -> Result<(), i64> {
    if data.is_empty() {
        return Ok(());
    }

    validate_user_mapping(ptr as u64, data.len(), true)?;

    for (offset, byte) in data.iter().enumerate() {
        unsafe {
            core::ptr::write_volatile(ptr.add(offset), *byte);
        }
    }

    Ok(())
}

#[no_mangle]
pub extern "C" fn syscall_handler(
    syscall_num: u64,
    arg0: u64,
    arg1: u64,
    arg2: u64,
    arg3: u64,
    arg4: u64,
    _arg5: u64,
) -> i64 {
    let syscall = match Syscall::from_u64(syscall_num) {
        Some(s) => s,
        None => {
            syscall_log!("[SYSCALL] invalid syscall number: {}\r\n", syscall_num);
            return ENOSYS;
        }
    };

    match syscall {
        Syscall::Exit => sys_exit(arg0 as i32),
        Syscall::Fork => sys_fork(),
        Syscall::Exec => sys_exec(arg0 as *const u8, arg1 as usize),
        Syscall::Read => sys_read(arg0 as u32, arg1 as *mut u8, arg2 as usize),
        Syscall::Write => sys_write(arg0 as u32, arg1 as *const u8, arg2 as usize),
        Syscall::Open => sys_open(arg0 as *const u8, arg1 as usize, arg2 as u32),
        Syscall::Close => sys_close(arg0 as u32),
        Syscall::Mmap => sys_mmap(arg0 as usize, arg1 as usize, arg2 as u32, arg3 as u32),
        Syscall::SendMessage => sys_send_message(arg0 as u32, arg1 as *const u8, arg2 as usize),
        Syscall::ReceiveMessage => sys_receive_message(arg0 as *mut u8, arg1 as usize),
        Syscall::GetFramebuffer => sys_get_framebuffer(arg0 as *mut FbInfo),
        Syscall::DrawPixel => sys_draw_pixel(arg0 as u32, arg1 as u32, arg2 as u32),
        Syscall::DrawRect => sys_draw_rect(
            arg0 as u32,
            arg1 as u32,
            arg2 as u32,
            arg3 as u32,
            arg4 as u32,
        ),
        Syscall::GetKey => sys_get_key(),
        Syscall::GetMousePos => sys_get_mouse_pos(arg0 as *mut u32, arg1 as *mut u32),
        Syscall::SpawnProcess => sys_spawn_process(arg0 as *const u8, arg1 as usize),
        Syscall::WaitProcess => sys_wait_process(arg0 as u32, arg1 as *mut WaitStatus),
        Syscall::GetPid => sys_get_pid(),
        Syscall::KillProcess => sys_kill_process(arg0 as u32),
        Syscall::Sleep => sys_sleep(arg0),
        Syscall::DebugLog => sys_debug_log(arg0),
        #[cfg(feature = "boot-smoke-tests")]
        Syscall::SmokeDone => sys_smoke_done(arg0 as i32),
        Syscall::GetCwd => sys_getcwd(arg0 as *mut u8, arg1 as usize),
        Syscall::Chdir => sys_chdir(arg0 as *const u8, arg1 as usize),
        Syscall::Yield => sys_yield(),
        Syscall::GetTerminalCursor => sys_get_terminal_cursor(arg0 as *mut TerminalCursorInfo),
        Syscall::GetSystemStats => sys_get_system_stats(arg0 as *mut SystemStats),
        Syscall::Readdir => sys_readdir(
            arg0 as *const u8,
            arg1 as usize,
            arg2 as *mut UserDirEntry,
            arg3 as usize,
        ),
        Syscall::Stat => sys_stat(arg0 as *const u8, arg1 as usize, arg2 as *mut UserFileStat),
        Syscall::ThreadCreate => sys_thread_create(arg0, arg1, arg2),
        Syscall::ThreadJoin => sys_thread_join(arg0, arg1 as *mut WaitStatus),
        Syscall::ThreadExit => sys_exit(arg0 as i32),
        Syscall::GetTid => crate::process::current_tid().map(|tid| tid.0 as i64).unwrap_or(0),
        Syscall::WaitEvent => sys_wait_event(arg0),
        Syscall::Munmap => sys_munmap(arg0 as usize, arg1 as usize),
        Syscall::Mprotect => sys_mprotect(arg0 as usize, arg1 as usize, arg2 as u32),
        Syscall::SharedVmCreate => sys_shared_vm_create(arg0 as usize),
        Syscall::SharedVmMap => sys_shared_vm_map(arg0, arg1 as usize, arg2 as u32),
        Syscall::SharedVmClose => sys_shared_vm_close(arg0),
        Syscall::SetThreadPointer => sys_set_thread_pointer(arg0),
        Syscall::GetThreadPointer => crate::process::current_thread_pointer()
            .map(|_| unsafe { crate::hal::get_fs_base() as i64 })
            .unwrap_or(EINVAL),
        Syscall::FutexWait => sys_futex_wait(arg0, arg1, arg2),
        Syscall::FutexWake => sys_futex_wake(arg0, arg1),
        Syscall::HandleCreateMemory => sys_handle_create_memory(arg0 as usize),
        Syscall::HandleCreateEndpoint => sys_handle_create_endpoint(arg0),
        Syscall::HandleRead => sys_handle_read(arg0 as u32, arg1 as *mut u8, arg2 as usize),
        Syscall::HandleWrite => sys_handle_write(arg0 as u32, arg1 as *const u8, arg2 as usize),
        Syscall::HandleMap => sys_handle_map(arg0 as u32, arg1 as usize, arg2 as usize),
        Syscall::HandleDup => sys_handle_dup(arg0 as u32, arg1 as u32),
        Syscall::HandleRights => sys_handle_rights(arg0 as u32),
        Syscall::HandleClose => sys_handle_close(arg0 as u32),
        Syscall::HandleSignal => sys_handle_signal(arg0 as u32, arg1),
        Syscall::HandleTakeSignals => sys_handle_take_signals(),
        Syscall::HandleDisplayAcquire => sys_handle_display_acquire(),
        Syscall::HandleTransfer => sys_handle_transfer(arg0 as u32, arg1),
        Syscall::HandleInputAcquire => sys_handle_input_acquire(),
        Syscall::HandleCreateShared => sys_handle_create_shared(arg0 as usize),
        Syscall::FbPresent => sys_fb_present(
            arg0 as *const u8,
            arg1 as u32,
            arg2 as u32,
            arg3 as u32,
            arg4 as u32,
        ),
    }
}

fn handle_error_to_errno(error: crate::handle::HandleError) -> i64 {
    use crate::handle::HandleError;
    match error {
        HandleError::BadHandle => EBADF,
        HandleError::AccessDenied => EPERM,
        HandleError::WrongType => EINVAL,
        HandleError::DisplayBusy => EBUSY,
        HandleError::TooLarge => EINVAL,
        HandleError::MapFailed => ENOMEM,
        HandleError::NoSuchTarget => ENOENT,
    }
}

fn sys_handle_create_memory(len: usize) -> i64 {
    match crate::process::current_process_mut() {
        Some(process) => process
            .handle_create_memory(len)
            .map(|h| h as i64)
            .unwrap_or_else(handle_error_to_errno),
        None => EINVAL,
    }
}

fn sys_handle_create_endpoint(target_pid: u64) -> i64 {
    match crate::process::current_process_mut() {
        Some(process) => {
            process.handle_create_endpoint(crate::process::ProcessId(target_pid)) as i64
        }
        None => EINVAL,
    }
}

fn sys_handle_read(handle: u32, user_buf: *mut u8, len: usize) -> i64 {
    if len == 0 || !is_valid_user_pointer(user_buf as u64, len) {
        return EINVAL;
    }
    // Читаем во временный буфер ядра, затем копируем в userspace через
    // проверенный путь copy_buffer_to_user.
    let mut scratch = alloc::vec![0u8; len];
    let copied = match crate::process::current_process_mut() {
        Some(process) => match process.handle_memory_read(handle, &mut scratch) {
            Ok(n) => n,
            Err(error) => return handle_error_to_errno(error),
        },
        None => return EINVAL,
    };
    if let Err(error) = copy_buffer_to_user(user_buf, &scratch[..copied]) {
        return error;
    }
    copied as i64
}

fn sys_handle_write(handle: u32, user_buf: *const u8, len: usize) -> i64 {
    if len == 0 || !is_valid_user_pointer(user_buf as u64, len) {
        return EINVAL;
    }
    let data = match copy_buffer_from_user(user_buf, len) {
        Ok(data) => data,
        Err(error) => return error,
    };
    match crate::process::current_process_mut() {
        Some(process) => process
            .handle_memory_write(handle, &data)
            .map(|n| n as i64)
            .unwrap_or_else(handle_error_to_errno),
        None => EINVAL,
    }
}

fn sys_handle_map(handle: u32, addr: usize, len: usize) -> i64 {
    match crate::process::current_process_mut() {
        Some(process) => process
            .handle_map(handle, addr, len)
            .map(|mapped| mapped as i64)
            .unwrap_or_else(handle_error_to_errno),
        None => EINVAL,
    }
}

fn sys_handle_dup(handle: u32, new_rights: u32) -> i64 {
    match crate::process::current_process_mut() {
        Some(process) => process
            .handle_dup(handle, new_rights)
            .map(|h| h as i64)
            .unwrap_or_else(handle_error_to_errno),
        None => EINVAL,
    }
}

fn sys_handle_rights(handle: u32) -> i64 {
    match crate::process::current_process_mut() {
        Some(process) => process
            .handle_rights(handle)
            .map(|r| r as i64)
            .unwrap_or_else(handle_error_to_errno),
        None => EINVAL,
    }
}

fn sys_handle_close(handle: u32) -> i64 {
    match crate::process::current_process_mut() {
        Some(process) => process
            .handle_close(handle)
            .map(|_| 0)
            .unwrap_or_else(handle_error_to_errno),
        None => EINVAL,
    }
}

fn sys_handle_signal(handle: u32, value: u64) -> i64 {
    match crate::process::current_process_mut() {
        Some(process) => process
            .handle_signal(handle, value)
            .map(|_| 0)
            .unwrap_or_else(handle_error_to_errno),
        None => EINVAL,
    }
}

fn sys_handle_take_signals() -> i64 {
    match crate::process::current_process_mut() {
        Some(process) => process.handle_take_signals() as i64,
        None => EINVAL,
    }
}

fn sys_handle_display_acquire() -> i64 {
    match crate::process::current_process_mut() {
        Some(process) => process
            .handle_display_acquire()
            .map(|h| h as i64)
            .unwrap_or_else(handle_error_to_errno),
        None => EINVAL,
    }
}

fn sys_handle_input_acquire() -> i64 {
    match crate::process::current_process_mut() {
        Some(process) => process
            .handle_input_acquire()
            .map(|h| h as i64)
            .unwrap_or_else(handle_error_to_errno),
        None => EINVAL,
    }
}

fn sys_handle_create_shared(len: usize) -> i64 {
    match crate::process::current_process_mut() {
        Some(process) => process
            .handle_create_shared(len)
            .map(|h| h as i64)
            .unwrap_or_else(handle_error_to_errno),
        None => EINVAL,
    }
}

fn sys_handle_transfer(handle: u32, target_pid: u64) -> i64 {
    let target = crate::process::ProcessId(target_pid);
    // Проверяем существование цели ДО изъятия объекта из таблицы-источника,
    // чтобы неудачная передача не «съела» capability.
    let process = match crate::process::current_process_mut() {
        Some(process) => process,
        None => return EINVAL,
    };
    let self_pid = process.pid;
    if target != self_pid && !crate::process::process_exists(target) {
        return ENOENT;
    }
    let (object, rights) = match process.handle_take_for_transfer(handle) {
        Ok(pair) => pair,
        Err(error) => return handle_error_to_errno(error),
    };
    if target == self_pid {
        return process.handle_receive(object, rights) as i64;
    }
    match crate::process::with_process_mut(target, |t| Ok(t.handle_receive(object, rights))) {
        Ok(new_handle) => new_handle as i64,
        Err(_) => ENOENT,
    }
}

fn sys_exit(code: i32) -> i64 {
    if let Some(pid) = crate::process::request_current_user_exit(code) {
        let tid = crate::process::current_tid().map(|id| id.0).unwrap_or(pid.0);
        if tid == pid.0 {
            syscall_log!("[PROCESS-RUN] exited pid={} code={}\r\n", pid.0, code);
        } else {
            syscall_log!("[THREAD] exited tid={} pid={} code={}\r\n", tid, pid.0, code);
        }
        return USER_CONTEXT_RETURN_MAGIC;
    }

    0
}

fn sys_fork() -> i64 {
    ENOSYS
}

fn sys_exec(path: *const u8, path_len: usize) -> i64 {
    if let Err(error) = copy_string_from_user_len(path, path_len, MAX_USER_PATH) {
        return error;
    }
    ENOSYS
}

fn sys_read(fd: u32, buf: *mut u8, count: usize) -> i64 {
    if !is_valid_fd_number(fd) {
        return EBADF;
    }

    if count == 0 {
        return 0;
    }

    if let Err(error) = validate_user_range(buf as u64, count) {
        return error;
    }

    match crate::process::get_fd(fd).map(|entry| entry.target) {
        Some(crate::process::FdTarget::Stdin) => {
            let mut input = Vec::new();
            input.resize(count, 0);
            match crate::process::take_terminal_stdin_for_current(&mut input) {
                Ok(Some(read)) => {
                    if let Err(error) = copy_buffer_to_user(buf, &input[..read]) {
                        return error;
                    }
                    return read as i64;
                }
                Ok(None) => {
                    if crate::process::request_terminal_stdin_for_current().is_ok() {
                        return EAGAIN;
                    }
                    return 0;
                }
                Err(error) => return process_error_to_errno(error),
            }
        }
        Some(crate::process::FdTarget::Stdout | crate::process::FdTarget::Stderr) => return EBADF,
        Some(crate::process::FdTarget::Vfs(_)) => {}
        None => return EBADF,
    }

    let vfs_fd = match process_vfs_fd(fd) {
        Ok(vfs_fd) => vfs_fd,
        Err(error) => return error,
    };

    let mut kernel_buf = Vec::new();
    kernel_buf.resize(count, 0);

    let bytes_read = match crate::fs::vfs::get_vfs().ok_or(EIO).and_then(|vfs| {
        vfs.read(vfs_fd, &mut kernel_buf)
            .map_err(vfs_error_to_errno)
    }) {
        Ok(bytes_read) => bytes_read,
        Err(error) => return error,
    };

    if let Err(error) = copy_buffer_to_user(buf, &kernel_buf[..bytes_read]) {
        return error;
    }

    bytes_read as i64
}

fn sys_write(fd: u32, buf: *const u8, count: usize) -> i64 {
    if !is_valid_fd_number(fd) {
        return EBADF;
    }

    if count == 0 {
        return 0;
    }

    let data = match copy_buffer_from_user(buf, count) {
        Ok(data) => data,
        Err(error) => return error,
    };

    match crate::process::get_fd(fd).map(|entry| entry.target) {
        Some(crate::process::FdTarget::Stdout) => {
            write_stdio("STDOUT", &data);
            write_terminal_foreground(&data);
            return data.len() as i64;
        }
        Some(crate::process::FdTarget::Stderr) => {
            write_stdio("STDERR", &data);
            write_terminal_foreground(&data);
            return data.len() as i64;
        }
        Some(crate::process::FdTarget::Stdin) => return EBADF,
        Some(crate::process::FdTarget::Vfs(_)) => {}
        None => return EBADF,
    }

    let vfs_fd = match process_vfs_fd(fd) {
        Ok(vfs_fd) => vfs_fd,
        Err(error) => return error,
    };

    match crate::fs::vfs::get_vfs()
        .ok_or(EIO)
        .and_then(|vfs| vfs.write(vfs_fd, &data).map_err(vfs_error_to_errno))
    {
        Ok(bytes_written) => bytes_written as i64,
        Err(error) => error,
    }
}

fn sys_open(path: *const u8, path_len: usize, flags: u32) -> i64 {
    let path = match copy_string_from_user_len(path, path_len, MAX_USER_PATH) {
        Ok(path) => path,
        Err(error) => return error,
    };

    let flags = match open_flags_from_u32(flags) {
        Ok(flags) => flags,
        Err(error) => return error,
    };

    let cwd = match crate::process::current_process() {
        Some(process) => process.cwd.clone(),
        None => return EINVAL,
    };

    let vfs_fd = match crate::fs::vfs::get_vfs()
        .ok_or(EIO)
        .and_then(|vfs| vfs.open_at(&cwd, &path, flags).map_err(vfs_error_to_errno))
    {
        Ok(fd) => fd,
        Err(error) => return error,
    };

    match crate::process::allocate_fd(crate::process::FdEntry::vfs(vfs_fd)) {
        Ok(fd) => fd as i64,
        Err(error) => {
            if let Some(vfs) = crate::fs::vfs::get_vfs() {
                let _ = vfs.close(vfs_fd);
            }
            process_error_to_errno(error)
        }
    }
}

fn sys_close(fd: u32) -> i64 {
    if !is_valid_fd_number(fd) {
        return EBADF;
    }

    let entry = match crate::process::get_fd(fd) {
        Some(entry) => *entry,
        None => return EBADF,
    };

    match entry.target {
        crate::process::FdTarget::Stdin
        | crate::process::FdTarget::Stdout
        | crate::process::FdTarget::Stderr => EOPNOTSUPP,
        crate::process::FdTarget::Vfs(vfs_fd) => {
            if let Err(error) = crate::fs::vfs::get_vfs()
                .ok_or(EIO)
                .and_then(|vfs| vfs.close(vfs_fd).map_err(vfs_error_to_errno))
            {
                return error;
            }

            match crate::process::close_fd(fd) {
                Ok(_) => 0,
                Err(error) => process_error_to_errno(error),
            }
        }
    }
}

fn user_file_type(file_type: crate::fs::vfs::FileType) -> u32 {
    match file_type {
        crate::fs::vfs::FileType::File => 1,
        crate::fs::vfs::FileType::Directory => 2,
        crate::fs::vfs::FileType::Device => 3,
    }
}

fn sys_readdir(
    path: *const u8,
    path_len: usize,
    entries: *mut UserDirEntry,
    capacity: usize,
) -> i64 {
    let path = match copy_string_from_user_len(path, path_len, MAX_USER_PATH) {
        Ok(path) => path,
        Err(error) => return error,
    };
    if capacity > MAX_USER_DIRENTS {
        return EINVAL;
    }
    if capacity > 0 {
        let bytes = capacity.saturating_mul(core::mem::size_of::<UserDirEntry>());
        if let Err(error) = validate_user_range(entries as u64, bytes) {
            return error;
        }
    }

    let cwd = match crate::process::current_process() {
        Some(process) => process.cwd.clone(),
        None => return EINVAL,
    };

    let mut kernel_entries = [crate::fs::vfs::DirEntry::empty(); MAX_USER_DIRENTS];
    let count = match crate::fs::vfs::get_vfs().ok_or(EIO).and_then(|vfs| {
        vfs.readdir_into_at(&cwd, &path, &mut kernel_entries[..capacity])
            .map_err(vfs_error_to_errno)
    }) {
        Ok(count) => count,
        Err(error) => return error,
    };

    for (index, entry) in kernel_entries.iter().take(count).enumerate() {
        let name = entry.name().as_bytes();
        let name_len = name.len().min(64);
        let mut user_entry = UserDirEntry {
            name: [0; 64],
            name_len,
            file_type: user_file_type(entry.file_type),
        };
        user_entry.name[..name_len].copy_from_slice(&name[..name_len]);
        let bytes = unsafe {
            core::slice::from_raw_parts(
                &user_entry as *const UserDirEntry as *const u8,
                core::mem::size_of::<UserDirEntry>(),
            )
        };
        if let Err(error) = copy_buffer_to_user(unsafe { entries.add(index) } as *mut u8, bytes) {
            return error;
        }
    }

    count as i64
}

fn sys_stat(path: *const u8, path_len: usize, stat: *mut UserFileStat) -> i64 {
    let path = match copy_string_from_user_len(path, path_len, MAX_USER_PATH) {
        Ok(path) => path,
        Err(error) => return error,
    };
    if let Err(error) = validate_user_range(stat as u64, core::mem::size_of::<UserFileStat>()) {
        return error;
    }

    let cwd = match crate::process::current_process() {
        Some(process) => process.cwd.clone(),
        None => return EINVAL,
    };

    let file_stat = match crate::fs::vfs::get_vfs()
        .ok_or(EIO)
        .and_then(|vfs| vfs.stat_at(&cwd, &path).map_err(vfs_error_to_errno))
    {
        Ok(stat) => stat,
        Err(error) => return error,
    };
    let user_stat = UserFileStat {
        file_type: user_file_type(file_stat.file_type),
        size: file_stat.size,
    };
    let bytes = unsafe {
        core::slice::from_raw_parts(
            &user_stat as *const UserFileStat as *const u8,
            core::mem::size_of::<UserFileStat>(),
        )
    };
    match copy_buffer_to_user(stat as *mut u8, bytes) {
        Ok(()) => 0,
        Err(error) => error,
    }
}

fn write_stdio(label: &str, data: &[u8]) {
    syscall_log!("[{}] ", label);
    for byte in data {
        let ch = *byte as char;
        syscall_log!("{}", ch);
    }
    if data.last().copied() != Some(b'\n') {
        syscall_log!("\r\n");
    }
}

fn write_terminal_foreground(data: &[u8]) {
    match crate::process::current_process_output_sink() {
        Some(crate::process::ProcessOutputSink::Terminal) => {
            let Some(console) = crate::terminal::get_console() else {
                return;
            };
            match core::str::from_utf8(data) {
                Ok(text) => console.write_display_str(text),
                Err(_) => {
                    for byte in data {
                        console.write_display_str(core::str::from_utf8(&[*byte]).unwrap_or("?"));
                    }
                }
            }
        }
        Some(crate::process::ProcessOutputSink::GuiTerminal) => {
            #[cfg(feature = "legacy_gui")]
            crate::ui_loop::gui_terminal_write_exec_output(data);
            #[cfg(not(feature = "legacy_gui"))]
            let _ = data;
        }
        _ => {}
    }
}

fn sys_mmap(addr: usize, length: usize, prot: u32, flags: u32) -> i64 {
    const PROT_READ: u32 = 1 << 0;
    const PROT_WRITE: u32 = 1 << 1;
    const PROT_EXEC: u32 = 1 << 2;
    const MAP_PRIVATE: u32 = 1 << 1;
    const MAP_ANONYMOUS: u32 = 1 << 5;
    const MAP_GUARD: u32 = 1 << 6;

    if length == 0 {
        return EINVAL;
    }
    if prot & !(PROT_READ | PROT_WRITE | PROT_EXEC) != 0
        || flags & !(MAP_PRIVATE | MAP_ANONYMOUS | MAP_GUARD) != 0
        || flags & (MAP_PRIVATE | MAP_ANONYMOUS) != (MAP_PRIVATE | MAP_ANONYMOUS)
        || prot & (PROT_WRITE | PROT_EXEC) == (PROT_WRITE | PROT_EXEC)
    {
        return EINVAL;
    }
    if addr != 0 && (addr & 0xFFF) != 0 {
        return EINVAL;
    }

    match crate::process::current_process_mut() {
        Some(process) => {
            match process.map_anonymous(
                addr, length, prot & PROT_WRITE != 0, prot & PROT_EXEC != 0,
                flags & MAP_GUARD != 0, prot != 0,
            )
            {
                Ok(mapped) => mapped as i64,
                Err(error) => process_error_to_errno(error),
            }
        }
        None => EINVAL,
    }
}

fn sys_munmap(addr: usize, length: usize) -> i64 {
    match crate::process::current_process_mut() {
        Some(process) => process.unmap_range(addr, length)
            .map(|_| 0).unwrap_or_else(process_error_to_errno),
        None => EINVAL,
    }
}

fn sys_mprotect(addr: usize, length: usize, prot: u32) -> i64 {
    const PROT_READ: u32 = 1;
    const PROT_WRITE: u32 = 2;
    const PROT_EXEC: u32 = 4;
    if prot & !(PROT_READ | PROT_WRITE | PROT_EXEC) != 0
        || prot & (PROT_WRITE | PROT_EXEC) == (PROT_WRITE | PROT_EXEC)
    {
        return EINVAL;
    }
    match crate::process::current_process_mut() {
        Some(process) => process.protect_range(
            addr, length, prot & PROT_WRITE != 0, prot & PROT_EXEC != 0, prot != 0,
        ).map(|_| 0).unwrap_or_else(process_error_to_errno),
        None => EINVAL,
    }
}

fn sys_shared_vm_create(length: usize) -> i64 {
    match crate::process::current_process_mut() {
        Some(process) => process.create_shared_vm(length)
            .map(|id| id as i64).unwrap_or_else(process_error_to_errno),
        None => EINVAL,
    }
}

fn sys_shared_vm_map(id: u64, addr: usize, prot: u32) -> i64 {
    if prot != 1 && prot != 3 { return EINVAL; }
    match crate::process::current_process_mut() {
        Some(process) => process.map_shared_vm(id, addr, prot & 2 != 0)
            .map(|mapped| mapped as i64).unwrap_or_else(process_error_to_errno),
        None => EINVAL,
    }
}

fn sys_shared_vm_close(id: u64) -> i64 {
    match crate::process::current_process_mut() {
        Some(process) => process.close_shared_vm(id)
            .map(|_| 0).unwrap_or_else(process_error_to_errno),
        None => EINVAL,
    }
}

fn sys_set_thread_pointer(base: u64) -> i64 {
    if base > USER_SPACE_END || (base != 0 && !is_valid_user_pointer(base, 1)) {
        return EINVAL;
    }
    match crate::process::set_current_thread_pointer(base) {
        Ok(()) => 0,
        Err(error) => process_error_to_errno(error),
    }
}

fn open_flags_from_u32(flags: u32) -> Result<crate::fs::vfs::OpenFlags, i64> {
    let flags = crate::fs::vfs::OpenFlags::from_bits(flags);
    if flags.is_valid() {
        Ok(flags)
    } else {
        Err(EINVAL)
    }
}

fn process_vfs_fd(fd: u32) -> Result<crate::fs::vfs::FileDescriptor, i64> {
    match crate::process::get_fd(fd).map(|entry| entry.target) {
        Some(crate::process::FdTarget::Vfs(vfs_fd)) => Ok(vfs_fd),
        Some(
            crate::process::FdTarget::Stdin
            | crate::process::FdTarget::Stdout
            | crate::process::FdTarget::Stderr,
        ) => Err(EBADF),
        None => Err(EBADF),
    }
}

fn process_error_to_errno(error: crate::process::ProcessError) -> i64 {
    match error {
        crate::process::ProcessError::NoCurrentProcess => EINVAL,
        crate::process::ProcessError::NoSuchProcess => ENOENT,
        crate::process::ProcessError::NotChild => ECHILD,
        crate::process::ProcessError::NotRunnable => EAGAIN,
        crate::process::ProcessError::SchedulerUnavailable => EAGAIN,
        crate::process::ProcessError::ProcessAlreadyExists => EEXIST,
        crate::process::ProcessError::AddressInUse => EEXIST,
        crate::process::ProcessError::OutOfMemory => ENOMEM,
        crate::process::ProcessError::InvalidFd => EBADF,
        crate::process::ProcessError::FdTableFull => ENFILE,
        crate::process::ProcessError::NoAddressSpace
        | crate::process::ProcessError::AddressSpaceCreateFailed
        | crate::process::ProcessError::NoKernelStack
        | crate::process::ProcessError::InvalidUserContext
        | crate::process::ProcessError::ProcessNotPrepared
        | crate::process::ProcessError::InvalidMemoryRange => EINVAL,
    }
}

fn vfs_error_to_errno(error: crate::fs::vfs::VfsError) -> i64 {
    match error {
        crate::fs::vfs::VfsError::NotFound => ENOENT,
        crate::fs::vfs::VfsError::PermissionDenied => EACCES,
        crate::fs::vfs::VfsError::InvalidDescriptor => EBADF,
        crate::fs::vfs::VfsError::AlreadyExists => EEXIST,
        crate::fs::vfs::VfsError::NotADirectory => ENOTDIR,
        crate::fs::vfs::VfsError::IsADirectory => EISDIR,
        crate::fs::vfs::VfsError::InvalidPath => EINVAL,
        crate::fs::vfs::VfsError::Unsupported => EOPNOTSUPP,
        crate::fs::vfs::VfsError::IoError => EIO,
    }
}

fn ipc_error_to_errno(error: crate::ipc::IpcError) -> i64 {
    match error {
        crate::ipc::IpcError::InvalidTarget => ENOENT,
        crate::ipc::IpcError::MessageTooLarge => EMSGSIZE,
        crate::ipc::IpcError::QueueFull => ENOBUFS,
        crate::ipc::IpcError::NoMessage => EAGAIN,
        crate::ipc::IpcError::Unavailable => EAGAIN,
    }
}

fn sys_send_message(target_pid: u32, msg: *const u8, len: usize) -> i64 {
    if target_pid == 0 {
        return EINVAL;
    }
    if len == 0 || len > crate::ipc::MAX_MESSAGE_SIZE {
        return EMSGSIZE;
    }

    let data = match copy_buffer_from_user(msg, len) {
        Ok(data) => data,
        Err(error) => return error,
    };
    let sender = match crate::process::current_process() {
        Some(process) => process.pid,
        None => return EINVAL,
    };
    let target = crate::process::ProcessId(target_pid as u64);
    match crate::ipc::send_bytes(sender, target, &data) {
        Ok(()) => {
            syscall_log(format_args!(
                "[IPC] send from={} to={} len={}\n",
                sender.0, target.0, len
            ));
            len as i64
        }
        Err(error) => ipc_error_to_errno(error),
    }
}

fn sys_receive_message(msg: *mut u8, len: usize) -> i64 {
    if len == 0 || len > crate::ipc::MAX_MESSAGE_SIZE {
        return EMSGSIZE;
    }
    let pid = match crate::process::current_process() {
        Some(process) => process.pid,
        None => return EINVAL,
    };
    let mut buffer = [0u8; crate::ipc::MAX_MESSAGE_SIZE];
    let received = match crate::ipc::recv_bytes(pid, &mut buffer[..len]) {
        Ok(received) => received,
        Err(error) => return ipc_error_to_errno(error),
    };
    if let Err(error) = copy_buffer_to_user(msg, &buffer[..received]) {
        return error;
    }
    syscall_log(format_args!("[IPC] recv pid={} len={}\n", pid.0, received));
    received as i64
}

fn sys_get_framebuffer(info: *mut FbInfo) -> i64 {
    unsafe {
        if KERNEL_FB_ADDR.load(Ordering::Relaxed) == 0 {
            return EINVAL;
        }
        let fb = FbInfo {
            addr: KERNEL_FB_ADDR.load(Ordering::Relaxed),
            width: KERNEL_FB_WIDTH.load(Ordering::Relaxed),
            height: KERNEL_FB_HEIGHT.load(Ordering::Relaxed),
            pitch: KERNEL_FB_PITCH.load(Ordering::Relaxed),
        };
        let bytes = core::slice::from_raw_parts(
            &fb as *const FbInfo as *const u8,
            core::mem::size_of::<FbInfo>(),
        );
        if let Err(error) = copy_buffer_to_user(info as *mut u8, bytes) {
            return error;
        }
    }
    0
}

fn sys_get_terminal_cursor(info: *mut TerminalCursorInfo) -> i64 {
    let Some(cursor) = crate::terminal::get_cursor_info() else {
        return EINVAL;
    };
    let user_info = TerminalCursorInfo {
        x: cursor.x,
        y: cursor.y,
        char_width: cursor.char_width,
        char_height: cursor.char_height,
    };
    let bytes = unsafe {
        core::slice::from_raw_parts(
            &user_info as *const TerminalCursorInfo as *const u8,
            core::mem::size_of::<TerminalCursorInfo>(),
        )
    };
    if let Err(error) = copy_buffer_to_user(info as *mut u8, bytes) {
        return error;
    }
    0
}

fn sys_get_system_stats(info: *mut SystemStats) -> i64 {
    let process = crate::process::process_stats();
    let (pmm_total, pmm_free) = match crate::memory::pmm::get_pmm() {
        Some(pmm) => (pmm.total_memory() as u64, pmm.available_memory() as u64),
        None => (0, 0),
    };
    let heap = crate::allocator::heap_stats();
    let ipc = crate::ipc::ipc_stats();
    let fs = crate::fs::vfs::root_memfs_stats();
    let net = crate::drivers::net::snapshot();
    let stats = SystemStats {
        process_total: process.total,
        process_prepared: process.prepared,
        process_ready: process.ready,
        process_running: process.running,
        process_blocked: process.blocked,
        process_dead: process.dead,
        process_reaped: process.reaped,
        pmm_total_bytes: pmm_total,
        pmm_free_bytes: pmm_free,
        pmm_used_bytes: pmm_total.saturating_sub(pmm_free),
        heap_total_bytes: heap.total_bytes,
        heap_free_bytes: heap.free_bytes,
        heap_used_bytes: heap.used_bytes,
        heap_free_blocks: heap.free_blocks,
        ipc_queue_count: ipc.queue_count,
        ipc_queued_messages: ipc.queued_messages,
        ipc_shared_regions: ipc.shared_regions,
        ipc_max_queue_messages: ipc.max_queue_messages,
        fs_files: fs.files,
        fs_directories: fs.directories,
        fs_bytes: fs.bytes,
        fs_open_handles: fs.open_handles,
        uptime_ticks: crate::interrupts::timer_ticks(),
        uptime_available: 1,
        // Discovery-only: these counts reflect PCI NIC probing, not a working
        // network stack (there is no packet I/O yet). See drivers::net.
        net_total_nics: net.total_nics as u64,
        net_supported_nics: net.supported_nics as u64,
        net_mmio_ready_nics: net.mmio_ready_nics as u64,
        net_mac_ready_nics: net.mac_ready_nics as u64,
    };
    let bytes = unsafe {
        core::slice::from_raw_parts(
            &stats as *const SystemStats as *const u8,
            core::mem::size_of::<SystemStats>(),
        )
    };
    if let Err(error) = copy_buffer_to_user(info as *mut u8, bytes) {
        return error;
    }
    0
}

fn sys_draw_pixel(x: u32, y: u32, color: u32) -> i64 {
    unsafe {
        if KERNEL_FB_ADDR.load(Ordering::Relaxed) == 0 {
            return EINVAL;
        }
        let fb = KERNEL_FB_ADDR.load(Ordering::Relaxed) as *mut u32;
        let w = KERNEL_FB_WIDTH.load(Ordering::Relaxed) as usize;
        let h = KERNEL_FB_HEIGHT.load(Ordering::Relaxed) as usize;
        if (x as usize) < w && (y as usize) < h {
            let pitch_pixels = KERNEL_FB_PITCH.load(Ordering::Relaxed) as usize / 4;
            core::ptr::write_volatile(fb.add(y as usize * pitch_pixels + x as usize), color);
        }
    }
    0
}

fn sys_draw_rect(x: u32, y: u32, w: u32, h: u32, color: u32) -> i64 {
    unsafe {
        if KERNEL_FB_ADDR.load(Ordering::Relaxed) == 0 {
            return EINVAL;
        }
        let fb = KERNEL_FB_ADDR.load(Ordering::Relaxed) as *mut u32;
        let fb_w = KERNEL_FB_WIDTH.load(Ordering::Relaxed) as usize;
        let fb_h = KERNEL_FB_HEIGHT.load(Ordering::Relaxed) as usize;
        let pitch_pixels = KERNEL_FB_PITCH.load(Ordering::Relaxed) as usize / 4;
        for dy in 0..h as usize {
            for dx in 0..w as usize {
                let px = x as usize + dx;
                let py = y as usize + dy;
                if px < fb_w && py < fb_h {
                    core::ptr::write_volatile(fb.add(py * pitch_pixels + px), color);
                }
            }
        }
    }
    0
}

fn sys_fb_present(src: *const u8, width: u32, height: u32, dst_x: u32, dst_y: u32) -> i64 {
    // Только владелец дисплея (compositor в userspace) может выводить в
    // фреймбуфер. Политика композитинга живёт в userspace — ядро даёт лишь
    // механизм блита. Владельца отслеживает глобальный DISPLAY_MASTER_OWNER.
    let pid = match crate::process::current_process() {
        Some(process) => process.pid.0,
        None => return EINVAL,
    };
    if pid == 0 || crate::handle::display_owner() != pid {
        return EACCES;
    }
    if KERNEL_FB_ADDR.load(Ordering::Relaxed) == 0 {
        return EINVAL;
    }
    let (w, h) = (width as usize, height as usize);
    let Some(bytes) = w.checked_mul(h).and_then(|px| px.checked_mul(4)) else {
        return EINVAL;
    };
    if bytes == 0 || bytes > 64 * 1024 * 1024 {
        return EINVAL;
    }
    if !is_valid_user_pointer(src as u64, bytes) {
        return EFAULT;
    }
    let data = match copy_buffer_from_user(src, bytes) {
        Ok(data) => data,
        Err(error) => return error,
    };
    let fb = KERNEL_FB_ADDR.load(Ordering::Relaxed) as *mut u8;
    let fb_w = KERNEL_FB_WIDTH.load(Ordering::Relaxed) as usize;
    let fb_h = KERNEL_FB_HEIGHT.load(Ordering::Relaxed) as usize;
    let pitch = KERNEL_FB_PITCH.load(Ordering::Relaxed) as usize;
    let (dx, dy) = (dst_x as usize, dst_y as usize);
    for row in 0..h {
        let py = dy + row;
        if py >= fb_h {
            break;
        }
        for col in 0..w {
            let px = dx + col;
            if px >= fb_w {
                continue;
            }
            let s = (row * w + col) * 4;
            let color = u32::from_le_bytes([data[s], data[s + 1], data[s + 2], data[s + 3]]);
            unsafe {
                let dst = fb.add(py * pitch + px * 4) as *mut u32;
                core::ptr::write_volatile(dst, color);
            }
        }
    }
    0
}

fn sys_get_key() -> i64 {
    if let Some(sc) = crate::drivers::keyboard::read_scancode() {
        sc as i64
    } else {
        -1
    }
}

fn sys_get_mouse_pos(x: *mut u32, y: *mut u32) -> i64 {
    let (mx, my) = crate::input::mouse_position();
    let x_bytes = (mx as u32).to_le_bytes();
    let y_bytes = (my as u32).to_le_bytes();

    if let Err(error) = copy_buffer_to_user(x as *mut u8, &x_bytes) {
        return error;
    }
    if let Err(error) = copy_buffer_to_user(y as *mut u8, &y_bytes) {
        return error;
    }

    0
}

fn sys_spawn_process(path: *const u8, path_len: usize) -> i64 {
    let path = match copy_string_from_user_len(path, path_len, MAX_USER_PATH) {
        Ok(path) => path,
        Err(error) => return error,
    };

    let cwd = match crate::process::current_process() {
        Some(process) => process.cwd.clone(),
        None => return EINVAL,
    };

    let resolved = match resolve_exec_path(&cwd, &path) {
        Ok(path) => path,
        Err(error) => return error,
    };

    let data = match read_vfs_file(&cwd, &resolved) {
        Ok(data) => data,
        Err(error) => return vfs_error_to_errno(error),
    };

    if crate::elf::ElfParser::new(&data).is_err() {
        return EIO;
    }

    let pid = match crate::process::create_user_process_record(resolved.clone(), true) {
        Ok(pid) => pid,
        Err(error) => return process_error_to_errno(error),
    };

    let argv0 = resolved
        .rsplit('/')
        .find(|part| !part.is_empty())
        .unwrap_or(resolved.as_str());
    let argv = [String::from(argv0)];
    if crate::elf::prepare_process_elf(pid, &data, &argv).is_err() {
        syscall_log(format_args!(
            "[SPAWN] prepare failed pid={} path={}\n",
            pid.0, resolved
        ));
        let _ = crate::process::autoreap_process(pid, "spawn-prepare-failed");
        return EIO;
    }

    syscall_log(format_args!(
        "[SPAWN] ready pid={} path={} execution=not-started\n",
        pid.0, resolved
    ));
    pid.0 as i64
}

fn sys_wait_process(pid: u32, status: *mut WaitStatus) -> i64 {
    let wait_record = match crate::process::wait_for_child(crate::process::ProcessId(pid as u64)) {
        Ok(record) => record,
        Err(error) => return process_error_to_errno(error),
    };
    let wait_status = WaitStatus {
        kind: wait_record.kind,
        code: wait_record.code,
    };
    syscall_log(format_args!(
        "[WAIT] pid={} kind={} code={}\n",
        wait_record.pid.0, wait_status.kind, wait_status.code
    ));
    let bytes = unsafe {
        core::slice::from_raw_parts(
            &wait_status as *const WaitStatus as *const u8,
            core::mem::size_of::<WaitStatus>(),
        )
    };

    if let Err(error) = copy_buffer_to_user(status as *mut u8, bytes) {
        return error;
    }

    wait_record.pid.0 as i64
}

fn sys_getcwd(buf: *mut u8, len: usize) -> i64 {
    if len == 0 {
        return EINVAL;
    }

    let cwd = match crate::process::current_process() {
        Some(process) => process.cwd.clone(),
        None => return EINVAL,
    };

    if cwd.len() >= len {
        return ENAMETOOLONG;
    }

    if let Err(error) = copy_buffer_to_user(buf, cwd.as_bytes()) {
        return error;
    }
    if let Err(error) = copy_buffer_to_user(unsafe { buf.add(cwd.len()) }, &[0]) {
        return error;
    }

    cwd.len() as i64
}

fn sys_yield() -> i64 {
    let current_pid = crate::process::current_tid()
        .map(|tid| crate::process::ProcessId(tid.0))
        .unwrap_or(crate::process::ProcessId(0));
    match crate::process::scheduler::pick_next_candidate_excluding(current_pid) {
        Some(pid) => match crate::process::save_current_user_context_for_yield(pid) {
            Ok(_) => USER_CONTEXT_RETURN_MAGIC,
            Err(error) => process_error_to_errno(error),
        },
        None => match crate::process::save_current_user_context_for_yield(current_pid) {
            Ok(_) => USER_CONTEXT_RETURN_MAGIC,
            Err(error) => process_error_to_errno(error),
        },
    }
}

fn sys_thread_create(entry: u64, stack_top: u64, arg: u64) -> i64 {
    if stack_top < 8 || stack_top & 15 != 0 {
        return EINVAL;
    }
    let return_slot = stack_top - 8;
    if let Err(error) = validate_user_mapping(return_slot, 8, true) {
        return error;
    }
    if let Err(error) = validate_user_mapping(entry, 1, false) {
        return error;
    }
    let entry_flags = crate::memory::vmm::active_user_page_flags(
        crate::memory::vmm::VirtualAddress::from_usize((entry & !0xfff) as usize),
    );
    match entry_flags {
        Ok(Some(flags)) if !flags.contains(crate::memory::vmm::PageFlags::NO_EXECUTE) => {}
        _ => return EACCES,
    }
    match crate::process::create_user_thread(entry, return_slot, arg) {
        Ok(tid) => {
            // A thread entry has the regular x86_64 call-frame alignment.
            // Returning without ThreadExit faults only this thread.
            unsafe { core::ptr::write(return_slot as *mut u64, 0); }
            tid.0 as i64
        }
        Err(error) => process_error_to_errno(error),
    }
}

fn sys_thread_join(tid: u64, status: *mut WaitStatus) -> i64 {
    if tid == 0 {
        return EINVAL;
    }
    if let Err(error) =
        validate_user_mapping(status as u64, core::mem::size_of::<WaitStatus>(), true)
    {
        return error;
    }
    let exit = match crate::process::wait_thread(crate::process::ThreadId(tid)) {
        Ok(exit) => exit,
        Err(error) => return process_error_to_errno(error),
    };
    let result = WaitStatus {
        kind: exit.kind_code(),
        code: exit.exit_code(),
    };
    unsafe { core::ptr::write_unaligned(status, result); }
    syscall_log!("[THREAD] joined tid={} kind={} code={}\r\n", tid, result.kind, result.code);
    tid as i64
}

fn sys_chdir(path: *const u8, path_len: usize) -> i64 {
    let path = match copy_string_from_user_len(path, path_len, MAX_USER_PATH) {
        Ok(path) => path,
        Err(error) => return error,
    };

    let cwd = match crate::process::current_process() {
        Some(process) => process.cwd.clone(),
        None => return EINVAL,
    };

    let normalized = match crate::fs::vfs::get_vfs().ok_or(EIO).and_then(|vfs| {
        let stat = vfs.stat_at(&cwd, &path).map_err(vfs_error_to_errno)?;
        if stat.file_type != crate::fs::vfs::FileType::Directory {
            return Err(ENOTDIR);
        }
        vfs.normalize_at(&cwd, &path).map_err(vfs_error_to_errno)
    }) {
        Ok(path) => path,
        Err(error) => return error,
    };

    match crate::process::current_process_mut() {
        Some(process) => {
            process.cwd = normalized;
            0
        }
        None => EINVAL,
    }
}

fn sys_get_pid() -> i64 {
    crate::process::current_process()
        .map(|process| process.pid.0 as i64)
        .unwrap_or(0)
}

fn sys_kill_process(pid: u32) -> i64 {
    if pid == 0 {
        return EINVAL;
    }
    match crate::process::kill_process(crate::process::ProcessId(pid as u64), -9) {
        Ok(true) => USER_CONTEXT_RETURN_MAGIC,
        Ok(false) => 0,
        Err(error) => process_error_to_errno(error),
    }
}

fn resolve_exec_path(cwd: &str, path: &str) -> Result<String, i64> {
    let vfs = crate::fs::vfs::get_vfs().ok_or(EIO)?;
    if path.contains('/') {
        let normalized = vfs.normalize_at(cwd, path).map_err(vfs_error_to_errno)?;
        let stat = vfs.stat_at("/", &normalized).map_err(vfs_error_to_errno)?;
        if stat.file_type == crate::fs::vfs::FileType::Directory {
            return Err(EISDIR);
        }
        return Ok(normalized);
    }

    let mut candidate = String::from(EXEC_PATH);
    candidate.push('/');
    candidate.push_str(path);
    let stat = vfs.stat_at("/", &candidate).map_err(vfs_error_to_errno)?;
    if stat.file_type == crate::fs::vfs::FileType::Directory {
        return Err(EISDIR);
    }
    Ok(candidate)
}

fn read_vfs_file(cwd: &str, path: &str) -> Result<Vec<u8>, crate::fs::vfs::VfsError> {
    let vfs = crate::fs::vfs::get_vfs().ok_or(crate::fs::vfs::VfsError::IoError)?;
    let fd = vfs.open_at(cwd, path, crate::fs::vfs::OpenFlags::READ)?;
    let mut data = Vec::new();
    let mut buf = [0u8; 512];

    loop {
        match vfs.read(fd, &mut buf) {
            Ok(0) => break,
            Ok(n) => data.extend_from_slice(&buf[..n]),
            Err(error) => {
                let _ = vfs.close(fd);
                return Err(error);
            }
        }
    }

    vfs.close(fd)?;
    Ok(data)
}

fn sys_sleep(ms: u64) -> i64 {
    if ms == 0 {
        return 0;
    }

    let deadline = crate::clock::Deadline::after_ms(ms).tick();
    match crate::process::block_current(None, Some(deadline)) {
        Ok(()) => USER_CONTEXT_RETURN_MAGIC,
        Err(error) => process_error_to_errno(error),
    }
}

/// Wait for an IPC event addressed to this process, with optional timeout.
/// Zero timeout means no deadline. The caller rechecks the queue on wake.
fn sys_wait_event(timeout_ms: u64) -> i64 {
    let Some(pid) = crate::process::current_process().map(|process| process.pid) else {
        return EINVAL;
    };
    if crate::ipc::get_ipc_manager().map(|manager| manager.has_messages(pid)).unwrap_or(false) {
        return 0;
    }
    let deadline = (timeout_ms != 0).then(|| crate::clock::Deadline::after_ms(timeout_ms).tick());
    match crate::process::block_current(Some(pid), deadline) {
        Ok(()) => USER_CONTEXT_RETURN_MAGIC,
        Err(error) => process_error_to_errno(error),
    }
}

/// Dunit-native futex wait. Parks the caller iff the u32 at `addr` still equals
/// `expected`, keyed by (owning process, user vaddr). `timeout_ms == 0` means no
/// deadline. Returns EAGAIN on value mismatch, EFAULT if the word is unmapped.
fn sys_futex_wait(addr: u64, expected: u64, timeout_ms: u64) -> i64 {
    if addr % 4 != 0 || !is_valid_user_pointer(addr, 4) {
        return EINVAL;
    }
    let deadline = (timeout_ms != 0).then(|| crate::clock::Deadline::after_ms(timeout_ms).tick());
    match crate::process::futex_wait(addr, expected as u32, deadline) {
        Ok(crate::process::FutexWaitOutcome::Parked) => USER_CONTEXT_RETURN_MAGIC,
        Ok(crate::process::FutexWaitOutcome::ValueMismatch) => EAGAIN,
        Err(crate::process::ProcessError::InvalidMemoryRange) => EFAULT,
        Err(error) => process_error_to_errno(error),
    }
}

/// Wake up to `count` futex waiters on `addr`. `count == 0` wakes all waiters.
fn sys_futex_wake(addr: u64, count: u64) -> i64 {
    if addr % 4 != 0 || !is_valid_user_pointer(addr, 4) {
        return EINVAL;
    }
    crate::process::futex_wake(addr, count as usize) as i64
}

fn sys_debug_log(code: u64) -> i64 {
    match code {
        #[cfg(feature = "boot-smoke-tests")]
        1 => {
            syscall_log!("[SYSCALL-TEST] userspace syscall OK\r\n");
            SYSCALL_SMOKE_OK.store(true, Ordering::SeqCst);
            0
        }
        #[cfg(feature = "boot-smoke-tests")]
        2 => {
            if user_fs_smoke_readback_ok() {
                SYSCALL_FS_SMOKE_OK.store(true, Ordering::SeqCst);
            }
            0
        }
        #[cfg(feature = "boot-smoke-tests")]
        3 => {
            SYSCALL_FS_SMOKE_OK.store(true, Ordering::SeqCst);
            SYSCALL_FS_SEMANTICS_OK.store(true, Ordering::SeqCst);
            0
        }
        #[cfg(feature = "boot-smoke-tests")]
        4 => {
            syscall_log!("[SYSCALL-FS-SEMANTICS-TEST] failed in userspace payload\r\n");
            0
        }
        _ => {
            syscall_log!("[SYSCALL] debug log code={}\r\n", code);
            0
        }
    }
}

#[cfg(feature = "boot-smoke-tests")]
fn sys_smoke_done(exit_code: i32) -> i64 {
    if let Some(pid) = crate::process::request_current_user_exit(exit_code) {
        syscall_log!(
            "[PROCESS-RUN] legacy smoke exit pid={} code={}\r\n",
            pid.0,
            exit_code
        );
        return USER_CONTEXT_RETURN_MAGIC;
    }

    if SYSCALL_SMOKE_OK.load(Ordering::SeqCst) {
        syscall_log!("[SYSCALL-TEST] userspace returned after syscall\r\n");
    } else {
        syscall_log!("[SYSCALL-TEST] smoke done before debug log\r\n");
    }

    USER_CONTEXT_RETURN_MAGIC
}

#[cfg(feature = "boot-smoke-tests")]
pub fn run_userspace_syscall_smoke() -> bool {
    SYSCALL_SMOKE_OK.store(false, Ordering::SeqCst);
    SYSCALL_FS_SMOKE_OK.store(false, Ordering::SeqCst);
    SYSCALL_FS_SEMANTICS_OK.store(false, Ordering::SeqCst);

    let entry = user_syscall_smoke_entry as *const () as usize;
    let stack_top = unsafe {
        let stack_base = USER_SMOKE_STACK.0.get() as *mut u8;
        stack_base.add(core::mem::size_of::<UserSmokeStack>()) as usize
    };

    unsafe {
        for offset in [0usize, 4096, 8192] {
            if crate::memory::vmm::mark_active_mapping_user(
                crate::memory::vmm::VirtualAddress::from_usize(entry + offset),
            )
            .is_err()
            {
                syscall_log!("[SYSCALL-TEST] failed to mark smoke entry user-accessible\r\n");
                return false;
            }
        }
        if crate::memory::vmm::mark_active_mapping_user(
            crate::memory::vmm::VirtualAddress::from_usize(stack_top - 1),
        )
        .is_err()
        {
            syscall_log!("[SYSCALL-TEST] failed to mark smoke stack user-accessible\r\n");
            return false;
        }
        if prepare_user_fs_smoke_page().is_err() {
            syscall_log!("[SYSCALL-FS-TEST] failed to prepare user smoke page\r\n");
            return false;
        }

        crate::hal::run_user_syscall_smoke(entry as u64, stack_top as u64);
    }

    let ok = SYSCALL_SMOKE_OK.load(Ordering::SeqCst);
    let fs_ok = SYSCALL_FS_SMOKE_OK.load(Ordering::SeqCst);
    let semantics_ok = SYSCALL_FS_SEMANTICS_OK.load(Ordering::SeqCst);
    if ok {
        syscall_log!("[SYSCALL-TEST] kernel resumed after userspace smoke\r\n");
    } else {
        syscall_log!("[SYSCALL-TEST] kernel resumed without OK marker\r\n");
    }
    if fs_ok {
        syscall_log!("[SYSCALL-FS-TEST] OK\r\n");
    } else {
        syscall_log!("[SYSCALL-FS-TEST] failed\r\n");
    }
    if semantics_ok {
        syscall_log!("[SYSCALL-FS-SEMANTICS-TEST] OK\r\n");
    } else {
        syscall_log!("[SYSCALL-FS-SEMANTICS-TEST] failed\r\n");
    }
    ok && fs_ok && semantics_ok
}

#[cfg(feature = "boot-smoke-tests")]
extern "C" fn user_syscall_smoke_entry() -> ! {
    unsafe {
        core::arch::asm!(
            "mov rax, 20",
            "mov rdi, 1",
            "syscall",

            // stdio stdout smoke path
            "mov rax, 4",
            "mov rdi, 1",
            "mov rsi, {stdout_buf}",
            "mov rdx, {stdout_len}",
            "syscall",
            "cmp rax, {stdout_len}",
            "jne 9f",

            // create/write/read/close success path
            "mov rax, 5",
            "mov rdi, {path}",
            "mov rsi, {path_len}",
            "mov rdx, {write_create_trunc}",
            "syscall",
            "test rax, rax",
            "js 9f",
            "mov r12, rax",
            "mov rax, 4",
            "mov rdi, r12",
            "mov rsi, {write_buf}",
            "mov rdx, {data_len}",
            "syscall",
            "cmp rax, {data_len}",
            "jne 9f",
            "mov rax, 6",
            "mov rdi, r12",
            "syscall",
            "test rax, rax",
            "js 9f",
            "mov rax, 5",
            "mov rdi, {path}",
            "mov rsi, {path_len}",
            "mov rdx, {read}",
            "syscall",
            "test rax, rax",
            "js 9f",
            "mov r12, rax",
            "mov rax, 3",
            "mov rdi, r12",
            "mov rsi, {read_buf}",
            "mov rdx, {data_len}",
            "syscall",
            "cmp rax, {data_len}",
            "jne 9f",
            "mov rax, 6",
            "mov rdi, r12",
            "syscall",
            "mov r14, {read_buf}",
            "cmp byte ptr [r14], 104",
            "jne 9f",
            "cmp byte ptr [r14 + 1], 101",
            "jne 9f",
            "cmp byte ptr [r14 + 2], 108",
            "jne 9f",
            "cmp byte ptr [r14 + 3], 108",
            "jne 9f",
            "cmp byte ptr [r14 + 4], 111",
            "jne 9f",

            // read from write-only must fail
            "mov rax, 5",
            "mov rdi, {path}",
            "mov rsi, {path_len}",
            "mov rdx, {write}",
            "syscall",
            "test rax, rax",
            "js 9f",
            "mov r12, rax",
            "mov rax, 3",
            "mov rdi, r12",
            "mov rsi, {read_buf}",
            "mov rdx, 1",
            "syscall",
            "test rax, rax",
            "jns 9f",
            "mov rax, 6",
            "mov rdi, r12",
            "syscall",

            // write to read-only must fail
            "mov rax, 5",
            "mov rdi, {path}",
            "mov rsi, {path_len}",
            "mov rdx, {read}",
            "syscall",
            "test rax, rax",
            "js 9f",
            "mov r12, rax",
            "mov rax, 4",
            "mov rdi, r12",
            "mov rsi, {write_buf}",
            "mov rdx, 1",
            "syscall",
            "test rax, rax",
            "jns 9f",
            "mov rax, 6",
            "mov rdi, r12",
            "syscall",

            // truncate clears old content
            "mov rax, 5",
            "mov rdi, {path}",
            "mov rsi, {path_len}",
            "mov rdx, {write_trunc}",
            "syscall",
            "test rax, rax",
            "js 9f",
            "mov r12, rax",
            "mov rax, 6",
            "mov rdi, r12",
            "syscall",
            "mov rax, 5",
            "mov rdi, {path}",
            "mov rsi, {path_len}",
            "mov rdx, {read}",
            "syscall",
            "test rax, rax",
            "js 9f",
            "mov r12, rax",
            "mov rax, 3",
            "mov rdi, r12",
            "mov rsi, {read_buf}",
            "mov rdx, {data_len}",
            "syscall",
            "test rax, rax",
            "jne 9f",
            "mov rax, 6",
            "mov rdi, r12",
            "syscall",

            // append writes at EOF on every write
            "mov rax, 5",
            "mov rdi, {path}",
            "mov rsi, {path_len}",
            "mov rdx, {write_create_trunc}",
            "syscall",
            "test rax, rax",
            "js 9f",
            "mov r12, rax",
            "mov rax, 4",
            "mov rdi, r12",
            "mov rsi, {append_a}",
            "mov rdx, 1",
            "syscall",
            "cmp rax, 1",
            "jne 9f",
            "mov rax, 6",
            "mov rdi, r12",
            "syscall",
            "mov rax, 5",
            "mov rdi, {path}",
            "mov rsi, {path_len}",
            "mov rdx, {write_append}",
            "syscall",
            "test rax, rax",
            "js 9f",
            "mov r12, rax",
            "mov rax, 4",
            "mov rdi, r12",
            "mov rsi, {append_b}",
            "mov rdx, 1",
            "syscall",
            "cmp rax, 1",
            "jne 9f",
            "mov rax, 6",
            "mov rdi, r12",
            "syscall",
            "mov rax, 5",
            "mov rdi, {path}",
            "mov rsi, {path_len}",
            "mov rdx, {read}",
            "syscall",
            "test rax, rax",
            "js 9f",
            "mov r12, rax",
            "mov rax, 3",
            "mov rdi, r12",
            "mov rsi, {read_buf}",
            "mov rdx, 2",
            "syscall",
            "cmp rax, 2",
            "jne 9f",
            "mov rax, 6",
            "mov rdi, r12",
            "syscall",
            "mov r14, {read_buf}",
            "cmp byte ptr [r14], 65",
            "jne 9f",
            "cmp byte ptr [r14 + 1], 66",
            "jne 9f",

            // invalid close must fail
            "mov rax, 6",
            "mov rdi, 999",
            "syscall",
            "test rax, rax",
            "jns 9f",

            "mov rax, 20",
            "mov rdi, 3",
            "syscall",
            "jmp 8f",
            "9:",
            "mov rax, 20",
            "mov rdi, 4",
            "syscall",
            "8:",
            "mov rax, 21",
            "syscall",
            "2:",
            "pause",
            "jmp 2b",
            path = const SMOKE_USER_PATH,
            path_len = const SMOKE_FS_PATH.len(),
            write_buf = const SMOKE_USER_WRITE,
            read_buf = const SMOKE_USER_READ,
            data_len = const SMOKE_FS_DATA.len(),
            append_a = const SMOKE_USER_APPEND_A,
            append_b = const SMOKE_USER_APPEND_B,
            stdout_buf = const SMOKE_USER_STDOUT,
            stdout_len = const SMOKE_STDOUT_DATA.len(),
            read = const crate::fs::vfs::OpenFlags::READ.bits(),
            write = const crate::fs::vfs::OpenFlags::WRITE.bits(),
            write_trunc = const (crate::fs::vfs::OpenFlags::WRITE.bits()
                | crate::fs::vfs::OpenFlags::TRUNC.bits()),
            write_append = const (crate::fs::vfs::OpenFlags::WRITE.bits()
                | crate::fs::vfs::OpenFlags::APPEND.bits()),
            write_create_trunc = const (crate::fs::vfs::OpenFlags::WRITE.bits()
                | crate::fs::vfs::OpenFlags::CREATE.bits()
                | crate::fs::vfs::OpenFlags::TRUNC.bits()),
            options(noreturn)
        );
    }
}

#[cfg(feature = "boot-smoke-tests")]
unsafe fn prepare_user_fs_smoke_page() -> Result<(), ()> {
    let hhdm = crate::memory::vmm::get_hhdm_offset() as usize;
    if hhdm == 0 {
        return Err(());
    }

    let pmm = crate::memory::pmm::get_pmm().ok_or(())?;
    let page_frame = pmm.alloc_frame().ok_or(())?.as_usize();
    let page = (page_frame + hhdm) as *mut u8;

    core::ptr::write_bytes(page, 0, 4096);
    core::ptr::copy_nonoverlapping(SMOKE_FS_PATH.as_ptr(), page.add(0), SMOKE_FS_PATH.len());
    core::ptr::copy_nonoverlapping(SMOKE_FS_DATA.as_ptr(), page.add(64), SMOKE_FS_DATA.len());
    core::ptr::copy_nonoverlapping(SMOKE_APPEND_A.as_ptr(), page.add(192), SMOKE_APPEND_A.len());
    core::ptr::copy_nonoverlapping(SMOKE_APPEND_B.as_ptr(), page.add(256), SMOKE_APPEND_B.len());
    core::ptr::copy_nonoverlapping(
        SMOKE_STDOUT_DATA.as_ptr(),
        page.add(320),
        SMOKE_STDOUT_DATA.len(),
    );

    crate::memory::vmm::map_active_user_frame(
        crate::memory::vmm::VirtualAddress::from_usize(SMOKE_USER_PAGE),
        crate::memory::pmm::PhysicalAddress(page_frame),
        crate::memory::vmm::PageFlags::WRITABLE,
    )
    .map_err(|_| ())?;
    Ok(())
}

#[cfg(feature = "boot-smoke-tests")]
fn user_fs_smoke_readback_ok() -> bool {
    for (offset, expected) in SMOKE_FS_DATA.iter().enumerate() {
        let actual = unsafe { core::ptr::read_volatile((SMOKE_USER_READ + offset) as *const u8) };
        if actual != *expected {
            syscall_log!(
                "[SYSCALL-FS-TEST] readback mismatch at {}: got {}, expected {}\r\n",
                offset,
                actual,
                expected
            );
            return false;
        }
    }
    true
}
