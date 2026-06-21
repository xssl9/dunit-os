//! Shared command core used by both the kernel terminal (terminal mode) and the
//! GUI terminal. Command handlers write to a `ShellSink` so the same dispatcher
//! drives `terminal::FbConsole` and the GUI line buffer without duplication.

use alloc::string::String;
use alloc::vec::Vec;

use crate::drivers;
use crate::fs::vfs::{self, DirEntry, FileType, OpenFlags, VfsError};
use crate::memory;
use crate::process;

/// Output abstraction for command handlers. `write_str` is the only required
/// method; color hints are optional and default to no-ops (the GUI line sink
/// ignores them, the framebuffer console honours them).
pub trait ShellSink {
    fn write_str(&mut self, s: &str);
    fn set_fg_color(&mut self, _color: u32) {}
    fn reset_fg_color(&mut self) {}
}

/// `core::fmt::Write` adapter so handlers can `write!` formatted values into a
/// `ShellSink`.
pub struct SinkWriter<'a> {
    pub sink: &'a mut dyn ShellSink,
}

impl core::fmt::Write for SinkWriter<'_> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        self.sink.write_str(s);
        Ok(())
    }
}

impl ShellSink for crate::terminal::FbConsole {
    fn write_str(&mut self, s: &str) {
        crate::terminal::FbConsole::write_str(self, s);
    }
    fn set_fg_color(&mut self, color: u32) {
        crate::terminal::FbConsole::set_fg_color(self, color);
    }
    fn reset_fg_color(&mut self) {
        crate::terminal::FbConsole::reset_fg_color(self);
    }
}

/// Result of dispatching a command line. Pure-output commands return `Handled`;
/// commands whose effect depends on the terminal (clear/exec/exit) return a
/// variant the caller interprets.
pub enum ShellOutcome {
    Handled,
    NotFound,
    Clear,
    Exec(String),
    Exit,
}

// ---------------------------------------------------------------------------
// Number / formatting helpers
// ---------------------------------------------------------------------------

pub fn write_i32(out: &mut dyn ShellSink, value: i32) {
    if value < 0 {
        out.write_str("-");
        write_u32(out, value.wrapping_neg() as u32);
    } else {
        write_u32(out, value as u32);
    }
}

pub fn write_u32(out: &mut dyn ShellSink, mut value: u32) {
    let mut buf = [0u8; 10];
    let mut index = buf.len();
    if value == 0 {
        out.write_str("0");
        return;
    }
    while value > 0 {
        index -= 1;
        buf[index] = b'0' + (value % 10) as u8;
        value /= 10;
    }
    out.write_str(core::str::from_utf8(&buf[index..]).unwrap_or("?"));
}

pub fn write_u64(out: &mut dyn ShellSink, mut value: u64) {
    let mut buf = [0u8; 20];
    let mut index = buf.len();
    if value == 0 {
        out.write_str("0");
        return;
    }
    while value > 0 {
        index -= 1;
        buf[index] = b'0' + (value % 10) as u8;
        value /= 10;
    }
    out.write_str(core::str::from_utf8(&buf[index..]).unwrap_or("?"));
}

pub fn write_usize(out: &mut dyn ShellSink, value: usize) {
    write_u64(out, value as u64);
}

fn write_hex_fixed(out: &mut dyn ShellSink, value: u64, digits: usize) {
    out.write_str("0x");
    write_hex_digits(out, value, digits);
}

fn write_hex_digits(out: &mut dyn ShellSink, mut value: u64, digits: usize) {
    let mut buf = [0u8; 16];
    let count = digits.min(buf.len());
    let mut index = count;
    while index > 0 {
        index -= 1;
        let nibble = (value & 0xF) as u8;
        buf[index] = if nibble < 10 {
            b'0' + nibble
        } else {
            b'a' + nibble - 10
        };
        value >>= 4;
    }
    out.write_str(core::str::from_utf8(&buf[..count]).unwrap_or("?"));
}

pub fn write_vfs_error(out: &mut dyn ShellSink, command: &str, error: VfsError) {
    out.write_str(command);
    out.write_str(": ");
    out.write_str(vfs_error_str(error));
    out.write_str("\n");
}

fn vfs_error_str(error: VfsError) -> &'static str {
    match error {
        VfsError::NotFound => "file not found",
        VfsError::PermissionDenied => "permission denied",
        VfsError::InvalidDescriptor => "invalid descriptor",
        VfsError::AlreadyExists => "already exists",
        VfsError::NotADirectory => "not a directory",
        VfsError::IsADirectory => "is a directory",
        VfsError::InvalidPath => "invalid path",
        VfsError::Unsupported => "unsupported",
        VfsError::IoError => "I/O error",
    }
}

