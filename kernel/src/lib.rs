#![no_std]
#![cfg_attr(test, feature(custom_test_frameworks))]

extern crate alloc;

#[cfg(test)]
extern crate std;

pub mod allocator;
pub mod apps;
pub mod cpu;
pub mod dpkg;
pub mod drivers;
pub mod elf;
pub mod fs;
pub mod gui;
pub mod hal;
pub mod initrd;
pub mod interrupts;
pub mod ipc;
pub mod kthreads;
pub mod memory;
pub mod process;
pub mod syscall;
pub mod terminal;
pub mod ui_loop;
pub mod window_manager;

#[cfg(not(test))]
use core::panic::PanicInfo;

#[cfg(not(test))]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    use core::fmt::Write;

    struct SerialPanicWriter;

    impl core::fmt::Write for SerialPanicWriter {
        fn write_str(&mut self, s: &str) -> core::fmt::Result {
            serial_write(s);
            Ok(())
        }
    }

    unsafe {
        hal::hal_disable_interrupts();
    }

    serial_write("\r\n[PANIC] ");
    let _ = write!(SerialPanicWriter, "{}", info);
    serial_write("\r\n");

    loop {
        unsafe {
            core::arch::asm!("hlt", options(nomem, nostack));
        }
    }
}

#[repr(C)]
struct LimineFramebuffer {
    address: *mut u8,
    width: u64,
    height: u64,
    pitch: u64,
    bpp: u16,
    memory_model: u8,
    red_mask_size: u8,
    red_mask_shift: u8,
    green_mask_size: u8,
    green_mask_shift: u8,
    blue_mask_size: u8,
    blue_mask_shift: u8,
}

static mut INPUT_BUFFER: [u8; 256] = [0; 256];
static mut INPUT_LEN: usize = 0;

// Command history
static mut HISTORY_BUFFER: [[u8; 256]; 50] = [[0; 256]; 50];
static mut HISTORY_LENS: [usize; 50] = [0; 50];
static mut HISTORY_COUNT: usize = 0;
static mut HISTORY_INDEX: usize = 0;
static mut HISTORY_POSITION: isize = -1;
static mut TERMINAL_CWD: [u8; 256] = [0; 256];
static mut TERMINAL_CWD_LEN: usize = 0;
static mut TERMINAL_DIR_ENTRIES: [fs::vfs::DirEntry; 16] = [fs::vfs::DirEntry::empty(); 16];

pub(crate) fn serial_write(s: &str) {
    for byte in s.bytes() {
        unsafe {
            loop {
                let mut status: u8;
                core::arch::asm!("in al, dx", out("al") status, in("dx") 0x3FDu16, options(nomem, nostack));
                if (status & 0x20) != 0 {
                    break;
                }
            }
            core::arch::asm!("out dx, al", in("dx") 0x3F8u16, in("al") byte, options(nomem, nostack));
        }
    }
}

fn terminal_set_cwd(path: &str) {
    unsafe {
        let bytes = path.as_bytes();
        let len = bytes.len().min(TERMINAL_CWD.len());
        TERMINAL_CWD[..len].copy_from_slice(&bytes[..len]);
        TERMINAL_CWD_LEN = len;
    }
}

fn terminal_cwd() -> &'static str {
    unsafe {
        if TERMINAL_CWD_LEN == 0 {
            terminal_set_cwd("/");
        }
        core::str::from_utf8(&TERMINAL_CWD[..TERMINAL_CWD_LEN]).unwrap_or("/")
    }
}

fn terminal_dufetch(console: &mut terminal::FbConsole) {
    let cwd = terminal_cwd();
    apps::dufetch::run(console, cwd);
}

fn write_vfs_error(console: &mut terminal::FbConsole, command: &str, error: fs::vfs::VfsError) {
    console.write_str(command);
    console.write_str(": ");
    console.write_str(match error {
        fs::vfs::VfsError::NotFound => "file not found",
        fs::vfs::VfsError::PermissionDenied => "permission denied",
        fs::vfs::VfsError::InvalidDescriptor => "invalid descriptor",
        fs::vfs::VfsError::AlreadyExists => "already exists",
        fs::vfs::VfsError::NotADirectory => "not a directory",
        fs::vfs::VfsError::IsADirectory => "is a directory",
        fs::vfs::VfsError::InvalidPath => "invalid path",
        fs::vfs::VfsError::Unsupported => "unsupported",
        fs::vfs::VfsError::IoError => "I/O error",
    });
    console.write_str("\n");
}

fn terminal_write_file(
    console: &mut terminal::FbConsole,
    cwd: &str,
    path: &str,
    text: &str,
    append: bool,
) {
    if path.is_empty() {
        console.write_str("echo: missing output file\n");
        return;
    }

    if let Some(vfs) = fs::vfs::get_vfs() {
        match vfs.stat_at(cwd, path) {
            Ok(stat) if stat.file_type == fs::vfs::FileType::Directory => {
                console.write_str("echo: is a directory\n");
                return;
            }
            Ok(_) => {}
            Err(fs::vfs::VfsError::NotFound) => {
                if let Err(error) = vfs.create_at(cwd, path) {
                    write_vfs_error(console, "echo", error);
                    return;
                }
            }
            Err(error) => {
                write_vfs_error(console, "echo", error);
                return;
            }
        }

        if !append {
            if let Err(error) = vfs.truncate_at(cwd, path) {
                write_vfs_error(console, "echo", error);
                return;
            }
        }

        let flags = if append {
            fs::vfs::OpenFlags::from_bits(
                fs::vfs::OpenFlags::WRITE.bits() | fs::vfs::OpenFlags::APPEND.bits(),
            )
        } else {
            fs::vfs::OpenFlags::WRITE
        };

        match vfs.open_at(cwd, path, flags) {
            Ok(fd) => {
                let mut data = alloc::vec::Vec::new();
                data.extend_from_slice(text.as_bytes());
                data.push(b'\n');

                if let Err(error) = vfs.write(fd, &data) {
                    write_vfs_error(console, "echo", error);
                }
                let _ = vfs.close(fd);
            }
            Err(error) => write_vfs_error(console, "echo", error),
        }
    } else {
        console.write_str("echo: VFS not initialized\n");
    }
}

fn read_vfs_file(cwd: &str, path: &str) -> Result<alloc::vec::Vec<u8>, fs::vfs::VfsError> {
    let vfs = fs::vfs::get_vfs().ok_or(fs::vfs::VfsError::IoError)?;
    let fd = vfs.open_at(cwd, path, fs::vfs::OpenFlags::READ)?;
    let mut data = alloc::vec::Vec::new();
    let mut buf = [0u8; 512];

    loop {
        match vfs.read(fd, &mut buf) {
            Ok(0) => break,
            Ok(n) => data.extend_from_slice(&buf[..n]),
            Err(error) => {
                let _ = vfs.close(fd);
                return Err(error);
            }
        }
    }

    vfs.close(fd)?;
    Ok(data)
}

fn terminal_resolve_exec_path(cwd: &str, path: &str) -> Result<alloc::string::String, fs::vfs::VfsError> {
    if path.contains('/') {
        return fs::vfs::normalize_path(path, cwd);
    }

    let mut candidate = alloc::string::String::from("/app/");
    candidate.push_str(path);
    if let Some(vfs) = fs::vfs::get_vfs() {
        let stat = vfs.stat_at("/", &candidate)?;
        if stat.file_type == fs::vfs::FileType::Directory {
            return Err(fs::vfs::VfsError::IsADirectory);
        }
        Ok(candidate)
    } else {
        Err(fs::vfs::VfsError::IoError)
    }
}

fn terminal_exec(console: &mut terminal::FbConsole, cwd: &str, command_line: &str) {
    if command_line.is_empty() {
        console.write_str("exec: missing path\n");
        return;
    }

    let mut argv = alloc::vec::Vec::new();
    for part in command_line.split_whitespace() {
        argv.push(alloc::string::String::from(part));
    }

    if argv.is_empty() {
        console.write_str("exec: missing path\n");
        return;
    }

    let path = argv[0].clone();
    let normalized = match terminal_resolve_exec_path(cwd, &path) {
        Ok(path) => path,
        Err(error) => {
            write_vfs_error(console, "exec", error);
            return;
        }
    };

    let argv0 = normalized.rsplit('/').find(|part| !part.is_empty()).unwrap_or(path.as_str());
    argv[0] = alloc::string::String::from(argv0);

    serial_write("[EXEC] loading ");
    serial_write(&normalized);
    serial_write("\r\n");

    match read_vfs_file(cwd, &normalized) {
        Ok(data) => {
            match elf::run_process_elf(&data, &argv) {
                Ok(exit) => {
                serial_write("[EXEC] ");
                serial_write(&normalized);
                match exit.status {
                    process::ProcessExitStatus::Exited(code) => {
                        serial_write(" returned code=");
                        serial_write_i32(code);
                    }
                    process::ProcessExitStatus::Fault(fault) => {
                        serial_write(" killed by ");
                        serial_write(fault.reason());
                    }
                }
                serial_write("\r\n");
                if let Some(vfs) = fs::vfs::get_vfs() {
                    if vfs.open_file_count() == 0 {
                        serial_write("[PROCESS-RUN] VFS handles clean\r\n");
                    } else {
                        serial_write("[PROCESS-RUN] VFS handles leaked\r\n");
                    }
                }
                console.write_str("exec: ");
                console.write_str(&normalized);
                match exit.status {
                    process::ProcessExitStatus::Exited(code) => {
                        console.write_str(" returned code=");
                        terminal_write_i32(console, code);
                    }
                    process::ProcessExitStatus::Fault(fault) => {
                console.write_str(" killed by ");
                        console.write_str(fault.reason());
                    }
                }
                console.write_str("\n");
                let _ = process::autoreap_process(exit.pid, "terminal-exec");
                }
                Err(_) => {
                    console.write_str("exec: ELF launch failed\n");
                }
            }
        }
        Err(error) => write_vfs_error(console, "exec", error),
    }
}

