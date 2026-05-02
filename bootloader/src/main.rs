#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let vga_buffer = 0xb8000 as *mut u16;
    let message = b"Rust Bootloader!";
    
    unsafe {
        for (i, &byte) in message.iter().enumerate() {
            *vga_buffer.offset(i as isize) = (byte as u16) | 0x0F00;
        }
    }
    
    loop {}
}
