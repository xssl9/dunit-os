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
    let mut buf = [0u8; 16];
    let read = libdunit::read(0, &mut buf);
    if read < 0 {
        libdunit::println("stdin_test: read failed");
        libdunit::exit(1);
    }

    libdunit::print("stdin_test: read=");
    libdunit::print_usize(read as usize);
    libdunit::print("\n");
    libdunit::exit(0);
}
