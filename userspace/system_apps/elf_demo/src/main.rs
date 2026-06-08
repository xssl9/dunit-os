#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    loop {
        unsafe {
            core::arch::asm!("pause", options(nomem, nostack));
        }
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    libdunit::println("hello from ELF userspace");
    libdunit::exit(0);
}
