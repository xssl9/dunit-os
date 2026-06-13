#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    loop {
        unsafe {
            core::arch::asm!("pause", options(nomem, nostack));
        }
    }
}

const COLOR_RESET: &str = "\x1b[0m";
const COLOR_HEADER: &str = "\x1b[1;37;44m";
const COLOR_TITLE: &str = "\x1b[1;32m";
const COLOR_BAR: &str = "\x1b[1;36m";
const COLOR_PROC: &str = "\x1b[37m";

#[no_mangle]
pub extern "C" fn _start(
    _argc: usize,
    _argv: libdunit::RawArgv,
    _envp: libdunit::RawEnvp,
) -> ! {
    // Clear screen and hide cursor
    libdunit::write(1, b"\x1b[2J\x1b[H\x1b[?25l");

    loop {
        // Move cursor to top
        libdunit::write(1, b"\x1b[H");

        draw_header();
        draw_cpu_mem();
        draw_process_list();

        // Check for any key to exit
        if libdunit::get_key().is_some() {
            break;
        }

        libdunit::sleep_ms(500);
    }

    // Show cursor and clear screen
    libdunit::write(1, b"\x1b[?25h\x1b[2J\x1b[H");
    libdunit::exit(0);
}

fn draw_header() {
    libdunit::write(1, COLOR_HEADER.as_bytes());
    libdunit::write(1, b" DUNIT TOP - System Monitor                       ");
    libdunit::write(1, COLOR_RESET.as_bytes());
    libdunit::write(1, b"\n");
}

fn draw_cpu_mem() {
    // CPU
    libdunit::write(1, COLOR_TITLE.as_bytes());
    libdunit::write(1, b" CPU ");
    libdunit::write(1, COLOR_RESET.as_bytes());
    libdunit::write(1, b"[");
    libdunit::write(1, COLOR_BAR.as_bytes());
    libdunit::write(1, b"||||||||||          ");
    libdunit::write(1, COLOR_RESET.as_bytes());
    libdunit::write(1, b"] 50.0%\n");

    // MEM from /proc/meminfo
    let mut mem_data = [0u8; 256];
    let fd = libdunit::open("/proc/meminfo", libdunit::OPEN_READ);
    if fd >= 0 {
        let n = libdunit::read(fd as usize, &mut mem_data);
        libdunit::close(fd as usize);
        if n > 0 {
            let s = core::str::from_utf8(&mem_data[..n as usize]).unwrap_or("");
            let total = parse_kv(s, "total:").unwrap_or(0);
            let free = parse_kv(s, "free:").unwrap_or(0);
            let used = total.saturating_sub(free);
            
            libdunit::write(1, COLOR_TITLE.as_bytes());
            libdunit::write(1, b" MEM ");
            libdunit::write(1, COLOR_RESET.as_bytes());
            libdunit::write(1, b"[");
            libdunit::write(1, COLOR_BAR.as_bytes());
            
            let bars = if total > 0 { used * 20 / total } else { 0 };
            for i in 0..20 {
                if i < bars {
                    libdunit::write(1, b"|");
                } else {
                    libdunit::write(1, b" ");
                }
            }
            
            libdunit::write(1, COLOR_RESET.as_bytes());
            libdunit::write(1, b"] ");
            print_usize(used / 1024 / 1024);
            libdunit::write(1, b"MB / ");
            print_usize(total / 1024 / 1024);
            libdunit::write(1, b"MB\n\n");
        }
    } else {
        libdunit::write(1, b" MEM [N/A]\n\n");
    }
}

fn draw_process_list() {
    libdunit::write(1, b"\x1b[7m PID  STATE      NAME                               \x1b[0m\n");
    
    let fd = libdunit::open("/proc/processes", libdunit::OPEN_READ);
    if fd >= 0 {
        let mut proc_data = [0u8; 1024];
        let n = libdunit::read(fd as usize, &mut proc_data);
        libdunit::close(fd as usize);
        if n > 0 {
            libdunit::write(1, COLOR_PROC.as_bytes());
            libdunit::write(1, &proc_data[..n as usize]);
            libdunit::write(1, COLOR_RESET.as_bytes());
        }
    }
}

fn print_usize(value: usize) {
    let mut buf = [0u8; 20];
    let mut len = 0;
    
    let mut v = value;
    let mut digits = [0u8; 20];
    let mut index = digits.len();
    
    if v == 0 {
        libdunit::write(1, b"0");
        return;
    }
    
    while v > 0 {
        index -= 1;
        digits[index] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    libdunit::write(1, &digits[index..]);
}

fn parse_kv(data: &str, key: &str) -> Option<usize> {
    for line in data.lines() {
        if line.starts_with(key) {
            let val_part = line[key.len()..].trim();
            return val_part.parse::<usize>().ok();
        }
    }
    None
}
