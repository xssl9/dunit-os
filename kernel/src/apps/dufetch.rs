use core::fmt::Write;

use crate::shell::{ShellSink, SinkWriter};
use crate::{memory, process};

const LOGO_WIDTH: usize = 80;
const LOGO: &str = include_str!("../../../assets/gui/dufetch_logo.txt");
const COLOR_DARK_GREEN: u32 = 0x2f7a4e;
const COLOR_SAGE: u32 = 0x9bb59c;
const COLOR_SOFT_WHITE: u32 = 0xfffdf1;
const COLOR_MUTED: u32 = 0xa9bda8;

fn write_padded(out: &mut dyn ShellSink, text: &str, width: usize) {
    let mut written = text.len();
    for ch in text.chars() {
        match ch {
            '@' | '%' => out.set_fg_color(COLOR_SOFT_WHITE),
            '*' | '+' => out.set_fg_color(COLOR_SAGE),
            '=' | '-' | ':' | '.' => out.set_fg_color(COLOR_DARK_GREEN),
            _ => out.set_fg_color(COLOR_MUTED),
        }
        let mut buf = [0u8; 4];
        out.write_str(ch.encode_utf8(&mut buf));
    }
    while written < width {
        out.write_str(" ");
        written += 1;
    }
    out.reset_fg_color();
}

fn write_label(out: &mut dyn ShellSink, label: &str) {
    out.set_fg_color(COLOR_DARK_GREEN);
    out.write_str(label);
    out.reset_fg_color();
}

fn write_value(out: &mut dyn ShellSink, value: &str) {
    out.set_fg_color(COLOR_SOFT_WHITE);
    out.write_str(value);
    out.reset_fg_color();
}

fn write_info(out: &mut dyn ShellSink, label: &str, value: &str) {
    write_label(out, label);
    write_value(out, value);
}

/// Compact, vertically stacked variant for narrow windows (the GUI terminal):
/// the original art first, then the info block below it, so no line overflows
/// the window width.
pub fn run_stacked(out: &mut dyn ShellSink, cwd: &str) {
    let pid = process::current_process()
        .map(|process| process.pid.0)
        .unwrap_or(0);

    let mut memory_available_kib = 0usize;
    let mut memory_total_kib = 0usize;
    if let Some(pmm) = memory::pmm::get_pmm() {
        memory_available_kib = pmm.available_memory() / 1024;
        memory_total_kib = pmm.total_memory() / 1024;
    }

    let mut started = false;
    for logo_line in LOGO.lines() {
        // Skip the leading blank lines of the art to save vertical space.
        if !started && logo_line.trim().is_empty() {
            continue;
        }
        started = true;
        out.set_fg_color(COLOR_SAGE);
        out.write_str(logo_line);
        out.reset_fg_color();
        out.write_str("\n");
    }

    out.write_str("\n");
    write_info(out, "OS: ", "Dunit OS");
    out.write_str("\n");
    write_info(out, "Kernel: ", "1.0.0 Green Tea");
    out.write_str("\n");
    write_info(out, "Arch: ", "x86_64");
    out.write_str("\n");
    write_info(out, "Mode: ", "GUI");
    out.write_str("\n");
    write_info(out, "Shell: ", "Dunit GUI Terminal");
    out.write_str("\n");
    write_info(out, "FS: ", "MemFS over VFS");
    out.write_str("\n");
    write_label(out, "PID: ");
    out.set_fg_color(COLOR_SOFT_WHITE);
    let _ = write!(SinkWriter { sink: &mut *out }, "{}", pid);
    out.reset_fg_color();
    out.write_str("\n");
    write_label(out, "CWD: ");
    write_value(out, cwd);
    out.write_str("\n");
    if memory_total_kib > 0 {
        let used_kib = memory_total_kib.saturating_sub(memory_available_kib);
        write_label(out, "Memory: ");
        out.set_fg_color(COLOR_SOFT_WHITE);
        let _ = write!(
            SinkWriter { sink: &mut *out },
            "{} KiB / {} KiB",
            used_kib,
            memory_total_kib
        );
        out.reset_fg_color();
        out.write_str("\n");
    } else {
        write_info(out, "Memory: ", "available");
        out.write_str("\n");
    }
    write_info(out, "Display: ", "Framebuffer");
    out.write_str("\n");
}

pub fn run(out: &mut dyn ShellSink, cwd: &str) {
    let pid = process::current_process()
        .map(|process| process.pid.0)
        .unwrap_or(0);

    let mut memory_available_kib = 0usize;
    let mut memory_total_kib = 0usize;
    if let Some(pmm) = memory::pmm::get_pmm() {
        memory_available_kib = pmm.available_memory() / 1024;
        memory_total_kib = pmm.total_memory() / 1024;
    }

    for (idx, logo_line) in LOGO.lines().enumerate() {
        write_padded(out, logo_line, LOGO_WIDTH);
        out.write_str("  ");

        match idx {
            3 => {
                out.set_fg_color(COLOR_SOFT_WHITE);
                out.write_str("Dunit OS");
                out.reset_fg_color();
            }
            4 => write_info(out, "OS: ", "Dunit OS"),
            5 => write_info(out, "Kernel: ", "1.0.0 Green Tea"),
            6 => write_info(out, "Arch: ", "x86_64"),
            7 => write_info(out, "Mode: ", "Terminal"),
            8 => write_info(out, "Shell: ", "Dunit Terminal"),
            9 => write_info(out, "FS: ", "MemFS over VFS"),
            10 => {
                write_label(out, "PID: ");
                out.set_fg_color(COLOR_SOFT_WHITE);
                let _ = write!(SinkWriter { sink: &mut *out }, "{}", pid);
                out.reset_fg_color();
            }
            11 => {
                write_label(out, "CWD: ");
                write_value(out, cwd);
            }
            12 => {
                if memory_total_kib > 0 {
                    let used_kib = memory_total_kib.saturating_sub(memory_available_kib);
                    write_label(out, "Memory: ");
                    out.set_fg_color(COLOR_SOFT_WHITE);
                    let _ = write!(
                        SinkWriter { sink: &mut *out },
                        "{} KiB / {} KiB",
                        used_kib,
                        memory_total_kib
                    );
                    out.reset_fg_color();
                } else {
                    write_info(out, "Memory: ", "available");
                }
            }
            13 => write_info(out, "Display: ", "Framebuffer"),
            15 => {
                out.set_fg_color(COLOR_DARK_GREEN);
                out.write_str("green");
                out.reset_fg_color();
                out.write_str("  ");
                out.set_fg_color(COLOR_SAGE);
                out.write_str("sage");
                out.reset_fg_color();
                out.write_str("  ");
                out.set_fg_color(COLOR_SOFT_WHITE);
                out.write_str("white");
                out.reset_fg_color();
            }
            _ => {}
        }

        out.write_str("\n");
    }
}
