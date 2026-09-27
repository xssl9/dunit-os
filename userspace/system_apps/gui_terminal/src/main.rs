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

use dunit_render::Surface;
use dunit_style::value::Color;
use dunit_text::Font;

// Control-message magics shared with the compositor.
const CTRL_MAGIC: u32 = 0x3150_4143; // "CAP1" — capability announce
const INPUT_MAGIC: u32 = 0x3150_4E49; // "INP1" — input events
const IN_KEY: u8 = 5;
const IN_SCROLL: u8 = 6;
const IN_QUIT: u8 = 9;

// Modifier bit in the IN_KEY `mods` field (mirrors libdunit::KEYMOD_CTRL).
const KEYMOD_CTRL: u8 = 1 << 1;

// PS/2 scancodes (sc & 0x7F) for keys the kernel leaves un-cooked (ascii = 0).
const SC_BACKSPACE: u8 = 0x0E;
const SC_HOME: u8 = 0x47;
const SC_UP: u8 = 0x48;
const SC_LEFT: u8 = 0x4B;
const SC_RIGHT: u8 = 0x4D;
const SC_END: u8 = 0x4F;
const SC_DOWN: u8 = 0x50;
const SC_DELETE: u8 = 0x53;
// Page keys are handled locally for scrollback (never forwarded to the pty).
const SC_PGUP: u8 = 0x49;
const SC_PGDN: u8 = 0x51;

/// Translate a widened IN_KEY event (scancode + mods + cooked ASCII) into the
/// byte sequence to feed the pty. Returns the number of bytes written to `out`.
///
/// - Ctrl + letter -> the control code (`c & 0x1F`): Ctrl-C=0x03, Ctrl-D=0x04,
///   Ctrl-L=0x0C, etc.
/// - Any other cooked byte (printable, Enter `\n`, Tab `\t`) -> itself.
/// - Backspace (no ASCII) -> 0x08.
/// - Arrows / Home / End / Delete (no ASCII) -> the usual xterm ESC sequences,
///   which `dsh` parses for line editing and history.
fn encode_key(scancode: u8, mods: u8, ascii: u8, out: &mut [u8; 4]) -> usize {
    if mods & KEYMOD_CTRL != 0 && ascii.is_ascii_alphabetic() {
        out[0] = ascii & 0x1F;
        return 1;
    }
    if ascii != 0 {
        out[0] = ascii;
        return 1;
    }
    // Extended / navigation keys arrive with ascii == 0.
    let esc: &[u8] = match scancode {
        SC_BACKSPACE => &[0x08],
        SC_UP => &[0x1B, b'[', b'A'],
        SC_DOWN => &[0x1B, b'[', b'B'],
        SC_RIGHT => &[0x1B, b'[', b'C'],
        SC_LEFT => &[0x1B, b'[', b'D'],
        SC_HOME => &[0x1B, b'[', b'H'],
        SC_END => &[0x1B, b'[', b'F'],
        SC_DELETE => &[0x1B, b'[', b'3', b'~'],
        _ => &[],
    };
    out[..esc.len()].copy_from_slice(esc);
    esc.len()
}

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

/// Load the configured TTF (`[desktop] font`) from the VFS, falling back to the
/// embedded `FONT_BYTES` on any error — the desktop font is a live config knob.
fn load_font() -> Result<Font, ()> {
    let cfg = dwm_settings::load();
    if let Some(bytes) = libdunit::read_binary(cfg.desktop.font.as_str(), 4 * 1024 * 1024) {
        if let Ok(f) = Font::parse(bytes) {
            return Ok(f);
        }
    }
    Font::parse(FONT_BYTES.to_vec()).map_err(|_| ())
}

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

/// Terminal colors (ARGB8888). Background is fixed; the default foreground
/// matches the old flat theme. The live `fg` is mutated by SGR (ESC[…m).
const BG: u32 = 0xFF0B0F14;
const FG_DEFAULT: u32 = 0xFFA6E3A1;
const FONT_PX: f32 = 13.0;
const ROW_PX: i32 = 16;

/// The 8 ANSI colors (30-37) and bright variants (90-97), tinted to the desktop
/// palette so program output stays coherent with the theme.
const ANSI: [u32; 8] = [
    0xFF45475A, 0xFFF38BA8, 0xFFA6E3A1, 0xFFF9E2AF,
    0xFF89B4FA, 0xFFCBA6F7, 0xFF94E2D5, 0xFFCDD6F4,
];
const ANSI_BRIGHT: [u32; 8] = [
    0xFF585B70, 0xFFEBA0AC, 0xFFA6E3A1, 0xFFFAB387,
    0xFF89DCEB, 0xFFF5C2E7, 0xFF94E2D5, 0xFFFFFFFF,
];

