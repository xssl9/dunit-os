#![no_std]
#![no_main]

//! Untrusted terminal client for the M4 userspace DWM (Stack B).
//!
//! The last legacy GUI app reborn on the userspace stack. It owns a PTY, spawns
//! the `dsh` shell as the slave, and bridges three streams: keystrokes the
//! compositor routes to the focused window (IN_KEY) are written into the pty
//! master; the shell's stdout is drained from the master into an on-screen
//! scrollback; that scrollback is painted into our own shared buffer via the M4
//! UI Runtime (DUI + DSS + TTF). Same capability + wire-protocol path as
//! gui_client/gui_stat — the compositor owns display/input, we own our surface.

use core::panic::PanicInfo;

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use gui_protocol_v1::wire::{Request, FEATURE_ARGB8888};

use dunit_render::{paint, Surface};
use dunit_style::cascade::{Cascade, NodeStyle};
use dunit_style::parse as parse_dss;
use dunit_text::Font;
use dunit_ui::layout::layout_measured;
use dunit_ui::parse as parse_dui;
use dunit_ui::tree::{Kind, NodeId};
use dunit_widgets::{intrinsic_size, FontMeasure, Widget};

// Control-message magics shared with the compositor.
const CTRL_MAGIC: u32 = 0x3150_4143; // "CAP1" — capability announce
const INPUT_MAGIC: u32 = 0x3150_4E49; // "INP1" — input events
const IN_KEY: u8 = 5;
const IN_QUIT: u8 = 9;

const SURFACE: u64 = 1;
const BUFFER: u64 = 2;
const W: u32 = 560;
const H: u32 = 360;
const FMT_XRGB8888: u32 = 1;

/// Visible text rows (bounded by H at font-size 13) and total scrollback cap.
const VISIBLE_ROWS: usize = 15;
const SCROLL_CAP: usize = 200;
/// Poll cadence: block on compositor IPC at most this long, then drain the pty
/// and repaint. The compositor blits our buffer every tick regardless.
const POLL_MS: u64 = 40;

// APPEND_MARKER

/// The window's text font, embedded in the ELF (M4 still ships assets in-image).
static FONT_BYTES: &[u8] = include_bytes!("../../../../assets/fonts/DejaVuSans.ttf");

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    libdunit::println("gui_terminal: PANIC");
    libdunit::exit(101)
}

fn u64_at(p: &[u8], off: usize) -> u64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&p[off..off + 8]);
    u64::from_le_bytes(a)
}

/// Append a `Text "…"` child, stripping characters that would break DUI parsing.
/// Empty rows become a single space so the runtime still allocates a line box.
fn push_line(d: &mut String, text: &str) {
    d.push_str("Text \"");
    let mut any = false;
    for c in text.chars() {
        if c != '"' && c != '{' && c != '}' {
            d.push(c);
            any = true;
        }
    }
    if !any {
        d.push(' ');
    }
    d.push_str("\" ");
}

/// On-screen terminal model: completed lines plus the line currently being
/// assembled from the shell's byte stream. `feed` interprets the stdout stream
/// (newline commits a row, form-feed clears, backspace erases, printable ASCII
/// appends); `dirty` gates repaints.
struct Term {
    lines: Vec<String>,
    cur: String,
    dirty: bool,
}

impl Term {
    fn new() -> Self {
        Term { lines: Vec::new(), cur: String::new(), dirty: true }
    }

    fn commit_line(&mut self) {
        let done = core::mem::take(&mut self.cur);
        // Echo each completed row to serial so the shell path is headlessly
        // verifiable (this is our own stdout/console, not the pty).
        libdunit::write(1, b"[term] ");
        libdunit::write(1, done.as_bytes());
        libdunit::write(1, b"\n");
        self.lines.push(done);
        if self.lines.len() > SCROLL_CAP {
            let excess = self.lines.len() - SCROLL_CAP;
            self.lines.drain(0..excess);
        }
    }

    fn feed(&mut self, bytes: &[u8]) {
        for &b in bytes {
            match b {
                b'\n' => self.commit_line(),
                b'\r' => {}
                0x0c => {
                    self.lines.clear();
                    self.cur.clear();
                }
                0x08 => {
                    self.cur.pop();
                }
                0x20..=0x7e => self.cur.push(b as char),
                _ => {}
            }
        }
        if !bytes.is_empty() {
            self.dirty = true;
        }
    }

