use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write;
use core::sync::atomic::{AtomicBool, Ordering};

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
    SmokeDone = 21,
    GetCwd = 22,
    Chdir = 23,
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
            21 => Some(Syscall::SmokeDone),
            22 => Some(Syscall::GetCwd),
            23 => Some(Syscall::Chdir),
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

pub static mut KERNEL_FB_ADDR: u64 = 0;
pub static mut KERNEL_FB_WIDTH: u32 = 0;
pub static mut KERNEL_FB_HEIGHT: u32 = 0;
pub static mut KERNEL_FB_PITCH: u32 = 0;

#[repr(C)]
pub struct FbInfo {
    pub addr: u64,
    pub width: u32,
    pub height: u32,
    pub pitch: u32,
}

const USER_SPACE_START: u64 = 0x0000_0000_0000_0000;
const USER_SPACE_END: u64 = 0x0000_7FFF_FFFF_FFFF;
const MAX_FD: u32 = 1024;
const MAX_USER_COPY: usize = 64 * 1024;
const MAX_USER_PATH: usize = 256;
const SMOKE_RETURN_MAGIC: i64 = 0x0051_5953_4341_4C4C;
const PAGE_PRESENT: u64 = 1 << 0;
const PAGE_WRITABLE: u64 = 1 << 1;
const PAGE_USER: u64 = 1 << 2;
const PAGE_HUGE: u64 = 1 << 7;
const SMOKE_USER_PAGE: usize = 0x0000_0000_0040_0000;
const SMOKE_USER_PATH: usize = SMOKE_USER_PAGE;
const SMOKE_USER_WRITE: usize = SMOKE_USER_PAGE + 64;
const SMOKE_USER_READ: usize = SMOKE_USER_PAGE + 128;
const SMOKE_USER_APPEND_A: usize = SMOKE_USER_PAGE + 192;
const SMOKE_USER_APPEND_B: usize = SMOKE_USER_PAGE + 256;
const SMOKE_USER_STDOUT: usize = SMOKE_USER_PAGE + 320;
const SMOKE_FS_PATH: &[u8] = b"/tmp/syscall-smoke.txt";
const SMOKE_FS_DATA: &[u8] = b"hello";
const SMOKE_APPEND_A: &[u8] = b"A";
const SMOKE_APPEND_B: &[u8] = b"B";
const SMOKE_STDOUT_DATA: &[u8] = b"[STDOUT-TEST] hello from userspace\n";
const EXEC_PATH: &str = "/app";

static SYSCALL_SMOKE_OK: AtomicBool = AtomicBool::new(false);
static SYSCALL_FS_SMOKE_OK: AtomicBool = AtomicBool::new(false);
static SYSCALL_FS_SEMANTICS_OK: AtomicBool = AtomicBool::new(false);
static ELF_TEST_RUNNING: AtomicBool = AtomicBool::new(false);
static ELF_TEST_RETURNED: AtomicBool = AtomicBool::new(false);

#[repr(C)]
#[derive(Clone, Copy)]
pub struct WaitStatus {
    pub kind: i32,
    pub code: i32,
}

#[repr(align(4096))]
struct UserSmokeStack([u8; 4096]);

static mut USER_SMOKE_STACK: UserSmokeStack = UserSmokeStack([0; 4096]);

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
        syscall_log!("[SYSCALL] invalid user pointer: ptr={:#x}, len={}\r\n", ptr, size);
        return Err(EFAULT);
    }

    Ok(())
}

/// Early user copy validation is range-only until Dunit grows per-process
/// address spaces and recoverable page-fault handling for kernel copies.
pub fn user_copy_is_range_checked_only() -> bool {
    true
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

    validate_user_range(ptr as u64, max_len)?;

    let mut out = String::new();
    for offset in 0..max_len {
        let byte = unsafe { core::ptr::read_volatile(ptr.add(offset)) };
        if byte == 0 {
            return Ok(out);
        }
        out.push(byte as char);
    }

    syscall_log!("[SYSCALL] unterminated user string: ptr={:#x}, max_len={}\r\n", ptr as u64, max_len);
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
        syscall_log!("[SYSCALL] user string too large: len={}, max={}\r\n", len, max_len);
        return Err(ENAMETOOLONG);
    }

    validate_user_range(ptr as u64, len)?;

    let mut out = String::new();
    for offset in 0..len {
        let byte = unsafe { core::ptr::read_volatile(ptr.add(offset)) };
        if byte == 0 {
            return Err(EINVAL);
        }
        out.push(byte as char);
    }

    Ok(out)
}

