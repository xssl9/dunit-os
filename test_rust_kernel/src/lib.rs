#![no_std]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

#[no_mangle]
pub extern "C" fn kernel_main() -> ! {
    let vga_buffer = 0xb8000 as *mut u8;
    
    unsafe {
        *vga_buffer.offset(0) = b'R';
        *vga_buffer.offset(1) = 0x0a;
        *vga_buffer.offset(2) = b'U';
        *vga_buffer.offset(3) = 0x0a;
        *vga_buffer.offset(4) = b'S';
        *vga_buffer.offset(5) = 0x0a;
        *vga_buffer.offset(6) = b'T';
        *vga_buffer.offset(7) = 0x0a;
    }
    
    loop {}
}
