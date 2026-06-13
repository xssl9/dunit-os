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

fn spawn_ready(label: &str) -> u32 {
    let pid = libdunit::spawn("elf_demo");
    if pid < 0 {
        libdunit::print("spawn_ready_test: spawn failed: ");
        libdunit::println(label);
        libdunit::exit(1);
    }

    let mut status = libdunit::WaitStatus::empty();
    let waited = libdunit::wait(pid as u32, &mut status);
    if waited != libdunit::EAGAIN {
        libdunit::print("spawn_ready_test: ready child wait should be EAGAIN: ");
        libdunit::println(label);
        libdunit::exit(2);
    }
    if status.kind != libdunit::WAIT_KIND_EMPTY || status.code != 0 {
        libdunit::print("spawn_ready_test: status changed unexpectedly: ");
        libdunit::println(label);
        libdunit::exit(3);
    }

    pid as u32
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    libdunit::println("spawn_ready_test: start");

    let first = spawn_ready("first");
    let second = spawn_ready("second");
    if first == second {
        libdunit::println("spawn_ready_test: duplicate pid");
        libdunit::exit(4);
    }

    let yielded = libdunit::yield_now();
    if yielded != 0 {
        libdunit::println("spawn_ready_test: yield should run one ready child");
        libdunit::exit(5);
    }

    let mut status = libdunit::WaitStatus::empty();
    let waited = libdunit::wait(first, &mut status);
    if waited != first as isize {
        libdunit::println("spawn_ready_test: first child wait failed");
        libdunit::exit(6);
    }
    if !status.exited() || status.code != 0 {
        libdunit::println("spawn_ready_test: first child status mismatch");
        libdunit::exit(7);
    }

    let second_wait = libdunit::wait(second, &mut status);
    if second_wait != libdunit::EAGAIN {
        libdunit::println("spawn_ready_test: second child should still be ready");
        libdunit::exit(8);
    }

    libdunit::println("spawn_ready_test: OK");
    libdunit::exit(0);
}