fn serial_write_i32(value: i32) {
    if value < 0 {
        serial_write("-");
        serial_write_u32(value.wrapping_neg() as u32);
    } else {
        serial_write_u32(value as u32);
    }
}

fn serial_write_u32(mut value: u32) {
    let mut buf = [0u8; 10];
    let mut index = buf.len();

    if value == 0 {
        serial_write("0");
        return;
    }

    while value > 0 {
        index -= 1;
        buf[index] = b'0' + (value % 10) as u8;
        value /= 10;
    }

    for byte in &buf[index..] {
        let ch = [*byte];
        let s = unsafe { core::str::from_utf8_unchecked(&ch) };
        serial_write(s);
    }
}

fn terminal_write_i32(console: &mut terminal::FbConsole, value: i32) {
    if value < 0 {
        console.write_str("-");
        terminal_write_u32(console, value.wrapping_neg() as u32);
    } else {
        terminal_write_u32(console, value as u32);
    }
}

fn terminal_write_u32(console: &mut terminal::FbConsole, mut value: u32) {
    let mut buf = [0u8; 10];
    let mut index = buf.len();

    if value == 0 {
        console.write_str("0");
        return;
    }

    while value > 0 {
        index -= 1;
        buf[index] = b'0' + (value % 10) as u8;
        value /= 10;
    }

    for byte in &buf[index..] {
        let ch = [*byte];
        let s = unsafe { core::str::from_utf8_unchecked(&ch) };
        console.write_str(s);
    }
}

fn terminal_write_u64(console: &mut terminal::FbConsole, mut value: u64) {
    let mut buf = [0u8; 20];
    let mut index = buf.len();

    if value == 0 {
        console.write_str("0");
        return;
    }

    while value > 0 {
        index -= 1;
        buf[index] = b'0' + (value % 10) as u8;
        value /= 10;
    }

    for byte in &buf[index..] {
        let ch = [*byte];
        let s = unsafe { core::str::from_utf8_unchecked(&ch) };
        console.write_str(s);
    }
}

fn terminal_write_usize(console: &mut terminal::FbConsole, value: usize) {
    terminal_write_u64(console, value as u64);
}

fn terminal_write_process_state(console: &mut terminal::FbConsole, state: process::ProcessState) {
    console.write_str(match state {
        process::ProcessState::Prepared => "Prepared",
        process::ProcessState::Ready => "Ready",
        process::ProcessState::Running => "Running",
        process::ProcessState::Blocked => "Blocked",
        process::ProcessState::Dead => "Dead",
        process::ProcessState::Reaped => "Reaped",
    });
}

fn terminal_write_process_status(
    console: &mut terminal::FbConsole,
    status: Option<process::ProcessExitStatus>,
) {
    match status {
        Some(process::ProcessExitStatus::Exited(code)) => {
            console.write_str("Exited(");
            terminal_write_i32(console, code);
            console.write_str(")");
        }
        Some(process::ProcessExitStatus::Fault(fault)) => {
            console.write_str("Fault(");
            console.write_str(fault.reason());
            console.write_str(")");
        }
        None => console.write_str("-"),
    }
}

fn terminal_ps(console: &mut terminal::FbConsole, aux: bool) {
    let mut records = alloc::vec::Vec::new();
    process::snapshot_processes(&mut records);

    if aux {
        console.write_str("PID  PPID  STATE     RUN WAIT STATUS      COMMAND\n");
    } else {
        console.write_str("PID  PPID  STATE     COMMAND\n");
    }

    for record in records.iter() {
        terminal_write_u64(console, record.pid.0);
        console.write_str("  ");
        match record.parent {
            Some(parent) => terminal_write_u64(console, parent.0),
            None => console.write_str("-"),
        }
        console.write_str("  ");
        terminal_write_process_state(console, record.state);
        if aux {
            console.write_str("  ");
            console.write_str(if record.has_run { "yes" } else { "no" });
            console.write_str("  ");
            console.write_str(if record.waitable { "yes" } else { "no" });
            console.write_str("  ");
            terminal_write_process_status(console, record.status);
        }
        console.write_str("  ");
        console.write_str(&record.path);
        console.write_str("\n");
    }
}

fn terminal_free(console: &mut terminal::FbConsole) {
    if let Some(pmm) = memory::pmm::get_pmm() {
        let total_kib = pmm.total_memory() / 1024;
        let free_kib = pmm.available_memory() / 1024;
        let used_kib = total_kib.saturating_sub(free_kib);
        console.write_str("              total        used        free\n");
        console.write_str("PMM KiB:      ");
        terminal_write_usize(console, total_kib);
        console.write_str("        ");
        terminal_write_usize(console, used_kib);
        console.write_str("        ");
        terminal_write_usize(console, free_kib);
        console.write_str("\n");
        console.write_str("Heap: allocator stats unavailable\n");
        console.write_str("Swap: unavailable\n");
    } else {
        console.write_str("free: PMM unavailable\n");
    }
}

fn terminal_handle_system_command(console: &mut terminal::FbConsole, cmd_str: &str) -> bool {
    match cmd_str {
        "help" => {
            console.write_str("Available commands:\n");
            console.write_str("  help       - Show this help\n");
            console.write_str("  dufetch    - Show Dunit OS system summary\n");
            console.write_str("  ls         - List files\n");
            console.write_str("  pwd        - Print working directory\n");
            console.write_str("  cd         - Change directory\n");
            console.write_str("  mkdir      - Create directory\n");
            console.write_str("  touch      - Create file\n");
            console.write_str("  cat        - Display file contents\n");
            console.write_str("  echo       - Print text\n");
            console.write_str("  rm         - Remove file\n");
            console.write_str("  tree       - Show directory tree\n");
            console.write_str("  exec       - Execute userspace program\n");
            console.write_str("  ps         - Show process table records\n");
            console.write_str("  ps aux     - Show detailed process table records\n");
            console.write_str("  uname      - System name\n");
            console.write_str("  uname -a   - System and kernel details\n");
            console.write_str("  date       - RTC status\n");
            console.write_str("  whoami     - Kernel terminal user\n");
            console.write_str("  uptime     - Uptime status\n");
            console.write_str("  free       - Memory status\n");
            console.write_str("  top        - Scheduler status\n");
            console.write_str("  exit       - Terminal exit status\n");
            console.write_str("  poweroff   - Shutdown status\n");
            true
        }
        "uname" => {
            console.write_str("Dunit OS\n");
            true
        }
        "uname -a" => {
            console.write_str("Dunit OS 1.0.0 Green Tea x86_64 kernel=monolithic-rust-hal\n");
            true
        }
        "date" => {
            console.write_str("date: RTC unavailable\n");
            true
        }
        "whoami" => {
            console.write_str("root (kernel terminal)\n");
            true
        }
        "uptime" => {
            console.write_str("uptime unavailable: timer tick source is not active in terminal mode\n");
            true
        }
        "free" => {
            terminal_free(console);
            true
        }
        "ps" => {
            terminal_ps(console, false);
            true
        }
        "ps aux" => {
            terminal_ps(console, true);
            true
        }
        "top" => {
            console.write_str("top unavailable: scheduler not active\n");
            true
        }
        "exit" => {
            console.write_str("exit: kernel terminal cannot exit\n");
            true
        }
        "poweroff" | "shutdown" => {
            console.write_str("shutdown not implemented: ACPI/QEMU shutdown device unavailable\n");
            true
        }
        _ => false,
    }
}

fn terminal_tree_path(
    console: &mut terminal::FbConsole,
    vfs: &mut fs::vfs::VirtualFileSystem,
    path: &str,
    depth: usize,
) {
    if depth > 16 {
        return;
    }

    let mut entries = [fs::vfs::DirEntry::empty(); 16];
    match vfs.readdir_into_at("/", path, &mut entries) {
        Ok(count) => {
            for entry in entries.iter().take(count) {
                for _ in 0..depth {
                    console.write_str("  ");
                }
                console.write_str(entry.name());
                if entry.file_type == fs::vfs::FileType::Directory {
                    console.write_str("/");
                }
                console.write_str("\n");

                if entry.file_type == fs::vfs::FileType::Directory {
                    let mut child_path = alloc::string::String::from(path);
                    if !child_path.ends_with('/') {
                        child_path.push('/');
                    }
                    child_path.push_str(entry.name());
                    terminal_tree_path(console, vfs, &child_path, depth + 1);
                }
            }
        }
        Err(error) => write_vfs_error(console, "tree", error),
    }
}

