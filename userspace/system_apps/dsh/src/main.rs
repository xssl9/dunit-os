#![no_std]
#![no_main]

//! Interactive PTY-slave shell for the M4 userspace terminal (Stack B).
//!
//! Spawned by `gui_terminal` through `pty_spawn`, so fd 0/1/2 are routed to the
//! pty rings (not the console). It runs a full line editor over the raw byte
//! stream the terminal forwards, and dispatches a command set that MIRRORS the
//! kernel text-mode shell (`kernel/src/shell.rs`) in syntax and output — the
//! same `ls / cd / pwd / mkdir / touch / cat / rm / tree / echo(>,>>)` FS verbs
//! and the same `uname / whoami / date / uptime / free / ps` system verbs — so
//! the GUI terminal behaves "just like a clean terminal". The FS verbs go
//! through the libdunit filesystem syscalls (resolved against this process's
//! cwd by the kernel); the system verbs read the read-only counters exposed by
//! `get_system_stats` (syscall 26). No kernel-internal APIs, no hardcoded
//! policy: pure userspace. Any other name is run as `/app/<name>` on a private
//! inner pty whose stdio we bridge until it exits (see `spawn_external`).

use core::panic::PanicInfo;

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use libdunit::{DirEntry, SystemStats};

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    libdunit::println("dsh: PANIC");
    libdunit::exit(101)
}

/// Fallback shell prompt when no `PROMPT` is present in the environment. The
/// terminal passes its configured `[terminal] prompt` (default "dsh") via the
/// pty-spawn env, so this only applies when dsh is launched without one.
const PROMPT_DEFAULT: &str = "dsh";

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

/// A readdir scratch buffer of `n` owned entries (heap, so large listings do
/// not blow the userspace stack).
fn dir_buf(n: usize) -> Vec<DirEntry> {
    let mut v: Vec<DirEntry> = Vec::new();
    v.resize(n, DirEntry::empty());
    v
}

/// Join a directory path and a child name into an absolute path.
fn join(dir: &str, name: &str) -> String {
    if dir.ends_with('/') {
        format!("{}{}", dir, name)
    } else {
        format!("{}/{}", dir, name)
    }
}

/// Resolve `path` (absolute or relative to `base`) into a canonical absolute
/// path, folding `.`/`..`/empty components — mirrors the kernel shell's
/// `normalize_at` so `tree`/`cd` behave identically to the text terminal.
fn normalize(base: &str, path: &str) -> String {
    let combined = if path.starts_with('/') {
        String::from(path)
    } else {
        join(base, path)
    };
    let mut stack: Vec<&str> = Vec::new();
    for comp in combined.split('/') {
        match comp {
            "" | "." => {}
            ".." => {
                stack.pop();
            }
            c => stack.push(c),
        }
    }
    let mut result = String::from("/");
    for (i, c) in stack.iter().enumerate() {
        if i > 0 {
            result.push('/');
        }
        result.push_str(c);
    }
    result
}

/// Split a command line into `(command, rest)` on the first run of whitespace;
/// `rest` is the trimmed remainder (a single argument/text/path, exactly like
/// the kernel shell's prefix dispatch).
fn split_cmd(line: &str) -> (&str, &str) {
    match line.find(|c: char| c.is_ascii_whitespace()) {
        Some(i) => (&line[..i], line[i..].trim_start()),
        None => (line, ""),
    }
}

