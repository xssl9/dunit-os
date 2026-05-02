#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let vga = 0xb8000 as *mut u16;
    let message = "=== Dunit OS (Green Tea) - System Ready! ===";
    
    unsafe {
        for (i, byte) in message.bytes().enumerate() {
            *vga.add(i) = (0x0A00 | byte as u16);
        }
    }
    
    loop {
        unsafe {
            core::arch::asm!("hlt");
        }
    }
}
