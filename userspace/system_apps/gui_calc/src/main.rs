#![no_std]
#![no_main]

//! Calculator client for the userspace DWM (Stack B).
//!
//! A modern, resizable calculator: config-driven palette (`apps/gui_calc.toml`
//! via `dwm_settings::CalcCfg`), TrueType text (`dunit_text`/`dunit_render`),
//! pointer + full keyboard input, and an f64 engine with guarded arithmetic and
//! hand-written float→string formatting (no libm). Same gui-v1 protocol path as
//! the other clients, including the server-pushed CONFIGURE resize (maximize /
//! restore): the button grid and display re-derive from the live surface size.

use core::panic::PanicInfo;

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use gui_protocol_v1::wire::{Request, FEATURE_ARGB8888, MAGIC};

use dunit_render::Surface;
use dunit_style::value::Color;
use dunit_text::Font;

// Control-message magics shared with the compositor.
const CTRL_MAGIC: u32 = 0x3150_4143; // "CAP1" — capability announce
const INPUT_MAGIC: u32 = 0x3150_4E49; // "INP1" — pointer input
const IN_DOWN: u8 = 2;
const IN_KEY: u8 = 5;
const IN_QUIT: u8 = 9;

// Cooked key bytes the compositor forwards in the IN_KEY `button` field.
const KEY_BACKSPACE: u8 = 0x08;
const KEY_DEL: u8 = 0x7f;
const KEY_ENTER_LF: u8 = 0x0a;
const KEY_ENTER_CR: u8 = 0x0d;
const KEY_ESC: u8 = 0x1b;

const SURFACE: u64 = 1;
const BUFFER: u64 = 2;
const W: u32 = 260;
const H: u32 = 360;
const FMT_XRGB8888: u32 = 1;
const FMT_ARGB8888: u32 = 2;

const COLS: i32 = 4;
const ROWS: i32 = 5;

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    libdunit::println("gui_calc: PANIC");
    libdunit::exit(101)
}

/// Load the configured TTF (`[desktop] font`) from the VFS, falling back to the
/// shipped /assets default font on any error — the desktop font is a live config knob.
fn load_font() -> Result<Font, ()> {
    let cfg = dwm_settings::load();
    if let Some(bytes) = libdunit::read_binary(cfg.desktop.font.as_str(), 4 * 1024 * 1024) {
        if let Ok(f) = Font::parse(bytes) {
            return Ok(f);
        }
    }
    libdunit::read_binary("/assets/fonts/DejaVuSans.ttf", 4 * 1024 * 1024)
        .ok_or(())
        .and_then(|b| Font::parse(b).map_err(|_| ()))
}

/// Unpack a `0xAARRGGBB` config word into a straight-alpha `Color`.
fn col(argb: u32) -> Color {
    Color::rgba(
        ((argb >> 16) & 0xFF) as u8,
        ((argb >> 8) & 0xFF) as u8,
        (argb & 0xFF) as u8,
        ((argb >> 24) & 0xFF) as u8,
    )
}

fn u64_at(p: &[u8], off: usize) -> u64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&p[off..off + 8]);
    u64::from_le_bytes(a)
}

fn round_i32(v: f32) -> i32 {
    if v <= 0.0 { 0 } else { (v + 0.5) as i32 }
}

/// Total advance width of `text` at `px`.
fn text_width(font: &Font, text: &str, px: f32) -> f32 {
    font.layout_line(text, px).1
}

/// Draw one line of `text` with its left edge at `x` and baseline at `baseline`.
fn draw_text(s: &mut Surface, font: &Font, x: i32, baseline: i32, px: f32, text: &str, color: Color) {
    let (glyphs, _adv) = font.layout_line(text, px);
    for g in glyphs {
        if let Some(bmp) = font.rasterize(g.glyph, px) {
            let ox = x + round_i32(g.x) + bmp.left;
            let oy = baseline - bmp.top;
            s.blit_glyph(&bmp, ox, oy, color);
        }
    }
}