// ---------------------------------------------------------------------------
// Diagnostics commands (PCI / USB / devices / block / processes / memory)
// ---------------------------------------------------------------------------

fn write_pci_addr(out: &mut dyn ShellSink, dev: drivers::pci::PciDevice) {
    write_hex_digits(out, dev.bus as u64, 2);
    out.write_str(":");
    write_hex_digits(out, dev.device as u64, 2);
    out.write_str(".");
    write_hex_digits(out, dev.function as u64, 1);
}

fn cmd_lspci(out: &mut dyn ShellSink) {
    let snapshot = drivers::pci::snapshot();
    out.write_str("PCI devices: ");
    write_usize(out, snapshot.total_devices);
    out.write_str(" stored=");
    write_usize(out, snapshot.stored_devices);
    out.write_str(" usb_controllers=");
    write_usize(out, snapshot.usb_controllers);
    out.write_str(" msi=");
    write_usize(out, snapshot.msi_devices);
    out.write_str(" msix=");
    write_usize(out, snapshot.msix_devices);
    out.write_str("\n");

    for entry in snapshot.devices.iter().take(snapshot.stored_devices) {
        let Some(dev) = entry else {
            continue;
        };
        write_pci_addr(out, *dev);
        out.write_str(" vendor=");
        write_hex_fixed(out, dev.vendor_id as u64, 4);
        out.write_str(" device=");
        write_hex_fixed(out, dev.device_id as u64, 4);
        out.write_str(" class=");
        write_hex_fixed(out, dev.class_code as u64, 2);
        out.write_str(" subclass=");
        write_hex_fixed(out, dev.subclass as u64, 2);
        out.write_str(" prog_if=");
        write_hex_fixed(out, dev.prog_if as u64, 2);
        out.write_str(" irq=");
        if dev.interrupt_line == 0xFF {
            out.write_str("none");
        } else {
            write_usize(out, dev.interrupt_line as usize);
        }
        out.write_str("/");
        write_usize(out, dev.interrupt_pin as usize);
        out.write_str(" caps=");
        write_usize(out, dev.capabilities.count as usize);
        if dev.capabilities.has_msi {
            out.write_str(" MSI");
        }
        if dev.capabilities.has_msix {
            out.write_str(" MSI-X");
        }
        if dev.class_code == 0x0C && dev.subclass == 0x03 {
            out.write_str(" USB");
        }
        out.write_str("\n");
    }
}

fn cmd_usb(out: &mut dyn ShellSink) {
    let status = drivers::usb::xhci::status();
    out.write_str("USB xHCI: found=");
    write_usize(out, status.found);
    out.write_str(" initialized=");
    write_usize(out, status.initialized);
    out.write_str(" connected_ports=");
    write_usize(out, status.connected_ports);
    if let Some(error) = status.last_error {
        out.write_str(" last_error=");
        out.write_str(error.as_str());
    } else {
        out.write_str(" last_error=none");
    }
    out.write_str("\n");
    out.write_str("USB HID mouse parser: boot-protocol reports supported; enumeration/polling not implemented\n");
}

fn cmd_devs(out: &mut dyn ShellSink) {
    let mut devices: [Option<drivers::registry::DeviceRegistration>; 32] = [None; 32];
    let count = drivers::registry::snapshot(&mut devices);

    out.write_str("DEVICE  CLASS           DRIVER\n");
    for entry in devices.iter().take(count) {
        let Some(device) = entry else {
            continue;
        };
        out.write_str(device.name);
        out.write_str("  ");
        out.write_str(drivers::registry::class_name(device.class));
        out.write_str("  ");
        out.write_str(device.driver);
        out.write_str("\n");
    }
}

fn cmd_blk(out: &mut dyn ShellSink) {
    let mut devices: [Option<drivers::block::BlockDeviceInfo>; 8] = [None; 8];
    let count = drivers::block::snapshot(&mut devices);

    out.write_str("DEVICE  DRIVER     BLOCKS  BLOCK_SIZE  BYTES  MODE\n");
    for entry in devices.iter().take(count) {
        let Some(device) = entry else {
            continue;
        };
        out.write_str(device.name);
        out.write_str("  ");
        out.write_str(device.driver);
        out.write_str("  ");
        write_u64(out, device.blocks);
        out.write_str("  ");
        write_usize(out, device.block_size);
        out.write_str("  ");
        write_u64(out, device.bytes());
        out.write_str("  ");
        out.write_str(if device.readonly { "ro" } else { "rw" });
        out.write_str("\n");
    }
}

