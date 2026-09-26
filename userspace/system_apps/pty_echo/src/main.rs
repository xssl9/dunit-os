#![no_std]
#![no_main]

//! PTY slave peer for the M4 slice 8 PTY-endpoint smoke test.
//!
//! Spawned by `pty_test` through `pty_spawn`, so fd 0/1/2 are routed to the pty
//! rings (not the console). It reads one chunk from stdin — cooperatively
//! yielding while the ring is empty (EAGAIN) — then echoes it back on stdout
//! prefixed with `echo:` and exits. That round-trip is what the master verifies.

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    libdunit::println("pty_echo: PANIC");
    libdunit::exit(101)
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut buf = [0u8; 128];
    loop {
        let n = libdunit::read(0, &mut buf);
        if n == libdunit::EAGAIN {
            libdunit::yield_now();
            continue;
        }
        if n <= 0 {
            // EOF (master gone) or error — nothing to echo.
            libdunit::exit(0);
        }
        libdunit::write(1, b"echo:");
        libdunit::write(1, &buf[..n as usize]);
        libdunit::exit(0);
    }
}
