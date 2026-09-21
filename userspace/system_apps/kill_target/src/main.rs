#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_: &PanicInfo) -> ! { loop { core::hint::spin_loop(); } }

#[no_mangle]
pub extern "C" fn _start() -> ! {
    loop { core::hint::spin_loop(); }
}
