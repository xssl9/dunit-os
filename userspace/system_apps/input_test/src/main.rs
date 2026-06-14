#![no_std]
#![no_main]

extern crate alloc;

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
pub extern "C" fn _start(
    argc: usize,
    argv: libdunit::RawArgv,
    envp: libdunit::RawEnvp,
) -> ! {
    libdunit::init_runtime(argc, argv, envp);
    libdunit::print("input_test> ");

    match libdunit::read_line() {
        Ok(line) => {
            libdunit::print("you typed: ");
            libdunit::println(&line);
            libdunit::exit(0);
        }
        Err(code) => {
            libdunit::print("input_test: read_line failed ");
            libdunit::println(libdunit::error_name(code));
            libdunit::exit(1);
        }
    }
}
