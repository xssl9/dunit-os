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
    libdunit::println("resumable_test: start");

    let child = libdunit::spawn("resumable_child");
    if child < 0 {
        libdunit::println("resumable_test: spawn failed");
        libdunit::exit(1);
    }

    if libdunit::yield_now() != 0 {
        libdunit::println("resumable_test: first yield failed");
        libdunit::exit(2);
    }

    let mut status = libdunit::WaitStatus::empty();
    if libdunit::wait(child as u32, &mut status) != libdunit::EAGAIN {
        libdunit::println("resumable_test: child should be ready after A");
        libdunit::exit(3);
    }

    libdunit::println("resumable_test: B");

    if libdunit::yield_now() != 0 {
        libdunit::println("resumable_test: second yield failed");
        libdunit::exit(4);
    }

    let waited = libdunit::wait(child as u32, &mut status);
    if waited != child {
        libdunit::println("resumable_test: wait returned wrong pid");
        libdunit::exit(5);
    }
    if !status.exited() || status.code != 7 {
        libdunit::println("resumable_test: child status mismatch");
        libdunit::exit(6);
    }

    libdunit::println("resumable_test: OK");
    libdunit::exit(0);
}
