#![no_std]

pub const SYSCALL_EXIT: usize = 0;
pub const SYSCALL_READ: usize = 3;
pub const SYSCALL_WRITE: usize = 4;
pub const SYSCALL_OPEN: usize = 5;
pub const SYSCALL_CLOSE: usize = 6;
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
pub const SYSCALL_DEBUG_LOG: usize = 20;
pub const SYSCALL_GETCWD: usize = 22;
pub const SYSCALL_CHDIR: usize = 23;
pub const SYSCALL_YIELD: usize = 24;

pub const EAGAIN: isize = -11;
pub const EOPNOTSUPP: isize = -95;

pub const OPEN_READ: usize = 1 << 0;
pub const OPEN_WRITE: usize = 1 << 1;
pub const OPEN_CREATE: usize = 1 << 2;
pub const OPEN_TRUNC: usize = 1 << 3;
pub const OPEN_APPEND: usize = 1 << 4;
pub const OPEN_READ_WRITE: usize = OPEN_READ | OPEN_WRITE;

pub type RawArgv = *const *const u8;
pub type RawEnvp = *const *const u8;

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

pub fn print(s: &str) {
    write_stdout(s);
}

pub fn println(s: &str) {
    write_stdout(s);
    write_stdout("\n");
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
        Self { kind: WAIT_KIND_EMPTY, code: 0 }
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
    syscall2(SYSCALL_SPAWN_PROCESS, path.as_ptr() as usize, path.len())
}

pub fn wait(pid: u32, status: &mut WaitStatus) -> isize {
    syscall2(SYSCALL_WAIT_PROCESS, pid as usize, status as *mut WaitStatus as usize)
}

pub fn yield_now() -> isize {
    syscall0(SYSCALL_YIELD)
}

pub fn getcwd(buf: &mut [u8]) -> isize {
    syscall2(SYSCALL_GETCWD, buf.as_mut_ptr() as usize, buf.len())
}

pub fn chdir(path: &str) -> isize {
    syscall2(SYSCALL_CHDIR, path.as_ptr() as usize, path.len())
}

pub fn debug_log(code: usize) -> isize {
    syscall1(SYSCALL_DEBUG_LOG, code)
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