pub fn copy_buffer_from_user(ptr: *const u8, len: usize) -> Result<Vec<u8>, i64> {
    if len == 0 {
        return Ok(Vec::new());
    }

    validate_user_range(ptr as u64, len)?;

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

    validate_user_range(ptr as u64, data.len())?;

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
        Syscall::SendMessage => sys_send_message(arg0 as u32, arg1 as *const u8),
        Syscall::ReceiveMessage => sys_receive_message(arg0 as *mut u8),
        Syscall::GetFramebuffer => sys_get_framebuffer(arg0 as *mut FbInfo),
        Syscall::DrawPixel => sys_draw_pixel(arg0 as u32, arg1 as u32, arg2 as u32),
        Syscall::DrawRect => sys_draw_rect(arg0 as u32, arg1 as u32, arg2 as u32, arg3 as u32, arg4 as u32),
        Syscall::GetKey => sys_get_key(),
        Syscall::GetMousePos => sys_get_mouse_pos(arg0 as *mut u32, arg1 as *mut u32),
        Syscall::SpawnProcess => sys_spawn_process(arg0 as *const u8, arg1 as usize),
        Syscall::WaitProcess => sys_wait_process(arg0 as u32, arg1 as *mut WaitStatus),
        Syscall::GetPid => sys_get_pid(),
        Syscall::KillProcess => sys_kill_process(arg0 as u32),
        Syscall::Sleep => sys_sleep(arg0),
        Syscall::DebugLog => sys_debug_log(arg0),
        Syscall::SmokeDone => sys_smoke_done(arg0 as i32),
        Syscall::GetCwd => sys_getcwd(arg0 as *mut u8, arg1 as usize),
        Syscall::Chdir => sys_chdir(arg0 as *const u8, arg1 as usize),
    }
}