/// Dispatch one command line. FS verbs go through libdunit syscalls (resolved
/// against this process's cwd by the kernel); system verbs read `get_system_stats`.
/// Anything else is executed as `/app/<name>` on a bridged inner pty.
fn run(line: &str) {
    let line = line.trim();
    if line.is_empty() {
        return;
    }
    let (cmd, rest) = split_cmd(line);
    match cmd {
        "help" => cmd_help(),
        "echo" => cmd_echo(rest),
        "pwd" => {
            out(&cwd());
            out("\n");
        }
        "ls" => cmd_ls(rest),
        "cd" => cmd_cd(rest),
        "mkdir" => cmd_mkdir(rest),
        "touch" => cmd_touch(rest),
        "cat" => cmd_cat(rest),
        "rm" => cmd_rm(rest),
        "tree" => cmd_tree(rest),
        "clear" => out("\x1b[2J\x1b[H"),
        "uname" => cmd_uname(rest),
        "whoami" => out("root\n"),
        "date" => out("date: RTC unavailable\n"),
        "uptime" => cmd_uptime(),
        "free" => cmd_free(),
        "ps" => cmd_ps(),
        "top" => cmd_top(),
        "poweroff" | "shutdown" => {
            out("shutdown not implemented: ACPI/QEMU shutdown device unavailable\n")
        }
        // Privileged hardware/block/fs-admin diagnostics: a userspace shell can't
        // touch PCI/AHCI/block itself, so these are rendered by the kernel (the
        // owner of those drivers) via the `terminal_diag` gateway and printed
        // verbatim — identical output to kernel Terminal Mode, one source of truth.
        "lspci" | "usb" | "blk" | "lsblk" | "ahci" | "devs" | "blkread" | "blkwrite"
        | "mkfs.dunit" | "mount.dunit" | "install.dunit" => run_diag(line),
        "exit" => libdunit::exit(0),
        // `exec <name>` runs /app/<name> — the same verb kernel Terminal Mode
        // uses to launch a program, so muscle memory carries over 1:1.
        "exec" => {
            let (name, _) = split_cmd(rest);
            if name.is_empty() {
                out("exec: missing operand\n");
            } else {
                let path = format!("/app/{}", name);
                if !spawn_external(&path) {
                    out(&format!("dsh: command not found: {}\n", name));
                }
            }
        }
        _ => {
            let path = format!("/app/{}", cmd);
            if !spawn_external(&path) {
                out(&format!("dsh: command not found: {}\n", cmd));
            }
        }
    }
}

fn cmd_help() {
    out("dsh — Dunit userspace shell (full parity with text-mode terminal)\n");
    out("filesystem: ls [path]  cd [path]  pwd  mkdir <dir>  touch <file>  cat <file>  rm <path>  tree [path]\n");
    out("text:       echo <text> [> file | >> file]\n");
    out("system:     uname [-a]  whoami  date  uptime  free  ps  top  dufetch\n");
    out("hardware:   lspci  usb  devs  blk  lsblk  ahci  blkread  blkwrite\n");
    out("disk/fs:    mkfs.dunit  mount.dunit  install.dunit\n");
    out("control:    clear  help  exit  poweroff  shutdown\n");
    out("any other name (or `exec <name>`) runs /app/<name> on a bridged pty\n");
}

fn cmd_echo(rest: &str) {
    if let Some(idx) = rest.find(">>") {
        let text = rest[..idx].trim_end();
        let path = rest[idx + 2..].trim();
        if path.is_empty() {
            out("echo: missing output file\n");
            return;
        }
        let data = format!("{}\n", text);
        if libdunit::append_string(path, &data).is_err() {
            out(&format!("echo: {}: write error\n", path));
        }
        return;
    }
    if let Some(idx) = rest.find('>') {
        let text = rest[..idx].trim_end();
        let path = rest[idx + 1..].trim();
        if path.is_empty() {
            out("echo: missing output file\n");
            return;
        }
        let data = format!("{}\n", text);
        if libdunit::write_string(path, &data).is_err() {
            out(&format!("echo: {}: write error\n", path));
        }
        return;
    }
    out(rest);
    out("\n");
}

fn cmd_ls(rest: &str) {
    let path = if rest.is_empty() { "." } else { rest };
    let mut entries = dir_buf(256);
    let n = libdunit::readdir(path, &mut entries);
    if n < 0 {
        out(&format!("ls: {}: {}\n", path, libdunit::error_name(n)));
        return;
    }
    for i in 0..(n as usize) {
        if i > 0 {
            out("  ");
        }
        out(entries[i].name());
    }
    out("\n");
}

fn cmd_cd(rest: &str) {
    let path = if rest.is_empty() { "/" } else { rest };
    let r = libdunit::chdir(path);
    if r < 0 {
        out(&format!("cd: {}: {}\n", path, libdunit::error_name(r)));
    }
}