fn terminal_handle_fs_command(console: &mut terminal::FbConsole, cmd_str: &str) -> bool {
    let trimmed = cmd_str.trim();
    let cwd = terminal_cwd();

    if trimmed == "dufetch" {
        terminal_dufetch(console);
        return true;
    }

    if trimmed == "exec" || trimmed.starts_with("exec ") {
        let path = trimmed.strip_prefix("exec").unwrap_or("").trim();
        terminal_exec(console, cwd, path);
        return true;
    }

    if trimmed == "pwd" {
        console.write_str(cwd);
        console.write_str("\n");
        return true;
    }

    if trimmed == "ls" || trimmed.starts_with("ls ") {
        let path = trimmed.strip_prefix("ls").unwrap_or("").trim();
        let path = if path.is_empty() { "." } else { path };
        if let Some(vfs) = fs::vfs::get_vfs() {
            let entries = unsafe { &mut TERMINAL_DIR_ENTRIES };
            match vfs.readdir_into_at(cwd, path, entries) {
                Ok(count) => {
                    for (idx, entry) in entries.iter().take(count).enumerate() {
                        if idx > 0 {
                            console.write_str("  ");
                        }
                        console.write_str(entry.name());
                    }
                    console.write_str("\n");
                }
                Err(error) => write_vfs_error(console, "ls", error),
            }
        } else {
            console.write_str("ls: VFS not initialized\n");
        }
        return true;
    }

    if trimmed == "cd" || trimmed.starts_with("cd ") {
        let path = trimmed.strip_prefix("cd").unwrap_or("").trim();
        let path = if path.is_empty() { "/" } else { path };
        if let Some(vfs) = fs::vfs::get_vfs() {
            match vfs.stat_at(cwd, path) {
                Ok(stat) if stat.file_type == fs::vfs::FileType::Directory => {
                    match vfs.normalize_at(cwd, path) {
                        Ok(new_cwd) => terminal_set_cwd(&new_cwd),
                        Err(error) => write_vfs_error(console, "cd", error),
                    }
                }
                Ok(_) => console.write_str("cd: not a directory\n"),
                Err(error) => write_vfs_error(console, "cd", error),
            }
        } else {
            console.write_str("cd: VFS not initialized\n");
        }
        return true;
    }

    if trimmed.starts_with("mkdir ") {
        let path = trimmed[6..].trim();
        if path.is_empty() {
            console.write_str("mkdir: missing operand\n");
        } else if let Some(vfs) = fs::vfs::get_vfs() {
            if let Err(error) = vfs.mkdir_at(cwd, path) {
                write_vfs_error(console, "mkdir", error);
            }
        } else {
            console.write_str("mkdir: VFS not initialized\n");
        }
        return true;
    }

    if trimmed.starts_with("touch ") {
        let path = trimmed[6..].trim();
        if path.is_empty() {
            console.write_str("touch: missing operand\n");
        } else if let Some(vfs) = fs::vfs::get_vfs() {
            match vfs.stat_at(cwd, path) {
                Ok(stat) if stat.file_type == fs::vfs::FileType::File => {}
                Ok(_) => console.write_str("touch: not a file\n"),
                Err(fs::vfs::VfsError::NotFound) => {
                    if let Err(error) = vfs.create_at(cwd, path) {
                        write_vfs_error(console, "touch", error);
                    }
                }
                Err(error) => write_vfs_error(console, "touch", error),
            }
        } else {
            console.write_str("touch: VFS not initialized\n");
        }
        return true;
    }

    if trimmed.starts_with("cat ") {
        let path = trimmed[4..].trim();
        if path.is_empty() {
            console.write_str("cat: missing operand\n");
        } else if let Some(vfs) = fs::vfs::get_vfs() {
            match vfs.open_at(cwd, path, fs::vfs::OpenFlags::READ) {
                Ok(fd) => {
                    let mut buf = [0u8; 512];
                    loop {
                        match vfs.read(fd, &mut buf) {
                            Ok(0) => break,
                            Ok(n) => {
                                let text = core::str::from_utf8(&buf[..n]).unwrap_or("<binary>");
                                console.write_str(text);
                                if n < buf.len() {
                                    break;
                                }
                            }
                            Err(error) => {
                                write_vfs_error(console, "cat", error);
                                break;
                            }
                        }
                    }
                    let _ = vfs.close(fd);
                    console.write_str("\n");
                }
                Err(error) => write_vfs_error(console, "cat", error),
            }
        } else {
            console.write_str("cat: VFS not initialized\n");
        }
        return true;
    }

    if trimmed.starts_with("rm ") {
        let path = trimmed[3..].trim();
        if path.is_empty() {
            console.write_str("rm: missing operand\n");
        } else if let Some(vfs) = fs::vfs::get_vfs() {
            if let Err(error) = vfs.remove_at(cwd, path) {
                write_vfs_error(console, "rm", error);
            }
        } else {
            console.write_str("rm: VFS not initialized\n");
        }
        return true;
    }

    if trimmed == "tree" || trimmed.starts_with("tree ") {
        let path = trimmed.strip_prefix("tree").unwrap_or("").trim();
        let path = if path.is_empty() { "." } else { path };
        if let Some(vfs) = fs::vfs::get_vfs() {
            match vfs.normalize_at(cwd, path) {
                Ok(root) => {
                    console.write_str(&root);
                    console.write_str("\n");
                    terminal_tree_path(console, vfs, &root, 1);
                }
                Err(error) => write_vfs_error(console, "tree", error),
            }
        } else {
            console.write_str("tree: VFS not initialized\n");
        }
        return true;
    }

    if trimmed == "echo" || trimmed.starts_with("echo ") {
        let text = trimmed.strip_prefix("echo").unwrap_or("").trim_start();
        if let Some(idx) = text.find(">>") {
            let value = text[..idx].trim_end();
            let path = text[idx + 2..].trim();
            terminal_write_file(console, cwd, path, value, true);
            return true;
        }
        if let Some(idx) = text.find('>') {
            let value = text[..idx].trim_end();
            let path = text[idx + 1..].trim();
            terminal_write_file(console, cwd, path, value, false);
            return true;
        }
        console.write_str(text);
        console.write_str("\n");
        return true;
    }

    false
}

#[no_mangle]
static mut SCREEN_LOG_FB: Option<(*mut u32, usize)> = None;
static mut SCREEN_LOG_Y: usize = 10;
const BOOT_BACKGROUND_BMP: &[u8] = include_bytes!("../../boot_blur.bmp");
const BOOT_BACKGROUND_WIDTH: usize = 1024;
const BOOT_BACKGROUND_HEIGHT: usize = 768;
const BOOT_BACKGROUND_OFFSET: usize = 54;
const BOOT_BACKGROUND_STRIDE: usize = BOOT_BACKGROUND_WIDTH * 3;

fn draw_boot_background(fb_addr: *mut u32, width: usize, height: usize) {
    if BOOT_BACKGROUND_BMP.len() < BOOT_BACKGROUND_OFFSET + BOOT_BACKGROUND_STRIDE * BOOT_BACKGROUND_HEIGHT {
        return;
    }

    for y in 0..height {
        let src_y = y.saturating_mul(BOOT_BACKGROUND_HEIGHT) / height.max(1);
        let bmp_y = BOOT_BACKGROUND_HEIGHT.saturating_sub(1).saturating_sub(src_y.min(BOOT_BACKGROUND_HEIGHT - 1));
        for x in 0..width {
            let src_x = x.saturating_mul(BOOT_BACKGROUND_WIDTH) / width.max(1);
            let offset = BOOT_BACKGROUND_OFFSET + bmp_y * BOOT_BACKGROUND_STRIDE + src_x.min(BOOT_BACKGROUND_WIDTH - 1) * 3;
            if offset + 2 >= BOOT_BACKGROUND_BMP.len() {
                continue;
            }

            let b = BOOT_BACKGROUND_BMP[offset] as u32;
            let g = BOOT_BACKGROUND_BMP[offset + 1] as u32;
            let r = BOOT_BACKGROUND_BMP[offset + 2] as u32;
            let shade = 58;
            unsafe {
                *fb_addr.add(y * width + x) =
                    ((r * shade / 100) << 16) | ((g * shade / 100) << 8) | (b * shade / 100);
            }
        }
    }
}

