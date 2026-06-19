#![no_std]
#![no_main]

use bootloader::{entry_point, BootInfo};
use core::panic::PanicInfo;

entry_point!(kernel_main);

fn kernel_main(_boot_info: &'static BootInfo) -> ! {
    let vga = 0xb8000 as *mut u16;

    unsafe {
        for i in 0..80 * 25 {
            *vga.add(i) = 0x0F20;
        }

        let msg = b"DUNIT OS WORKS!";
        for (i, &c) in msg.iter().enumerate() {
            *vga.add(i) = 0x0A00 | c as u16;
        }
    }

    loop {
        unsafe {
            core::arch::asm!("hlt");
        }
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}