/// Shrink `target_px` just enough that `text` fits within `max_w` (min 8px).
fn fit_px(font: &Font, text: &str, target_px: f32, max_w: f32) -> f32 {
    let w = text_width(font, text, target_px);
    if w <= max_w || w <= 0.0 {
        target_px
    } else {
        let scaled = target_px * max_w / w;
        if scaled < 8.0 { 8.0 } else { scaled }
    }
}

/// Resolved calculator palette, built once from `CalcCfg` (config→behavior).
/// The window background carries the configured `bg_alpha` in `clear_bg`; every
/// button/text color here is forced opaque.
#[derive(Clone, Copy)]
struct Palette {
    display_bg: Color,
    text: Color,
    muted: Color,
    btn: Color,
    btn_fn: Color,
    btn_op: Color,
    btn_eq: Color,
}

/// A calculator key. Operators carry their ASCII byte (`+ - * /`) internally so
/// the engine and the keyboard map stay ASCII; the face renders Unicode glyphs.
#[derive(Clone, Copy, PartialEq)]
enum Key {
    Digit(u8), // 0..=9
    Dot,
    Op(u8),
    Equals,
    Clear,
    Back,
    Sign,
    Percent,
}

/// The 5×4 face. Modern layout: no duplicate keys, a real decimal point, a sign
/// toggle and a percent key. Column 3 is the operator column; the bottom row is
/// `± 0 . =`.
const KEYS: [[Key; 4]; 5] = [
    [Key::Clear,    Key::Back,     Key::Percent,  Key::Op(b'/')],
    [Key::Digit(7), Key::Digit(8), Key::Digit(9), Key::Op(b'*')],
    [Key::Digit(4), Key::Digit(5), Key::Digit(6), Key::Op(b'-')],
    [Key::Digit(1), Key::Digit(2), Key::Digit(3), Key::Op(b'+')],
    [Key::Sign,     Key::Digit(0), Key::Dot,      Key::Equals],
];

fn key_label(k: Key) -> &'static str {
    match k {
        Key::Digit(0) => "0",
        Key::Digit(1) => "1",
        Key::Digit(2) => "2",
        Key::Digit(3) => "3",
        Key::Digit(4) => "4",
        Key::Digit(5) => "5",
        Key::Digit(6) => "6",
        Key::Digit(7) => "7",
        Key::Digit(8) => "8",
        Key::Digit(9) => "9",
        Key::Digit(_) => "?",
        Key::Dot => ".",
        Key::Op(b'+') => "+",
        Key::Op(b'-') => "\u{2212}", // − minus sign
        Key::Op(b'*') => "\u{00D7}", // × multiplication sign
        Key::Op(b'/') => "\u{00F7}", // ÷ division sign
        Key::Op(_) => "?",
        Key::Equals => "=",
        Key::Clear => "C",
        Key::Back => "\u{232B}", // ⌫ erase to the left
        Key::Sign => "\u{00B1}", // ± plus-minus
        Key::Percent => "%",
    }
}

fn key_color(k: Key, p: &Palette) -> Color {
    match k {
        Key::Digit(_) | Key::Dot => p.btn,
        Key::Op(_) => p.btn_op,
        Key::Equals => p.btn_eq,
        _ => p.btn_fn, // Clear, Back, Sign, Percent
    }
}

/// The display symbol for an operator on the pending-expression line.
fn op_sym(o: u8) -> &'static str {
    match o {
        b'+' => "+",
        b'-' => "\u{2212}",
        b'*' => "\u{00D7}",
        b'/' => "\u{00F7}",
        _ => "?",
    }
}

