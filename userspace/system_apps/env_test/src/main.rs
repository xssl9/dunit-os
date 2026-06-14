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
pub extern "C" fn _start(
    argc: usize,
    argv: libdunit::RawArgv,
    envp: libdunit::RawEnvp,
) -> ! {
    libdunit::init_runtime(argc, argv, envp);
    libdunit::println("env_test: start");
    print_env("PATH");
    print_env("SHELL");
    print_env("CWD");
    libdunit::print("argc=");
    libdunit::print_usize(libdunit::argc());
    libdunit::print("\nargv0=");
    libdunit::println(libdunit::arg(0).unwrap_or("<missing>"));
    libdunit::println("env_test: OK");
    libdunit::exit(0);
}

fn print_env(name: &str) {
    libdunit::print(name);
    libdunit::print("=");
    libdunit::println(libdunit::getenv(name).unwrap_or("<missing>"));
}
