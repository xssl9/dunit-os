#![no_std]
#![no_main]

//! Minimal PTY-slave shell for the M4 userspace terminal (Stack B).
//!
//! Spawned by `gui_terminal` through `pty_spawn`, so fd 0/1/2 are routed to the
//! pty rings (not the console). It reads bytes from stdin, echoes printable
//! characters back on stdout so the terminal window shows what is being typed
//! (cooked-style local echo), and on a newline runs one of a small set of
//! builtins, writing the result to stdout. The gui_terminal client interprets
//! that stdout byte stream into its on-screen scrollback.

use core::panic::PanicInfo;

extern crate alloc;

use alloc::string::String;

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    libdunit::println("dsh: PANIC");
    libdunit::exit(101)
}

const PROMPT: &str = "dunit$ ";

fn out(s: &str) {
    libdunit::write(1, s.as_bytes());
}

/// Current working directory as a String, defaulting to "/" on error.
fn cwd() -> String {
    let mut b = [0u8; 256];
    let n = libdunit::getcwd(&mut b);
    if n > 0 {
        String::from(core::str::from_utf8(&b[..n as usize]).unwrap_or("/"))
    } else {
        String::from("/")
    }
}

fn cmd_ls(path: &str) {
    let mut raw = [libdunit::DirEntry::empty(); 64];
    let n = libdunit::readdir(path, &mut raw);
    if n < 0 {
        out("ls: cannot read directory\n");
        return;
    }
    for e in raw.iter().take(n as usize) {
        out(e.name());
        if e.file_type == libdunit::FILE_TYPE_DIRECTORY {
            out("/");
        }
        out("\n");
    }
}

/// Execute one entered command line.
fn run(line: &str) {
    let line = line.trim();
    if line.is_empty() {
        return;
    }
    let (cmd, rest) = match line.find(' ') {
        Some(i) => (&line[..i], line[i + 1..].trim()),
        None => (line, ""),
    };
    match cmd {
        "help" => out("builtins: help echo pwd ls cd clear uname exit\n"),
        "echo" => {
            out(rest);
            out("\n");
        }
        "pwd" => {
            out(&cwd());
            out("\n");
        }
        "ls" => {
            let p = if rest.is_empty() { cwd() } else { String::from(rest) };
            cmd_ls(&p);
        }
        "cd" => {
            let p = if rest.is_empty() { "/" } else { rest };
            if libdunit::chdir(p) < 0 {
                out("cd: no such directory\n");
            }
        }
        "uname" => out("Dunit OS x86_64 (M4)\n"),
        // Form feed: the terminal treats 0x0C as "clear the scrollback".
        "clear" => out("\x0c"),
        "exit" => {
            out("bye\n");
            libdunit::exit(0);
        }
        _ => {
            out(cmd);
            out(": command not found\n");
        }
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    out(PROMPT);
    let mut line = String::new();
    let mut buf = [0u8; 128];
    loop {
        let n = libdunit::read(0, &mut buf);
        if n == libdunit::EAGAIN {
            // Block on a short timer instead of spinning on yield: the timer-wake
            // path reliably re-schedules us, whereas a bare yield can be dropped
            // from the round-robin under timer preemption.
            libdunit::sleep_ms(5);
            continue;
        }
        if n <= 0 {
            // EOF (master gone) or error — nothing more to do.
            libdunit::exit(0);
        }
        for &b in &buf[..n as usize] {
            match b {
                b'\r' | b'\n' => {
                    out("\n");
                    let entered = core::mem::take(&mut line);
                    run(&entered);
                    out(PROMPT);
                }
                0x20..=0x7e => {
                    if line.len() < 256 {
                        line.push(b as char);
                        libdunit::write(1, &[b]); // local echo
                    }
                }
                _ => {}
            }
        }
    }
}
