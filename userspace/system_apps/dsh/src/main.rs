#![no_std]
#![no_main]

//! Interactive PTY-slave shell for the M4 userspace terminal (Stack B).
//!
//! Spawned by `gui_terminal` through `pty_spawn`, so fd 0/1/2 are routed to the
//! pty rings (not the console). It runs a full line editor over the raw byte
//! stream the terminal forwards: printable insert at the cursor, Backspace and
//! Delete, cursor movement (←/→, Home/End) and history (↑/↓) via the xterm ESC
//! sequences `gui_terminal::encode_key` emits, plus the control codes Ctrl-C
//! (abort line), Ctrl-D (EOF on an empty line) and Ctrl-L (clear). On Enter it
//! runs one of a small set of builtins, or — for any other name — spawns
//! `/app/<name>` as a child on a private inner pty and bridges its stdio to the
//! terminal until it exits (see `spawn_external`). Output is written to stdout,
//! which the gui_terminal client interprets into its on-screen scrollback.

use core::panic::PanicInfo;

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

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

/// Launch an external program as a child on a private (inner) pty and bridge it
/// to our own stdio for the duration, so a non-builtin command behaves like a
/// real shell exec. `dsh` is itself the pty *slave* of `gui_terminal` (fd 0/1 are
/// the outer rings); here we flip roles and become the *master* of a fresh inner
/// pty whose slave is the child. The bridge then pumps two directions until the
/// child exits: child stdout (inner master read) → our stdout (fd 1 → terminal),
/// and terminal keystrokes (our stdin, fd 0) → child stdin (inner master write).
/// Ctrl-C on the way through kills the child. Returns false if `cmd` could not be
/// spawned (unknown path / not an ELF), so the caller can report "not found".
fn spawn_external(cmd: &str) -> bool {
    let inner = libdunit::pty_create();
    if inner <= 0 {
        return false;
    }
    let inner = inner as u32;
    // `pty_spawn` resolves a bare name against /app (see resolve_exec_path); a
    // negative return means the program does not exist or is not a valid ELF.
    let child = libdunit::pty_spawn(cmd, inner);
    if child <= 0 {
        libdunit::pty_close(inner);
        return false;
    }
    let child = child as u32;

    let mut obuf = [0u8; 256];
    let mut ibuf = [0u8; 128];
    loop {
        // 1) Drain everything the child has produced, fast, before touching input.
        let r = libdunit::pty_read(inner, &mut obuf);
        if r > 0 {
            libdunit::write(1, &obuf[..r as usize]);
            continue;
        }
        if r == libdunit::EPIPE {
            break; // child exited and its output is fully drained
        }
        // 2) Forward any terminal input to the child. Ctrl-C interrupts it.
        let k = libdunit::read(0, &mut ibuf);
        if k > 0 {
            let bytes = &ibuf[..k as usize];
            if bytes.contains(&0x03) {
                libdunit::kill(child);
            }
            libdunit::pty_write(inner, bytes);
            continue;
        }
        // 3) Nothing either way: yield on a short timer (see the main loop note).
        libdunit::sleep_ms(5);
    }
    libdunit::pty_close(inner);
    true
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
        "help" => out("builtins: help echo pwd ls cd clear uname exit — other names run /app/<name>\n"),
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
            // Not a builtin: try to run it as an external program from /app.
            if !spawn_external(cmd) {
                out(cmd);
                out(": command not found\n");
            }
        }
    }
}

/// Repaint the editable line in the terminal. The terminal renderer only
/// understands "append printable" and "backspace pops the last char", so we
/// erase the previously-echoed typed portion with backspaces (never touching
/// the prompt) and rewrite the whole buffer. The visual caret therefore always
/// sits at end-of-line even when the logical cursor is mid-line.
fn redraw(line: &str, echoed: &mut usize) {
    for _ in 0..*echoed {
        libdunit::write(1, &[0x08]);
    }
    libdunit::write(1, line.as_bytes());
    *echoed = line.len();
}

/// xterm ESC-sequence parser state (`ESC` `[` `<param>` `<final>`).
enum Esc {
    Normal,
    Esc,
    Csi,
}

/// The line editor: the current buffer, a logical cursor (byte index; input is
/// ASCII so byte == char), the count of typed chars currently on screen, the
/// command history with a navigation index, and the ESC parser state.
struct Ed {
    line: String,
    cursor: usize,
    echoed: usize,
    history: Vec<String>,
    hist_idx: usize,
    esc: Esc,
    param: u32,
}