fn cmd_mkdir(rest: &str) {
    if rest.is_empty() {
        out("mkdir: missing operand\n");
        return;
    }
    let r = libdunit::mkdir(rest);
    if r < 0 {
        out(&format!("mkdir: {}: {}\n", rest, libdunit::error_name(r)));
    }
}

fn cmd_touch(rest: &str) {
    if rest.is_empty() {
        out("touch: missing operand\n");
        return;
    }
    let fd = libdunit::open(rest, libdunit::OPEN_CREATE | libdunit::OPEN_WRITE);
    if fd < 0 {
        out(&format!("touch: {}: {}\n", rest, libdunit::error_name(fd)));
    } else {
        libdunit::close(fd as usize);
    }
}

fn cmd_cat(rest: &str) {
    if rest.is_empty() {
        out("cat: missing operand\n");
        return;
    }
    let fd = libdunit::open(rest, libdunit::OPEN_READ);
    if fd < 0 {
        out(&format!("cat: {}: {}\n", rest, libdunit::error_name(fd)));
        return;
    }
    let fd = fd as usize;
    let mut buf = [0u8; 512];
    loop {
        let n = libdunit::read(fd, &mut buf);
        if n <= 0 {
            break;
        }
        match core::str::from_utf8(&buf[..n as usize]) {
            Ok(s) => out(s),
            Err(_) => out("<binary>"),
        }
    }
    libdunit::close(fd);
    out("\n");
}

fn cmd_rm(rest: &str) {
    if rest.is_empty() {
        out("rm: missing operand\n");
        return;
    }
    let r = libdunit::unlink(rest);
    if r < 0 {
        out(&format!("rm: {}: {}\n", rest, libdunit::error_name(r)));
    }
}

fn cmd_tree(rest: &str) {
    let base = normalize(&cwd(), if rest.is_empty() { "." } else { rest });
    out(&base);
    out("\n");
    tree_recurse(&base, 1);
}

fn tree_recurse(dir: &str, depth: usize) {
    if depth > 16 {
        return;
    }
    let mut entries = dir_buf(256);
    let n = libdunit::readdir(dir, &mut entries);
    if n < 0 {
        return;
    }
    for i in 0..(n as usize) {
        let name = entries[i].name();
        for _ in 0..depth {
            out("  ");
        }
        out(name);
        if entries[i].file_type == libdunit::FILE_TYPE_DIRECTORY {
            out("/\n");
            let child = join(dir, name);
            tree_recurse(&child, depth + 1);
        } else {
            out("\n");
        }
    }
}

fn cmd_uname(rest: &str) {
    if rest == "-a" {
        out("Dunit OS 1.0.0 Green Tea x86_64 kernel=monolithic-rust-hal\n");
    } else {
        out("Dunit OS\n");
    }
}

fn cmd_uptime() {
    let mut s = SystemStats::default();
    libdunit::get_system_stats(&mut s);
    let hz = if s.uptime_hz == 0 { 100 } else { s.uptime_hz };
    let total = s.uptime_ticks / hz;
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let sec = total % 60;
    out(&format!(
        "up {}h {}m {}s ({} ticks @ {} Hz)\n",
        h, m, sec, s.uptime_ticks, hz
    ));
}

fn cmd_free() {
    let mut s = SystemStats::default();
    libdunit::get_system_stats(&mut s);
    let kib = |b: u64| b / 1024;
    out("              total        used        free\n");
    out(&format!(
        "PMM KiB:   {:>10}  {:>10}  {:>10}\n",
        kib(s.pmm_total_bytes),
        kib(s.pmm_used_bytes),
        kib(s.pmm_free_bytes)
    ));
    out(&format!(
        "Heap KiB:  {:>10}  {:>10}  {:>10}\n",
        kib(s.heap_total_bytes),
        kib(s.heap_used_bytes),
        kib(s.heap_free_bytes)
    ));
    out("Swap: unavailable\n");
}

fn cmd_ps() {
    let mut s = SystemStats::default();
    libdunit::get_system_stats(&mut s);
    out(&format!(
        "processes: total={} running={} ready={} prepared={} blocked={} dead={} reaped={}\n",
        s.process_total,
        s.process_running,
        s.process_ready,
        s.process_prepared,
        s.process_blocked,
        s.process_dead,
        s.process_reaped
    ));
}