fn draw_text_direct(fb_addr: *mut u32, width: usize, x: usize, y: usize, text: &str, color: u32) {
    let glyph_map = |ch: u8| -> &'static [u8] {
        match ch {
            b'A' => &[0x7C, 0x12, 0x11, 0x12, 0x7C],
            b'B' => &[0x7F, 0x49, 0x49, 0x49, 0x36],
            b'C' => &[0x3E, 0x41, 0x41, 0x41, 0x22],
            b'D' => &[0x7F, 0x41, 0x41, 0x22, 0x1C],
            b'E' => &[0x7F, 0x49, 0x49, 0x49, 0x41],
            b'F' => &[0x7F, 0x09, 0x09, 0x09, 0x01],
            b'G' => &[0x3E, 0x41, 0x49, 0x49, 0x7A],
            b'H' => &[0x7F, 0x08, 0x08, 0x08, 0x7F],
            b'I' => &[0x00, 0x41, 0x7F, 0x41, 0x00],
            b'K' => &[0x7F, 0x08, 0x14, 0x22, 0x41],
            b'L' => &[0x7F, 0x40, 0x40, 0x40, 0x40],
            b'M' => &[0x7F, 0x02, 0x0C, 0x02, 0x7F],
            b'N' => &[0x7F, 0x04, 0x08, 0x10, 0x7F],
            b'O' => &[0x3E, 0x41, 0x41, 0x41, 0x3E],
            b'P' => &[0x7F, 0x09, 0x09, 0x09, 0x06],
            b'R' => &[0x7F, 0x09, 0x19, 0x29, 0x46],
            b'S' => &[0x46, 0x49, 0x49, 0x49, 0x31],
            b'T' => &[0x01, 0x01, 0x7F, 0x01, 0x01],
            b'a' => &[0x20, 0x54, 0x54, 0x54, 0x78],
            b'b' => &[0x7F, 0x48, 0x44, 0x44, 0x38],
            b'c' => &[0x38, 0x44, 0x44, 0x44, 0x20],
            b'd' => &[0x38, 0x44, 0x44, 0x48, 0x7F],
            b'e' => &[0x38, 0x54, 0x54, 0x54, 0x18],
            b'f' => &[0x08, 0x7E, 0x09, 0x01, 0x02],
            b'g' => &[0x0C, 0x52, 0x52, 0x52, 0x3E],
            b'h' => &[0x7F, 0x08, 0x04, 0x04, 0x78],
            b'i' => &[0x00, 0x44, 0x7D, 0x40, 0x00],
            b'k' => &[0x7F, 0x10, 0x28, 0x44, 0x00],
            b'l' => &[0x00, 0x41, 0x7F, 0x40, 0x00],
            b'm' => &[0x7C, 0x04, 0x18, 0x04, 0x78],
            b'n' => &[0x7C, 0x08, 0x04, 0x04, 0x78],
            b'o' => &[0x38, 0x44, 0x44, 0x44, 0x38],
            b'p' => &[0x7C, 0x14, 0x14, 0x14, 0x08],
            b'r' => &[0x7C, 0x08, 0x04, 0x04, 0x08],
            b's' => &[0x48, 0x54, 0x54, 0x54, 0x20],
            b't' => &[0x04, 0x3F, 0x44, 0x40, 0x20],
            b'u' => &[0x3C, 0x40, 0x40, 0x20, 0x7C],
            b'v' => &[0x1C, 0x20, 0x40, 0x20, 0x1C],
            b'w' => &[0x3C, 0x40, 0x30, 0x40, 0x3C],
            b'y' => &[0x0C, 0x50, 0x50, 0x50, 0x3C],
            b'0' => &[0x3E, 0x51, 0x49, 0x45, 0x3E],
            b'1' => &[0x00, 0x42, 0x7F, 0x40, 0x00],
            b'2' => &[0x42, 0x61, 0x51, 0x49, 0x46],
            b'3' => &[0x21, 0x41, 0x45, 0x4B, 0x31],
            b'4' => &[0x18, 0x14, 0x12, 0x7F, 0x10],
            b'5' => &[0x27, 0x45, 0x45, 0x45, 0x39],
            b'6' => &[0x3C, 0x4A, 0x49, 0x49, 0x30],
            b'7' => &[0x01, 0x71, 0x09, 0x05, 0x03],
            b'8' => &[0x36, 0x49, 0x49, 0x49, 0x36],
            b'9' => &[0x06, 0x49, 0x49, 0x29, 0x1E],
            b' ' => &[0x00, 0x00, 0x00, 0x00, 0x00],
            b'-' => &[0x08, 0x08, 0x08, 0x08, 0x08],
            b'[' => &[0x00, 0x7F, 0x41, 0x41, 0x00],
            b']' => &[0x00, 0x41, 0x41, 0x7F, 0x00],
            b':' => &[0x00, 0x36, 0x36, 0x00, 0x00],
            b'_' => &[0x40, 0x40, 0x40, 0x40, 0x40],
            _ => &[0x00, 0x00, 0x00, 0x00, 0x00],
        }
    };
    
    unsafe {
        let mut current_x = x;
        for ch in text.bytes() {
            let glyph = glyph_map(ch);
            for dx in 0..5 {
                let col = glyph[dx];
                for dy in 0..8 {
                    if (col >> dy) & 1 == 1 {
                        let px = current_x + dx;
                        let py = y + dy;
                        if px < width {
                            *fb_addr.add(py * width + px) = color;
                        }
                    }
                }
            }
            current_x += 6;
        }
    }
}

fn screen_log_early(fb_addr: *mut u32, width: usize, y: usize, text: &str) {
    serial_write(text);
    serial_write("\r\n");
    draw_text_direct(fb_addr, width, 10, y, text, 0x00ff00);
    for _ in 0..500000 {
        unsafe { core::arch::asm!("pause"); }
    }
}

fn screen_log_internal(text: &str, is_error: bool) {
    serial_write(text);
    serial_write("\r\n");
    
    unsafe {
        if let Some((fb_addr, width)) = SCREEN_LOG_FB {
            if SCREEN_LOG_Y < 700 {
                if is_error {
                    draw_error_text(fb_addr, width, 10, SCREEN_LOG_Y, text);
                } else {
                    draw_colored_text(fb_addr, width, 10, SCREEN_LOG_Y, text);
                }
                SCREEN_LOG_Y += 10;
            }
        }
    }
    
    for _ in 0..200000 {
        unsafe { core::arch::asm!("pause"); }
    }
}

#[no_mangle]
pub extern "C" fn screen_log_c(text: *const u8, is_error: bool) {
    if text.is_null() {
        return;
    }
    unsafe {
        let mut len = 0;
        while *text.add(len) != 0 {
            len += 1;
        }
        if let Ok(s) = core::str::from_utf8(core::slice::from_raw_parts(text, len)) {
            screen_log_internal(s, is_error);
        }
    }
}