impl Ed {
    fn new() -> Self {
        Ed {
            line: String::new(),
            cursor: 0,
            echoed: 0,
            history: Vec::new(),
            hist_idx: 0,
            esc: Esc::Normal,
            param: 0,
        }
    }

    fn insert(&mut self, c: char) {
        if self.line.len() >= 256 {
            return;
        }
        self.line.insert(self.cursor, c);
        self.cursor += 1;
        redraw(&self.line, &mut self.echoed);
    }

    fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        self.cursor -= 1;
        self.line.remove(self.cursor);
        redraw(&self.line, &mut self.echoed);
    }

    fn del_forward(&mut self) {
        if self.cursor >= self.line.len() {
            return;
        }
        self.line.remove(self.cursor);
        redraw(&self.line, &mut self.echoed);
    }

    fn left(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    fn right(&mut self) {
        if self.cursor < self.line.len() {
            self.cursor += 1;
        }
    }

    fn home(&mut self) {
        self.cursor = 0;
    }

    fn end(&mut self) {
        self.cursor = self.line.len();
    }

    fn hist_prev(&mut self) {
        if self.hist_idx == 0 {
            return;
        }
        self.hist_idx -= 1;
        self.line = self.history[self.hist_idx].clone();
        self.cursor = self.line.len();
        redraw(&self.line, &mut self.echoed);
    }

    fn hist_next(&mut self) {
        if self.hist_idx >= self.history.len() {
            return;
        }
        self.hist_idx += 1;
        if self.hist_idx == self.history.len() {
            self.line.clear();
        } else {
            self.line = self.history[self.hist_idx].clone();
        }
        self.cursor = self.line.len();
        redraw(&self.line, &mut self.echoed);
    }

    fn ctrl_c(&mut self) {
        out("^C\n");
        self.line.clear();
        self.cursor = 0;
        self.echoed = 0;
        self.hist_idx = self.history.len();
        out(PROMPT);
    }

    fn clear_screen(&mut self) {
        out("\x0c"); // form feed: gui_terminal clears the scrollback
        out(PROMPT);
        self.echoed = 0;
        redraw(&self.line, &mut self.echoed);
    }

    fn submit(&mut self) {
        out("\n");
        let entered = core::mem::take(&mut self.line);
        self.cursor = 0;
        self.echoed = 0;
        let trimmed = entered.trim();
        if !trimmed.is_empty() && self.history.last().map(|s| s.as_str()) != Some(trimmed) {
            self.history.push(String::from(trimmed));
        }
        self.hist_idx = self.history.len();
        run(&entered);
        out(PROMPT);
    }

    /// Feed one raw byte from the pty through the editor / ESC state machine.
    fn byte(&mut self, b: u8) {
        match self.esc {
            Esc::Esc => {
                self.esc = if b == b'[' { Esc::Csi } else { Esc::Normal };
                return;
            }
            Esc::Csi => {
                match b {
                    b'0'..=b'9' => {
                        self.param = self.param.saturating_mul(10).saturating_add((b - b'0') as u32);
                        return; // keep collecting the numeric parameter
                    }
                    b'A' => self.hist_prev(),
                    b'B' => self.hist_next(),
                    b'C' => self.right(),
                    b'D' => self.left(),
                    b'H' => self.home(),
                    b'F' => self.end(),
                    b'~' => {
                        if self.param == 3 {
                            self.del_forward(); // ESC[3~ = Delete
                        }
                    }
                    _ => {}
                }
                self.param = 0;
                self.esc = Esc::Normal;
                return;
            }
            Esc::Normal => {}
        }
        match b {
            0x1b => self.esc = Esc::Esc,
            b'\r' | b'\n' => self.submit(),
            0x08 | 0x7f => self.backspace(),
            0x03 => self.ctrl_c(),
            0x04 => {
                if self.line.is_empty() {
                    out("\n");
                    libdunit::exit(0);
                }
            }
            0x0c => self.clear_screen(),
            0x20..=0x7e => self.insert(b as char),
            _ => {}
        }
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    out(PROMPT);
    let mut ed = Ed::new();
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
            ed.byte(b);
        }
    }
}