/// A one-shot `top`-style snapshot (dsh has no alternate screen, so it prints
/// once rather than refreshing): tasks + physical/heap memory + IPC + FS, all
/// read from the single `get_system_stats` syscall.
fn cmd_top() {
    let mut s = SystemStats::default();
    libdunit::get_system_stats(&mut s);
    let kib = |b: u64| b / 1024;
    out(&format!(
        "tasks: {} total, {} running, {} ready, {} blocked\n",
        s.process_total, s.process_running, s.process_ready, s.process_blocked
    ));
    out(&format!(
        "mem KiB:  {} total, {} used, {} free\n",
        kib(s.pmm_total_bytes), kib(s.pmm_used_bytes), kib(s.pmm_free_bytes)
    ));
    out(&format!(
        "heap KiB: {} total, {} used, {} free ({} blocks)\n",
        kib(s.heap_total_bytes), kib(s.heap_used_bytes), kib(s.heap_free_bytes),
        s.heap_free_blocks
    ));
    out(&format!(
        "ipc: {} queues, {} queued, {} shared regions\n",
        s.ipc_queue_count, s.ipc_queued_messages, s.ipc_shared_regions
    ));
    out(&format!("fs: {} files, {} dirs\n", s.fs_files, s.fs_directories));
}

/// Render a PRIVILEGED diagnostic by asking the kernel (which owns the hardware)
/// through the `terminal_diag` gateway, then print its text verbatim — identical
/// output to kernel Terminal Mode. 16 KiB covers the largest listing.
fn run_diag(line: &str) {
    let mut buf = [0u8; 16384];
    let n = libdunit::terminal_diag(line, &mut buf);
    if n > 0 {
        libdunit::write(1, &buf[..n as usize]);
    } else if n < 0 {
        out(&format!("dsh: {}: diagnostic unavailable (err {})\n", line, n));
    }
}

/// Run an external program `path` (typically `/app/<name>`) on a private inner
/// pty and bridge its stdio to ours until it exits. dsh becomes the MASTER of
/// the inner pty; the child's stdout is forwarded to our stdout (the terminal).
///
/// On the input side dsh applies a COOKED LINE DISCIPLINE identical to kernel
/// Terminal Mode (`kernel/src/lib.rs::terminal_collect_foreground_input`):
/// classic terminal-mode programs (e.g. `calc`) do NOT echo their own keystrokes
/// and expect to receive whole lines terminated by '\n'. So dsh echoes printable
/// keystrokes back to our stdout, erases on Backspace, swallows ESC/CSI cursor
/// sequences, and only forwards a complete line (plus the terminating '\n') to
/// the child when Enter is pressed. Ctrl-C (0x03) kills the child and drops the
/// pending line. Keeping this discipline in dsh (userspace policy) is what makes
/// the GUI terminal behave "just like a clean terminal" WITHOUT the kernel pty
/// (a raw byte mechanism) growing any line discipline of its own. Returns false
/// if the program could not be spawned (so the caller can print "not found").
fn spawn_external(path: &str) -> bool {
    let inner = libdunit::pty_create();
    if inner < 0 {
        return false;
    }
    let inner = inner as u32;
    let child = libdunit::pty_spawn(path, inner);
    if child < 0 {
        libdunit::pty_close(inner);
        return false;
    }
    let child = child as u32;
    let mut out_buf = [0u8; 512];
    let mut in_buf = [0u8; 256];
    // Cooked line buffer: accumulates one input line; the last slot is reserved
    // for the '\n' appended on Enter, so printables fill at most line.len()-1.
    let mut line = [0u8; 256];
    let mut llen = 0usize;
    // ESC-sequence state: 0 = normal, 1 = saw ESC (0x1B), 2 = inside CSI (ESC [).
    let mut esc = 0u8;
    loop {
        let mut progressed = false;
        // Child stdout -> our stdout (the terminal). EPIPE => child exited and
        // its output is fully drained: we are done.
        let n = libdunit::pty_read(inner, &mut out_buf);
        if n > 0 {
            libdunit::write(1, &out_buf[..n as usize]);
            progressed = true;
        } else if n == libdunit::EPIPE {
            break;
        }
        // Our stdin (terminal keystrokes) -> cooked line discipline -> child.
        let m = libdunit::read(0, &mut in_buf);
        if m > 0 {
            for &b in &in_buf[..m as usize] {
                match esc {
                    1 => {
                        esc = if b == b'[' { 2 } else { 0 };
                        continue;
                    }
                    2 => {
                        // A CSI final byte (0x40..=0x7E) ends the sequence.
                        if (0x40..=0x7e).contains(&b) {
                            esc = 0;
                        }
                        continue;
                    }
                    _ => {}
                }
                match b {
                    0x1B => esc = 1, // begin an escape sequence (arrows etc.)
                    0x03 => {
                        // Ctrl-C: interrupt the child, discard the pending line.
                        libdunit::kill(child);
                        llen = 0;
                    }
                    b'\n' => {
                        out("\n"); // echo the newline the way Terminal Mode does
                        if llen < line.len() {
                            line[llen] = b'\n';
                            llen += 1;
                        }
                        libdunit::pty_write(inner, &line[..llen]);
                        llen = 0;
                    }
                    b'\r' => {}  // Enter arrives as '\n'; ignore bare CR
                    b'\t' => {}  // ignore Tab (matches Terminal Mode)
                    0x08 | 0x7F => {
                        if llen > 0 {
                            llen -= 1;
                            // Destructive erase (the Term treats 0x08 as a plain
                            // non-destructive cursor-left), so blank + back up.
                            out("\x08 \x08");
                        }
                    }
                    0x20..=0x7E => {
                        if llen < line.len() - 1 {
                            line[llen] = b;
                            llen += 1;
                            libdunit::write(1, &[b]); // echo the typed character
                        }
                    }
                    _ => {}
                }
            }
            progressed = true;
        }
        if !progressed {
            libdunit::sleep_ms(5);
        }
    }
    let mut st = libdunit::WaitStatus::empty();
    let _ = libdunit::wait(child, &mut st);
    libdunit::pty_close(inner);
    true
}

