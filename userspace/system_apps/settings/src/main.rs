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
const ACCENT: u32 = 0xb58900;
const WHITE: u32 = 0xffffff;
const FG: u32 = 0x839496;

fn draw_rect(fb: u64, pitch: u32, x: u32, y: u32, w: u32, h: u32, color: u32) {
    let fb_ptr = fb as *mut u32;
    let pp = pitch as usize / 4;
    for dy in 0..h as usize {
        for dx in 0..w as usize {
            unsafe { core::ptr::write_volatile(fb_ptr.add((y as usize + dy) * pp + x as usize + dx), color); }
        }
    }
}

fn draw_char(fb: u64, pitch: u32, x: u32, y: u32, ch: u8, color: u32) {
    let glyph: &[u8] = match ch {
        b'S' => &[0x46, 0x49, 0x49, 0x49, 0x31],
        b'e' => &[0x38, 0x54, 0x54, 0x54, 0x18],
        b't' => &[0x04, 0x3F, 0x44, 0x40, 0x20],
        b'i' => &[0x00, 0x44, 0x7D, 0x40, 0x00],
        b'n' => &[0x7C, 0x08, 0x04, 0x04, 0x78],
        b'g' => &[0x0C, 0x52, 0x52, 0x52, 0x3E],
        b's' => &[0x48, 0x54, 0x54, 0x54, 0x20],
        b'D' => &[0x7F, 0x41, 0x41, 0x22, 0x1C],
        b'u' => &[0x3C, 0x40, 0x40, 0x20, 0x7C],
        b'O' => &[0x3E, 0x41, 0x41, 0x41, 0x3E],
        b'T' => &[0x01, 0x01, 0x7F, 0x01, 0x01],
        b'h' => &[0x7F, 0x08, 0x04, 0x04, 0x78],
        b'm' => &[0x7C, 0x04, 0x18, 0x04, 0x78],
        b'r' => &[0x7C, 0x08, 0x04, 0x04, 0x08],
        b'a' => &[0x20, 0x54, 0x54, 0x54, 0x78],
        b'l' => &[0x00, 0x41, 0x7F, 0x40, 0x00],
        b'o' => &[0x38, 0x44, 0x44, 0x44, 0x38],
        b'k' => &[0x7F, 0x10, 0x28, 0x44, 0x00],
        b'c' => &[0x38, 0x44, 0x44, 0x44, 0x20],
        b'p' => &[0x7C, 0x14, 0x14, 0x14, 0x08],
        b'y' => &[0x0C, 0x50, 0x50, 0x50, 0x3C],
        b'G' => &[0x3E, 0x41, 0x49, 0x49, 0x7A],
        b'R' => &[0x7F, 0x09, 0x19, 0x29, 0x46],
        b'1' => &[0x00, 0x42, 0x7F, 0x40, 0x00],
        b'.' => &[0x00, 0x60, 0x60, 0x00, 0x00],
        b'0' => &[0x3E, 0x51, 0x49, 0x45, 0x3E],
        b' ' => &[0x00, 0x00, 0x00, 0x00, 0x00],
        b'-' => &[0x08, 0x08, 0x08, 0x08, 0x08],
        _ => &[0x3E, 0x41, 0x41, 0x41, 0x3E],
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
        draw_char(fb, pitch, x + i as u32 * 6, y, ch, color);
    }
}

const SETTINGS: [(&str, &str); 5] = [
    ("Theme", "Solarized Dark"),
    ("Resolution", "1024x768"),
    ("Language", "English"),
    ("Hostname", "dunit-os"),
    ("OS", "Dunit OS 1.0"),
];

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
    draw_str(fb.addr, fb.pitch, 10, 10, "Settings", WHITE);

    for (i, &(key, val)) in SETTINGS.iter().enumerate() {
        let y = 60 + i as u32 * 40;
        draw_rect(fb.addr, fb.pitch, 20, y, fb.width - 40, 32, PANEL);
        draw_str(fb.addr, fb.pitch, 30, y + 10, key, ACCENT);
        draw_str(fb.addr, fb.pitch, 200, y + 10, val, FG);
    }

    loop {
        let sc = syscall0(SYSCALL_GET_KEY);
        if sc >= 0 && (sc as u8) == 0x01 {
            syscall1(SYSCALL_EXIT, 0);
        }
        for _ in 0..10000 {
            unsafe { core::arch::asm!("pause"); }
        }
    }
}
