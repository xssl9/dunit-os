#![no_std]
#![no_main]

use core::panic::PanicInfo;

mod sys {
    pub const SYSCALL_EXIT: usize = 0;
    pub const SYSCALL_GET_FRAMEBUFFER: usize = 10;
    pub const SYSCALL_DRAW_RECT: usize = 12;
    pub const SYSCALL_GET_KEY: usize = 13;
    pub const SYSCALL_GET_MOUSE_POS: usize = 14;
    pub const SYSCALL_SPAWN_PROCESS: usize = 15;
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
            core::arch::asm!("syscall", in("rax") num, lateout("rax") ret, options(nostack));
        }
        ret
    }

    #[inline(always)]
    pub fn syscall1(num: usize, a1: usize) -> isize {
        let ret: isize;
        unsafe {
            core::arch::asm!("syscall", in("rax") num, in("rdi") a1, lateout("rax") ret, options(nostack));
        }
        ret
    }

    #[inline(always)]
    pub fn syscall3(num: usize, a1: usize, a2: usize, a3: usize) -> isize {
        let ret: isize;
        unsafe {
            core::arch::asm!("syscall", in("rax") num, in("rdi") a1, in("rsi") a2, in("rdx") a3, lateout("rax") ret, options(nostack));
        }
        ret
    }

    #[inline(always)]
    pub fn syscall5(num: usize, a1: usize, a2: usize, a3: usize, a4: usize, a5: usize) -> isize {
        let ret: isize;
        unsafe {
            core::arch::asm!("syscall", in("rax") num, in("rdi") a1, in("rsi") a2, in("rdx") a3, in("r10") a4, in("r8") a5, lateout("rax") ret, options(nostack));
        }
        ret
    }
}

use sys::*;

const BG_COLOR: u32 = 0x002b36;
const PANEL_COLOR: u32 = 0x073642;
const PLANK_COLOR: u32 = 0x1c1c1c;
const PLANK_ICON_BG: u32 = 0x2c2c2c;
const PLANK_ICON_HOVER: u32 = 0x3c3c3c;
const WHITE: u32 = 0xffffff;

const ICON_SIZE: u32 = 48;
const ICON_SPACING: u32 = 8;
const PLANK_HEIGHT: u32 = 64;
const PANEL_HEIGHT: u32 = 40;

const APPS: [(&str, u32, &str); 5] = [
    ("Terminal", 0x268bd2, "/boot/userspace/terminal"),
    ("Files",    0x859900, "/boot/userspace/file_manager"),
    ("Settings", 0xb58900, "/boot/userspace/settings"),
    ("Monitor",  0xdc322f, "/boot/userspace/system_monitor"),
    ("Editor",   0x6c71c4, "/boot/userspace/text_editor"),
];

fn draw_rect(fb: u64, pitch: u32, x: u32, y: u32, w: u32, h: u32, color: u32) {
    let fb_ptr = fb as *mut u32;
    let pitch_px = pitch as usize / 4;
    for dy in 0..h as usize {
        for dx in 0..w as usize {
            unsafe {
                core::ptr::write_volatile(
                    fb_ptr.add((y as usize + dy) * pitch_px + x as usize + dx),
                    color,
                );
            }
        }
    }
}