fn parse_u64(text: &str) -> Option<u64> {
    if text.is_empty() {
        return None;
    }
    let mut value = 0u64;
    for byte in text.bytes() {
        if !byte.is_ascii_digit() {
            return None;
        }
        value = value.checked_mul(10)?;
        value = value.checked_add((byte - b'0') as u64)?;
    }
    Some(value)
}

fn cmd_blkread(out: &mut dyn ShellSink, args: &str) {
    let mut parts = args.split_whitespace();
    let Some(device) = parts.next() else {
        out.write_str("blkread: missing device\n");
        return;
    };
    let Some(lba_text) = parts.next() else {
        out.write_str("blkread: missing lba\n");
        return;
    };
    let Some(lba) = parse_u64(lba_text) else {
        out.write_str("blkread: invalid lba\n");
        return;
    };

    let mut block = [0u8; 512];
    match drivers::block::read_block(device, lba, &mut block) {
        Ok(bytes) => {
            out.write_str(device);
            out.write_str(" lba=");
            write_u64(out, lba);
            out.write_str(" bytes=");
            write_usize(out, bytes);
            out.write_str("\n");
            hex_dump(out, &block[..64]);
        }
        Err(error) => {
            out.write_str("blkread: ");
            out.write_str(match error {
                drivers::block::BlockError::NotFound => "device not found",
                drivers::block::BlockError::OutOfRange => "lba out of range",
                drivers::block::BlockError::BufferTooSmall => "buffer too small",
                drivers::block::BlockError::Io => "I/O error",
            });
            out.write_str("\n");
        }
    }
}

fn hex_dump(out: &mut dyn ShellSink, data: &[u8]) {
    let mut offset = 0usize;
    while offset < data.len() {
        write_hex_digits(out, offset as u64, 4);
        out.write_str(": ");

        let mut index = 0usize;
        while index < 16 && offset + index < data.len() {
            write_hex_digits(out, data[offset + index] as u64, 2);
            out.write_str(" ");
            index += 1;
        }

        out.write_str(" ");
        index = 0;
        while index < 16 && offset + index < data.len() {
            let byte = data[offset + index];
            if byte.is_ascii_graphic() || byte == b' ' {
                let ch = [byte];
                if let Ok(text) = core::str::from_utf8(&ch) {
                    out.write_str(text);
                }
            } else {
                out.write_str(".");
            }
            index += 1;
        }

        out.write_str("\n");
        offset += 16;
    }
}

fn write_process_state(out: &mut dyn ShellSink, state: process::ProcessState) {
    out.write_str(match state {
        process::ProcessState::Prepared => "Prepared",
        process::ProcessState::Ready => "Ready",
        process::ProcessState::Running => "Running",
        process::ProcessState::Blocked => "Blocked",
        process::ProcessState::Dead => "Dead",
        process::ProcessState::Reaped => "Reaped",
    });
}

fn write_process_status(out: &mut dyn ShellSink, status: Option<process::ProcessExitStatus>) {
    match status {
        Some(process::ProcessExitStatus::Exited(code)) => {
            out.write_str("Exited(");
            write_i32(out, code);
            out.write_str(")");
        }
        Some(process::ProcessExitStatus::Fault(fault)) => {
            out.write_str("Fault(");
            out.write_str(fault.reason());
            out.write_str(")");
        }
        None => out.write_str("-"),
    }
}

fn cmd_ps(out: &mut dyn ShellSink, aux: bool) {
    let mut records = Vec::new();
    process::snapshot_processes(&mut records);

    if aux {
        out.write_str("PID  PPID  STATE     RUN WAIT STATUS      COMMAND\n");
    } else {
        out.write_str("PID  PPID  STATE     COMMAND\n");
    }

    for record in records.iter() {
        write_u64(out, record.pid.0);
        out.write_str("  ");
        match record.parent {
            Some(parent) => write_u64(out, parent.0),
            None => out.write_str("-"),
        }
        out.write_str("  ");
        write_process_state(out, record.state);
        if aux {
            out.write_str("  ");
            out.write_str(if record.has_run { "yes" } else { "no" });
            out.write_str("  ");
            out.write_str(if record.waitable { "yes" } else { "no" });
            out.write_str("  ");
            write_process_status(out, record.status);
        }
        out.write_str("  ");
        out.write_str(&record.path);
        out.write_str("\n");
    }
}