/// ANSI escape-parser state for the line editor: bytes arriving from the
/// terminal may be raw ASCII or `ESC [ … final` control sequences (arrows,
/// Home/End/Delete) synthesized by `gui_terminal::encode_key`.
enum EscState {
    Normal,
    Esc,
    Csi,
}

/// A one-line editor with history and a real caret. It reprints the whole line
/// on every edit (`\r` + prompt + text + `ESC[K`, then `ESC[nD` to place the
/// caret), so the terminal only needs CR / EL / CUB — no absolute addressing.
struct Ed {
    line: String,
    cursor: usize,
    prompt: String,
    history: Vec<String>,
    hist_pos: usize,
    esc: EscState,
    csi_param: usize,
}

impl Ed {
    fn new(prompt_word: &str) -> Self {
        Ed {
            line: String::new(),
            cursor: 0,
            prompt: format!("{} ", prompt_word),
            history: Vec::new(),
            hist_pos: 0,
            esc: EscState::Normal,
            csi_param: 0,
        }
    }

    fn start(&self) {
        out(&self.prompt);
    }

    fn feed(&mut self, b: u8) {
        match self.esc {
            EscState::Normal => self.feed_normal(b),
            EscState::Esc => {
                if b == b'[' {
                    self.esc = EscState::Csi;
                    self.csi_param = 0;
                } else {
                    self.esc = EscState::Normal;
                }
            }
            EscState::Csi => self.feed_csi(b),
        }
    }

    fn feed_normal(&mut self, b: u8) {
        match b {
            0x1B => self.esc = EscState::Esc,
            b'\r' | b'\n' => self.submit(),
            0x08 | 0x7F => self.backspace(),
            0x03 => self.cancel(),                          // Ctrl-C
            0x0C => self.clear_screen(),                    // Ctrl-L
            0x01 => self.move_home(),                       // Ctrl-A
            0x05 => self.move_end(),                        // Ctrl-E
            0x15 => {
                // Ctrl-U: kill the whole line
                self.line.clear();
                self.cursor = 0;
                self.redraw();
            }
            0x20..=0x7E => self.insert(b as char),
            _ => {}
        }
    }

