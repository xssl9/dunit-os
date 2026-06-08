#![no_std]

pub const SYSCALL_EXIT: usize = 0;
pub const SYSCALL_READ: usize = 3;
pub const SYSCALL_WRITE: usize = 4;
pub const SYSCALL_GET_FRAMEBUFFER: usize = 10;
pub const SYSCALL_DRAW_PIXEL: usize = 11;
pub const SYSCALL_DRAW_RECT: usize = 12;
pub const SYSCALL_GET_KEY: usize = 13;
pub const SYSCALL_GET_MOUSE_POS: usize = 14;
pub const SYSCALL_SPAWN_PROCESS: usize = 15;
pub const SYSCALL_GET_PID: usize = 17;
pub const SYSCALL_KILL_PROCESS: usize = 18;
pub const SYSCALL_SLEEP: usize = 19;

#[repr(C)]
pub struct FbInfo {
    pub addr: u64,
    pub width: u32,
    pub height: u32,
    pub pitch: u32,
}

#[inline(always)]
pub fn syscall0(num: usize) -> isize {
    let ret: isize;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") num,
            lateout("rax") ret,
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
            in("rdi") a1,
            lateout("rax") ret,
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
            in("rdi") a1,
            in("rsi") a2,
            in("rdx") a3,
            lateout("rax") ret,
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
            in("rdi") a1,
            in("rsi") a2,
            in("rdx") a3,
            in("r10") a4,
            in("r8") a5,
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

pub fn print(s: &str) {
    write(1, s.as_bytes());
}

pub fn get_framebuffer(info: &mut FbInfo) -> bool {
    syscall1(SYSCALL_GET_FRAMEBUFFER, info as *mut FbInfo as usize) == 0
}

pub fn draw_pixel(x: u32, y: u32, color: u32) {
    syscall3(SYSCALL_DRAW_PIXEL, x as usize, y as usize, color as usize);
}

pub fn draw_rect(x: u32, y: u32, w: u32, h: u32, color: u32) {
    syscall5(SYSCALL_DRAW_RECT, x as usize, y as usize, w as usize, h as usize, color as usize);
}

pub fn get_key() -> Option<u8> {
    let k = syscall0(SYSCALL_GET_KEY);
    if k < 0 { None } else { Some(k as u8) }
}

pub fn get_mouse_pos() -> (u32, u32) {
    let mut x: u32 = 0;
    let mut y: u32 = 0;
    syscall3(SYSCALL_GET_MOUSE_POS, &mut x as *mut u32 as usize, &mut y as *mut u32 as usize, 0);
    (x, y)
}

pub fn sleep_ms(ms: u64) {
    syscall1(SYSCALL_SLEEP, ms as usize);
}

pub fn get_pid() -> u32 {
    syscall0(SYSCALL_GET_PID) as u32
}

pub fn kill(pid: u32) {
    syscall1(SYSCALL_KILL_PROCESS, pid as usize);
}

pub fn spawn(path: &str) -> isize {
    syscall1(SYSCALL_SPAWN_PROCESS, path.as_ptr() as usize)
}

pub fn scancode_to_char(sc: u8) -> Option<char> {
    match sc {
        0x1E => Some('a'), 0x30 => Some('b'), 0x2E => Some('c'),
        0x20 => Some('d'), 0x12 => Some('e'), 0x21 => Some('f'),
        0x22 => Some('g'), 0x23 => Some('h'), 0x17 => Some('i'),
        0x24 => Some('j'), 0x25 => Some('k'), 0x26 => Some('l'),
        0x32 => Some('m'), 0x31 => Some('n'), 0x18 => Some('o'),
        0x19 => Some('p'), 0x10 => Some('q'), 0x13 => Some('r'),
        0x1F => Some('s'), 0x14 => Some('t'), 0x16 => Some('u'),
        0x2F => Some('v'), 0x11 => Some('w'), 0x2D => Some('x'),
        0x15 => Some('y'), 0x2C => Some('z'),
        0x02 => Some('1'), 0x03 => Some('2'), 0x04 => Some('3'),
        0x05 => Some('4'), 0x06 => Some('5'), 0x07 => Some('6'),
        0x08 => Some('7'), 0x09 => Some('8'), 0x0A => Some('9'),
        0x0B => Some('0'), 0x39 => Some(' '), 0x1C => Some('\n'),
        _ => None,
    }
}