fn cmd_free(out: &mut dyn ShellSink) {
    if let Some(pmm) = memory::pmm::get_pmm() {
        let total_kib = pmm.total_memory() / 1024;
        let free_kib = pmm.available_memory() / 1024;
        let used_kib = total_kib.saturating_sub(free_kib);
        out.write_str("              total        used        free\n");
        out.write_str("PMM KiB:      ");
        write_usize(out, total_kib);
        out.write_str("        ");
        write_usize(out, used_kib);
        out.write_str("        ");
        write_usize(out, free_kib);
        out.write_str("\n");
        out.write_str("Heap: allocator stats unavailable\n");
        out.write_str("Swap: unavailable\n");
    } else {
        out.write_str("free: PMM unavailable\n");
    }
}

// ---------------------------------------------------------------------------
// Filesystem commands
// ---------------------------------------------------------------------------

fn write_file(out: &mut dyn ShellSink, cwd: &str, path: &str, text: &str, append: bool) {
    if path.is_empty() {
        out.write_str("echo: missing output file\n");
        return;
    }

    let Some(vfs) = vfs::get_vfs() else {
        out.write_str("echo: VFS not initialized\n");
        return;
    };

    match vfs.stat_at(cwd, path) {
        Ok(stat) if stat.file_type == FileType::Directory => {
            out.write_str("echo: is a directory\n");
            return;
        }
        Ok(_) => {}
        Err(VfsError::NotFound) => {
            if let Err(error) = vfs.create_at(cwd, path) {
                write_vfs_error(out, "echo", error);
                return;
            }
        }
        Err(error) => {
            write_vfs_error(out, "echo", error);
            return;
        }
    }

    if !append {
        if let Err(error) = vfs.truncate_at(cwd, path) {
            write_vfs_error(out, "echo", error);
            return;
        }
    }

    let flags = if append {
        OpenFlags::from_bits(OpenFlags::WRITE.bits() | OpenFlags::APPEND.bits())
    } else {
        OpenFlags::WRITE
    };

    match vfs.open_at(cwd, path, flags) {
        Ok(fd) => {
            let mut data = Vec::new();
            data.extend_from_slice(text.as_bytes());
            data.push(b'\n');
            if let Err(error) = vfs.write(fd, &data) {
                write_vfs_error(out, "echo", error);
            }
            let _ = vfs.close(fd);
        }
        Err(error) => write_vfs_error(out, "echo", error),
    }
}

fn tree_path(
    out: &mut dyn ShellSink,
    vfs: &mut vfs::VirtualFileSystem,
    path: &str,
    depth: usize,
) {
    if depth > 16 {
        return;
    }

    let mut entries = [DirEntry::empty(); 32];
    match vfs.readdir_into_at("/", path, &mut entries) {
        Ok(count) => {
            for entry in entries.iter().take(count) {
                for _ in 0..depth {
                    out.write_str("  ");
                }
                out.write_str(entry.name());
                if entry.file_type == FileType::Directory {
                    out.write_str("/");
                }
                out.write_str("\n");

                if entry.file_type == FileType::Directory {
                    let mut child_path = String::from(path);
                    if !child_path.ends_with('/') {
                        child_path.push('/');
                    }
                    child_path.push_str(entry.name());
                    tree_path(out, vfs, &child_path, depth + 1);
                }
            }
        }
        Err(error) => write_vfs_error(out, "tree", error),
    }
}