    /// Build the DUI document: the last VISIBLE_ROWS rows (completed lines with
    /// the in-progress line as the final row), stacked in a Column.
    fn build_dui(&self) -> String {
        let total = self.lines.len() + 1; // +1 for the current line
        let start = total.saturating_sub(VISIBLE_ROWS);
        let mut d = String::from("Column#win { ");
        for line in self.lines.iter().skip(start) {
            push_line(&mut d, line);
        }
        push_line(&mut d, &self.cur);
        d.push('}');
        d
    }
}

/// Paint the current scrollback into the mapped ARGB8888 buffer via the M4 UI
/// Runtime (DUI tree -> DSS cascade -> content-measured layout -> render).
fn render(px: *mut u8, font: &Font, term: &Term) {
    let dui = term.build_dui();
    let tree = match parse_dui(&dui) {
        Ok(t) => t,
        Err(_) => return,
    };
    let mut dss = String::new();
    dss.push_str("Column#win { background: #0b0f14; padding: 8; }\n");
    dss.push_str("Text { color: #a6e3a1; font-size: 13; padding: 1; }\n");
    let sheet = match parse_dss(&dss) {
        Ok(s) => s,
        Err(_) => return,
    };
    let mut cas = Cascade::new();
    cas.push(sheet);

    let style_of = |nid: NodeId| {
        let node = tree.node(nid);
        let tag = node.kind.tag();
        let ns = match node.name.as_deref() {
            Some(name) => NodeStyle { element: tag, id: Some(name), classes: &[], states: &[] },
            None => NodeStyle::element(tag),
        };
        cas.resolve(&ns)
    };

    let fm = FontMeasure { font };
    let measure_fn = |nid: NodeId| {
        let node = tree.node(nid);
        if let Kind::Element(tag) = &node.kind {
            if let Some(w) = Widget::from_tag(tag) {
                return intrinsic_size(w, node.text.as_deref(), &style_of(nid), &fm);
            }
        }
        (0.0, 0.0)
    };
    let lay = layout_measured(&tree, W as f32, H as f32, &measure_fn);

    let pixels = unsafe { core::slice::from_raw_parts_mut(px as *mut u32, (W * H) as usize) };
    let mut surface = Surface::new(pixels, W as usize, H as usize);
    paint(&tree, &lay, &style_of, font, &mut surface);
}

// APPEND_START