    fn feed_csi(&mut self, b: u8) {
        if b.is_ascii_digit() {
            self.csi_param = self.csi_param.saturating_mul(10) + (b - b'0') as usize;
            return;
        }
        match b {
            b'A' => self.history_prev(),
            b'B' => self.history_next(),
            b'C' => self.move_right(),
            b'D' => self.move_left(),
            b'H' => self.move_home(),
            b'F' => self.move_end(),
            b'~' => match self.csi_param {
                1 | 7 => self.move_home(),
                4 | 8 => self.move_end(),
                3 => self.delete_forward(),
                _ => {}
            },
            _ => {}
        }
        self.esc = EscState::Normal;
    }

    fn insert(&mut self, c: char) {
        self.line.insert(self.cursor, c);
        self.cursor += c.len_utf8();
        self.redraw();
    }

    fn backspace(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            self.line.remove(self.cursor);
            self.redraw();
        }
    }

    fn delete_forward(&mut self) {
        if self.cursor < self.line.len() {
            self.line.remove(self.cursor);
            self.redraw();
        }
    }

    fn move_left(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            self.redraw();
        }
    }

    fn move_right(&mut self) {
        if self.cursor < self.line.len() {
            self.cursor += 1;
            self.redraw();
        }
    }

    fn move_home(&mut self) {
        self.cursor = 0;
        self.redraw();
    }

    fn move_end(&mut self) {
        self.cursor = self.line.len();
        self.redraw();
    }

    fn history_prev(&mut self) {
        if self.hist_pos > 0 {
            self.hist_pos -= 1;
            self.line = self.history[self.hist_pos].clone();
            self.cursor = self.line.len();
            self.redraw();
        }
    }

    fn history_next(&mut self) {
        if self.hist_pos < self.history.len() {
            self.hist_pos += 1;
            if self.hist_pos == self.history.len() {
                self.line.clear();
            } else {
                self.line = self.history[self.hist_pos].clone();
            }
            self.cursor = self.line.len();
            self.redraw();
        }
    }

    fn cancel(&mut self) {
        out("\n");
        self.line.clear();
        self.cursor = 0;
        self.hist_pos = self.history.len();
        out(&self.prompt);
    }

    fn clear_screen(&mut self) {
        out("\x1b[2J\x1b[H");
        self.redraw();
    }

    fn submit(&mut self) {
        out("\n");
        let entered = core::mem::take(&mut self.line);
        self.cursor = 0;
        let trimmed = entered.trim();
        if !trimmed.is_empty()
            && self.history.last().map(|s| s.as_str()) != Some(entered.as_str())
        {
            self.history.push(entered.clone());
        }
        self.hist_pos = self.history.len();
        run(&entered);
        out(&self.prompt);
    }

    fn redraw(&self) {
        out("\r");
        out(&self.prompt);
        out(&self.line);
        out("\x1b[K");
        let back = self.line.len() - self.cursor;
        if back > 0 {
            out(&format!("\x1b[{}D", back));
        }
    }
}

#[no_mangle]
pub extern "C" fn _start(
    argc: usize,
    argv: *const *const u8,
    envp: *const *const u8,
) -> ! {
    libdunit::init_runtime(argc, argv, envp);
    // The terminal passes its configured `[terminal] prompt` via the pty-spawn
    // environment; fall back to the built-in default when launched without one.
    let prompt_word = libdunit::getenv("PROMPT").unwrap_or(PROMPT_DEFAULT);
    let mut ed = Ed::new(prompt_word);
    ed.start();

    let mut buf = [0u8; 128];
    loop {
        let n = libdunit::read(0, &mut buf);
        if n > 0 {
            for i in 0..(n as usize) {
                ed.feed(buf[i]);
            }
        } else if n == 0 {
            // EOF on stdin: the terminal (pty master) went away — exit cleanly.
            libdunit::exit(0);
        } else {
            // EAGAIN or a transient error: nothing to read yet, back off briefly.
            libdunit::sleep_ms(5);
        }
    }
}