fn draw_char(fb: u64, pitch: u32, x: u32, y: u32, ch: u8, color: u32) {
    let glyph: &[u8] = match ch {
        b'A' => &[0x7C, 0x12, 0x11, 0x12, 0x7C],
        b'B' => &[0x7F, 0x49, 0x49, 0x49, 0x36],
        b'C' => &[0x3E, 0x41, 0x41, 0x41, 0x22],
        b'D' => &[0x7F, 0x41, 0x41, 0x22, 0x1C],
        b'E' => &[0x7F, 0x49, 0x49, 0x49, 0x41],
        b'F' => &[0x7F, 0x09, 0x09, 0x09, 0x01],
        b'G' => &[0x3E, 0x41, 0x49, 0x49, 0x7A],
        b'H' => &[0x7F, 0x08, 0x08, 0x08, 0x7F],
        b'I' => &[0x00, 0x41, 0x7F, 0x41, 0x00],
        b'J' => &[0x20, 0x40, 0x41, 0x3F, 0x01],
        b'K' => &[0x7F, 0x08, 0x14, 0x22, 0x41],
        b'L' => &[0x7F, 0x40, 0x40, 0x40, 0x40],
        b'M' => &[0x7F, 0x02, 0x0C, 0x02, 0x7F],
        b'N' => &[0x7F, 0x04, 0x08, 0x10, 0x7F],
        b'O' => &[0x3E, 0x41, 0x41, 0x41, 0x3E],
        b'P' => &[0x7F, 0x09, 0x09, 0x09, 0x06],
        b'R' => &[0x7F, 0x09, 0x19, 0x29, 0x46],
        b'S' => &[0x46, 0x49, 0x49, 0x49, 0x31],
        b'T' => &[0x01, 0x01, 0x7F, 0x01, 0x01],
        b'U' => &[0x3F, 0x40, 0x40, 0x40, 0x3F],
        b'V' => &[0x1F, 0x20, 0x40, 0x20, 0x1F],
        b'W' => &[0x3F, 0x40, 0x38, 0x40, 0x3F],
        b'X' => &[0x63, 0x14, 0x08, 0x14, 0x63],
        b'Y' => &[0x07, 0x08, 0x70, 0x08, 0x07],
        b'Z' => &[0x61, 0x51, 0x49, 0x45, 0x43],
        b'a' => &[0x20, 0x54, 0x54, 0x54, 0x78],
        b'b' => &[0x7F, 0x48, 0x44, 0x44, 0x38],
        b'c' => &[0x38, 0x44, 0x44, 0x44, 0x20],
        b'd' => &[0x38, 0x44, 0x44, 0x48, 0x7F],
        b'e' => &[0x38, 0x54, 0x54, 0x54, 0x18],
        b'f' => &[0x08, 0x7E, 0x09, 0x01, 0x02],
        b'g' => &[0x0C, 0x52, 0x52, 0x52, 0x3E],
        b'h' => &[0x7F, 0x08, 0x04, 0x04, 0x78],
        b'i' => &[0x00, 0x44, 0x7D, 0x40, 0x00],
        b'j' => &[0x20, 0x40, 0x44, 0x3D, 0x00],
        b'k' => &[0x7F, 0x10, 0x28, 0x44, 0x00],
        b'l' => &[0x00, 0x41, 0x7F, 0x40, 0x00],
        b'm' => &[0x7C, 0x04, 0x18, 0x04, 0x78],
        b'n' => &[0x7C, 0x08, 0x04, 0x04, 0x78],
        b'o' => &[0x38, 0x44, 0x44, 0x44, 0x38],
        b'p' => &[0x7C, 0x14, 0x14, 0x14, 0x08],
        b'r' => &[0x7C, 0x08, 0x04, 0x04, 0x08],
        b's' => &[0x48, 0x54, 0x54, 0x54, 0x20],
        b't' => &[0x04, 0x3F, 0x44, 0x40, 0x20],
        b'u' => &[0x3C, 0x40, 0x40, 0x20, 0x7C],
        b'v' => &[0x1C, 0x20, 0x40, 0x20, 0x1C],
        b'w' => &[0x3C, 0x40, 0x30, 0x40, 0x3C],
        b'x' => &[0x44, 0x28, 0x10, 0x28, 0x44],
        b'y' => &[0x0C, 0x50, 0x50, 0x50, 0x3C],
        b'z' => &[0x44, 0x64, 0x54, 0x4C, 0x44],
        _ => &[0x00, 0x00, 0x00, 0x00, 0x00],
    };
    let fb_ptr = fb as *mut u32;
    let pitch_px = pitch as usize / 4;
    for (col, &bits) in glyph.iter().enumerate() {
        for row in 0..8usize {
            if bits & (1 << row) != 0 {
                unsafe {
                    core::ptr::write_volatile(
                        fb_ptr.add((y as usize + row) * pitch_px + x as usize + col),
                        color,
                    );
                }
            }
        }
    }
}

