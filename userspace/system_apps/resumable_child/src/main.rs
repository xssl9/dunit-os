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
    libdunit::println("resumable_child: A");
    if libdunit::yield_now() != 0 {
        libdunit::println("resumable_child: yield failed");
        libdunit::exit(1);
    }
    libdunit::println("resumable_child: C");
    libdunit::exit(7);
}
