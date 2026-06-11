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
    libdunit::println("path_test: spawn elf_demo");

    let pid = libdunit::spawn("elf_demo");
    if pid < 0 {
        libdunit::println("path_test: spawn failed");
        libdunit::exit(1);
    }

    let mut status = libdunit::WaitStatus::empty();
    let waited = libdunit::wait(pid as u32, &mut status);
    if waited < 0 {
        libdunit::println("path_test: wait failed");
        libdunit::exit(2);
    }

    if status.exited() {
        libdunit::println("path_test: spawn falsely reported exit");
        libdunit::exit(3);
    }

    if !status.spawn_prepared() || status.code != 0 {
        libdunit::println("path_test: bad prepared status");
        libdunit::exit(4);
    }

    libdunit::println("path_test: prepared/not-started OK");
    libdunit::exit(0);
}