/// Map a cooked ASCII key (from IN_KEY) to a calculator key.
fn key_from_ascii(a: u8) -> Option<Key> {
    match a {
        b'0'..=b'9' => Some(Key::Digit(a - b'0')),
        b'.' | b',' => Some(Key::Dot),
        b'+' => Some(Key::Op(b'+')),
        b'-' => Some(Key::Op(b'-')),
        b'*' | b'x' | b'X' => Some(Key::Op(b'*')),
        b'/' => Some(Key::Op(b'/')),
        b'%' => Some(Key::Percent),
        b'=' | KEY_ENTER_LF | KEY_ENTER_CR => Some(Key::Equals),
        KEY_BACKSPACE | KEY_DEL => Some(Key::Back),
        KEY_ESC | b'c' | b'C' => Some(Key::Clear),
        _ => None,
    }
}

/// Geometry derived from the live surface size (the compositor can resize us).
#[derive(Clone, Copy)]
struct Layout {
    w: i32,
    pad: i32,
    disp_h: i32,
    cw: i32,
    ch: i32,
    grid_y0: i32,
}

impl Layout {
    fn new(w: i32, h: i32) -> Layout {
        let pad = (w / 40).max(6);
        let disp_h = (h * 27 / 100).max(48);
        let grid_y0 = disp_h;
        let cw = ((w - pad * (COLS + 1)) / COLS).max(1);
        let avail = h - grid_y0 - pad * (ROWS + 1);
        let ch = (avail / ROWS).max(1);
        Layout { w, pad, disp_h, cw, ch, grid_y0 }
    }

    /// Screen-local rect of the (col,row) grid cell.
    fn cell(&self, col: i32, row: i32) -> (i32, i32, i32, i32) {
        let x = self.pad + col * (self.cw + self.pad);
        let y = self.grid_y0 + self.pad + row * (self.ch + self.pad);
        (x, y, self.cw, self.ch)
    }

    /// Map a client-local pointer position to a grid key, if any.
    fn key_at(&self, px: i32, py: i32) -> Option<Key> {
        if py < self.grid_y0 {
            return None;
        }
        for r in 0..ROWS {
            for c in 0..COLS {
                let (x, y, w, h) = self.cell(c, r);
                if px >= x && px < x + w && py >= y && py < y + h {
                    return Some(KEYS[r as usize][c as usize]);
                }
            }
        }
        None
    }
}

const ENTRY_CAP: usize = 16;

/// True for NaN or ±infinity, using pure arithmetic (no libm / `is_finite`):
/// NaN is the only value not equal to itself; ±inf falls outside the f64 range.
fn bad(v: f64) -> bool {
    v != v || v > 1.0e308 || v < -1.0e308
}

/// Parse the entry buffer (only `[-]digits[.digits]`, which we author) into an
/// f64 by exact integer accumulation — no `core` dec2flt dependency.
fn parse_entry(s: &str) -> f64 {
    let b = s.as_bytes();
    let mut i = 0usize;
    let neg = !b.is_empty() && b[0] == b'-';
    if neg {
        i = 1;
    }
    let mut int_part = 0.0f64;
    while i < b.len() && b[i].is_ascii_digit() {
        int_part = int_part * 10.0 + (b[i] - b'0') as f64;
        i += 1;
    }
    let mut frac = 0.0f64;
    let mut scale = 1.0f64;
    if i < b.len() && b[i] == b'.' {
        i += 1;
        while i < b.len() && b[i].is_ascii_digit() {
            frac = frac * 10.0 + (b[i] - b'0') as f64;
            scale *= 10.0;
            i += 1;
        }
    }
    let v = int_part + frac / scale;
    if neg { -v } else { v }
}

fn pow10(n: u32) -> u128 {
    let mut r = 1u128;
    let mut i = 0;
    while i < n {
        r *= 10;
        i += 1;
    }
    r
}

fn append_u128(s: &mut String, mut n: u128) {
    if n == 0 {
        s.push('0');
        return;
    }
    let mut tmp = [0u8; 40];
    let mut t = 0usize;
    while n > 0 {
        tmp[t] = b'0' + (n % 10) as u8;
        n /= 10;
        t += 1;
    }
    while t > 0 {
        t -= 1;
        s.push(tmp[t] as char);
    }
}