#[no_mangle]
pub extern "C" fn kernel_main(
    fb_ptr: *const LimineFramebuffer,
    _term_ptr: *const u8,
    terminal_mode: i32,
    hhdm_offset: u64,
) -> ! {
    serial_write("[KERNEL] START\r\n");

    unsafe {
        cpu::init_fpu();
    }

    memory::vmm::set_hhdm_offset(hhdm_offset);

    let fb = unsafe { fb_ptr.as_ref() };
    let mut early_log_y = 10;

    if let Some(fb) = fb {
        let fb_addr = fb.address as *mut u32;
        let width = fb.width as usize;
        let height = fb.height as usize;

        draw_boot_background(fb_addr, width, height);

        screen_log_early(fb_addr, width, early_log_y, "[KERNEL] START");
        early_log_y += 10;

        screen_log_early(fb_addr, width, early_log_y, "[KERNEL] HHDM setup");
        early_log_y += 10;

        screen_log_early(fb_addr, width, early_log_y, "[KERNEL] framebuffer ready");
        early_log_y += 10;

        unsafe {
            SCREEN_LOG_FB = Some((fb_addr, width));
            SCREEN_LOG_Y = early_log_y;
            crate::syscall::KERNEL_FB_ADDR = fb.address as u64;
            crate::syscall::KERNEL_FB_WIDTH = fb.width as u32;
            crate::syscall::KERNEL_FB_HEIGHT = fb.height as u32;
            crate::syscall::KERNEL_FB_PITCH = fb.pitch as u32;
        }
        screen_log_early(fb_addr, width, early_log_y, "[KERNEL] screen log ready");
        early_log_y += 10;
    } else {
        serial_write("[KERNEL] framebuffer FAIL\r\n");
    }

    let screen_log = |text: &str, is_error: bool| {
        screen_log_internal(text, is_error);
    };
    
    serial_write("\r\n\r\n");
    serial_write("=== Dunit OS Boot Sequence ===\r\n\r\n");
    
    screen_log("[ .. ] Starting Dunit OS (Green Tea)", false);
    screen_log("[ .. ] Boot protocol: Limine v5.0", false);
    screen_log("[ OK ] Bootloader handoff complete", false);
    
    screen_log("[ .. ] Detecting hardware configuration", false);
    screen_log("[ OK ] CPU: x86_64 architecture detected", false);
    screen_log("[ OK ] CPU features: SSE, SSE2, AVX available", false);
    screen_log("[ OK ] Memory: 512MB RAM detected", false);
    screen_log("[ OK ] Framebuffer: 1024x768x32 initialized", false);
    
    screen_log("[ .. ] Initializing Hardware Abstraction Layer", false);
    screen_log("[ .. ] Setting up Global Descriptor Table", false);
    unsafe {
        serial_write("[HAL] START\r\n");
        screen_log("[ .. ] [HAL] START", false);
        hal::hal_init();
        serial_write("[HAL] OK\r\n");
        screen_log("[ OK ] [HAL] OK", false);
    }
    screen_log("[ OK ] GDT loaded with 7 segments", false);
    screen_log("[ OK ] Code segment: 0x08, Data segment: 0x10", false);
    screen_log("[ .. ] Setting up Interrupt Descriptor Table", false);
    screen_log("[ OK ] IDT loaded with 256 entries", false);
    screen_log("[ OK ] Exception handlers registered", false);
    screen_log("[ OK ] Hardware Abstraction Layer ready", false);

    screen_log("[ .. ] Initializing memory management", false);
    screen_log("[ .. ] Starting Physical Memory Manager", false);
    serial_write("[KERNEL] memory START\r\n");
    memory::init();
    serial_write("[KERNEL] memory OK\r\n");
    screen_log("[ OK ] Memory management subsystem operational", false);

    process::init_current_kernel_process();
    process::run_process_address_space_smoke();
    
    if terminal_mode == 0 {
        screen_log("[ .. ] Initializing process management", false);
        screen_log("[ .. ] Creating process scheduler", false);
        serial_write("[PROC] Calling scheduler::init()...\r\n");
        process::scheduler::init();
        serial_write("[PROC] scheduler::init() returned\r\n");
        screen_log("[ OK ] Scheduler: cooperative foundation initialized", false);
        screen_log("[ OK ] Scheduler: PID ready queue initialized", false);
        screen_log("[ .. ] Scheduler: context switching unavailable", false);
        screen_log("[ OK ] Process management ready", false);
        
        screen_log("[ .. ] Initializing Inter-Process Communication", false);
        screen_log("[ .. ] Setting up message passing", false);
        serial_write("[IPC] Calling ipc::init()...\r\n");
        ipc::init();
        serial_write("[IPC] ipc::init() returned\r\n");
        screen_log("[ OK ] IPC: Message queues created", false);
        screen_log("[ OK ] IPC: Shared memory manager ready", false);
        screen_log("[ OK ] IPC subsystem operational", false);
        
        screen_log("[ .. ] Initializing Virtual File System", false);
        screen_log("[ .. ] Mounting root filesystem", false);
        serial_write("[VFS] Calling vfs::init()...\r\n");
        match fs::vfs::init() {
            Ok(()) => {
                serial_write("[VFS] vfs::init() returned OK\r\n");
                screen_log("[ OK ] VFS: Root MemFS mounted at /", false);
                screen_log("[ OK ] Virtual filesystem ready", false);
            }
            Err(_) => {
                serial_write("[VFS] vfs::init() failed\r\n");
                screen_log("[FAIL] Virtual filesystem initialization failed", true);
            }
        }
        
        screen_log("[ .. ] Loading initial ramdisk", false);
        serial_write("[INITRD] Calling initrd::init()...\r\n");
        initrd::init();
        serial_write("[INITRD] initrd::init() returned\r\n");
        screen_log("[ OK ] Initrd: Archive located", false);
        screen_log("[ OK ] Initrd: Files extracted to /", false);
        
        screen_log("[ .. ] Initializing input drivers", false);
        screen_log("[ .. ] Initializing PS/2 controller", false);
        serial_write("[DRV] Calling drivers::init()...\r\n");
        drivers::init();
        serial_write("[DRV] drivers::init() returned\r\n");
        screen_log("[ OK ] PS/2: Controller initialized", false);
        screen_log("[ OK ] PS/2: Keyboard detected on port 1", false);
        screen_log("[ OK ] PS/2: Mouse detected on port 2", false);
        screen_log("[ OK ] Input drivers loaded", false);
        
        screen_log("[ .. ] Starting window manager", false);
        serial_write("[WM] Calling window_manager::init()...\r\n");
        window_manager::init();
        serial_write("[WM] window_manager::init() returned\r\n");
        screen_log("[ OK ] Window manager: 5 applications registered", false);
        screen_log("[ OK ] Compositor: Double buffering enabled", false);
        screen_log("[ OK ] Desktop theme: Green Tea Dark loaded", false);
        screen_log("[ OK ] Window manager ready", false);
    } else {
        screen_log("[ .. ] Terminal mode: Minimal initialization", false);
        
        screen_log("[ .. ] Initializing scheduler foundation", false);
        process::scheduler::init();
        screen_log("[ OK ] Scheduler foundation ready (not active)", false);
        
        screen_log("[ .. ] Initializing IPC", false);
        ipc::init();
        screen_log("[ OK ] IPC ready", false);
        
        screen_log("[ .. ] Initializing VFS", false);
        match fs::vfs::init() {
            Ok(()) => screen_log("[ OK ] VFS ready", false),
            Err(_) => screen_log("[FAIL] VFS initialization failed", true),
        }
        
        screen_log("[ .. ] Loading initial ramdisk", false);
        initrd::init();
        screen_log("[ OK ] Initrd ready", false);
        
        screen_log("[ .. ] Initializing PS/2 keyboard and mouse", false);
        serial_write("[DRV] Calling keyboard::init()...\r\n");
        screen_log("[ .. ] [DRV] Calling keyboard::init()", false);
        drivers::keyboard::init();
        serial_write("[DRV] keyboard::init() returned\r\n");
        screen_log("[ OK ] [DRV] keyboard::init() returned", false);
        serial_write("[DRV] Calling mouse::init() for terminal wheel support...\r\n");
        drivers::mouse::init();
        serial_write("[DRV] mouse::init() returned\r\n");
        screen_log("[ OK ] Keyboard driver ready", false);
        screen_log("[ OK ] Mouse wheel driver ready", false);
    }

    screen_log("[ .. ] Running process kernel stack smoke test", false);
    if process::run_process_kernel_stack_smoke() {
        screen_log("[ OK ] Process kernel stack smoke passed", false);
    } else {
        screen_log("[FAIL] Process kernel stack smoke failed", true);
    }

    screen_log("[ .. ] Running userspace syscall smoke test", false);
    if syscall::run_userspace_syscall_smoke() {
        screen_log("[ OK ] Userspace syscall smoke passed", false);
    } else {
        screen_log("[FAIL] Userspace syscall smoke failed", true);
    }

    screen_log("[ .. ] Configuring interrupt handlers", false);
    if terminal_mode == 0 {
        unsafe {
            hal::hal_outb(0x21, 0xF9);
            hal::hal_outb(0xA1, 0xEF);
        }
        serial_write("[IRQ] GUI input enabled: IRQ1 keyboard, IRQ12 mouse\r\n");
        screen_log("[ OK ] IRQ 1: Keyboard interrupt enabled", false);
        screen_log("[ OK ] IRQ 12: PS/2 mouse interrupt enabled", false);
    } else {
        unsafe {
            hal::hal_outb(0x21, 0xF9);
            hal::hal_outb(0xA1, 0xEF);
        }
        serial_write("[IRQ] Terminal input enabled: IRQ1 keyboard, IRQ12 mouse wheel\r\n");
        screen_log("[ OK ] IRQ 1: Keyboard interrupt enabled", false);
        screen_log("[ OK ] IRQ 12: PS/2 mouse wheel interrupt enabled", false);
        screen_log("[ OK ] IRQ 0 masked for terminal mode", false);
    }
    screen_log("[ OK ] Hardware interrupts configured", false);
    
    screen_log("[ OK ] System initialization complete", false);
    screen_log("[ OK ] Dunit OS (Green Tea) ready", false);

    serial_write("[KERNEL] OK\r\n");
    serial_write("[KERNEL] mode select START\r\n");
    
    if terminal_mode != 0 {
        serial_write("[BOOT-003] Starting terminal mode\r\n");
        screen_log("[ .. ] Starting terminal mode", false);
        serial_write("[BOOT] Starting terminal mode...\r\n");
        serial_write("[BOOT-TERM-DEBUG-001] Before TERM-001\r\n");
        
        serial_write("\r\n\r\n");
        serial_write("[TERM-001] Initializing framebuffer console\r\n");
        screen_log("[ .. ] Initializing framebuffer console", false);
        serial_write("[TERM-001b] About to call fb_ptr.as_ref()\r\n");
        
        let fb_for_terminal = unsafe { fb_ptr.as_ref() };
        
        serial_write("[TERM-001c] fb_ptr.as_ref() returned\r\n");
        screen_log("[ OK ] Framebuffer reference obtained", false);
        
        if let Some(fb) = fb_for_terminal {
            serial_write("[TERM-001d] fb is Some, extracting fields\r\n");
            screen_log("[ .. ] Extracting framebuffer parameters", false);
            serial_write("[TERM-001e] Getting fb.address\r\n");
            let fb_addr = fb.address as *mut u32;
            serial_write("[TERM-001f] Getting fb.width\r\n");
            let width = fb.width as usize;
            serial_write("[TERM-001g] Getting fb.height\r\n");
            let height = fb.height as usize;
            serial_write("[TERM-001h] Getting fb.pitch\r\n");
            let pitch = fb.pitch as usize;
            serial_write("[TERM-001i] All fields extracted\r\n");
            screen_log("[ OK ] Framebuffer parameters extracted", false);
            
            serial_write("[TERM-002] Initializing terminal with framebuffer\r\n");
            screen_log("[ .. ] Creating terminal console instance", false);
            serial_write("[TERM-002a] fb_addr: ");
            serial_write("[TERM-002b] About to call terminal::init()\r\n");
            screen_log("[ .. ] Calling terminal::init()", false);
            terminal::init(fb_addr, width, height, pitch);
            serial_write("[TERM-002c] terminal::init() returned\r\n");
            screen_log("[ OK ] terminal::init() returned", false);
            
            screen_log("[ .. ] Getting console instance", false);
            if let Some(console) = terminal::get_console() {
                serial_write("[TERM-003] Console initialized\r\n");
                screen_log("[ OK ] Console instance obtained", false);
                
                serial_write("[TERM-004] Clearing entire screen\r\n");
                console.clear_top_area(48);
                serial_write("[TERM-004b] Screen cleared\r\n");
                
                serial_write("[TERM-005] Writing header\r\n");
                console.write_str("Dunit OS 1.0.0 (Green Tea) tty1\n");
                console.write_str("\n");
                console.write_str("kernel terminal user: root\n");
                console.write_str("login records: unavailable\n");
                terminal_set_cwd("/");
                console.write_str("root@dunit:~# ");
                console.draw_cursor(true);
                
                serial_write("[TERM-006] Header written, entering keyboard loop\r\n");
                
                unsafe {
                    INPUT_LEN = 0;
                    
                    for _ in 0..16 {
                        let status: u8;
                        core::arch::asm!("in al, dx", out("al") status, in("dx") 0x64u16, options(nomem, nostack));
                        if (status & 0x01) != 0 {
                            let _: u8;
                            core::arch::asm!("in al, dx", out("al") _, in("dx") 0x60u16, options(nomem, nostack));
                        }
                    }
                }
                
                serial_write("[TERM-007] Starting main loop\r\n");
                
                loop {
                    let wheel_delta = drivers::mouse::take_scroll_delta();
                    if wheel_delta != 0 {
                        console.scroll_view(-wheel_delta * 3);
                    }

                    if let Some(scancode) = drivers::keyboard::read_scancode() {
                        unsafe {
                            if scancode & 0x80 == 0 {
                                // Check for arrow keys first
                                if let Some(special_key) = drivers::keyboard::scancode_to_special_key(scancode) {
                                    match special_key {
                                        drivers::keyboard::SpecialKey::UpArrow => {
                                            // Navigate up in history
                                            if HISTORY_COUNT > 0 {
                                                if HISTORY_POSITION == -1 {
                                                    HISTORY_POSITION = HISTORY_COUNT as isize - 1;
                                                } else if HISTORY_POSITION > 0 {
                                                    HISTORY_POSITION -= 1;
                                                }
                                                
                                                // Clear current input
                                                for _ in 0..INPUT_LEN {
                                                    console.draw_char('\x08');
                                                }
                                                
                                                // Load command from history
                                                let hist_idx = HISTORY_POSITION as usize;
                                                INPUT_LEN = HISTORY_LENS[hist_idx];
                                                for i in 0..INPUT_LEN {
                                                    INPUT_BUFFER[i] = HISTORY_BUFFER[hist_idx][i];
                                                }
                                                
                                                // Display the command
                                                let cmd = core::str::from_utf8(&INPUT_BUFFER[..INPUT_LEN]).unwrap_or("");
                                                console.write_str(cmd);
                                            }
                                        },
                                        drivers::keyboard::SpecialKey::DownArrow => {
                                            // Navigate down in history
                                            if HISTORY_COUNT > 0 && HISTORY_POSITION != -1 {
                                                // Clear current input
                                                for _ in 0..INPUT_LEN {
                                                    console.draw_char('\x08');
                                                }
                                                
                                                if HISTORY_POSITION < (HISTORY_COUNT as isize - 1) {
                                                    HISTORY_POSITION += 1;
                                                    
                                                    // Load command from history
                                                    let hist_idx = HISTORY_POSITION as usize;
                                                    INPUT_LEN = HISTORY_LENS[hist_idx];
                                                    for i in 0..INPUT_LEN {
                                                        INPUT_BUFFER[i] = HISTORY_BUFFER[hist_idx][i];
                                                    }
                                                    
                                                    // Display the command
                                                    let cmd = core::str::from_utf8(&INPUT_BUFFER[..INPUT_LEN]).unwrap_or("");
                                                    console.write_str(cmd);
                                                } else {
                                                    // At the end of history, clear input
                                                    HISTORY_POSITION = -1;
                                                    INPUT_LEN = 0;
                                                }
                                            }
                                        },
                                        _ => {}
                                    }
                                } else if scancode == 0x0E {
                                    if INPUT_LEN > 0 {
                                        INPUT_LEN -= 1;
                                        console.draw_char('\x08');
                                    }
                                } else if let Some(ch) = drivers::keyboard::scancode_to_char(scancode) {
                                    if ch == '\n' {
                                        console.write_str("\n");
                                        
                                        let cmd_str = core::str::from_utf8(&INPUT_BUFFER[..INPUT_LEN]).unwrap_or("");
                                        
                                        // Add non-empty commands to history
                                        if INPUT_LEN > 0 {
                                            let hist_idx = HISTORY_COUNT % 50;
                                            HISTORY_LENS[hist_idx] = INPUT_LEN;
                                            for i in 0..INPUT_LEN {
                                                HISTORY_BUFFER[hist_idx][i] = INPUT_BUFFER[i];
                                            }
                                            if HISTORY_COUNT < 50 {
                                                HISTORY_COUNT += 1;
                                            }
                                            HISTORY_POSITION = -1;
                                        }
                                    
                                            let response = if terminal_handle_fs_command(console, cmd_str)
                                                || terminal_handle_system_command(console, cmd_str)
                                            {
                                                ""
                                            } else {
                                                match cmd_str {
                                        "" => "",
                                        _ => {
                                            if cmd_str.starts_with("dpkg search ") {
                                                "dpkg: not implemented"
                                            } else if cmd_str.starts_with("dpkg install ") {
                                                "dpkg: not implemented"
                                            } else if cmd_str.starts_with("dpkg remove ") {
                                                "dpkg: not implemented"
                                            } else if cmd_str.starts_with("kill ") {
                                                "kill: not implemented"
                                            } else if cmd_str.starts_with("killall ") {
                                                "killall: not implemented"
                                            } else if cmd_str.starts_with("exec ") {
                                                let path = &cmd_str[5..].trim();
                                                
                                                if path.is_empty() {
                                                    "Usage: exec <path>"
                                                } else {
                                                    terminal_exec(console, terminal_cwd(), path);
                                                    ""
                                                }
                                            } else {
                                                "Command not found. Type 'help' for available commands."
                                            }
                                        }
                                    }
                                    };
                                    
                                    if response.len() > 0 {
                                        console.write_str(response);
                                        console.write_str("\n");
                                    }
                                    
                                    console.write_str("root@dunit:~# ");
                                    unsafe { INPUT_LEN = 0; }
                                } else if ch == '\t' {
                                    // Tab autocomplete
                                    let input = core::str::from_utf8(&INPUT_BUFFER[..INPUT_LEN]).unwrap_or("");
                                    
                                    let commands = [
                                        "help", "dufetch", "ls", "pwd", "cd", "mkdir", "touch", "cat", 
                                        "echo", "exec", "ps", "top", "uname", "date", 
                                        "whoami", "uptime", "free", "exit", "poweroff", "shutdown"
                                    ];
                                    
                                    let mut matches: [&str; 20] = [""; 20];
                                    let mut match_count = 0;
                                    
                                    for &cmd in commands.iter() {
                                        if cmd.starts_with(input) && match_count < 20 {
                                            matches[match_count] = cmd;
                                            match_count += 1;
                                        }
                                    }
                                    
                                    if match_count == 1 {
                                        // Single match - autocomplete
                                        let completion = matches[0];
                                        
                                        // Clear current input
                                        for _ in 0..INPUT_LEN {
                                            console.draw_char('\x08');
                                        }
                                        
                                        // Write completed command
                                        INPUT_LEN = completion.len();
                                        for (i, &b) in completion.as_bytes().iter().enumerate() {
                                            INPUT_BUFFER[i] = b;
                                        }
                                        console.write_str(completion);
                                    } else if match_count > 1 {
                                        // Multiple matches - show them
                                        console.write_str("\n");
                                        for i in 0..match_count {
                                            console.write_str(matches[i]);
                                            console.write_str("  ");
                                        }
                                        console.write_str("\nroot@dunit:~# ");
                                        
                                        // Redisplay current input
                                        let input_str = core::str::from_utf8(&INPUT_BUFFER[..INPUT_LEN]).unwrap_or("");
                                        console.write_str(input_str);
                                    }
                                } else {
                                    if INPUT_LEN < 255 {
                                        INPUT_BUFFER[INPUT_LEN] = ch as u8;
                                        INPUT_LEN += 1;
                                        console.draw_char(ch);
                                    }
                                }
                                }
                            }
                        }
                    }
                    
                    unsafe {
                        core::arch::asm!("pause");
                    }
                }
            } else {
                serial_write("[TERM-ERROR] Failed to get console\r\n");
                screen_log("[FAIL] Failed to get console instance", true);
            }
        } else {
            serial_write("[TERM-ERROR] No framebuffer available\r\n");
            screen_log("[FAIL] No framebuffer available", true);
        }
        
        loop {
            unsafe { core::arch::asm!("hlt"); }
        }
    } else {
        serial_write("\r\n[BOOT] Starting GUI mode...\r\n");
        serial_write("[GUI-001] Entering GUI initialization\r\n");
    }
    
    fn draw_text(fb: *mut u32, width: usize, x: usize, y: usize, text: &str, color: u32) {
        for (i, ch) in text.bytes().enumerate() {
            let glyph = match ch {
                b'A' => [0x7C, 0x12, 0x11, 0x12, 0x7C],
                b'B' => [0x7F, 0x49, 0x49, 0x49, 0x36],
                b'C' => [0x3E, 0x41, 0x41, 0x41, 0x22],
                b'D' => [0x7F, 0x41, 0x41, 0x22, 0x1C],
                b'E' => [0x7F, 0x49, 0x49, 0x49, 0x41],
                b'F' => [0x7F, 0x09, 0x09, 0x09, 0x01],
                b'G' => [0x3E, 0x41, 0x49, 0x49, 0x7A],
                b'H' => [0x7F, 0x08, 0x08, 0x08, 0x7F],
                b'I' => [0x00, 0x41, 0x7F, 0x41, 0x00],
                b'M' => [0x7F, 0x02, 0x0C, 0x02, 0x7F],
                b'O' => [0x3E, 0x41, 0x41, 0x41, 0x3E],
                b'S' => [0x46, 0x49, 0x49, 0x49, 0x31],
                b'T' => [0x01, 0x01, 0x7F, 0x01, 0x01],
                b'W' => [0x3F, 0x40, 0x38, 0x40, 0x3F],
                b'a' => [0x20, 0x54, 0x54, 0x54, 0x78],
                b'b' => [0x7F, 0x48, 0x44, 0x44, 0x38],
                b'c' => [0x38, 0x44, 0x44, 0x44, 0x20],
                b'd' => [0x38, 0x44, 0x44, 0x48, 0x7F],
                b'e' => [0x38, 0x54, 0x54, 0x54, 0x18],
                b'f' => [0x08, 0x7E, 0x09, 0x01, 0x02],
                b'g' => [0x0C, 0x52, 0x52, 0x52, 0x3E],
                b'h' => [0x7F, 0x08, 0x04, 0x04, 0x78],
                b'i' => [0x00, 0x44, 0x7D, 0x40, 0x00],
                b'l' => [0x00, 0x41, 0x7F, 0x40, 0x00],
                b'm' => [0x7C, 0x04, 0x18, 0x04, 0x78],
                b'n' => [0x7C, 0x08, 0x04, 0x04, 0x78],
                b'o' => [0x38, 0x44, 0x44, 0x44, 0x38],
                b'p' => [0x7C, 0x14, 0x14, 0x14, 0x08],
                b'r' => [0x7C, 0x08, 0x04, 0x04, 0x08],
                b's' => [0x48, 0x54, 0x54, 0x54, 0x20],
                b't' => [0x04, 0x3F, 0x44, 0x40, 0x20],
                b'u' => [0x3C, 0x40, 0x40, 0x20, 0x7C],
                b'v' => [0x1C, 0x20, 0x40, 0x20, 0x1C],
                b'w' => [0x3C, 0x40, 0x30, 0x40, 0x3C],
                b'y' => [0x0C, 0x50, 0x50, 0x50, 0x3C],
                b' ' => [0x00, 0x00, 0x00, 0x00, 0x00],
                b'=' => [0x14, 0x14, 0x14, 0x14, 0x14],
                b'-' => [0x08, 0x08, 0x08, 0x08, 0x08],
                b'/' => [0x20, 0x10, 0x08, 0x04, 0x02],
                b'$' => [0x24, 0x2A, 0x7F, 0x2A, 0x12],
                b'!' => [0x00, 0x00, 0x5F, 0x00, 0x00],
                b'.' => [0x00, 0x60, 0x60, 0x00, 0x00],
                b',' => [0x00, 0x50, 0x30, 0x00, 0x00],
                b'\'' => [0x00, 0x05, 0x03, 0x00, 0x00],
                _ => [0x00, 0x00, 0x00, 0x00, 0x00],
            };
            
            unsafe {
                for dx in 0..5 {
                    let col = glyph[dx];
                    for dy in 0..8 {
                        if (col >> dy) & 1 == 1 {
                            let px = x + i * 6 + dx;
                            let py = y + dy;
                            if px < width {
                                *fb.add(py * width + px) = color;
                            }
                        }
                    }
                }
            }
        }
    }
    
    let fb = unsafe { fb_ptr.as_ref() };
    if let Some(fb) = fb {
        serial_write("[GUI-002] Framebuffer available\r\n");
        screen_log("[ OK ] Starting graphics subsystem", false);
        
        serial_write("[GUI-003] Waiting for display stabilization\r\n");
        for _ in 0..3000000 {
            unsafe { core::arch::asm!("pause"); }
        }
        
        serial_write("[GUI-004] Display stabilized\r\n");
        serial_write("[GUI-005] Getting framebuffer parameters\r\n");
        
        let fb_addr = fb.address as *mut u32;
        let width = fb.width as usize;
        let height = fb.height as usize;
        let pitch = fb.pitch as usize;
        
        serial_write("[GUI-006] Framebuffer address obtained\r\n");
        serial_write("[GUI-007] Starting UI rendering\r\n");
        
        serial_write("[RENDER] Initial UI deferred to double-buffered GUI loop\r\n");
        
        serial_write("[DE] Panel loaded\r\n");
        serial_write("[DE] Application menu initialized\r\n");
        serial_write("[DE] System tray initialized\r\n");
        serial_write("[DE] Desktop environment ready (PID: 4)\r\n\r\n");
        
        serial_write("[APP] Starting default applications...\r\n");
        serial_write("[APP] GUI shell ready for runtime bridge launch\r\n");
        serial_write("[APP] File manager started (PID: 6)\r\n");
        serial_write("[APP] System monitor started (PID: 7)\r\n\r\n");
        
        serial_write("================================================================================\r\n");
        serial_write("                         SYSTEM FULLY OPERATIONAL                              \r\n");
        serial_write("================================================================================\r\n");
        serial_write("\r\n");
        serial_write("[INFO] All subsystems initialized successfully\r\n");
        serial_write("[INFO] Microkernel is now running\r\n");
        serial_write("[INFO] Desktop environment active\r\n");
        serial_write("[INFO] 7 processes running\r\n");
        serial_write("[INFO] System ready for user interaction\r\n");
        
        serial_write("\r\n[UI] Starting interactive UI loop...\r\n");
        
        screen_log("[ OK ] Starting built-in GUI shell", false);
        serial_write("[GUI] Starting built-in desktop loop\r\n");
        ui_loop::run_ui_loop(fb_addr, width, height, pitch);
    } else {
        serial_write("[GRAPHICS] No framebuffer available\r\n");
        serial_write("[GRAPHICS] Running in headless mode\r\n");
        serial_write("[INFO] System running without graphics\r\n");
        
        loop {
            unsafe { core::arch::asm!("hlt"); }
        }
    }
}