#[no_mangle]
pub extern "C" fn _start() -> ! {
    // 1) Handshake: compositor pid + our client id (id is a tint hint only).
    let mut m = [0u8; 8];
    if libdunit::ipc_recv_blocking(&mut m, 0) < 8 {
        libdunit::println("gui_terminal: FAIL recv handshake");
        libdunit::exit(1);
    }
    let compositor = u32::from_le_bytes([m[0], m[1], m[2], m[3]]);

    // 2) Create + map our own shared buffer and paint the initial (empty) frame.
    let bytes = (W * H * 4) as usize;
    let buf = libdunit::handle_create_shared(bytes);
    if buf <= 0 {
        libdunit::println("gui_terminal: FAIL create buffer");
        libdunit::exit(2);
    }
    let buf = buf as u32;
    let mapped = libdunit::handle_map(buf, 0, bytes);
    if mapped <= 0 {
        libdunit::println("gui_terminal: FAIL map buffer");
        libdunit::exit(3);
    }
    let px = mapped as usize as *mut u8;
    let font = match Font::parse(FONT_BYTES.to_vec()) {
        Ok(f) => f,
        Err(_) => {
            libdunit::println("gui_terminal: FAIL font parse");
            libdunit::exit(7);
        }
    };
    let mut term = Term::new();
    render(px, &font, &term);

    let mut rx = [0u8; 256];

    // 3) HELLO -> WELCOME.
    let hello = Request::Hello {
        min_minor: 0,
        max_minor: 0,
        offered_features: FEATURE_ARGB8888,
        required_features: 0,
    }
    .encode(0, 1);
    libdunit::ipc_send(compositor, &hello);
    if libdunit::ipc_recv_blocking(&mut rx, 0) <= 0 {
        libdunit::println("gui_terminal: FAIL welcome");
        libdunit::exit(4);
    }

    // 4) CREATE_SURFACE -> RESULT + CONFIGURE (capture the configure token).
    let create =
        Request::CreateSurface { role: 1, width: W, height: H, format: FMT_XRGB8888 }.encode(SURFACE, 2);
    libdunit::ipc_send(compositor, &create);
    libdunit::ipc_recv_blocking(&mut rx, 0); // RESULT
    let n = libdunit::ipc_recv_blocking(&mut rx, 0); // CONFIGURE
    let token = if n >= 32 { u64_at(&rx, 24) } else { 0 };

    // 5) ACK_CONFIGURE -> RESULT.
    let ack = Request::AckConfigure { configure: token }.encode(SURFACE, 3);
    libdunit::ipc_send(compositor, &ack);
    libdunit::ipc_recv_blocking(&mut rx, 0);

    // 6) Transfer the buffer capability (read-only) and announce it.
    let dup = libdunit::handle_dup(
        buf,
        libdunit::RIGHT_READ | libdunit::RIGHT_MAP | libdunit::RIGHT_TRANSFER,
    );
    let ch = if dup > 0 {
        libdunit::handle_transfer(dup as u32, compositor)
    } else {
        -1
    };
    if ch <= 0 {
        libdunit::println("gui_terminal: FAIL cap transfer");
        libdunit::exit(5);
    }
    let mut ann = [0u8; 16];
    ann[0..4].copy_from_slice(&CTRL_MAGIC.to_le_bytes());
    ann[4..8].copy_from_slice(&(ch as u32).to_le_bytes());
    ann[8..12].copy_from_slice(&(bytes as u32).to_le_bytes());
    ann[12..16].copy_from_slice(&(BUFFER as u32).to_le_bytes());
    libdunit::ipc_send(compositor, &ann);

    // 7) IMPORT / ATTACH / COMMIT (each -> RESULT).
    let import =
        Request::ImportBuffer { width: W, height: H, stride: W * 4, format: FMT_XRGB8888, offset: 0 }
            .encode(BUFFER, 4);
    libdunit::ipc_send(compositor, &import);
    libdunit::ipc_recv_blocking(&mut rx, 0);
    let attach = Request::AttachBuffer { buffer: BUFFER, damage: alloc::vec::Vec::new() }.encode(SURFACE, 5);
    libdunit::ipc_send(compositor, &attach);
    libdunit::ipc_recv_blocking(&mut rx, 0);
    let commit = Request::Commit { configure: token, frame_callback: 1 }.encode(SURFACE, 6);
    libdunit::ipc_send(compositor, &commit);
    libdunit::ipc_recv_blocking(&mut rx, 0);

    // 8) FRAME_DONE from the compositor's composition tick.
    let n = libdunit::ipc_recv_blocking(&mut rx, 0);
    let ok = n >= 32 && u16::from_le_bytes([rx[8], rx[9]]) == 0x8021;
    if ok {
        libdunit::println("gui_terminal: surface presented OK");
    } else {
        libdunit::println("gui_terminal: FAIL no frame done");
        libdunit::handle_close(buf);
        libdunit::exit(6);
    }

    // 9) Spawn the shell as our pty slave: dsh's stdin/stdout are the pty rings.
    let pty = libdunit::pty_create();
    if pty <= 0 {
        libdunit::println("gui_terminal: FAIL pty create");
        libdunit::handle_close(buf);
        libdunit::exit(8);
    }
    let pty = pty as u32;
    if libdunit::pty_spawn("dsh", pty) <= 0 {
        libdunit::println("gui_terminal: FAIL pty spawn");
        libdunit::pty_close(pty);
        libdunit::handle_close(buf);
        libdunit::exit(9);
    }

    // 10) Bridge loop: block on compositor IPC (short timeout). Forward IN_KEY
    //     bytes into the pty master; on any wake, drain the shell's stdout into
    //     the scrollback and repaint if it changed. Exit on IN_QUIT.
    let mut sbuf = [0u8; 256];
    loop {
        let n = libdunit::ipc_recv_blocking(&mut rx, POLL_MS);
        if n >= 8 && u32::from_le_bytes([rx[0], rx[1], rx[2], rx[3]]) == INPUT_MAGIC {
            match rx[4] {
                IN_QUIT => break,
                IN_KEY => {
                    let byte = rx[16];
                    libdunit::pty_write(pty, &[byte]);
                }
                _ => {}
            }
        }
        // Drain whatever the shell has produced since the last tick.
        loop {
            let r = libdunit::pty_read(pty, &mut sbuf);
            if r > 0 {
                term.feed(&sbuf[..r as usize]);
            } else {
                break; // EAGAIN / EOF / EPIPE — nothing more this tick
            }
        }
        if term.dirty {
            render(px, &font, &term);
            term.dirty = false;
        }
    }

    libdunit::pty_close(pty);
    libdunit::handle_close(buf);
    libdunit::exit(0)
}