/// Fractional digits kept before trailing zeros are trimmed.
const FMT_DECIMALS: u32 = 8;

/// Format an f64 for the display: integers show without a decimal point,
/// fractions are rounded to `FMT_DECIMALS` places with trailing zeros trimmed,
/// non-finite → "Error". Integer casts + manual rounding, no libm.
fn fmt_f64(v: f64) -> String {
    let mut s = String::new();
    if bad(v) {
        return String::from("Error");
    }
    let neg = v < 0.0;
    let x = if neg { -v } else { v };
    // Beyond ~1e18 there is no fractional precision left; render the integer.
    if x >= 1.0e18 {
        if x >= 3.0e38 {
            return String::from("Error"); // beyond u128
        }
        if neg {
            s.push('-');
        }
        append_u128(&mut s, x as u128);
        return s;
    }
    let scale = pow10(FMT_DECIMALS);
    let rounded = (x * scale as f64 + 0.5) as u128; // round half up
    let ip = rounded / scale;
    let fp = rounded % scale;
    if neg && rounded != 0 {
        s.push('-');
    }
    append_u128(&mut s, ip);
    if fp != 0 {
        let mut frac = [0u8; FMT_DECIMALS as usize];
        let mut r = fp;
        let mut i = FMT_DECIMALS as usize;
        while i > 0 {
            i -= 1;
            frac[i] = b'0' + (r % 10) as u8;
            r /= 10;
        }
        let mut end = FMT_DECIMALS as usize;
        while end > 0 && frac[end - 1] == b'0' {
            end -= 1;
        }
        if end > 0 {
            s.push('.');
            for &b in &frac[..end] {
                s.push(b as char);
            }
        }
    }
    s
}

/// The calculator engine: a running accumulator, a pending operator, and the
/// number currently being typed (`entry`). `entering` selects what the display
/// shows (the live `entry` vs. the computed `acc`).
struct Calc {
    entry: String,
    entering: bool,
    acc: f64,
    op: Option<u8>,
    error: bool,
    expr: String,
}

impl Calc {
    fn new() -> Calc {
        Calc {
            entry: String::from("0"),
            entering: true,
            acc: 0.0,
            op: None,
            error: false,
            expr: String::new(),
        }
    }

    /// The value the next operation should consume.
    fn current(&self) -> f64 {
        if self.entering {
            parse_entry(&self.entry)
        } else {
            self.acc
        }
    }

    /// The string to paint in the display.
    fn display(&self) -> String {
        if self.error {
            String::from("Error")
        } else if self.entering {
            self.entry.clone()
        } else {
            fmt_f64(self.acc)
        }
    }

    fn press(&mut self, k: Key) {
        // In the error state only Clear is live.
        if self.error && !matches!(k, Key::Clear) {
            return;
        }
        match k {
            Key::Digit(d) => self.digit(d),
            Key::Dot => self.dot(),
            Key::Op(o) => self.set_op(o),
            Key::Equals => self.equals(),
            Key::Clear => *self = Calc::new(),
            Key::Back => self.back(),
            Key::Sign => self.sign(),
            Key::Percent => self.percent(),
        }
    }

    fn start_entry(&mut self) {
        self.entry.clear();
        self.entering = true;
    }

    fn digit(&mut self, d: u8) {
        if !self.entering {
            self.start_entry();
        }
        if self.entry == "0" {
            self.entry.clear();
        } else if self.entry == "-0" {
            self.entry.truncate(1); // keep the sign
        }
        if self.entry.len() < ENTRY_CAP {
            self.entry.push((b'0' + d) as char);
        }
        if self.entry.is_empty() || self.entry == "-" {
            self.entry.push('0');
        }
    }

