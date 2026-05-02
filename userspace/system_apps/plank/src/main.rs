#![no_std]
#![no_main]

extern crate plank;

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    plank::plank_main();
    loop {}
}