fn draw_str(fb: u64, pitch: u32, x: u32, y: u32, s: &str, color: u32) {
    for (i, ch) in s.bytes().enumerate() {
        draw_char(fb, pitch, x + i as u32 * 6, y, ch, color);
    }
}

fn draw_icon(fb: u64, pitch: u32, x: u32, y: u32, color: u32, hovered: bool, label: &str) {
    let bg = if hovered { PLANK_ICON_HOVER } else { PLANK_ICON_BG };
    draw_rect(fb, pitch, x, y, ICON_SIZE, ICON_SIZE, bg);
    draw_rect(fb, pitch, x + 2, y + 2, ICON_SIZE - 4, ICON_SIZE - 4, color);
    let label_x = x + ICON_SIZE / 2 - (label.len() as u32 * 3);
    draw_str(fb, pitch, label_x, y + ICON_SIZE + 2, label, WHITE);
}

fn draw_ui(fb: u64, pitch: u32, width: u32, height: u32, mouse_x: u32, mouse_y: u32) {
    draw_rect(fb, pitch, 0, 0, width, height, BG_COLOR);
    draw_rect(fb, pitch, 0, 0, width, PANEL_HEIGHT, PANEL_COLOR);
    draw_str(fb, pitch, 10, 14, "Dunit OS", WHITE);

    let plank_y = height - PLANK_HEIGHT;
    let total_w = APPS.len() as u32 * (ICON_SIZE + ICON_SPACING) - ICON_SPACING;
    let plank_x = (width - total_w) / 2;
    let plank_bg_x = plank_x - 16;
    let plank_bg_w = total_w + 32;
    draw_rect(fb, pitch, plank_bg_x, plank_y, plank_bg_w, PLANK_HEIGHT, PLANK_COLOR);

    for (i, &(label, color, _path)) in APPS.iter().enumerate() {
        let ix = plank_x + i as u32 * (ICON_SIZE + ICON_SPACING);
        let iy = plank_y + 8;
        let hovered = mouse_x >= ix && mouse_x < ix + ICON_SIZE
            && mouse_y >= iy && mouse_y < iy + ICON_SIZE;
        draw_icon(fb, pitch, ix, iy, color, hovered, label);
    }
}

fn icon_at(width: u32, height: u32, mx: u32, my: u32) -> Option<usize> {
    let plank_y = height - PLANK_HEIGHT;
    let total_w = APPS.len() as u32 * (ICON_SIZE + ICON_SPACING) - ICON_SPACING;
    let plank_x = (width - total_w) / 2;
    for i in 0..APPS.len() {
        let ix = plank_x + i as u32 * (ICON_SIZE + ICON_SPACING);
        let iy = plank_y + 8;
        if mx >= ix && mx < ix + ICON_SIZE && my >= iy && my < iy + ICON_SIZE {
            return Some(i);
        }
    }
    None
}

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    loop {}
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut fb = FbInfo { addr: 0, width: 0, height: 0, pitch: 0 };
    if syscall1(SYSCALL_GET_FRAMEBUFFER, &mut fb as *mut FbInfo as usize) != 0 {
        loop {}
    }

    let mut prev_mx = 0u32;
    let mut prev_my = 0u32;
    let mut prev_btn = false;

    draw_ui(fb.addr, fb.pitch, fb.width, fb.height, prev_mx, prev_my);

    loop {
        let mut mx = 0u32;
        let mut my = 0u32;
        syscall3(SYSCALL_GET_MOUSE_POS, &mut mx as *mut u32 as usize, &mut my as *mut u32 as usize, 0);

        let key = syscall0(SYSCALL_GET_KEY);
        let btn_pressed = key == 0x01;

        if mx != prev_mx || my != prev_my {
            draw_ui(fb.addr, fb.pitch, fb.width, fb.height, mx, my);
            prev_mx = mx;
            prev_my = my;
        }

        if btn_pressed && !prev_btn {
            if let Some(idx) = icon_at(fb.width, fb.height, mx, my) {
                let path = APPS[idx].2;
                syscall1(SYSCALL_SPAWN_PROCESS, path.as_ptr() as usize);
            }
        }
        prev_btn = btn_pressed;

        for _ in 0..10000 {
            unsafe { core::arch::asm!("pause"); }
        }
    }
}