    fn dot(&mut self) {
        if !self.entering {
            self.start_entry();
        }
        if self.entry.is_empty() || self.entry == "-" {
            self.entry.push('0');
        }
        if !self.entry.contains('.') && self.entry.len() < ENTRY_CAP {
            self.entry.push('.');
        }
    }

    fn back(&mut self) {
        if !self.entering {
            return;
        }
        self.entry.pop();
        if self.entry.is_empty() || self.entry == "-" {
            self.entry = String::from("0");
        }
    }

    fn sign(&mut self) {
        if self.entering {
            if self.entry.starts_with('-') {
                self.entry.remove(0);
            } else if self.entry != "0" {
                self.entry.insert(0, '-');
            }
        } else {
            self.acc = -self.acc;
        }
    }

    fn percent(&mut self) {
        self.acc = self.current() / 100.0;
        self.entering = false;
    }

    fn set_op(&mut self, o: u8) {
        let cur = self.current();
        if self.op.is_some() && self.entering {
            self.fold(cur); // chain left-to-right
        } else {
            self.acc = cur;
        }
        if self.error {
            return;
        }
        self.op = Some(o);
        self.entering = false;
        self.expr = {
            let mut s = fmt_f64(self.acc);
            s.push(' ');
            s.push_str(op_sym(o));
            s
        };
    }

    fn equals(&mut self) {
        if self.op.is_some() {
            let rhs = self.current();
            self.fold(rhs);
            self.op = None;
            self.expr.clear();
            self.entering = false;
        }
    }

    /// `acc = acc <op> rhs`, guarding divide-by-zero and non-finite → error.
    fn fold(&mut self, rhs: f64) {
        let Some(o) = self.op else {
            return;
        };
        let r = match o {
            b'+' => self.acc + rhs,
            b'-' => self.acc - rhs,
            b'*' => self.acc * rhs,
            b'/' => {
                if rhs == 0.0 {
                    self.error = true;
                    return;
                }
                self.acc / rhs
            }
            _ => return,
        };
        if bad(r) {
            self.error = true;
            return;
        }
        self.acc = r;
        self.entering = false;
    }
}

/// Paint the whole calculator into the mapped buffer at the given geometry.
fn render(px: *mut u8, w: u32, h: u32, font: &Font, calc: &Calc, pal: &Palette, clear_bg: Color) {
    let pixels = unsafe { core::slice::from_raw_parts_mut(px as *mut u32, (w * h) as usize) };
    let mut s = Surface::new(pixels, w as usize, h as usize);
    // Base fill carries the configured alpha (translucent when bg_alpha < 255).
    s.clear(clear_bg);

    let ly = Layout::new(w as i32, h as i32);
    let dx = ly.pad;
    let dw = ly.w - 2 * ly.pad;
    let dh = ly.disp_h - ly.pad;
    s.fill_rect(dx as f32, ly.pad as f32, dw as f32, dh as f32, pal.display_bg);
    let inner = ly.pad + 6;
    let left_limit = dx + inner;

    // Pending-expression line (muted, small, top-right).
    if !calc.expr.is_empty() {
        let epx = (ly.disp_h as f32 * 0.20).max(10.0);
        let ew = text_width(font, &calc.expr, epx) as i32;
        let ex = (dx + dw - inner - ew).max(left_limit);
        let ebase = ly.pad + (ly.disp_h as f32 * 0.30) as i32;
        draw_text(&mut s, font, ex, ebase, epx, &calc.expr, pal.muted);
    }

    // Main value (primary text, large, bottom-right, auto-shrunk to fit).
    let disp = calc.display();
    let target = (ly.disp_h as f32 * 0.42).max(14.0);
    let vpx = fit_px(font, &disp, target, (dw - 2 * inner) as f32);
    let vw = text_width(font, &disp, vpx) as i32;
    let vx = (dx + dw - inner - vw).max(left_limit);
    let vbase = ly.pad + dh - (ly.disp_h as f32 * 0.14) as i32;
    draw_text(&mut s, font, vx, vbase, vpx, &disp, pal.text);

    // Button grid.
    for r in 0..ROWS {
        for c in 0..COLS {
            let k = KEYS[r as usize][c as usize];
            let (x, y, bw, bh) = ly.cell(c, r);
            s.fill_rect(x as f32, y as f32, bw as f32, bh as f32, key_color(k, pal));
            let label = key_label(k);
            let lpx = (bh as f32 * 0.44).max(12.0);
            let lw = text_width(font, label, lpx) as i32;
            let lx = x + (bw - lw) / 2;
            let lbase = y + bh / 2 + (lpx * 0.34) as i32;
            draw_text(&mut s, font, lx, lbase, lpx, label, pal.text);
        }
    }
}

