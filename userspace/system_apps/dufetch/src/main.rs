#![no_std]
#![no_main]

//! dufetch — a neofetch-style system summary for Dunit OS, as a standalone
//! userspace program (`/app/dufetch`).
//!
//! Runs in BOTH terminals: kernel framebuffer Terminal Mode (`exec dufetch`,
//! stdout mirrored to serial) and the GUI terminal via `dsh` (`dufetch`, bridged
//! on an inner pty). It writes ONLY ANSI SGR color codes — the terminal's active
//! theme decides what each color looks like, so dufetch renders in whatever
//! palette the terminal is themed with (the whole point of the theme system).
//! Every fact is read through libdunit syscalls; nothing is hardcoded policy.

use core::panic::PanicInfo;

extern crate alloc;

use alloc::format;
use alloc::string::String;

use libdunit::{FbInfo, SystemStats};

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    libdunit::println("dufetch: PANIC");
    libdunit::exit(101)
}

/// The ASCII-art logo, shared with the (now-removed) kernel builtin. Baked into
/// the ELF at build time — not loaded from the VFS — so dufetch is self-contained.
const LOGO: &str = include_str!("../../../../assets/gui/dufetch_logo.txt");

// Logical ANSI colors (the terminal's theme maps these to real RGB).
const C_RESET: &str = "\x1b[0m";
const C_LOGO: &str = "\x1b[32m"; // green
const C_LABEL: &str = "\x1b[1;36m"; // bold cyan
const C_VALUE: &str = "\x1b[97m"; // bright white
const C_DIM: &str = "\x1b[90m"; // bright black (dim)

fn out(s: &str) {
    libdunit::write(1, s.as_bytes());
}

/// One "Label: value" line, label themed one color, value another.
fn info(label: &str, value: &str) {
    out(C_LABEL);
    out(label);
    out(C_RESET);
    out(C_VALUE);
    out(value);
    out(C_RESET);
    out("\n");
}

fn cwd_string() -> String {
    let mut b = [0u8; 256];
    let n = libdunit::getcwd(&mut b);
    if n > 0 {
        String::from(core::str::from_utf8(&b[..n as usize]).unwrap_or("/"))
    } else {
        String::from("/")
    }
}

// APPEND_RENDER

fn render() {
    let mut s = SystemStats::default();
    libdunit::get_system_stats(&mut s);
    let mut fb = FbInfo { addr: 0, width: 0, height: 0, pitch: 0 };
    libdunit::get_framebuffer(&mut fb);
    let pid = libdunit::get_pid();
    let cwd = cwd_string();

    // Plain-ASCII title: doubles as a stable serial marker (no SGR bytes in it).
    out("dufetch: system summary\n\n");

    // Logo, themed (leading blank lines of the art are skipped to save height).
    let mut started = false;
    for line in LOGO.lines() {
        if !started && line.trim().is_empty() {
            continue;
        }
        started = true;
        out(C_LOGO);
        out(line);
        out(C_RESET);
        out("\n");
    }
    out("\n");

    info("OS:      ", "Dunit OS");
    info("Kernel:  ", "1.0.0 Green Tea");
    info("Arch:    ", "x86_64");
    info("Shell:   ", "dsh");
    info("FS:      ", "MemFS over VFS");
    if fb.width > 0 && fb.height > 0 {
        info("Display: ", &format!("{}x{}", fb.width, fb.height));
    } else {
        info("Display: ", "framebuffer");
    }
    info("PID:     ", &format!("{}", pid));
    info("CWD:     ", &cwd);
    info(
        "Memory:  ",
        &format!("{} KiB / {} KiB", s.pmm_used_bytes / 1024, s.pmm_total_bytes / 1024),
    );
    info(
        "Tasks:   ",
        &format!("{} ({} running)", s.process_total, s.process_running),
    );

    // Color swatches: the active theme's 8 normal + 8 bright ANSI slots, drawn
    // with ASCII '#' (renders in both terminals; a block glyph would not).
    out("\n");
    out(C_DIM);
    out("palette: ");
    out(C_RESET);
    for i in 0..8 {
        out(&format!("\x1b[3{}m###", i));
    }
    out(C_RESET);
    out("\n         ");
    for i in 0..8 {
        out(&format!("\x1b[9{}m###", i));
    }
    out(C_RESET);
    out("\n");
}

#[no_mangle]
pub extern "C" fn _start(
    argc: usize,
    argv: *const *const u8,
    envp: *const *const u8,
) -> ! {
    libdunit::init_runtime(argc, argv, envp);
    render();
    libdunit::exit(0)
}