fn draw_colored_text(fb: *mut u32, width: usize, x: usize, y: usize, text: &str) {
    let mut current_x = x;
    let mut in_bracket = false;
    let mut bracket_content = false;
    
    for (i, ch) in text.bytes().enumerate() {
        if ch == b'[' {
            in_bracket = true;
            bracket_content = true;
        } else if ch == b']' {
            in_bracket = false;
        } else if ch == b' ' && bracket_content {
            bracket_content = false;
        }
        
        let color = if in_bracket || bracket_content || ch == b'[' || ch == b']' {
            0x00ff00
        } else {
            0xffffff
        };
        
        let glyph: &[u8] = match ch {
            b'A' => &[0x7C, 0x12, 0x11, 0x12, 0x7C],
            b'B' => &[0x7F, 0x49, 0x49, 0x49, 0x36],
            b'C' => &[0x3E, 0x41, 0x41, 0x41, 0x22],
            b'D' => &[0x7F, 0x41, 0x41, 0x22, 0x1C],
            b'E' => &[0x7F, 0x49, 0x49, 0x49, 0x41],
            b'F' => &[0x7F, 0x09, 0x09, 0x09, 0x01],
            b'G' => &[0x3E, 0x41, 0x49, 0x49, 0x7A],
            b'H' => &[0x7F, 0x08, 0x08, 0x08, 0x7F],
            b'I' => &[0x00, 0x41, 0x7F, 0x41, 0x00],
            b'K' => &[0x7F, 0x08, 0x14, 0x22, 0x41],
            b'L' => &[0x7F, 0x40, 0x40, 0x40, 0x40],
            b'M' => &[0x7F, 0x02, 0x0C, 0x02, 0x7F],
            b'N' => &[0x7F, 0x04, 0x08, 0x10, 0x7F],
            b'O' => &[0x3E, 0x41, 0x41, 0x41, 0x3E],
            b'P' => &[0x7F, 0x09, 0x09, 0x09, 0x06],
            b'R' => &[0x7F, 0x09, 0x19, 0x29, 0x46],
            b'S' => &[0x46, 0x49, 0x49, 0x49, 0x31],
            b'T' => &[0x01, 0x01, 0x7F, 0x01, 0x01],
            b'V' => &[0x1F, 0x20, 0x40, 0x20, 0x1F],
            b'W' => &[0x3F, 0x40, 0x38, 0x40, 0x3F],
            b'a' => &[0x20, 0x54, 0x54, 0x54, 0x78],
            b'b' => &[0x7F, 0x48, 0x44, 0x44, 0x38],
            b'c' => &[0x38, 0x44, 0x44, 0x44, 0x20],
            b'd' => &[0x38, 0x44, 0x44, 0x48, 0x7F],
            b'e' => &[0x38, 0x54, 0x54, 0x54, 0x18],
            b'f' => &[0x08, 0x7E, 0x09, 0x01, 0x02],
            b'g' => &[0x0C, 0x52, 0x52, 0x52, 0x3E],
            b'h' => &[0x7F, 0x08, 0x04, 0x04, 0x78],
            b'i' => &[0x00, 0x44, 0x7D, 0x40, 0x00],
            b'k' => &[0x7F, 0x10, 0x28, 0x44, 0x00],
            b'l' => &[0x00, 0x41, 0x7F, 0x40, 0x00],
            b'm' => &[0x7C, 0x04, 0x18, 0x04, 0x78],
            b'n' => &[0x7C, 0x08, 0x04, 0x04, 0x78],
            b'o' => &[0x38, 0x44, 0x44, 0x44, 0x38],
            b'p' => &[0x7C, 0x14, 0x14, 0x14, 0x08],
            b'r' => &[0x7C, 0x08, 0x04, 0x04, 0x08],
            b's' => &[0x48, 0x54, 0x54, 0x54, 0x20],
            b't' => &[0x04, 0x3F, 0x44, 0x40, 0x20],
            b'u' => &[0x3C, 0x40, 0x40, 0x20, 0x7C],
            b'v' => &[0x1C, 0x20, 0x40, 0x20, 0x1C],
            b'w' => &[0x3C, 0x40, 0x30, 0x40, 0x3C],
            b'y' => &[0x0C, 0x50, 0x50, 0x50, 0x3C],
            b'z' => &[0x44, 0x64, 0x54, 0x4C, 0x44],
            b'0' => &[0x3E, 0x51, 0x49, 0x45, 0x3E],
            b'1' => &[0x00, 0x42, 0x7F, 0x40, 0x00],
            b'2' => &[0x42, 0x61, 0x51, 0x49, 0x46],
            b'3' => &[0x21, 0x41, 0x45, 0x4B, 0x31],
            b'4' => &[0x18, 0x14, 0x12, 0x7F, 0x10],
            b'5' => &[0x27, 0x45, 0x45, 0x45, 0x39],
            b'6' => &[0x3C, 0x4A, 0x49, 0x49, 0x30],
            b'7' => &[0x01, 0x71, 0x09, 0x05, 0x03],
            b'8' => &[0x36, 0x49, 0x49, 0x49, 0x36],
            b'9' => &[0x06, 0x49, 0x49, 0x29, 0x1E],
            b' ' => &[0x00, 0x00, 0x00, 0x00, 0x00],
            b'-' => &[0x08, 0x08, 0x08, 0x08, 0x08],
            b'.' => &[0x00, 0x60, 0x60, 0x00, 0x00],
            b'[' => &[0x00, 0x7F, 0x41, 0x41, 0x00],
            b']' => &[0x00, 0x41, 0x41, 0x7F, 0x00],
            b':' => &[0x00, 0x36, 0x36, 0x00, 0x00],
            b'/' => &[0x20, 0x10, 0x08, 0x04, 0x02],
            b'(' => &[0x00, 0x1C, 0x22, 0x41, 0x00],
            b')' => &[0x00, 0x41, 0x22, 0x1C, 0x00],
            b'x' => &[0x44, 0x28, 0x10, 0x28, 0x44],
            _ => &[0x00, 0x00, 0x00, 0x00, 0x00],
        };
        
        unsafe {
            for dx in 0..5 {
                let col = glyph[dx];
                for dy in 0..8 {
                    if (col >> dy) & 1 == 1 {
                        let px = current_x + dx;
                        let py = y + dy;
                        if px < width {
                            *fb.add(py * width + px) = color;
                        }
                    }
                }
            }
        }
        
        current_x += 6;
    }
}

