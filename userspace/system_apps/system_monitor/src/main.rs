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

const BG: u32 = 0x002b36;
const PANEL: u32 = 0x073642;
const RED: u32 = 0xdc322f;
const GREEN: u32 = 0x859900;
const YELLOW: u32 = 0xb58900;
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
        b'y' => &[0x0C, 0x50, 0x50, 0x50, 0x3C],
        b's' => &[0x48, 0x54, 0x54, 0x54, 0x20],
        b't' => &[0x04, 0x3F, 0x44, 0x40, 0x20],
        b'e' => &[0x38, 0x54, 0x54, 0x54, 0x18],
        b'm' => &[0x7C, 0x04, 0x18, 0x04, 0x78],
        b'M' => &[0x7F, 0x02, 0x0C, 0x02, 0x7F],
        b'o' => &[0x38, 0x44, 0x44, 0x44, 0x38],
        b'n' => &[0x7C, 0x08, 0x04, 0x04, 0x78],
        b'i' => &[0x00, 0x44, 0x7D, 0x40, 0x00],
        b'r' => &[0x7C, 0x08, 0x04, 0x04, 0x08],
        b'C' => &[0x3E, 0x41, 0x41, 0x41, 0x22],
        b'P' => &[0x7F, 0x09, 0x09, 0x09, 0x06],
        b'U' => &[0x3F, 0x40, 0x40, 0x40, 0x3F],
        b'R' => &[0x7F, 0x09, 0x19, 0x29, 0x46],
        b'A' => &[0x7C, 0x12, 0x11, 0x12, 0x7C],
        b'a' => &[0x20, 0x54, 0x54, 0x54, 0x78],
        b'l' => &[0x00, 0x41, 0x7F, 0x40, 0x00],
        b'u' => &[0x3C, 0x40, 0x40, 0x20, 0x7C],
        b'g' => &[0x0C, 0x52, 0x52, 0x52, 0x3E],
        b'p' => &[0x7C, 0x14, 0x14, 0x14, 0x08],
        b'k' => &[0x7F, 0x10, 0x28, 0x44, 0x00],
        b'I' => &[0x00, 0x41, 0x7F, 0x41, 0x00],
        b'D' => &[0x7F, 0x41, 0x41, 0x22, 0x1C],
        b'N' => &[0x7F, 0x04, 0x08, 0x10, 0x7F],
        b'T' => &[0x01, 0x01, 0x7F, 0x01, 0x01],
        b'h' => &[0x7F, 0x08, 0x04, 0x04, 0x78],
        b'c' => &[0x38, 0x44, 0x44, 0x44, 0x20],
        b'd' => &[0x38, 0x44, 0x44, 0x48, 0x7F],
        b'0' => &[0x3E, 0x51, 0x49, 0x45, 0x3E],
        b'1' => &[0x00, 0x42, 0x7F, 0x40, 0x00],
        b'2' => &[0x42, 0x61, 0x51, 0x49, 0x46],
        b'3' => &[0x21, 0x41, 0x45, 0x4B, 0x31],
        b'4' => &[0x18, 0x14, 0x12, 0x7F, 0x10],
        b'%' => &[0x23, 0x13, 0x08, 0x64, 0x62],
        b'/' => &[0x20, 0x10, 0x08, 0x04, 0x02],
        b'B' => &[0x7F, 0x49, 0x49, 0x49, 0x36],
        b' ' => &[0x00, 0x00, 0x00, 0x00, 0x00],
        b':' => &[0x00, 0x36, 0x36, 0x00, 0x00],
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

fn draw_bar(fb: u64, pitch: u32, x: u32, y: u32, w: u32, h: u32, pct: u32, color: u32) {
    draw_rect(fb, pitch, x, y, w, h, PANEL);
    let filled = w * pct / 100;
    if filled > 0 {
        draw_rect(fb, pitch, x, y, filled, h, color);
    }
}

const PROCESSES: [(&str, u32, &str); 4] = [
    ("init",     1, "S"),
    ("kernel",   2, "R"),
    ("plank",    3, "S"),
    ("terminal", 4, "S"),
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
    draw_str(fb.addr, fb.pitch, 10, 10, "System Monitor", WHITE);

    draw_str(fb.addr, fb.pitch, 20, 50, "CPU:", FG);
    draw_bar(fb.addr, fb.pitch, 80, 50, 200, 12, 3, GREEN);
    draw_str(fb.addr, fb.pitch, 290, 50, "3%", GREEN);

    draw_str(fb.addr, fb.pitch, 20, 80, "RAM:", FG);
    draw_bar(fb.addr, fb.pitch, 80, 80, 200, 12, 12, YELLOW);
    draw_str(fb.addr, fb.pitch, 290, 80, "64/512 MB", YELLOW);

    draw_str(fb.addr, fb.pitch, 20, 120, "PID  NAME       STATE", FG);
    draw_rect(fb.addr, fb.pitch, 20, 130, fb.width - 40, 1, PANEL);

    for (i, &(name, pid, state)) in PROCESSES.iter().enumerate() {
        let y = 140 + i as u32 * 20;
        let pid_str = match pid {
            1 => "1",
            2 => "2",
            3 => "3",
            4 => "4",
            _ => "?",
        };
        draw_str(fb.addr, fb.pitch, 20, y, pid_str, FG);
        draw_str(fb.addr, fb.pitch, 60, y, name, WHITE);
        let sc = if *state == *"R" { RED } else { GREEN };
        draw_str(fb.addr, fb.pitch, 180, y, state, sc);
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