fn sys_exit(code: i32) -> i64 {
    if let Some(pid) = crate::process::request_current_user_exit(code) {
        syscall_log!("[PROCESS-RUN] exited pid={} code={}\r\n", pid.0, code);
        return SMOKE_RETURN_MAGIC;
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
        Some(crate::process::FdTarget::Stdin) => return 0,
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

    let bytes_read = match crate::fs::vfs::get_vfs()
        .ok_or(EIO)
        .and_then(|vfs| vfs.read(vfs_fd, &mut kernel_buf).map_err(vfs_error_to_errno))
    {
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
            return data.len() as i64;
        }
        Some(crate::process::FdTarget::Stderr) => {
            write_stdio("STDERR", &data);
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

fn sys_mmap(addr: usize, length: usize, _prot: u32, _flags: u32) -> i64 {
    if length == 0 {
        return EINVAL;
    }
    
    if addr != 0 && !is_valid_user_pointer(addr as u64, length) {
        return EINVAL;
    }
    
    ENOSYS
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
        crate::process::ProcessError::InvalidFd => EBADF,
        crate::process::ProcessError::FdTableFull => ENFILE,
        crate::process::ProcessError::NoAddressSpace
        | crate::process::ProcessError::AddressSpaceCreateFailed
        | crate::process::ProcessError::NoKernelStack
        | crate::process::ProcessError::InvalidUserContext => EINVAL,
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

fn sys_send_message(target_pid: u32, msg: *const u8) -> i64 {
    if target_pid == 0 {
        return EINVAL;
    }
    
    if let Err(error) = copy_buffer_from_user(msg, 256).map(|_| ()) {
        return error;
    }
    
    ENOSYS
}

fn sys_receive_message(msg: *mut u8) -> i64 {
    let empty = [0u8; 256];
    if let Err(error) = copy_buffer_to_user(msg, &empty) {
        return error;
    }
    
    ENOSYS
}

fn sys_get_framebuffer(info: *mut FbInfo) -> i64 {
    unsafe {
        if KERNEL_FB_ADDR == 0 {
            return EINVAL;
        }
        let fb = FbInfo {
            addr: KERNEL_FB_ADDR,
            width: KERNEL_FB_WIDTH,
            height: KERNEL_FB_HEIGHT,
            pitch: KERNEL_FB_PITCH,
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

fn sys_draw_pixel(x: u32, y: u32, color: u32) -> i64 {
    unsafe {
        if KERNEL_FB_ADDR == 0 {
            return EINVAL;
        }
        let fb = KERNEL_FB_ADDR as *mut u32;
        let w = KERNEL_FB_WIDTH as usize;
        let h = KERNEL_FB_HEIGHT as usize;
        if (x as usize) < w && (y as usize) < h {
            let pitch_pixels = KERNEL_FB_PITCH as usize / 4;
            core::ptr::write_volatile(fb.add(y as usize * pitch_pixels + x as usize), color);
        }
    }
    0
}

fn sys_draw_rect(x: u32, y: u32, w: u32, h: u32, color: u32) -> i64 {
    unsafe {
        if KERNEL_FB_ADDR == 0 {
            return EINVAL;
        }
        let fb = KERNEL_FB_ADDR as *mut u32;
        let fb_w = KERNEL_FB_WIDTH as usize;
        let fb_h = KERNEL_FB_HEIGHT as usize;
        let pitch_pixels = KERNEL_FB_PITCH as usize / 4;
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

fn sys_get_key() -> i64 {
    if let Some(sc) = crate::drivers::keyboard::read_scancode() {
        sc as i64
    } else {
        -1
    }
}

fn sys_get_mouse_pos(x: *mut u32, y: *mut u32) -> i64 {
    let (mx, my) = crate::drivers::mouse::get_position();
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

    let pid = crate::process::create_process_record(resolved.clone(), true);
    syscall_log(format_args!(
        "[SPAWN] prepared pid={} path={} execution=not-started\n",
        pid.0,
        resolved
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

fn sys_chdir(path: *const u8, path_len: usize) -> i64 {
    let path = match copy_string_from_user_len(path, path_len, MAX_USER_PATH) {
        Ok(path) => path,
        Err(error) => return error,
    };

    let cwd = match crate::process::current_process() {
        Some(process) => process.cwd.clone(),
        None => return EINVAL,
    };

    let normalized = match crate::fs::vfs::get_vfs()
        .ok_or(EIO)
        .and_then(|vfs| {
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

fn sys_kill_process(_pid: u32) -> i64 {
    ENOSYS
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
    let iters = ms * 1000;
    for _ in 0..iters {
        unsafe { core::arch::asm!("pause"); }
    }
    0
}

fn sys_debug_log(code: u64) -> i64 {
    match code {
        1 => {
            syscall_log!("[SYSCALL-TEST] userspace syscall OK\r\n");
            SYSCALL_SMOKE_OK.store(true, Ordering::SeqCst);
            0
        }
        2 => {
            if user_fs_smoke_readback_ok() {
                SYSCALL_FS_SMOKE_OK.store(true, Ordering::SeqCst);
            }
            0
        }
        3 => {
            SYSCALL_FS_SMOKE_OK.store(true, Ordering::SeqCst);
            SYSCALL_FS_SEMANTICS_OK.store(true, Ordering::SeqCst);
            0
        }
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

fn sys_smoke_done(exit_code: i32) -> i64 {
    if let Some(pid) = crate::process::request_current_user_exit(exit_code) {
        syscall_log!(
            "[PROCESS-RUN] legacy smoke exit pid={} code={}\r\n",
            pid.0,
            exit_code
        );
        return SMOKE_RETURN_MAGIC;
    }

    if SYSCALL_SMOKE_OK.load(Ordering::SeqCst) {
        syscall_log!("[SYSCALL-TEST] userspace returned after syscall\r\n");
    } else {
        syscall_log!("[SYSCALL-TEST] smoke done before debug log\r\n");
    }

    SMOKE_RETURN_MAGIC
}

pub fn run_userspace_syscall_smoke() -> bool {
    SYSCALL_SMOKE_OK.store(false, Ordering::SeqCst);
    SYSCALL_FS_SMOKE_OK.store(false, Ordering::SeqCst);
    SYSCALL_FS_SEMANTICS_OK.store(false, Ordering::SeqCst);

    let entry = user_syscall_smoke_entry as *const () as usize;
    let stack_top = unsafe {
        let stack_base = core::ptr::addr_of_mut!(USER_SMOKE_STACK.0) as *mut u8;
        stack_base.add(core::mem::size_of::<UserSmokeStack>()) as usize
    };

    unsafe {
        for offset in [0usize, 4096, 8192] {
            if mark_current_mapping_user(entry + offset).is_err() {
                syscall_log!("[SYSCALL-TEST] failed to mark smoke entry user-accessible\r\n");
                return false;
            }
        }
        if mark_current_mapping_user(stack_top - 1).is_err() {
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

pub fn begin_elf_test() {
    ELF_TEST_RETURNED.store(false, Ordering::SeqCst);
    ELF_TEST_RUNNING.store(true, Ordering::SeqCst);
}

pub fn finish_elf_test() -> bool {
    ELF_TEST_RUNNING.store(false, Ordering::SeqCst);
    ELF_TEST_RETURNED.load(Ordering::SeqCst)
}

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
    core::ptr::copy_nonoverlapping(SMOKE_STDOUT_DATA.as_ptr(), page.add(320), SMOKE_STDOUT_DATA.len());

    map_current_user_page(SMOKE_USER_PAGE, page_frame)?;
    Ok(())
}

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

unsafe fn map_current_user_page(virt: usize, phys: usize) -> Result<(), ()> {
    if virt & 0xfff != 0 || phys & 0xfff != 0 {
        return Err(());
    }

    let hhdm = crate::memory::vmm::get_hhdm_offset() as usize;
    if hhdm == 0 {
        return Err(());
    }

    let mut cr3: usize;
    core::arch::asm!("mov {}, cr3", out(reg) cr3, options(nomem, nostack, preserves_flags));
    let pml4 = ((cr3 & !0xfff) + hhdm) as *mut u64;

    let p4 = (virt >> 39) & 0x1ff;
    let p3 = (virt >> 30) & 0x1ff;
    let p2 = (virt >> 21) & 0x1ff;
    let p1 = (virt >> 12) & 0x1ff;

    let pdpt = ensure_next_table(pml4.add(p4), hhdm)?;
    let pd = ensure_next_table(pdpt.add(p3), hhdm)?;
    let pt = ensure_next_table(pd.add(p2), hhdm)?;
    let pte = pt.add(p1);

    core::ptr::write_volatile(pte, (phys as u64) | PAGE_PRESENT | PAGE_WRITABLE | PAGE_USER);
    flush_user_mapping(virt);
    Ok(())
}

unsafe fn ensure_next_table(entry: *mut u64, hhdm: usize) -> Result<*mut u64, ()> {
    let mut value = core::ptr::read_volatile(entry);
    if value & PAGE_PRESENT != 0 {
        if value & PAGE_HUGE != 0 {
            return Err(());
        }
        if value & PAGE_USER == 0 || value & PAGE_WRITABLE == 0 {
            value |= PAGE_USER | PAGE_WRITABLE;
            core::ptr::write_volatile(entry, value);
        }
        return Ok((((value as usize) & !0xfff) + hhdm) as *mut u64);
    }

    let pmm = crate::memory::pmm::get_pmm().ok_or(())?;
    let frame = pmm.alloc_frame().ok_or(())?.as_usize();
    let table = (frame + hhdm) as *mut u64;
    core::ptr::write_bytes(table as *mut u8, 0, 4096);
    core::ptr::write_volatile(entry, (frame as u64) | PAGE_PRESENT | PAGE_WRITABLE | PAGE_USER);
    Ok(table)
}

unsafe fn mark_current_mapping_user(virt: usize) -> Result<(), ()> {
    let hhdm = crate::memory::vmm::get_hhdm_offset() as usize;
    if hhdm == 0 {
        return Err(());
    }

    let mut cr3: usize;
    core::arch::asm!("mov {}, cr3", out(reg) cr3, options(nomem, nostack, preserves_flags));
    let pml4 = ((cr3 & !0xfff) + hhdm) as *mut u64;

    let p4 = (virt >> 39) & 0x1ff;
    let p3 = (virt >> 30) & 0x1ff;
    let p2 = (virt >> 21) & 0x1ff;
    let p1 = (virt >> 12) & 0x1ff;

    let pml4e = pml4.add(p4);
    set_user_bit(pml4e)?;
    let pdpt = (((*pml4e as usize) & !0xfff) + hhdm) as *mut u64;

    let pdpte = pdpt.add(p3);
    set_user_bit(pdpte)?;
    if *pdpte & PAGE_HUGE != 0 {
        flush_user_mapping(virt);
        return Ok(());
    }

    let pd = (((*pdpte as usize) & !0xfff) + hhdm) as *mut u64;
    let pde = pd.add(p2);
    set_user_bit(pde)?;
    if *pde & PAGE_HUGE != 0 {
        flush_user_mapping(virt);
        return Ok(());
    }

    let pt = (((*pde as usize) & !0xfff) + hhdm) as *mut u64;
    let pte = pt.add(p1);
    set_user_bit(pte)?;
    flush_user_mapping(virt);
    Ok(())
}

unsafe fn set_user_bit(entry: *mut u64) -> Result<(), ()> {
    let value = core::ptr::read_volatile(entry);
    if value & PAGE_PRESENT == 0 {
        return Err(());
    }
    core::ptr::write_volatile(entry, value | PAGE_USER);
    Ok(())
}

unsafe fn flush_user_mapping(virt: usize) {
    core::arch::asm!("invlpg [{}]", in(reg) virt, options(nostack, preserves_flags));
}