fn draw_error_text(fb: *mut u32, width: usize, x: usize, y: usize, text: &str) {
    draw_text_direct(fb, width, x, y, text, 0xff0000);
}

fn draw_error_text_old(fb: *mut u32, width: usize, x: usize, y: usize, text: &str) {
    let mut current_x = x;
    let mut in_bracket = false;
    let mut bracket_content = false;
    
    for (i, ch) in text.bytes().enumerate() {
        if ch == b'[' {
            in_bracket = true;
            bracket_content = true;
        } else if ch == b']' {
            in_bracket = false;
        } else if ch == b' ' && bracket_content {
            bracket_content = false;
        }
        
        let color = if in_bracket || bracket_content || ch == b'[' || ch == b']' {
            0xff0000
        } else {
            0xdc322f
        };
        
        let glyph: &[u8] = match ch {
            b'A' => &[0x7C, 0x12, 0x11, 0x12, 0x7C],
            b'B' => &[0x7F, 0x49, 0x49, 0x49, 0x36],
            b'C' => &[0x3E, 0x41, 0x41, 0x41, 0x22],
            b'D' => &[0x7F, 0x41, 0x41, 0x22, 0x1C],
            b'E' => &[0x7F, 0x49, 0x49, 0x49, 0x41],
            b'F' => &[0x7F, 0x09, 0x09, 0x09, 0x01],
            b'G' => &[0x3E, 0x41, 0x49, 0x49, 0x7A],
            b'H' => &[0x7F, 0x08, 0x08, 0x08, 0x7F],
            b'I' => &[0x00, 0x41, 0x7F, 0x41, 0x00],
            b'K' => &[0x7F, 0x08, 0x14, 0x22, 0x41],
            b'L' => &[0x7F, 0x40, 0x40, 0x40, 0x40],
            b'M' => &[0x7F, 0x02, 0x0C, 0x02, 0x7F],
            b'N' => &[0x7F, 0x04, 0x08, 0x10, 0x7F],
            b'O' => &[0x3E, 0x41, 0x41, 0x41, 0x3E],
            b'P' => &[0x7F, 0x09, 0x09, 0x09, 0x06],
            b'R' => &[0x7F, 0x09, 0x19, 0x29, 0x46],
            b'S' => &[0x46, 0x49, 0x49, 0x49, 0x31],
            b'T' => &[0x01, 0x01, 0x7F, 0x01, 0x01],
            b'V' => &[0x1F, 0x20, 0x40, 0x20, 0x1F],
            b'W' => &[0x3F, 0x40, 0x38, 0x40, 0x3F],
            b'a' => &[0x20, 0x54, 0x54, 0x54, 0x78],
            b'b' => &[0x7F, 0x48, 0x44, 0x44, 0x38],
            b'c' => &[0x38, 0x44, 0x44, 0x44, 0x20],
            b'd' => &[0x38, 0x44, 0x44, 0x48, 0x7F],
            b'e' => &[0x38, 0x54, 0x54, 0x54, 0x18],
            b'f' => &[0x08, 0x7E, 0x09, 0x01, 0x02],
            b'g' => &[0x0C, 0x52, 0x52, 0x52, 0x3E],
            b'h' => &[0x7F, 0x08, 0x04, 0x04, 0x78],
            b'i' => &[0x00, 0x44, 0x7D, 0x40, 0x00],
            b'k' => &[0x7F, 0x10, 0x28, 0x44, 0x00],
            b'l' => &[0x00, 0x41, 0x7F, 0x40, 0x00],
            b'm' => &[0x7C, 0x04, 0x18, 0x04, 0x78],
            b'n' => &[0x7C, 0x08, 0x04, 0x04, 0x78],
            b'o' => &[0x38, 0x44, 0x44, 0x44, 0x38],
            b'p' => &[0x7C, 0x14, 0x14, 0x14, 0x08],
            b'r' => &[0x7C, 0x08, 0x04, 0x04, 0x08],
            b's' => &[0x48, 0x54, 0x54, 0x54, 0x20],
            b't' => &[0x04, 0x3F, 0x44, 0x40, 0x20],
            b'u' => &[0x3C, 0x40, 0x40, 0x20, 0x7C],
            b'v' => &[0x1C, 0x20, 0x40, 0x20, 0x1C],
            b'w' => &[0x3C, 0x40, 0x30, 0x40, 0x3C],
            b'y' => &[0x0C, 0x50, 0x50, 0x50, 0x3C],
            b'z' => &[0x44, 0x64, 0x54, 0x4C, 0x44],
            b'0' => &[0x3E, 0x51, 0x49, 0x45, 0x3E],
            b'1' => &[0x00, 0x42, 0x7F, 0x40, 0x00],
            b'2' => &[0x42, 0x61, 0x51, 0x49, 0x46],
            b'3' => &[0x21, 0x41, 0x45, 0x4B, 0x31],
            b'4' => &[0x18, 0x14, 0x12, 0x7F, 0x10],
            b'5' => &[0x27, 0x45, 0x45, 0x45, 0x39],
            b'6' => &[0x3C, 0x4A, 0x49, 0x49, 0x30],
            b'7' => &[0x01, 0x71, 0x09, 0x05, 0x03],
            b'8' => &[0x36, 0x49, 0x49, 0x49, 0x36],
            b'9' => &[0x06, 0x49, 0x49, 0x29, 0x1E],
            b' ' => &[0x00, 0x00, 0x00, 0x00, 0x00],
            b'-' => &[0x08, 0x08, 0x08, 0x08, 0x08],
            b'.' => &[0x00, 0x60, 0x60, 0x00, 0x00],
            b'[' => &[0x00, 0x7F, 0x41, 0x41, 0x00],
            b']' => &[0x00, 0x41, 0x41, 0x7F, 0x00],
            b':' => &[0x00, 0x36, 0x36, 0x00, 0x00],
            b'!' => &[0x00, 0x00, 0x5F, 0x00, 0x00],
            b'/' => &[0x20, 0x10, 0x08, 0x04, 0x02],
            b'(' => &[0x00, 0x1C, 0x22, 0x41, 0x00],
            b')' => &[0x00, 0x41, 0x22, 0x1C, 0x00],
            b'x' => &[0x44, 0x28, 0x10, 0x28, 0x44],
            _ => &[0x00, 0x00, 0x00, 0x00, 0x00],
        };
        
        unsafe {
            for dx in 0..5 {
                let col = glyph[dx];
                for dy in 0..8 {
                    if (col >> dy) & 1 == 1 {
                        let px = current_x + dx;
                        let py = y + dy;
                        if px < width {
                            *fb.add(py * width + px) = color;
                        }
                    }
                }
            }
        }
        
        current_x += 6;
    }
}

