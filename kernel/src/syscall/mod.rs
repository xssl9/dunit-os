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
            _ => None,
        }
    }
}

pub const EFAULT: i64 = -14;
pub const EINVAL: i64 = -22;
pub const EBADF: i64 = -9;
pub const ENOSYS: i64 = -38;

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
        Syscall::GetFramebuffer => sys_get_framebuffer(arg1 as *mut FbInfo),
        Syscall::DrawPixel => sys_draw_pixel(arg1 as u32, arg2 as u32, arg3 as u32),
        Syscall::DrawRect => sys_draw_rect(arg1 as u32, arg2 as u32, arg3 as u32, arg4 as u32, _arg5 as u32),
        Syscall::GetKey => sys_get_key(),
        Syscall::GetMousePos => sys_get_mouse_pos(arg1 as *mut u32, arg2 as *mut u32),
        Syscall::SpawnProcess => sys_spawn_process(arg1 as *const u8),
        Syscall::WaitProcess => sys_wait_process(arg1 as u32),
        Syscall::GetPid => sys_get_pid(),
        Syscall::KillProcess => sys_kill_process(arg1 as u32),
        Syscall::Sleep => sys_sleep(arg1),
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

fn sys_get_framebuffer(info: *mut FbInfo) -> i64 {
    if !is_valid_user_pointer(info as u64, core::mem::size_of::<FbInfo>()) {
        return EFAULT;
    }
    unsafe {
        if KERNEL_FB_ADDR == 0 {
            return EINVAL;
        }
        let fb = &mut *info;
        fb.addr = KERNEL_FB_ADDR;
        fb.width = KERNEL_FB_WIDTH;
        fb.height = KERNEL_FB_HEIGHT;
        fb.pitch = KERNEL_FB_PITCH;
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
    if !is_valid_user_pointer(x as u64, 4) || !is_valid_user_pointer(y as u64, 4) {
        return EFAULT;
    }
    let (mx, my) = crate::drivers::mouse::get_position();
    unsafe {
        *x = mx as u32;
        *y = my as u32;
    }
    0
}

fn sys_spawn_process(_path: *const u8) -> i64 {
    ENOSYS
}

fn sys_wait_process(_pid: u32) -> i64 {
    ENOSYS
}

fn sys_get_pid() -> i64 {
    0
}

fn sys_kill_process(_pid: u32) -> i64 {
    ENOSYS
}

fn sys_sleep(ms: u64) -> i64 {
    let iters = ms * 1000;
    for _ in 0..iters {
        unsafe { core::arch::asm!("pause"); }
    }
    0
}
