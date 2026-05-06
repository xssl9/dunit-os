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
const ACCENT: u32 = 0x859900;
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

fn draw_char(fb: u64, pitch: u32, x: u32, y: u32, ch: u8, color: u32) {
    let glyph: &[u8] = match ch {
        b'F' => &[0x7F, 0x09, 0x09, 0x09, 0x01],
        b'i' => &[0x00, 0x44, 0x7D, 0x40, 0x00],
        b'l' => &[0x00, 0x41, 0x7F, 0x40, 0x00],
        b'e' => &[0x38, 0x54, 0x54, 0x54, 0x18],
        b'M' => &[0x7F, 0x02, 0x0C, 0x02, 0x7F],
        b'a' => &[0x20, 0x54, 0x54, 0x54, 0x78],
        b'n' => &[0x7C, 0x08, 0x04, 0x04, 0x78],
        b'g' => &[0x0C, 0x52, 0x52, 0x52, 0x3E],
        b'r' => &[0x7C, 0x08, 0x04, 0x04, 0x08],
        b'/' => &[0x20, 0x10, 0x08, 0x04, 0x02],
        b'b' => &[0x7F, 0x48, 0x44, 0x44, 0x38],
        b'o' => &[0x38, 0x44, 0x44, 0x44, 0x38],
        b't' => &[0x04, 0x3F, 0x44, 0x40, 0x20],
        b'u' => &[0x3C, 0x40, 0x40, 0x20, 0x7C],
        b's' => &[0x48, 0x54, 0x54, 0x54, 0x20],
        b'p' => &[0x7C, 0x14, 0x14, 0x14, 0x08],
        b'c' => &[0x38, 0x44, 0x44, 0x44, 0x20],
        b'd' => &[0x38, 0x44, 0x44, 0x48, 0x7F],
        b'h' => &[0x7F, 0x08, 0x04, 0x04, 0x78],
        b'm' => &[0x7C, 0x04, 0x18, 0x04, 0x78],
        b'v' => &[0x1C, 0x20, 0x40, 0x20, 0x1C],
        b'w' => &[0x3C, 0x40, 0x30, 0x40, 0x3C],
        b'k' => &[0x7F, 0x10, 0x28, 0x44, 0x00],
        b' ' => &[0x00, 0x00, 0x00, 0x00, 0x00],
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

const FILES: [&str; 8] = ["bin", "dev", "home", "proc", "tmp", "usr", "var", "etc"];

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
    draw_str(fb.addr, fb.pitch, 10, 10, "File Manager - /", WHITE);

    for (i, &name) in FILES.iter().enumerate() {
        let col = (i % 4) as u32;
        let row = (i / 4) as u32;
        let x = 20 + col * 160;
        let y = 60 + row * 80;
        draw_rect(fb.addr, fb.pitch, x, y, 120, 60, PANEL);
        draw_str(fb.addr, fb.pitch, x + 10, y + 24, name, ACCENT);
    }

    loop {
        let sc = syscall0(SYSCALL_GET_KEY);
        if sc >= 0 {
            let sc = sc as u8;
            if sc == 0x01 {
                syscall1(SYSCALL_EXIT, 0);
            }
        }
        for _ in 0..10000 {
            unsafe { core::arch::asm!("pause"); }
        }
    }
}