/// ARGB8888 -> render Color.
fn col(argb: u32) -> Color {
    Color::rgba(
        ((argb >> 16) & 0xFF) as u8,
        ((argb >> 8) & 0xFF) as u8,
        (argb & 0xFF) as u8,
        ((argb >> 24) & 0xFF) as u8,
    )
}

/// One colored run of text within a scrollback line.
struct Span {
    text: String,
    fg: u32,
}

/// ESC-sequence parser state for interpreting the shell's stdout byte stream.
enum Esc {
    Normal,
    Esc,
    Csi,
}

/// On-screen terminal model: completed lines (each a run of colored `Span`s)
/// plus the line currently being assembled. `feed` interprets the byte stream —
/// printable ASCII appends to the active span, `\n` commits a row, `\f` clears,
/// `\b` erases, and CSI sequences (`ESC [ … m` etc.) drive SGR colors. `dirty`
/// gates repaints.
struct Term {
    lines: Vec<Vec<Span>>,
    cur: Vec<Span>,
    fg: u32,
    dirty: bool,
    /// Rows scrolled up from the live bottom (0 = following new output).
    scroll: usize,
    esc: Esc,
    params: [u32; 8],
    nparams: usize,
    param: u32,
    has_param: bool,
}

impl Term {
    fn new() -> Self {
        Term {
            lines: Vec::new(),
            cur: Vec::new(),
            fg: FG_DEFAULT,
            dirty: true,
            scroll: 0,
            esc: Esc::Normal,
            params: [0; 8],
            nparams: 0,
            param: 0,
            has_param: false,
        }
    }

    /// Move the view up (`up = true`) or down through the scrollback by `rows`,
    /// clamped so it never scrolls past the top or below the live bottom.
    fn scroll_by(&mut self, rows: usize, up: bool) {
        let max = self.lines.len().saturating_sub(1);
        self.scroll = if up {
            (self.scroll + rows).min(max)
        } else {
            self.scroll.saturating_sub(rows)
        };
        self.dirty = true;
    }

    /// Append one printable char to the active span, opening a new run when the
    /// current foreground color differs from the last span's.
    fn push_char(&mut self, c: char) {
        let need_new = match self.cur.last() {
            Some(s) => s.fg != self.fg,
            None => true,
        };
        if need_new {
            self.cur.push(Span { text: String::new(), fg: self.fg });
        }
        self.cur.last_mut().unwrap().text.push(c);
    }

    fn commit_line(&mut self) {
        let done = core::mem::take(&mut self.cur);
        // Echo the row's text to serial so the shell path stays headlessly
        // verifiable (our own stdout/console, not the pty).
        libdunit::write(1, b"[term] ");
        for s in &done {
            libdunit::write(1, s.text.as_bytes());
        }
        libdunit::write(1, b"\n");
        self.lines.push(done);
        if self.lines.len() > SCROLL_CAP {
            let excess = self.lines.len() - SCROLL_CAP;
            self.lines.drain(0..excess);
        }
        self.scroll = 0; // new output snaps the view back to the live bottom
    }

    /// Erase the last char of the active line (crossing span boundaries).
    fn backspace(&mut self) {
        while let Some(s) = self.cur.last_mut() {
            if s.text.pop().is_some() {
                if s.text.is_empty() {
                    self.cur.pop();
                }
                return;
            }
            self.cur.pop();
        }
    }

    /// Apply the collected SGR (`ESC [ … m`) params to the live foreground.
    fn apply_sgr(&mut self) {
        let n = if self.nparams == 0 { 1 } else { self.nparams };
        for i in 0..n {
            let p = if self.nparams == 0 { 0 } else { self.params[i] };
            match p {
                0 | 39 => self.fg = FG_DEFAULT,
                30..=37 => self.fg = ANSI[(p - 30) as usize],
                90..=97 => self.fg = ANSI_BRIGHT[(p - 90) as usize],
                _ => {} // bold/reverse/background — not modeled
            }
        }
    }

