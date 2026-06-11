#![no_std]
#![no_main]

use core::panic::PanicInfo;

const IMAGE_W: usize = 16;
const IMAGE_H: usize = 16;
const SCALE: u32 = 10;

const TRANSPARENT: u32 = 0xffff_ffff;
const BLACK: u32 = 0x0012_1720;
const OUTLINE: u32 = 0x0026_3442;
const GREEN: u32 = 0x004f_d184;
const GREEN_DARK: u32 = 0x0024_8f5a;
const TEA: u32 = 0x00d9_f99d;
const WHITE: u32 = 0x00f5_faf3;
const AMBER: u32 = 0x00f2_c14e;

const IMAGE: [u32; IMAGE_W * IMAGE_H] = [
    TRANSPARENT, TRANSPARENT, TRANSPARENT, TRANSPARENT, OUTLINE, OUTLINE, OUTLINE, OUTLINE, OUTLINE, OUTLINE, OUTLINE, OUTLINE, TRANSPARENT, TRANSPARENT, TRANSPARENT, TRANSPARENT,
    TRANSPARENT, TRANSPARENT, OUTLINE, OUTLINE, GREEN_DARK, GREEN_DARK, GREEN_DARK, GREEN_DARK, GREEN_DARK, GREEN_DARK, GREEN_DARK, GREEN_DARK, OUTLINE, OUTLINE, TRANSPARENT, TRANSPARENT,
    TRANSPARENT, OUTLINE, GREEN_DARK, GREEN, GREEN, GREEN, GREEN, GREEN, GREEN, GREEN, GREEN, GREEN, GREEN_DARK, GREEN_DARK, OUTLINE, TRANSPARENT,
    OUTLINE, GREEN_DARK, GREEN, GREEN, GREEN, GREEN, TEA, TEA, TEA, TEA, GREEN, GREEN, GREEN, GREEN_DARK, GREEN_DARK, OUTLINE,
    OUTLINE, GREEN_DARK, GREEN, GREEN, TEA, TEA, WHITE, WHITE, WHITE, WHITE, TEA, TEA, GREEN, GREEN, GREEN_DARK, OUTLINE,
    OUTLINE, GREEN_DARK, GREEN, TEA, WHITE, WHITE, WHITE, AMBER, AMBER, WHITE, WHITE, WHITE, TEA, GREEN, GREEN_DARK, OUTLINE,
    OUTLINE, GREEN_DARK, GREEN, TEA, WHITE, WHITE, AMBER, BLACK, BLACK, AMBER, WHITE, WHITE, TEA, GREEN, GREEN_DARK, OUTLINE,
    OUTLINE, GREEN_DARK, GREEN, TEA, WHITE, AMBER, BLACK, WHITE, WHITE, BLACK, AMBER, WHITE, TEA, GREEN, GREEN_DARK, OUTLINE,
    OUTLINE, GREEN_DARK, GREEN, TEA, WHITE, AMBER, BLACK, WHITE, WHITE, BLACK, AMBER, WHITE, TEA, GREEN, GREEN_DARK, OUTLINE,
    OUTLINE, GREEN_DARK, GREEN, TEA, WHITE, WHITE, AMBER, BLACK, BLACK, AMBER, WHITE, WHITE, TEA, GREEN, GREEN_DARK, OUTLINE,
    OUTLINE, GREEN_DARK, GREEN, TEA, WHITE, WHITE, WHITE, AMBER, AMBER, WHITE, WHITE, WHITE, TEA, GREEN, GREEN_DARK, OUTLINE,
    OUTLINE, GREEN_DARK, GREEN, GREEN, TEA, TEA, WHITE, WHITE, WHITE, WHITE, TEA, TEA, GREEN, GREEN, GREEN_DARK, OUTLINE,
    OUTLINE, GREEN_DARK, GREEN_DARK, GREEN, GREEN, GREEN, TEA, TEA, TEA, TEA, GREEN, GREEN, GREEN, GREEN_DARK, GREEN_DARK, OUTLINE,
    TRANSPARENT, OUTLINE, GREEN_DARK, GREEN_DARK, GREEN, GREEN, GREEN, GREEN, GREEN, GREEN, GREEN, GREEN, GREEN_DARK, GREEN_DARK, OUTLINE, TRANSPARENT,
    TRANSPARENT, TRANSPARENT, OUTLINE, OUTLINE, GREEN_DARK, GREEN_DARK, GREEN_DARK, GREEN_DARK, GREEN_DARK, GREEN_DARK, GREEN_DARK, GREEN_DARK, OUTLINE, OUTLINE, TRANSPARENT, TRANSPARENT,
    TRANSPARENT, TRANSPARENT, TRANSPARENT, TRANSPARENT, OUTLINE, OUTLINE, OUTLINE, OUTLINE, OUTLINE, OUTLINE, OUTLINE, OUTLINE, TRANSPARENT, TRANSPARENT, TRANSPARENT, TRANSPARENT,
];

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    loop {
        unsafe {
            core::arch::asm!("pause", options(nomem, nostack));
        }
    }
}

fn draw_image(x: u32, y: u32) {
    for row in 0..IMAGE_H {
        for col in 0..IMAGE_W {
            let color = IMAGE[row * IMAGE_W + col];
            if color != TRANSPARENT {
                libdunit::draw_rect(
                    x + col as u32 * SCALE,
                    y + row as u32 * SCALE,
                    SCALE,
                    SCALE,
                    color,
                );
            }
        }
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut fb = libdunit::FbInfo {
        addr: 0,
        width: 0,
        height: 0,
        pitch: 0,
    };

    if !libdunit::get_framebuffer(&mut fb) {
        libdunit::println("image_demo: framebuffer unavailable");
        libdunit::exit(1);
    }

    let image_w = IMAGE_W as u32 * SCALE;
    let image_h = IMAGE_H as u32 * SCALE;
    let x = fb.width.saturating_sub(image_w) / 2;
    let y = fb.height.saturating_sub(image_h) / 2;

    libdunit::draw_rect(x.saturating_sub(24), y.saturating_sub(24), image_w + 48, image_h + 48, 0x0008_1014);
    libdunit::draw_rect(x.saturating_sub(16), y.saturating_sub(16), image_w + 32, image_h + 32, 0x0018_252d);
    draw_image(x, y);

    libdunit::println("image_demo: rendered embedded 16x16 bitmap");
    libdunit::exit(0);
}