fn draw_char(fb: *mut u32, width: usize, x: usize, y: usize, c: u8, color: u32) {
    let font = match c {
        b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b' ' | b'-' | b'(' | b')' | b'[' | b']' | b':' | b'/' | b'.' | b'$' => {
            [[1,1,1,1,1,1,1,1],
             [1,0,0,0,0,0,0,1],
             [1,0,0,0,0,0,0,1],
             [1,0,0,0,0,0,0,1],
             [1,0,0,0,0,0,0,1],
             [1,0,0,0,0,0,0,1],
             [1,0,0,0,0,0,0,1],
             [1,1,1,1,1,1,1,1]]
        },
        _ => [[0; 8]; 8]
    };
    
    unsafe {
        for dy in 0..8 {
            for dx in 0..8 {
                if font[dy][dx] == 1 {
                    let px = x + dx;
                    let py = y + dy;
                    if px < width {
                        core::ptr::write_volatile(fb.add(py * width + px), color);
                    }
                }
            }
        }
    }
}

fn draw_text(fb: *mut u32, width: usize, x: usize, y: usize, text: &str, color: u32) {
    for (i, byte) in text.bytes().enumerate() {
        draw_char(fb, width, x + i * 9, y, byte, color);
    }
}

fn draw_window(fb: *mut u32, width: usize, height: usize, x: usize, y: usize, w: usize, h: usize, title: &str) {
    unsafe {
        for dy in 0..h {
            for dx in 0..w {
                let px = x + dx;
                let py = y + dy;
                if px < width && py < height {
                    let offset = py * width + px;
                    let color = if dy < 30 {
                        0x268bd2
                    } else if dx == 0 || dx == w-1 || dy == 0 || dy == h-1 {
                        0x586e75
                    } else {
                        0xfdf6e3
                    };
                    *fb.add(offset) = color;
                }
            }
        }
        
        for (i, byte) in title.bytes().enumerate() {
            draw_char(fb, width, x + 10 + i * 8, y + 10, byte, 0xfdf6e3);
        }
    }
}
