#![no_std]
#![no_main]

//! Tiny console fixture: prints a handful of SGR-colored lines and exits.
//!
//! Launched from `dsh` (`color_test`), it exercises the full path — external
//! program launch through the inner-pty bridge plus the gui_terminal ANSI/SGR
//! parser — with a deterministic, self-terminating workload (unlike the
//! full-screen `dtop`). Each line writes an `ESC[<code>m` prefix, the label,
//! and `ESC[0m` to reset, so the terminal renders it in the mapped color.

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    libdunit::exit(101)
}

/// Write one colored line: `ESC[<sgr>m<label>ESC[0m\n`.
fn line(sgr: &[u8], label: &[u8]) {
    libdunit::write(1, b"\x1b[");
    libdunit::write(1, sgr);
    libdunit::write(1, b"m");
    libdunit::write(1, label);
    libdunit::write(1, b"\x1b[0m\n");
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    libdunit::write(1, b"color_test: SGR palette\n");
    line(b"31", b"red 31");
    line(b"32", b"green 32");
    line(b"33", b"yellow 33");
    line(b"34", b"blue 34");
    line(b"35", b"magenta 35");
    line(b"36", b"cyan 36");
    line(b"1;37", b"bright white 1;37");
    line(b"90", b"bright black 90");
    libdunit::write(1, b"color_test: done\n");
    libdunit::exit(0)
}
