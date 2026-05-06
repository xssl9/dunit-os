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
    unsafe { core::arch::asm!("syscall", in("rax") num, lateout("rax") ret, options(nostack)); }
    ret
}

fn syscall1(num: usize, a1: usize) -> isize {
    let ret: isize;
    unsafe { core::arch::asm!("syscall", in("rax") num, in("rdi") a1, lateout("rax") ret, options(nostack)); }
    ret
}

const BG: u32 = 0x002b36;
const PANEL: u32 = 0x073642;
const FG: u32 = 0x839496;
const ACCENT: u32 = 0x6c71c4;
const CURSOR_COLOR: u32 = 0x268bd2;
const WHITE: u32 = 0xffffff;

fn draw_rect(fb: u64, pitch: u32, x: u32, y: u32, w: u32, h: u32, color: u32) {
    let fb_ptr = fb as *mut u32;
    let pp = pitch as usize / 4;
    for dy in 0..h as usize {
        for dx in 0..w as usize {
            unsafe { core::ptr::write_volatile(fb_ptr.add((y as usize + dy) * pp + x as usize + dx), color); }
        }
    }
}

fn draw_char_px(fb: u64, pitch: u32, x: u32, y: u32, ch: u8, color: u32) {
    let glyph: [u8; 5] = match ch {
        b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b' ' | b'.' | b',' | b'!' | b'?' | b'\'' | b'"' | b'-' | b'_' | b':' | b';' | b'/' | b'\\' | b'(' | b')' | b'[' | b']' => {
            [0x3E, 0x41, 0x41, 0x41, 0x3E]
        }
        _ => [0x00; 5],
    };
    let fb_ptr = fb as *mut u32;
    let pp = pitch as usize / 4;
    for (cx, &bits) in glyph.iter().enumerate() {
        for ry in 0..8usize {
            let c = if bits & (1 << ry) != 0 { color } else { BG };
            unsafe { core::ptr::write_volatile(fb_ptr.add((y as usize + ry) * pp + x as usize + cx), c); }
        }
    }
}

fn draw_str(fb: u64, pitch: u32, x: u32, y: u32, s: &str, color: u32) {
    for (i, ch) in s.bytes().enumerate() {
        draw_char_px(fb, pitch, x + i as u32 * 6, y, ch, color);
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

#[panic_handler]
fn panic(_: &PanicInfo) -> ! { loop {} }

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut fb = FbInfo { addr: 0, width: 0, height: 0, pitch: 0 };
    if syscall1(SYSCALL_GET_FRAMEBUFFER, &mut fb as *mut FbInfo as usize) != 0 {
        loop {}
    }

    draw_rect(fb.addr, fb.pitch, 0, 0, fb.width, fb.height, BG);
    draw_rect(fb.addr, fb.pitch, 0, 0, fb.width, 32, PANEL);
    draw_str(fb.addr, fb.pitch, 10, 10, "Text Editor - Untitled", WHITE);
    draw_rect(fb.addr, fb.pitch, 0, 32, fb.width, 1, ACCENT);

    let mut buf = [0u8; 4096];
    let mut len = 0usize;
    let mut col = 0u32;
    let mut row = 0u32;
    let cols = fb.width / 6;
    let rows = (fb.height - 40) / 10;

    let draw_cursor = |fb: u64, pitch: u32, col: u32, row: u32| {
        let x = col * 6;
        let y = 40 + row * 10;
        let fb_ptr = fb as *mut u32;
        let pp = pitch as usize / 4;
        for ry in 0..8usize {
            unsafe { core::ptr::write_volatile(fb_ptr.add((y as usize + ry) * pp + x as usize), CURSOR_COLOR); }
        }
    };

    draw_cursor(fb.addr, fb.pitch, col, row);

    loop {
        let sc = syscall0(SYSCALL_GET_KEY);
        if sc >= 0 {
            let sc = sc as u8;
            if sc == 0x01 {
                syscall1(SYSCALL_EXIT, 0);
            }
            if sc & 0x80 == 0 {
                if let Some(ch) = scancode_to_char(sc) {
                    draw_char_px(fb.addr, fb.pitch, col * 6, 40 + row * 10, b' ', BG);
                    if ch == '\n' {
                        col = 0;
                        row += 1;
                        if row >= rows { row = rows - 1; }
                    } else if ch == '\x08' {
                        if col > 0 {
                            col -= 1;
                            draw_char_px(fb.addr, fb.pitch, col * 6, 40 + row * 10, b' ', BG);
                            if len > 0 { len -= 1; }
                        }
                    } else if len < 4095 {
                        buf[len] = ch as u8;
                        len += 1;
                        draw_char_px(fb.addr, fb.pitch, col * 6, 40 + row * 10, ch as u8, FG);
                        col += 1;
                        if col >= cols { col = 0; row += 1; if row >= rows { row = rows - 1; } }
                    }
                    draw_cursor(fb.addr, fb.pitch, col, row);
                }
            }
        }
        for _ in 0..1000 {
            unsafe { core::arch::asm!("pause"); }
        }
    }
}
