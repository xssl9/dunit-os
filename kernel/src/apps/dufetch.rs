use core::fmt::Write;

use crate::{memory, process, terminal};

const LOGO_WIDTH: usize = 80;
const LOGO: &str = include_str!("../../../assets/gui/dufetch_logo.txt");
const COLOR_DARK_GREEN: u32 = 0x2f7a4e;
const COLOR_SAGE: u32 = 0x9bb59c;
const COLOR_SOFT_WHITE: u32 = 0xfffdf1;
const COLOR_MUTED: u32 = 0xa9bda8;

struct ConsoleWriter<'a> {
    console: &'a mut terminal::FbConsole,
}

impl core::fmt::Write for ConsoleWriter<'_> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        self.console.write_str(s);
        Ok(())
    }
}

fn write_padded(console: &mut terminal::FbConsole, text: &str, width: usize) {
    let mut written = text.len();
    for ch in text.chars() {
        match ch {
            '@' | '%' => console.set_fg_color(COLOR_SOFT_WHITE),
            '*' | '+' => console.set_fg_color(COLOR_SAGE),
            '=' | '-' | ':' | '.' => console.set_fg_color(COLOR_DARK_GREEN),
            _ => console.set_fg_color(COLOR_MUTED),
        }
        let mut buf = [0u8; 4];
        console.write_str(ch.encode_utf8(&mut buf));
    }
    while written < width {
        console.write_str(" ");
        written += 1;
    }
    console.reset_fg_color();
}

fn write_label(console: &mut terminal::FbConsole, label: &str) {
    console.set_fg_color(COLOR_DARK_GREEN);
    console.write_str(label);
    console.reset_fg_color();
}

fn write_value(console: &mut terminal::FbConsole, value: &str) {
    console.set_fg_color(COLOR_SOFT_WHITE);
    console.write_str(value);
    console.reset_fg_color();
}

fn write_info(console: &mut terminal::FbConsole, label: &str, value: &str) {
    write_label(console, label);
    write_value(console, value);
}

pub fn run(console: &mut terminal::FbConsole, cwd: &str) {
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
        write_padded(console, logo_line, LOGO_WIDTH);
        console.write_str("  ");

        match idx {
            3 => {
                console.set_fg_color(COLOR_SOFT_WHITE);
                console.write_str("Dunit OS");
                console.reset_fg_color();
            }
            4 => write_info(console, "OS: ", "Dunit OS"),
            5 => write_info(console, "Kernel: ", "1.0.0 Green Tea"),
            6 => write_info(console, "Arch: ", "x86_64"),
            7 => write_info(console, "Mode: ", "Terminal"),
            8 => write_info(console, "Shell: ", "Dunit Terminal"),
            9 => write_info(console, "FS: ", "MemFS over VFS"),
            10 => {
                write_label(console, "PID: ");
                console.set_fg_color(COLOR_SOFT_WHITE);
                let _ = write!(ConsoleWriter { console: &mut *console }, "{}", pid);
                console.reset_fg_color();
            }
            11 => {
                write_label(console, "CWD: ");
                write_value(console, cwd);
            }
            12 => {
                if memory_total_kib > 0 {
                    let used_kib = memory_total_kib.saturating_sub(memory_available_kib);
                    write_label(console, "Memory: ");
                    console.set_fg_color(COLOR_SOFT_WHITE);
                    let _ = write!(
                        ConsoleWriter { console: &mut *console },
                        "{} KiB / {} KiB",
                        used_kib,
                        memory_total_kib
                    );
                    console.reset_fg_color();
                } else {
                    write_info(console, "Memory: ", "available");
                }
            }
            13 => write_info(console, "Display: ", "Framebuffer"),
            15 => {
                console.set_fg_color(COLOR_DARK_GREEN);
                console.write_str("green");
                console.reset_fg_color();
                console.write_str("  ");
                console.set_fg_color(COLOR_SAGE);
                console.write_str("sage");
                console.reset_fg_color();
                console.write_str("  ");
                console.set_fg_color(COLOR_SOFT_WHITE);
                console.write_str("white");
                console.reset_fg_color();
            }
            _ => {}
        }

        console.write_str("\n");
    }
}