    fn push_param(&mut self) {
        if self.nparams < self.params.len() {
            self.params[self.nparams] = self.param;
            self.nparams += 1;
        }
        self.param = 0;
        self.has_param = false;
    }
    /// Feed one chunk of raw shell stdout through the ESC state machine.
    fn feed(&mut self, bytes: &[u8]) {
        for &b in bytes {
            match self.esc {
                Esc::Esc => {
                    self.esc = if b == b'[' { Esc::Csi } else { Esc::Normal };
                    continue;
                }
                Esc::Csi => {
                    match b {
                        b'0'..=b'9' => {
                            self.param =
                                self.param.saturating_mul(10).saturating_add((b - b'0') as u32);
                            self.has_param = true;
                        }
                        b';' => self.push_param(),
                        0x40..=0x7e => {
                            if self.has_param || self.nparams > 0 {
                                self.push_param();
                            }
                            if b == b'm' {
                                self.apply_sgr();
                            } else if b == b'J' {
                                // ESC[2J (and bare) — clear the scrollback.
                                self.lines.clear();
                                self.cur.clear();
                            }
                            // Other finals (H/K/cursor moves, `?` private modes)
                            // are consumed but not modeled by the scroll view.
                            self.nparams = 0;
                            self.param = 0;
                            self.has_param = false;
                            self.esc = Esc::Normal;
                        }
                        _ => {} // '?' prefix and intermediates: keep scanning
                    }
                    continue;
                }
                Esc::Normal => {}
            }
            match b {
                0x1b => self.esc = Esc::Esc,
                b'\n' => self.commit_line(),
                b'\r' => {}
                0x0c => {
                    self.lines.clear();
                    self.cur.clear();
                }
                0x08 => self.backspace(),
                0x20..=0x7e => self.push_char(b as char),
                _ => {}
            }
        }
        if !bytes.is_empty() {
            self.dirty = true;
        }
    }

}

/// Draw one line of text with its top-left at (x, y_top); returns the pen
/// advance in px. Mirrors the runtime painter's baseline math so it looks
/// identical to DUI-rendered text.
fn draw_text(surf: &mut Surface, font: &Font, x: i32, y_top: i32, color: Color, text: &str) -> f32 {
    let baseline = y_top as f32 + font.line_metrics(FONT_PX).ascent;
    let (glyphs, adv) = font.layout_line(text, FONT_PX);
    for g in glyphs {
        if let Some(bmp) = font.rasterize(g.glyph, FONT_PX) {
            let ox = (x as f32 + g.x + 0.5) as i32 + bmp.left;
            let oy = (baseline + 0.5) as i32 - bmp.top;
            surf.blit_glyph(&bmp, ox, oy, color);
        }
    }
    adv
}

/// Paint the scrollback into the mapped ARGB8888 buffer: fill the background,
/// then draw the last VISIBLE_ROWS rows (completed lines plus the in-progress
/// line) as sequences of colored spans, advancing the pen per span.
fn render(px: *mut u8, font: &Font, term: &Term) {
    let pixels = unsafe { core::slice::from_raw_parts_mut(px as *mut u32, (W * H) as usize) };
    let mut surf = Surface::new(pixels, W as usize, H as usize);
    surf.fill_rect(0.0, 0.0, W as f32, H as f32, col(BG));

    // The document is `lines` (completed rows) followed by the in-progress
    // `cur` row at index `lines.len()`. `scroll` counts how many rows the view
    // is lifted above the live bottom (0 = pinned to `cur`).
    let total = term.lines.len() + 1; // +1 for the in-progress line
    let bottom = (total - 1).saturating_sub(term.scroll); // last visible row index
    let start = (bottom + 1).saturating_sub(VISIBLE_ROWS);
    let mut y = 8; // top padding
    let mut draw_row = |spans: &[Span]| {
        let mut x = 8; // left padding
        for s in spans {
            if !s.text.is_empty() {
                x += draw_text(&mut surf, font, x, y, col(s.fg), &s.text) as i32;
            }
        }
        y += ROW_PX;
    };
    for idx in start..=bottom {
        if idx < term.lines.len() {
            draw_row(&term.lines[idx]);
        } else {
            draw_row(&term.cur);
        }
    }
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
    let font = match load_font() {
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
                IN_SCROLL => {
                    // Wheel delta (i32) in the lx field: positive = scroll up
                    // into the backlog, negative = back toward the live bottom.
                    let delta = i32::from_le_bytes([rx[8], rx[9], rx[10], rx[11]]);
                    if delta != 0 {
                        term.scroll_by(delta.unsigned_abs() as usize * 3, delta > 0);
                    }
                }
                IN_KEY => {
                    // Widened IN_KEY: scancode@rx[8..12], mods@rx[12..16],
                    // cooked ASCII@rx[16] (see gui_server::send_input). Decode
                    // into a pty byte sequence: control codes for Ctrl+letter,
                    // the cooked byte for printables/Enter/Tab, and xterm ESC
                    // sequences for the extended nav keys (arrows/Home/End/Del).
                    let scancode = rx[8];
                    let mods = rx[12];
                    let ascii = rx[16];
                    // PgUp/PgDn scroll the local view a page at a time and are
                    // never forwarded to the pty.
                    if scancode == SC_PGUP {
                        term.scroll_by(VISIBLE_ROWS - 1, true);
                    } else if scancode == SC_PGDN {
                        term.scroll_by(VISIBLE_ROWS - 1, false);
                    } else {
                        let mut seq = [0u8; 4];
                        let len = encode_key(scancode, mods, ascii, &mut seq);
                        if len > 0 {
                            libdunit::pty_write(pty, &seq[..len]);
                        }
                    }
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