/// Handle a filesystem command. Returns true if `trimmed` matched one.
fn handle_fs_command(out: &mut dyn ShellSink, cwd: &mut String, trimmed: &str) -> bool {
    if trimmed == "dufetch" {
        crate::apps::dufetch::run(out, cwd);
        return true;
    }

    if trimmed == "pwd" {
        out.write_str(cwd);
        out.write_str("\n");
        return true;
    }

    if trimmed == "ls" || trimmed.starts_with("ls ") {
        let path = trimmed.strip_prefix("ls").unwrap_or("").trim();
        let path = if path.is_empty() { "." } else { path };
        if let Some(vfs) = vfs::get_vfs() {
            let mut entries = [DirEntry::empty(); 32];
            match vfs.readdir_into_at(cwd, path, &mut entries) {
                Ok(count) => {
                    for (idx, entry) in entries.iter().take(count).enumerate() {
                        if idx > 0 {
                            out.write_str("  ");
                        }
                        out.write_str(entry.name());
                    }
                    out.write_str("\n");
                }
                Err(error) => write_vfs_error(out, "ls", error),
            }
        } else {
            out.write_str("ls: VFS not initialized\n");
        }
        return true;
    }

    if trimmed == "cd" || trimmed.starts_with("cd ") {
        let path = trimmed.strip_prefix("cd").unwrap_or("").trim();
        let path = if path.is_empty() { "/" } else { path };
        if let Some(vfs) = vfs::get_vfs() {
            match vfs.stat_at(cwd, path) {
                Ok(stat) if stat.file_type == FileType::Directory => {
                    match vfs.normalize_at(cwd, path) {
                        Ok(new_cwd) => *cwd = new_cwd,
                        Err(error) => write_vfs_error(out, "cd", error),
                    }
                }
                Ok(_) => out.write_str("cd: not a directory\n"),
                Err(error) => write_vfs_error(out, "cd", error),
            }
        } else {
            out.write_str("cd: VFS not initialized\n");
        }
        return true;
    }

    if trimmed.starts_with("mkdir ") {
        let path = trimmed[6..].trim();
        if path.is_empty() {
            out.write_str("mkdir: missing operand\n");
        } else if let Some(vfs) = vfs::get_vfs() {
            if let Err(error) = vfs.mkdir_at(cwd, path) {
                write_vfs_error(out, "mkdir", error);
            }
        } else {
            out.write_str("mkdir: VFS not initialized\n");
        }
        return true;
    }

    if trimmed.starts_with("touch ") {
        let path = trimmed[6..].trim();
        if path.is_empty() {
            out.write_str("touch: missing operand\n");
        } else if let Some(vfs) = vfs::get_vfs() {
            match vfs.stat_at(cwd, path) {
                Ok(stat) if stat.file_type == FileType::File => {}
                Ok(_) => out.write_str("touch: not a file\n"),
                Err(VfsError::NotFound) => {
                    if let Err(error) = vfs.create_at(cwd, path) {
                        write_vfs_error(out, "touch", error);
                    }
                }
                Err(error) => write_vfs_error(out, "touch", error),
            }
        } else {
            out.write_str("touch: VFS not initialized\n");
        }
        return true;
    }

    if trimmed.starts_with("cat ") {
        let path = trimmed[4..].trim();
        if path.is_empty() {
            out.write_str("cat: missing operand\n");
        } else if let Some(vfs) = vfs::get_vfs() {
            match vfs.open_at(cwd, path, OpenFlags::READ) {
                Ok(fd) => {
                    let mut buf = [0u8; 512];
                    loop {
                        match vfs.read(fd, &mut buf) {
                            Ok(0) => break,
                            Ok(n) => {
                                let text = core::str::from_utf8(&buf[..n]).unwrap_or("<binary>");
                                out.write_str(text);
                                if n < buf.len() {
                                    break;
                                }
                            }
                            Err(error) => {
                                write_vfs_error(out, "cat", error);
                                break;
                            }
                        }
                    }
                    let _ = vfs.close(fd);
                    out.write_str("\n");
                }
                Err(error) => write_vfs_error(out, "cat", error),
            }
        } else {
            out.write_str("cat: VFS not initialized\n");
        }
        return true;
    }

    if trimmed.starts_with("rm ") {
        let path = trimmed[3..].trim();
        if path.is_empty() {
            out.write_str("rm: missing operand\n");
        } else if let Some(vfs) = vfs::get_vfs() {
            if let Err(error) = vfs.remove_at(cwd, path) {
                write_vfs_error(out, "rm", error);
            }
        } else {
            out.write_str("rm: VFS not initialized\n");
        }
        return true;
    }

    if trimmed == "tree" || trimmed.starts_with("tree ") {
        let path = trimmed.strip_prefix("tree").unwrap_or("").trim();
        let path = if path.is_empty() { "." } else { path };
        if let Some(vfs) = vfs::get_vfs() {
            match vfs.normalize_at(cwd, path) {
                Ok(root) => {
                    out.write_str(&root);
                    out.write_str("\n");
                    tree_path(out, vfs, &root, 1);
                }
                Err(error) => write_vfs_error(out, "tree", error),
            }
        } else {
            out.write_str("tree: VFS not initialized\n");
        }
        return true;
    }

    if trimmed == "echo" || trimmed.starts_with("echo ") {
        let text = trimmed.strip_prefix("echo").unwrap_or("").trim_start();
        if let Some(idx) = text.find(">>") {
            let value = text[..idx].trim_end();
            let path = text[idx + 2..].trim();
            write_file(out, cwd, path, value, true);
            return true;
        }
        if let Some(idx) = text.find('>') {
            let value = text[..idx].trim_end();
            let path = text[idx + 1..].trim();
            write_file(out, cwd, path, value, false);
            return true;
        }
        out.write_str(text);
        out.write_str("\n");
        return true;
    }

    false
}

