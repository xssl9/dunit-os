#![no_std]
#![no_main]

//! PTY master driver for the M4 slice 8 PTY-endpoint smoke test.
//!
//! The terminal-emulator side of the pty. Creates a pty, spawns `pty_echo` as
//! its slave (stdin/stdout routed to the pty rings), writes a line to the
//! slave's stdin via the master, then polls the master side until the echoed
//! `echo:ping` comes back — proving master->slave->master byte streaming end to
//! end. Exits 0 on a verified round-trip, 1 otherwise; the kernel smoke harness
//! checks that exit code.

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    libdunit::println("pty_test: PANIC");
    libdunit::exit(101)
}

/// Naive substring search — no alloc, small fixed buffers only.
fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() {
        return true;
    }
    if haystack.len() < needle.len() {
        return false;
    }
    let last = haystack.len() - needle.len();
    let mut i = 0;
    while i <= last {
        if &haystack[i..i + needle.len()] == needle {
            return true;
        }
        i += 1;
    }
    false
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    // 1. Open a pty; we become its master.
    let id = libdunit::pty_create();
    if id <= 0 {
        libdunit::println("pty_test: pty_create failed");
        libdunit::exit(1);
    }
    let id = id as u32;

    // 2. Spawn the echo peer as the slave end (its fd 0/1/2 -> pty rings).
    let child = libdunit::pty_spawn("pty_echo", id);
    if child <= 0 {
        libdunit::println("pty_test: pty_spawn failed");
        libdunit::exit(1);
    }

    // 3. Push a line into the slave's stdin from the master side.
    let msg = b"ping\n";
    if libdunit::pty_write(id, msg) <= 0 {
        libdunit::println("pty_test: pty_write failed");
        libdunit::exit(1);
    }

    // 4. Poll the master for the echoed bytes, accumulating across reads.
    let mut acc = [0u8; 256];
    let mut acc_len: usize = 0;
    let mut chunk = [0u8; 128];
    // Bounded spins so a wedged slave can't hang the smoke test forever.
    let mut spins = 0u32;
    loop {
        let n = libdunit::pty_read(id, &mut chunk);
        if n == libdunit::EPIPE {
            // Slave hung up and drained — no more bytes will arrive.
            libdunit::println("pty_test: slave hangup before echo");
            libdunit::exit(1);
        }
        if n == libdunit::EAGAIN || n == 0 {
            spins += 1;
            if spins > 100_000 {
                libdunit::println("pty_test: timed out waiting for echo");
                libdunit::exit(1);
            }
            libdunit::yield_now();
            continue;
        }
        if n < 0 {
            libdunit::println("pty_test: pty_read error");
            libdunit::exit(1);
        }
        // Append what we got, clamping to the accumulator capacity.
        let got = n as usize;
        let space = acc.len() - acc_len;
        let take = if got > space { space } else { got };
        acc[acc_len..acc_len + take].copy_from_slice(&chunk[..take]);
        acc_len += take;

        if contains(&acc[..acc_len], b"echo:ping") {
            libdunit::pty_close(id);
            libdunit::exit(0);
        }
        if acc_len == acc.len() {
            // Filled up without a match — the peer is echoing garbage.
            libdunit::println("pty_test: echo mismatch");
            libdunit::exit(1);
        }
    }
}
