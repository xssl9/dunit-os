#![no_std]
#![no_main]

//! dsh ⇄ calc cooked-bridge smoke driver (M4 Subtask 4).
//!
//! Proves that a classic terminal-mode userspace program — `calc`, which does
//! NOT echo its own input and relies on a cooked line discipline (whole lines
//! terminated by '\n') — runs correctly when launched from `dsh`'s
//! `spawn_external` bridge. That is the exact path the GUI terminal takes
//! (gui_terminal → dsh → child), so a pass here means the GUI terminal behaves
//! like kernel Terminal Mode.
//!
//! We become dsh's pty MASTER (the role gui_terminal plays), drive the dsh
//! prompt to launch `calc`, then feed an expression and assert BOTH that dsh
//! echoed our keystrokes back (the cooked discipline — the fix under test; calc
//! itself never echoes) and that calc evaluated and answered. Sends are STAGED
//! on observed output so a line meant for calc can't be misrouted to dsh's own
//! line editor. Exits 0 on a verified session, 1 otherwise; the kernel smoke
//! harness checks the exit code.

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    libdunit::println("dsh_calc_test: PANIC");
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

/// Cumulative capture of everything read off the pty master, so a needle that
/// spans two reads (or already arrived earlier) is still found.
struct Capture {
    buf: [u8; 1024],
    len: usize,
}

impl Capture {
    fn new() -> Self {
        Capture {
            buf: [0u8; 1024],
            len: 0,
        }
    }

    /// Drop everything captured so far, so a later milestone starts with the
    /// full buffer instead of accumulating earlier output toward the overflow
    /// cap. Only called at points where no needle we still need has arrived yet.
    fn reset(&mut self) {
        self.len = 0;
    }

    /// Pump the pty master until `needle` appears in the cumulative capture.
    /// Returns false on hangup-before-match, read error, overflow, or timeout.
    fn pump_until(&mut self, id: u32, needle: &[u8]) -> bool {
        if contains(&self.buf[..self.len], needle) {
            return true;
        }
        let mut chunk = [0u8; 128];
        // Bounded spins so a wedged child can't hang the smoke test forever.
        let mut spins = 0u32;
        loop {
            let n = libdunit::pty_read(id, &mut chunk);
            if n == libdunit::EPIPE {
                return false; // slave hung up before the needle arrived
            }
            if n == libdunit::EAGAIN || n == 0 {
                spins += 1;
                if spins > 2_000_000 {
                    return false;
                }
                libdunit::yield_now();
                continue;
            }
            if n < 0 {
                return false;
            }
            let got = n as usize;
            let space = self.buf.len() - self.len;
            if space == 0 {
                return false; // filled up without a match
            }
            let take = if got > space { space } else { got };
            self.buf[self.len..self.len + take].copy_from_slice(&chunk[..take]);
            self.len += take;
            if contains(&self.buf[..self.len], needle) {
                return true;
            }
        }
    }
}

/// Report a failure, tear down the dsh subtree, and exit non-zero.
fn fail(id: u32, child: u32, msg: &str) -> ! {
    libdunit::println(msg);
    libdunit::kill(child);
    libdunit::pty_close(id);
    libdunit::exit(1)
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    // Become dsh's pty master — exactly the role gui_terminal plays.
    let id = libdunit::pty_create();
    if id <= 0 {
        libdunit::println("dsh_calc_test: pty_create failed");
        libdunit::exit(1);
    }
    let id = id as u32;

    // Spawn dsh as the pty slave (its fd 0/1/2 routed to the pty rings).
    let child = libdunit::pty_spawn("dsh", id);
    if child <= 0 {
        libdunit::println("dsh_calc_test: pty_spawn dsh failed");
        libdunit::pty_close(id);
        libdunit::exit(1);
    }
    let child = child as u32;

    let mut cap = Capture::new();

    // 1. dsh prints its prompt on startup -> dsh booted on the pty.
    if !cap.pump_until(id, b"dsh ") {
        fail(id, child, "dsh_calc_test: no dsh prompt");
    }

    // 1b. `ls /` exercises dsh's readdir() syscall — the exact path that failed
    //     with EINVAL in the GUI terminal, because dsh asks for 256 directory
    //     entries while the kernel used to cap a request at 64 and reject it. A
    //     real root entry ("system") in the output proves readdir now serves the
    //     full request instead of erroring.
    if libdunit::pty_write(id, b"ls /\n") <= 0 {
        fail(id, child, "dsh_calc_test: pty_write ls failed");
    }
    if !cap.pump_until(id, b"system") {
        fail(id, child, "dsh_calc_test: ls / did not list the root (readdir EINVAL?)");
    }
    cap.reset();

    // 2. Launch calc through dsh's dispatch (the spawn_external bridge). Wait
    //    for calc's own prompt BEFORE sending the expression, so dsh has fully
    //    entered the bridge and the next line reaches calc, not dsh's editor.
    if libdunit::pty_write(id, b"calc\n") <= 0 {
        fail(id, child, "dsh_calc_test: pty_write calc failed");
    }
    if !cap.pump_until(id, b"calc> ") {
        fail(id, child, "dsh_calc_test: calc did not start under dsh");
    }

    // 3. Feed an expression. "12+30"/"42" appear nowhere in calc's banner, so
    //    the echo back is unambiguously dsh's cooked discipline (calc never
    //    echoes) and the answer is unambiguously calc's evaluation.
    if libdunit::pty_write(id, b"12+30\n") <= 0 {
        fail(id, child, "dsh_calc_test: pty_write expr failed");
    }
    if !cap.pump_until(id, b"12+30") {
        fail(id, child, "dsh_calc_test: no cooked echo of input");
    }
    if !cap.pump_until(id, b"42") {
        fail(id, child, "dsh_calc_test: calc did not evaluate the expression");
    }

    // 4. Leave calc; its goodbye proves the child exited cleanly through the
    //    bridge and dsh regained control.
    if libdunit::pty_write(id, b"exit\n") <= 0 {
        fail(id, child, "dsh_calc_test: pty_write exit failed");
    }
    if !cap.pump_until(id, b"calc: bye") {
        fail(id, child, "dsh_calc_test: calc did not exit cleanly");
    }

    // Verified. Tear down the dsh subtree so nothing lingers past the smoke.
    libdunit::kill(child);
    libdunit::pty_close(id);
    libdunit::exit(0)
}