fn cmd_help(out: &mut dyn ShellSink) {
    out.write_str("Available commands:\n");
    out.write_str("  help       - Show this help\n");
    out.write_str("  dufetch    - Show Dunit OS system summary\n");
    out.write_str("  ls         - List files\n");
    out.write_str("  pwd        - Print working directory\n");
    out.write_str("  cd         - Change directory\n");
    out.write_str("  mkdir      - Create directory\n");
    out.write_str("  touch      - Create file\n");
    out.write_str("  cat        - Display file contents\n");
    out.write_str("  echo       - Print text (supports > and >>)\n");
    out.write_str("  rm         - Remove file\n");
    out.write_str("  tree       - Show directory tree\n");
    out.write_str("  clear      - Clear the screen\n");
    out.write_str("  exec       - Execute a program (GUI apps open a window)\n");
    out.write_str("  devs       - Show registered devices\n");
    out.write_str("  blk        - Show block devices\n");
    out.write_str("  blkread    - Read a block device sector\n");
    out.write_str("  lspci      - Show PCI devices\n");
    out.write_str("  usb        - Show USB/xHCI driver status\n");
    out.write_str("  ps         - Show process table records\n");
    out.write_str("  ps aux     - Show detailed process table records\n");
    out.write_str("  uname      - System name (uname -a for details)\n");
    out.write_str("  date       - RTC status\n");
    out.write_str("  whoami     - Current user\n");
    out.write_str("  uptime     - Uptime status\n");
    out.write_str("  free       - Memory status\n");
    out.write_str("  top        - Scheduler status\n");
    out.write_str("  exit       - Exit the terminal\n");
    out.write_str("  poweroff   - Shutdown status\n");
}

/// Dispatch a command line shared by both terminals. `cwd` is owned by the
/// caller and mutated by `cd`.
pub fn run_command(out: &mut dyn ShellSink, cwd: &mut String, line: &str) -> ShellOutcome {
    let trimmed = line.trim();

    if trimmed.is_empty() {
        return ShellOutcome::Handled;
    }

    if trimmed == "clear" {
        return ShellOutcome::Clear;
    }

    if trimmed == "exit" {
        return ShellOutcome::Exit;
    }

    if trimmed == "exec" || trimmed.starts_with("exec ") {
        let args = trimmed.strip_prefix("exec").unwrap_or("").trim();
        return ShellOutcome::Exec(String::from(args));
    }

    if handle_fs_command(out, cwd, trimmed) {
        return ShellOutcome::Handled;
    }

    match trimmed {
        "help" => cmd_help(out),
        "uname" => out.write_str("Dunit OS\n"),
        "uname -a" => {
            out.write_str("Dunit OS 1.0.0 Green Tea x86_64 kernel=monolithic-rust-hal\n")
        }
        "date" => out.write_str("date: RTC unavailable\n"),
        "whoami" => out.write_str("root (kernel terminal)\n"),
        "uptime" => {
            out.write_str("uptime unavailable: timer tick source is not active in terminal mode\n")
        }
        "free" => cmd_free(out),
        "ps" => cmd_ps(out, false),
        "ps aux" => cmd_ps(out, true),
        "lspci" => cmd_lspci(out),
        "devs" => cmd_devs(out),
        "blk" => cmd_blk(out),
        "usb" => cmd_usb(out),
        "top" => out.write_str("top unavailable: scheduler not active\n"),
        "poweroff" | "shutdown" => {
            out.write_str("shutdown not implemented: ACPI/QEMU shutdown device unavailable\n")
        }
        "blkread" => cmd_blkread(out, ""),
        _ if trimmed.starts_with("blkread ") => cmd_blkread(out, &trimmed["blkread ".len()..]),
        _ => return ShellOutcome::NotFound,
    }

    ShellOutcome::Handled
}
