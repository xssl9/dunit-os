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

fn expect_wait_error(pid: u32, label: &str) {
    let mut status = libdunit::WaitStatus::empty();
    if libdunit::wait(pid, &mut status) >= 0 {
        libdunit::print("path_test: unexpected wait success: ");
        libdunit::println(label);
        libdunit::exit(10);
    }
}

fn spawn_and_reap(label: &str, use_wait_any: bool) -> u32 {
    let pid = libdunit::spawn("elf_demo");
    if pid < 0 {
        libdunit::print("path_test: spawn failed: ");
        libdunit::println(label);
        libdunit::exit(1);
    }

    let mut status = libdunit::WaitStatus::empty();
    let waited = if use_wait_any {
        libdunit::wait(0, &mut status)
    } else {
        libdunit::wait(pid as u32, &mut status)
    };
    if waited < 0 {
        libdunit::print("path_test: wait failed: ");
        libdunit::println(label);
        libdunit::exit(2);
    }
    if waited as u32 != pid as u32 {
        libdunit::print("path_test: wrong waited pid: ");
        libdunit::println(label);
        libdunit::exit(3);
    }
    if status.exited() {
        libdunit::print("path_test: spawn falsely reported exit: ");
        libdunit::println(label);
        libdunit::exit(4);
    }
    if !status.spawn_prepared() || status.code != 0 {
        libdunit::print("path_test: bad prepared status: ");
        libdunit::println(label);
        libdunit::exit(5);
    }

    pid as u32
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    libdunit::println("path_test: spawn/wait foundation");

    expect_wait_error(0xFFFF_FFFE, "invalid-pid");
    expect_wait_error(libdunit::get_pid(), "non-child-self");

    let reaped_pid = spawn_and_reap("prepared-by-pid", false);
    expect_wait_error(reaped_pid, "reaped-pid");

    let any_pid = spawn_and_reap("prepared-by-wait-any", true);
    expect_wait_error(any_pid, "wait-any-reaped-pid");
    expect_wait_error(0, "empty-wait-any");

    let mut index = 0;
    while index < 3 {
        spawn_and_reap("repeated", false);
        index += 1;
    }

    let cleanup_pid = libdunit::spawn("elf_demo");
    if cleanup_pid < 0 {
        libdunit::println("path_test: parent-cleanup spawn failed");
        libdunit::exit(6);
    }

    libdunit::println("path_test: prepared/reap OK");
    libdunit::exit(0);
}
