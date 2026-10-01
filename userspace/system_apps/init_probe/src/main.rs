#![no_std]
#![no_main]

//! Test-only fixture for the init service manager (M5 item 3).
//!
//! It prints a marker and exits with a non-zero code on every run so init's
//! `restart = on-failure` policy and restart-budget exhaustion are exercised
//! deterministically under tools/qemu_test.py. Shipped as a service only for the
//! smoke configs (see services/test/init-probe.toml + tools/pack_initrd.py); the
//! production desktop never spawns it.

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    loop {
        core::hint::spin_loop();
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    libdunit::println("[PROBE] init-probe run; exiting with failure to exercise restart");
    libdunit::exit(7);
}