/// Re-negotiate a fresh buffer at a new geometry in response to a server-pushed
/// CONFIGURE (maximize/restore), mirroring the file-manager's resize path.
#[allow(clippy::too_many_arguments)]
fn resize_surface(
    compositor: u32,
    old_buf: u32,
    font: &Font,
    calc: &Calc,
    pal: &Palette,
    clear_bg: Color,
    fmt: u32,
    new_w: u32,
    new_h: u32,
    obj: u64,
    token: u64,
    serial: &mut u64,
) -> Option<(u32, *mut u8)> {
    let bytes = (new_w * new_h * 4) as usize;
    let nb = libdunit::handle_create_shared(bytes);
    if nb <= 0 {
        return None;
    }
    let nb = nb as u32;
    let mapped = libdunit::handle_map(nb, 0, bytes);
    if mapped <= 0 {
        libdunit::handle_close(nb);
        return None;
    }
    let npx = mapped as usize as *mut u8;
    render(npx, new_w, new_h, font, calc, pal, clear_bg);

    let ack = Request::AckConfigure { configure: token }.encode(SURFACE, *serial);
    *serial += 1;
    libdunit::ipc_send(compositor, &ack);

    let dup = libdunit::handle_dup(
        nb,
        libdunit::RIGHT_READ | libdunit::RIGHT_MAP | libdunit::RIGHT_TRANSFER,
    );
    let ch = if dup > 0 {
        libdunit::handle_transfer(dup as u32, compositor)
    } else {
        -1
    };
    if ch <= 0 {
        libdunit::handle_close(nb);
        return None;
    }
    let mut ann = [0u8; 16];
    ann[0..4].copy_from_slice(&CTRL_MAGIC.to_le_bytes());
    ann[4..8].copy_from_slice(&(ch as u32).to_le_bytes());
    ann[8..12].copy_from_slice(&(bytes as u32).to_le_bytes());
    ann[12..16].copy_from_slice(&(obj as u32).to_le_bytes());
    libdunit::ipc_send(compositor, &ann);

    let import = Request::ImportBuffer {
        width: new_w,
        height: new_h,
        stride: new_w * 4,
        format: fmt,
        offset: 0,
    }
    .encode(obj, *serial);
    *serial += 1;
    libdunit::ipc_send(compositor, &import);
    let attach = Request::AttachBuffer { buffer: obj, damage: Vec::new() }.encode(SURFACE, *serial);
    *serial += 1;
    libdunit::ipc_send(compositor, &attach);
    let commit = Request::Commit { configure: token, frame_callback: 1 }.encode(SURFACE, *serial);
    *serial += 1;
    libdunit::ipc_send(compositor, &commit);

    libdunit::handle_close(old_buf);
    Some((nb, npx))
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    // 0) Load this app's config (apps/gui_calc.toml). A missing/garbage file
    //    yields the built-in baseline. The resolved values below actually drive
    //    the palette and background transparency (config→behavior).
    let ccfg = dwm_settings::CalcCfg::load("gui_calc");
    let fmt = if ccfg.bg_alpha < 255 { FMT_ARGB8888 } else { FMT_XRGB8888 };
    libdunit::println(&alloc::format!(
        "gui_calc: cfg from_file={} bg={:#010X} btn_op={:#010X} bg_alpha={} fmt={}",
        ccfg.from_file as u32,
        ccfg.bg,
        ccfg.btn_op,
        ccfg.bg_alpha,
        if fmt == FMT_ARGB8888 { "argb8888" } else { "xrgb8888" },
    ));

    let pal = Palette {
        display_bg: col(ccfg.display_bg | 0xFF00_0000),
        text: col(ccfg.text | 0xFF00_0000),
        muted: col(ccfg.muted | 0xFF00_0000),
        btn: col(ccfg.btn | 0xFF00_0000),
        btn_fn: col(ccfg.btn_fn | 0xFF00_0000),
        btn_op: col(ccfg.btn_op | 0xFF00_0000),
        btn_eq: col(ccfg.btn_eq | 0xFF00_0000),
    };
    let clear_bg = col((ccfg.bg_alpha << 24) | (ccfg.bg & 0x00FF_FFFF));

    // A per-app `[calc] font` override wins over the desktop font; empty inherits.
    let font = if !ccfg.font.as_str().is_empty() {
        match libdunit::read_binary(ccfg.font.as_str(), 4 * 1024 * 1024)
            .and_then(|b| Font::parse(b).ok())
        {
            Some(f) => f,
            None => match load_font() {
                Ok(f) => f,
                Err(_) => {
                    libdunit::println("gui_calc: FAIL font parse");
                    libdunit::exit(7);
                }
            },
        }
    } else {
        match load_font() {
            Ok(f) => f,
            Err(_) => {
                libdunit::println("gui_calc: FAIL font parse");
                libdunit::exit(7);
            }
        }
    };

    // 1) Handshake: compositor pid + our client id (id is a tint hint only).
    let mut m = [0u8; 8];
    if libdunit::ipc_recv_blocking(&mut m, 0) < 8 {
        libdunit::println("gui_calc: FAIL recv handshake");
        libdunit::exit(1);
    }
    let compositor = u32::from_le_bytes([m[0], m[1], m[2], m[3]]);

    // 2) Create + map our own shared buffer and paint the initial frame.
    let bytes = (W * H * 4) as usize;
    let buf = libdunit::handle_create_shared(bytes);
    if buf <= 0 {
        libdunit::println("gui_calc: FAIL create buffer");
        libdunit::exit(2);
    }
    let buf = buf as u32;
    let mapped = libdunit::handle_map(buf, 0, bytes);
    if mapped <= 0 {
        libdunit::println("gui_calc: FAIL map buffer");
        libdunit::exit(3);
    }
    let mut px = mapped as usize as *mut u8;
    let mut calc = Calc::new();
    let mut cur_buf = buf;
    let mut cur_w = W;
    let mut cur_h = H;
    render(px, cur_w, cur_h, &font, &calc, &pal, clear_bg);

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
        libdunit::println("gui_calc: FAIL welcome");
        libdunit::exit(4);
    }

    // 4) CREATE_SURFACE -> RESULT + CONFIGURE (capture the configure token).
    let create =
        Request::CreateSurface { role: 1, width: W, height: H, format: fmt }.encode(SURFACE, 2);
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
    let cha = if dup > 0 {
        libdunit::handle_transfer(dup as u32, compositor)
    } else {
        -1
    };
    if cha <= 0 {
        libdunit::println("gui_calc: FAIL cap transfer");
        libdunit::exit(5);
    }
    let mut ann = [0u8; 16];
    ann[0..4].copy_from_slice(&CTRL_MAGIC.to_le_bytes());
    ann[4..8].copy_from_slice(&(cha as u32).to_le_bytes());
    ann[8..12].copy_from_slice(&(bytes as u32).to_le_bytes());
    ann[12..16].copy_from_slice(&(BUFFER as u32).to_le_bytes());
    libdunit::ipc_send(compositor, &ann);

    // 7) IMPORT / ATTACH / COMMIT (each -> RESULT).
    let import =
        Request::ImportBuffer { width: W, height: H, stride: W * 4, format: fmt, offset: 0 }
            .encode(BUFFER, 4);
    libdunit::ipc_send(compositor, &import);
    libdunit::ipc_recv_blocking(&mut rx, 0);
    let attach = Request::AttachBuffer { buffer: BUFFER, damage: Vec::new() }.encode(SURFACE, 5);
    libdunit::ipc_send(compositor, &attach);
    libdunit::ipc_recv_blocking(&mut rx, 0);
    let commit = Request::Commit { configure: token, frame_callback: 1 }.encode(SURFACE, 6);
    libdunit::ipc_send(compositor, &commit);
    libdunit::ipc_recv_blocking(&mut rx, 0);

    // 8) FRAME_DONE from the compositor's composition tick.
    let n = libdunit::ipc_recv_blocking(&mut rx, 0);
    let ok = n >= 32 && u16::from_le_bytes([rx[8], rx[9]]) == 0x8021;
    if ok {
        libdunit::println("gui_calc: surface presented OK");
    } else {
        libdunit::println("gui_calc: FAIL no frame done");
        libdunit::handle_close(buf);
        libdunit::exit(6);
    }

    // 9) Interactive loop. Pointer button-down maps to a grid key; IN_KEY maps a
    //    cooked ASCII byte to the same keys (full keyboard control); a server
    //    CONFIGURE re-negotiates the buffer at the new geometry. Any state change
    //    repaints the shared buffer (the compositor blits it every tick).
    let mut next_obj: u64 = 3; // fresh buffer object id per resize (> import id 2)
    let mut serial: u64 = 7; // client request serial, continues past the handshake
    loop {
        let n = libdunit::ipc_recv_blocking(&mut rx, 0);
        if n < 8 {
            continue;
        }
        let magic = u32::from_le_bytes([rx[0], rx[1], rx[2], rx[3]]);
        if magic == MAGIC && n >= 48 && u16::from_le_bytes([rx[8], rx[9]]) == 0x8010 {
            let tok = u64_at(&rx, 24);
            let nw = u32::from_le_bytes([rx[32], rx[33], rx[34], rx[35]]);
            let nh = u32::from_le_bytes([rx[36], rx[37], rx[38], rx[39]]);
            if nw == 0 || nh == 0 || (nw == cur_w && nh == cur_h) {
                let ack = Request::AckConfigure { configure: tok }.encode(SURFACE, serial);
                serial += 1;
                libdunit::ipc_send(compositor, &ack);
            } else if let Some((nb, npx)) = resize_surface(
                compositor, cur_buf, &font, &calc, &pal, clear_bg, fmt, nw, nh, next_obj, tok,
                &mut serial,
            ) {
                cur_buf = nb;
                px = npx;
                cur_w = nw;
                cur_h = nh;
                next_obj += 1;
                libdunit::println("gui_calc: reconfigured OK");
            }
            continue;
        }
        if magic != INPUT_MAGIC {
            continue;
        }
        let kind = rx[4];
        if kind == IN_QUIT {
            break;
        }
        let mut dirty = false;
        if kind == IN_DOWN {
            let lx = i32::from_le_bytes([rx[8], rx[9], rx[10], rx[11]]);
            let ly = i32::from_le_bytes([rx[12], rx[13], rx[14], rx[15]]);
            if let Some(k) = Layout::new(cur_w as i32, cur_h as i32).key_at(lx, ly) {
                calc.press(k);
                dirty = true;
            }
        } else if kind == IN_KEY {
            if let Some(k) = key_from_ascii(rx[16]) {
                calc.press(k);
                dirty = true;
            }
        }
        if dirty {
            render(px, cur_w, cur_h, &font, &calc, &pal, clear_bg);
        }
    }

    libdunit::handle_close(cur_buf);
    libdunit::exit(0)
}
