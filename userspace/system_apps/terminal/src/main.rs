#![no_std]
#![no_main]

use core::panic::PanicInfo;

const SYSCALL_EXIT: usize = 0;
const SYSCALL_GET_FRAMEBUFFER: usize = 10;
const SYSCALL_GET_KEY: usize = 13;

#[repr(C)]
struct FbInfo { addr: u64, width: u32, height: u32, pitch: u32 }

fn syscall0(num: usize) -> isize {
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

fn syscall1(num: usize, a1: usize) -> isize {
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

const BG: u32 = 0x000000;
const FG: u32 = 0xffffff;
const PROMPT_COLOR: u32 = 0x00ff00;
const CURSOR_COLOR: u32 = 0xffffff;

fn get_glyph(ch: u8) -> [u8; 5] {
    match ch {
        b'A' => [0x7C, 0x12, 0x11, 0x12, 0x7C],
        b'B' => [0x7F, 0x49, 0x49, 0x49, 0x36],
        b'C' => [0x3E, 0x41, 0x41, 0x41, 0x22],
        b'D' => [0x7F, 0x41, 0x41, 0x22, 0x1C],
        b'E' => [0x7F, 0x49, 0x49, 0x49, 0x41],
        b'F' => [0x7F, 0x09, 0x09, 0x09, 0x01],
        b'G' => [0x3E, 0x41, 0x49, 0x49, 0x7A],
        b'H' => [0x7F, 0x08, 0x08, 0x08, 0x7F],
        b'I' => [0x00, 0x41, 0x7F, 0x41, 0x00],
        b'J' => [0x20, 0x40, 0x41, 0x3F, 0x01],
        b'K' => [0x7F, 0x08, 0x14, 0x22, 0x41],
        b'L' => [0x7F, 0x40, 0x40, 0x40, 0x40],
        b'M' => [0x7F, 0x02, 0x0C, 0x02, 0x7F],
        b'N' => [0x7F, 0x04, 0x08, 0x10, 0x7F],
        b'O' => [0x3E, 0x41, 0x41, 0x41, 0x3E],
        b'P' => [0x7F, 0x09, 0x09, 0x09, 0x06],
        b'R' => [0x7F, 0x09, 0x19, 0x29, 0x46],
        b'S' => [0x46, 0x49, 0x49, 0x49, 0x31],
        b'T' => [0x01, 0x01, 0x7F, 0x01, 0x01],
        b'U' => [0x3F, 0x40, 0x40, 0x40, 0x3F],
        b'V' => [0x1F, 0x20, 0x40, 0x20, 0x1F],
        b'W' => [0x3F, 0x40, 0x38, 0x40, 0x3F],
        b'X' => [0x63, 0x14, 0x08, 0x14, 0x63],
        b'Y' => [0x07, 0x08, 0x70, 0x08, 0x07],
        b'Z' => [0x61, 0x51, 0x49, 0x45, 0x43],
        b'a' => [0x20, 0x54, 0x54, 0x54, 0x78],
        b'b' => [0x7F, 0x48, 0x44, 0x44, 0x38],
        b'c' => [0x38, 0x44, 0x44, 0x44, 0x20],
        b'd' => [0x38, 0x44, 0x44, 0x48, 0x7F],
        b'e' => [0x38, 0x54, 0x54, 0x54, 0x18],
        b'f' => [0x08, 0x7E, 0x09, 0x01, 0x02],
        b'g' => [0x0C, 0x52, 0x52, 0x52, 0x3E],
        b'h' => [0x7F, 0x08, 0x04, 0x04, 0x78],
        b'i' => [0x00, 0x44, 0x7D, 0x40, 0x00],
        b'j' => [0x20, 0x40, 0x44, 0x3D, 0x00],
        b'k' => [0x7F, 0x10, 0x28, 0x44, 0x00],
        b'l' => [0x00, 0x41, 0x7F, 0x40, 0x00],
        b'm' => [0x7C, 0x04, 0x18, 0x04, 0x78],
        b'n' => [0x7C, 0x08, 0x04, 0x04, 0x78],
        b'o' => [0x38, 0x44, 0x44, 0x44, 0x38],
        b'p' => [0x7C, 0x14, 0x14, 0x14, 0x08],
        b'r' => [0x7C, 0x08, 0x04, 0x04, 0x08],
        b's' => [0x48, 0x54, 0x54, 0x54, 0x20],
        b't' => [0x04, 0x3F, 0x44, 0x40, 0x20],
        b'u' => [0x3C, 0x40, 0x40, 0x20, 0x7C],
        b'v' => [0x1C, 0x20, 0x40, 0x20, 0x1C],
        b'w' => [0x3C, 0x40, 0x30, 0x40, 0x3C],
        b'x' => [0x44, 0x28, 0x10, 0x28, 0x44],
        b'y' => [0x0C, 0x50, 0x50, 0x50, 0x3C],
        b'z' => [0x44, 0x64, 0x54, 0x4C, 0x44],
        b'0' => [0x3E, 0x51, 0x49, 0x45, 0x3E],
        b'1' => [0x00, 0x42, 0x7F, 0x40, 0x00],
        b'2' => [0x42, 0x61, 0x51, 0x49, 0x46],
        b'3' => [0x21, 0x41, 0x45, 0x4B, 0x31],
        b'4' => [0x18, 0x14, 0x12, 0x7F, 0x10],
        b'5' => [0x27, 0x45, 0x45, 0x45, 0x39],
        b'6' => [0x3C, 0x4A, 0x49, 0x49, 0x30],
        b'7' => [0x01, 0x71, 0x09, 0x05, 0x03],
        b'8' => [0x36, 0x49, 0x49, 0x49, 0x36],
        b'9' => [0x06, 0x49, 0x49, 0x29, 0x1E],
        b' ' => [0x00, 0x00, 0x00, 0x00, 0x00],
        b'/' => [0x20, 0x10, 0x08, 0x04, 0x02],
        b'.' => [0x00, 0x60, 0x60, 0x00, 0x00],
        b':' => [0x00, 0x36, 0x36, 0x00, 0x00],
        b'#' => [0x14, 0x7F, 0x14, 0x7F, 0x14],
        b'@' => [0x3E, 0x41, 0x5D, 0x55, 0x1E],
        b'-' => [0x08, 0x08, 0x08, 0x08, 0x08],
        b'_' => [0x40, 0x40, 0x40, 0x40, 0x40],
        b'>' => [0x41, 0x22, 0x14, 0x08, 0x00],
        b'<' => [0x00, 0x08, 0x14, 0x22, 0x41],
        b'[' => [0x00, 0x7F, 0x41, 0x41, 0x00],
        b']' => [0x00, 0x41, 0x41, 0x7F, 0x00],
        b'!' => [0x00, 0x00, 0x5F, 0x00, 0x00],
        b'?' => [0x02, 0x01, 0x51, 0x09, 0x06],
        _ => [0x00, 0x00, 0x00, 0x00, 0x00],
    }
}

struct Console {
    fb: u64,
    pitch: u32,
    width: u32,
    height: u32,
    col: u32,
    row: u32,
    cols: u32,
    rows: u32,
}

impl Console {
    fn new(fb: u64, pitch: u32, width: u32, height: u32) -> Self {
        Self { fb, pitch, width, height, col: 0, row: 0, cols: width / 6, rows: height / 10 }
    }

    fn draw_char_at(&self, col: u32, row: u32, ch: u8, color: u32) {
        let x = col * 6;
        let y = row * 10;
        let glyph = get_glyph(ch);
        let fb_ptr = self.fb as *mut u32;
        let pp = self.pitch as usize / 4;
        for (cx, &bits) in glyph.iter().enumerate() {
            for ry in 0..8usize {
                let c = if bits & (1 << ry) != 0 { color } else { BG };
                unsafe { core::ptr::write_volatile(fb_ptr.add((y as usize + ry) * pp + x as usize + cx), c); }
            }
        }
    }

    fn clear(&self) {
        let fb_ptr = self.fb as *mut u32;
        let pp = self.pitch as usize / 4;
        for y in 0..self.height as usize {
            for x in 0..self.width as usize {
                unsafe { core::ptr::write_volatile(fb_ptr.add(y * pp + x), BG); }
            }
        }
    }

    fn scroll(&mut self) {
        let fb_ptr = self.fb as *mut u32;
        let pp = self.pitch as usize / 4;
        let row_h = 10usize;
        for y in row_h..self.height as usize {
            for x in 0..self.width as usize {
                unsafe {
                    let px = core::ptr::read_volatile(fb_ptr.add(y * pp + x));
                    core::ptr::write_volatile(fb_ptr.add((y - row_h) * pp + x), px);
                }
            }
        }
        let last_y = (self.rows as usize - 1) * row_h;
        for y in last_y..last_y + row_h {
            for x in 0..self.width as usize {
                unsafe { core::ptr::write_volatile(fb_ptr.add(y * pp + x), BG); }
            }
        }
    }

    fn newline(&mut self) {
        self.col = 0;
        self.row += 1;
        if self.row >= self.rows {
            self.scroll();
            self.row = self.rows - 1;
        }
    }

    fn write_char(&mut self, ch: char, color: u32) {
        match ch {
            '\n' => self.newline(),
            '\x08' => {
                if self.col > 0 {
                    self.col -= 1;
                    self.draw_char_at(self.col, self.row, b' ', BG);
                }
            }
            _ => {
                self.draw_char_at(self.col, self.row, ch as u8, color);
                self.col += 1;
                if self.col >= self.cols { self.newline(); }
            }
        }
    }

    fn write_str(&mut self, s: &str, color: u32) {
        for ch in s.chars() { self.write_char(ch, color); }
    }

    fn draw_cursor(&self) {
        let x = self.col * 6;
        let y = self.row * 10;
        let fb_ptr = self.fb as *mut u32;
        let pp = self.pitch as usize / 4;
        for ry in 0..8usize {
            unsafe { core::ptr::write_volatile(fb_ptr.add((y as usize + ry) * pp + x as usize), CURSOR_COLOR); }
        }
    }
}

fn scancode_to_char(sc: u8) -> Option<char> {
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
        0x0E => Some('\x08'),
        _ => None,
    }
}

fn handle_command(con: &mut Console, cmd: &str) {
    match cmd {
        "help" => con.write_str("Commands: help ls pwd uname whoami date free ps exit\n", FG),
        "ls" => con.write_str("bin  dev  home  proc  tmp  usr  var  etc\n", FG),
        "pwd" => con.write_str("/root\n", FG),
        "uname" => con.write_str("Dunit OS 1.0 Green Tea x86_64\n", FG),
        "whoami" => con.write_str("root\n", FG),
        "date" => con.write_str("Dunit OS - no RTC yet\n", FG),
        "free" => con.write_str("Mem: 512MB total\n", FG),
        "ps" => con.write_str("PID  CMD\n  1  init\n  2  kernel\n  3  plank\n  4  terminal\n", FG),
        "exit" => {
            con.write_str("Closing...\n", FG);
            syscall1(SYSCALL_EXIT, 0);
        }
        "" => {}
        _ => {
            con.write_str("not found: ", 0xdc322f);
            con.write_str(cmd, 0xdc322f);
            con.write_str("\n", FG);
        }
    }
}

#[panic_handler]
fn panic(_: &PanicInfo) -> ! { loop {} }

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut fb = FbInfo { addr: 0, width: 0, height: 0, pitch: 0 };
    if syscall1(SYSCALL_GET_FRAMEBUFFER, &mut fb as *mut FbInfo as usize) != 0 {
        loop {}
    }

    let mut con = Console::new(fb.addr, fb.pitch, fb.width, fb.height);
    con.clear();
    con.write_str("Dunit OS Terminal v1.0\n", 0x268bd2);
    con.write_str("Type 'help' for commands\n\n", FG);
    con.write_str("root@dunit:~# ", PROMPT_COLOR);
    con.draw_cursor();

    let mut input = [0u8; 256];
    let mut input_len = 0usize;

    loop {
        let sc = syscall0(SYSCALL_GET_KEY);
        if sc >= 0 {
            let sc = sc as u8;
            if sc & 0x80 == 0 {
                if let Some(ch) = scancode_to_char(sc) {
                    if ch == '\n' {
                        con.write_char('\n', FG);
                        let cmd = core::str::from_utf8(&input[..input_len]).unwrap_or("");
                        handle_command(&mut con, cmd);
                        input_len = 0;
                        con.write_str("root@dunit:~# ", PROMPT_COLOR);
                        con.draw_cursor();
                    } else if ch == '\x08' {
                        if input_len > 0 {
                            input_len -= 1;
                            con.write_char('\x08', FG);
                            con.draw_cursor();
                        }
                    } else if input_len < 255 {
                        input[input_len] = ch as u8;
                        input_len += 1;
                        con.write_char(ch, FG);
                        con.draw_cursor();
                    }
                }
            }
        }
        for _ in 0..1000 {
            unsafe { core::arch::asm!("pause"); }
        }
    }
}
