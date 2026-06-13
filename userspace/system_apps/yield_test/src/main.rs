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
    libdunit::println("yield_test: start");

    let child = libdunit::spawn("yield_child");
    if child < 0 {
        libdunit::println("yield_test: spawn failed");
        libdunit::exit(1);
    }

    if libdunit::yield_now() != 0 {
        libdunit::println("yield_test: yield failed");
        libdunit::exit(2);
    }

    let mut status = libdunit::WaitStatus::empty();
    let waited = libdunit::wait(child as u32, &mut status);
    if waited != child {
        libdunit::println("yield_test: wait returned wrong pid");
        libdunit::exit(3);
    }
    if !status.exited() || status.code != 0 {
        libdunit::println("yield_test: child status mismatch");
        libdunit::exit(4);
    }

    libdunit::println("yield_test: OK");
    libdunit::exit(0);
}
