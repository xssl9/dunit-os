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

fn expect_wait_again(pid: u32, label: &str) {
    let mut status = libdunit::WaitStatus::empty();
    let ret = libdunit::wait(pid, &mut status);
    if ret != libdunit::EAGAIN {
        libdunit::print("scheduler_test: wait should be EAGAIN: ");
        libdunit::println(label);
        libdunit::exit(2);
    }
    if status.kind != libdunit::WAIT_KIND_EMPTY || status.code != 0 {
        libdunit::print("scheduler_test: wait mutated status: ");
        libdunit::println(label);
        libdunit::exit(3);
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    libdunit::println("scheduler_test: start");

    let no_candidate = libdunit::yield_now();
    if no_candidate != libdunit::EAGAIN {
        libdunit::println("scheduler_test: empty yield should be EAGAIN");
        libdunit::exit(1);
    }

    let pid = libdunit::spawn("elf_demo");
    if pid < 0 {
        libdunit::println("scheduler_test: spawn failed");
        libdunit::exit(4);
    }

    expect_wait_again(pid as u32, "ready-child");

    let candidate = libdunit::yield_now();
    if candidate != libdunit::EOPNOTSUPP {
        libdunit::println("scheduler_test: yield should report switch unsupported");
        libdunit::exit(5);
    }

    libdunit::println("scheduler_test: OK");
    libdunit::exit(0);
}
