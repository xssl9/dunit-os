use core::fmt::Write;

use crate::{memory, process, terminal};

const LOGO_WIDTH: usize = 80;
const LOGO: &str = include_str!("../../assets/dufetch_logo.txt");

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
    console.write_str(text);
    let mut written = text.len();
    while written < width {
        console.write_str(" ");
        written += 1;
    }
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
            4 => console.write_str("OS: Dunit OS"),
            5 => console.write_str("Kernel: 1.0.0 Green Tea"),
            6 => console.write_str("Arch: x86_64"),
            7 => console.write_str("Mode: Terminal"),
            8 => console.write_str("Shell: Dunit Terminal"),
            9 => console.write_str("FS: MemFS over VFS"),
            10 => {
                let _ = write!(ConsoleWriter { console: &mut *console }, "PID: {}", pid);
            }
            11 => {
                console.write_str("CWD: ");
                console.write_str(cwd);
            }
            12 => {
                if memory_total_kib > 0 {
                    let used_kib = memory_total_kib.saturating_sub(memory_available_kib);
                    let _ = write!(
                        ConsoleWriter { console: &mut *console },
                        "Memory: {} KiB / {} KiB",
                        used_kib,
                        memory_total_kib
                    );
                } else {
                    console.write_str("Memory: available");
                }
            }
            13 => console.write_str("Display: Framebuffer"),
            _ => {}
        }

        console.write_str("\n");
    }
}
