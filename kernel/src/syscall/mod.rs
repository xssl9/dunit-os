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
            _ => None,
        }
    }
}

pub const EFAULT: i64 = -14;
pub const EINVAL: i64 = -22;
pub const EBADF: i64 = -9;
pub const ENOSYS: i64 = -38;

const USER_SPACE_START: u64 = 0x0000_0000_0000_0000;
const USER_SPACE_END: u64 = 0x0000_7FFF_FFFF_FFFF;
const MAX_FD: u32 = 1024;

fn is_valid_user_pointer(ptr: u64, size: usize) -> bool {
    if ptr == 0 {
        return false;
    }
    
    let end = ptr.saturating_add(size as u64);
    
    if end < ptr {
        return false;
    }
    
    ptr >= USER_SPACE_START && end <= USER_SPACE_END
}

fn is_valid_fd(fd: u32) -> bool {
    fd < MAX_FD
}

fn is_valid_string_pointer(ptr: *const u8) -> bool {
    if ptr.is_null() {
        return false;
    }
    
    let addr = ptr as u64;
    is_valid_user_pointer(addr, 1)
}

#[no_mangle]
pub extern "C" fn syscall_handler(
    syscall_num: u64,
    arg1: u64,
    arg2: u64,
    arg3: u64,
    arg4: u64,
    _arg5: u64,
) -> i64 {
    let syscall = match Syscall::from_u64(syscall_num) {
        Some(s) => s,
        None => return ENOSYS,
    };

    match syscall {
        Syscall::Exit => sys_exit(arg1 as i32),
        Syscall::Fork => sys_fork(),
        Syscall::Exec => sys_exec(arg1 as *const u8),
        Syscall::Read => sys_read(arg1 as u32, arg2 as *mut u8, arg3 as usize),
        Syscall::Write => sys_write(arg1 as u32, arg2 as *const u8, arg3 as usize),
        Syscall::Open => sys_open(arg1 as *const u8, arg2 as u32),
        Syscall::Close => sys_close(arg1 as u32),
        Syscall::Mmap => sys_mmap(arg1 as usize, arg2 as usize, arg3 as u32, arg4 as u32),
        Syscall::SendMessage => sys_send_message(arg1 as u32, arg2 as *const u8),
        Syscall::ReceiveMessage => sys_receive_message(arg1 as *mut u8),
    }
}

fn sys_exit(_code: i32) -> i64 {
    0
}

fn sys_fork() -> i64 {
    ENOSYS
}

fn sys_exec(path: *const u8) -> i64 {
    if !is_valid_string_pointer(path) {
        return EFAULT;
    }
    ENOSYS
}

fn sys_read(fd: u32, buf: *mut u8, count: usize) -> i64 {
    if !is_valid_fd(fd) {
        return EBADF;
    }
    
    if !is_valid_user_pointer(buf as u64, count) {
        return EFAULT;
    }
    
    if count == 0 {
        return 0;
    }
    
    ENOSYS
}

fn sys_write(fd: u32, buf: *const u8, count: usize) -> i64 {
    if !is_valid_fd(fd) {
        return EBADF;
    }
    
    if !is_valid_user_pointer(buf as u64, count) {
        return EFAULT;
    }
    
    if count == 0 {
        return 0;
    }
    
    ENOSYS
}

fn sys_open(path: *const u8, _flags: u32) -> i64 {
    if !is_valid_string_pointer(path) {
        return EFAULT;
    }
    ENOSYS
}

fn sys_close(fd: u32) -> i64 {
    if !is_valid_fd(fd) {
        return EBADF;
    }
    ENOSYS
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

fn sys_send_message(target_pid: u32, msg: *const u8) -> i64 {
    if target_pid == 0 {
        return EINVAL;
    }
    
    if !is_valid_user_pointer(msg as u64, 256) {
        return EFAULT;
    }
    
    ENOSYS
}

fn sys_receive_message(msg: *mut u8) -> i64 {
    if !is_valid_user_pointer(msg as u64, 256) {
        return EFAULT;
    }
    
    ENOSYS
}
