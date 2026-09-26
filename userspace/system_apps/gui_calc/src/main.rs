#![no_std]
#![no_main]

//! Untrusted calculator client for the M4 userspace DWM (Stack B).
//!
//! Same capability + gui-v1 protocol path as `gui_client`, but it paints a real
//! working calculator and turns compositor pointer events into button presses.
//! Self-contained: only libdunit + gui_protocol_v1, a small built-in bitmap font
//! (no runtime crates), so it stays a minimal reference for porting apps.

use core::panic::PanicInfo;

extern crate alloc;

use gui_protocol_v1::wire::{Request, FEATURE_ARGB8888};

// Control-message magics shared with the compositor.
const CTRL_MAGIC: u32 = 0x3150_4143; // "CAP1" — capability announce
const INPUT_MAGIC: u32 = 0x3150_4E49; // "INP1" — pointer input
const IN_DOWN: u8 = 2;
const IN_QUIT: u8 = 9;

const SURFACE: u64 = 1;
const BUFFER: u64 = 2;
const W: u32 = 200;
const H: u32 = 280;
const FMT_XRGB8888: u32 = 1;

// XRGB8888 little-endian: 0xAARRGGBB per u32 (the compositor's format).
const COLOR_BG: u32 = 0xFF15191F;
const COLOR_DISPLAY: u32 = 0xFF0B0F14;
const COLOR_TEXT: u32 = 0xFFEFEFEF;
const COLOR_BTN: u32 = 0xFF2B3542;
const COLOR_BTN_OP: u32 = 0xFF256D85;
const COLOR_BTN_EQ: u32 = 0xFF2D7D46;
const COLOR_BTN_CLR: u32 = 0xFF8F3842;

// 4-column x 5-row grid below a display strip.
const PAD: i32 = 6;
const COLS: i32 = 4;
const ROWS: i32 = 5;
const GRID_Y0: i32 = 60;
const INPUT_CAP: usize = 18;

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    libdunit::println("gui_calc: PANIC");
    libdunit::exit(101)
}

// APPEND_MARKER

#[derive(Clone, Copy)]
enum Button {
    Digit(u8),
    Op(u8),
    Equals,
    Clear,
    Backspace,
}

// Layout matches the legacy gui_calculator so the muscle memory carries over.
const BUTTONS: [[Button; 4]; 5] = [
    [Button::Clear, Button::Backspace, Button::Op(b'/'), Button::Op(b'*')],
    [Button::Digit(b'7'), Button::Digit(b'8'), Button::Digit(b'9'), Button::Op(b'-')],
    [Button::Digit(b'4'), Button::Digit(b'5'), Button::Digit(b'6'), Button::Op(b'+')],
    [Button::Digit(b'1'), Button::Digit(b'2'), Button::Digit(b'3'), Button::Equals],
    [Button::Digit(b'0'), Button::Clear, Button::Backspace, Button::Equals],
];

fn button_label(b: Button) -> u8 {
    match b {
        Button::Digit(d) => d,
        Button::Op(o) => o,
        Button::Equals => b'=',
        Button::Clear => b'C',
        Button::Backspace => b'<',
    }
}

fn button_color(b: Button) -> u32 {
    match b {
        Button::Digit(_) => COLOR_BTN,
        Button::Op(_) => COLOR_BTN_OP,
        Button::Equals => COLOR_BTN_EQ,
        Button::Clear | Button::Backspace => COLOR_BTN_CLR,
    }
}

/// Screen-local rect of the (col,row) grid cell.
fn cell_rect(col: i32, row: i32) -> (i32, i32, i32, i32) {
    let cw = (W as i32 - PAD * (COLS + 1)) / COLS;
    let ch = (H as i32 - GRID_Y0 - PAD * (ROWS + 1)) / ROWS;
    let x = PAD + col * (cw + PAD);
    let y = GRID_Y0 + PAD + row * (ch + PAD);
    (x, y, cw, ch)
}

/// Map a client-local pointer position to a grid button, if any.
fn button_at(px: i32, py: i32) -> Option<Button> {
    for row in 0..ROWS {
        for col in 0..COLS {
            let (x, y, w, h) = cell_rect(col, row);
            if px >= x && px < x + w && py >= y && py < y + h {
                return Some(BUTTONS[row as usize][col as usize]);
            }
        }
    }
    None
}

// APPEND_MARKER2

/// A 3x5 bitmap font: digits, operators and the two symbol labels. Each row is a
/// 3-bit mask (bit 2 = leftmost pixel). Enough for a calculator face.
fn glyph_3x5(c: u8) -> Option<[u8; 5]> {
    Some(match c {
        b'0' => [7, 5, 5, 5, 7],
        b'1' => [2, 6, 2, 2, 7],
        b'2' => [7, 1, 7, 4, 7],
        b'3' => [7, 1, 7, 1, 7],
        b'4' => [5, 5, 7, 1, 1],
        b'5' => [7, 4, 7, 1, 7],
        b'6' => [7, 4, 7, 5, 7],
        b'7' => [7, 1, 2, 2, 2],
        b'8' => [7, 5, 7, 5, 7],
        b'9' => [7, 5, 7, 1, 7],
        b'+' => [0, 2, 7, 2, 0],
        b'-' => [0, 0, 7, 0, 0],
        b'*' => [5, 2, 5, 0, 0],
        b'/' => [1, 1, 2, 4, 4],
        b'=' => [0, 7, 0, 7, 0],
        b'<' => [1, 2, 4, 2, 1],
        b'C' => [7, 4, 4, 4, 7],
        _ => return None,
    })
}

const GLYPH_W: i32 = 3;

fn fill_rect(buf: &mut [u32], x: i32, y: i32, w: i32, h: i32, color: u32) {
    let bw = W as i32;
    let bh = H as i32;
    for yy in y.max(0)..(y + h).min(bh) {
        let row = yy as usize * W as usize;
        for xx in x.max(0)..(x + w).min(bw) {
            buf[row + xx as usize] = color;
        }
    }
}

/// Draw one glyph at scale. Returns the advance in pixels.
fn draw_glyph(buf: &mut [u32], x: i32, y: i32, scale: i32, color: u32, c: u8) -> i32 {
    if let Some(rows) = glyph_3x5(c) {
        for (ry, mask) in rows.iter().enumerate() {
            for bit in 0..GLYPH_W {
                if mask & (1 << (GLYPH_W - 1 - bit)) != 0 {
                    fill_rect(buf, x + bit * scale, y + ry as i32 * scale, scale, scale, color);
                }
            }
        }
    }
    (GLYPH_W + 1) * scale
}

/// Draw a byte string left-to-right starting at x.
fn draw_text(buf: &mut [u32], x: i32, y: i32, scale: i32, color: u32, s: &[u8]) {
    let mut cx = x;
    for &c in s {
        cx += draw_glyph(buf, cx, y, scale, color, c);
    }
}

/// Total pixel width of a string at scale (for right alignment).
fn text_width(s: &[u8], scale: i32) -> i32 {
    s.len() as i32 * (GLYPH_W + 1) * scale
}

// APPEND_MARKER3

struct Calculator {
    input: [u8; INPUT_CAP],
    input_len: usize,
    lhs: Option<i64>,
    op: Option<u8>,
    just_evaluated: bool,
}

impl Calculator {
    fn new() -> Calculator {
        let mut c = Calculator {
            input: [0; INPUT_CAP],
            input_len: 1,
            lhs: None,
            op: None,
            just_evaluated: false,
        };
        c.input[0] = b'0';
        c
    }

    fn press(&mut self, button: Button) {
        match button {
            Button::Digit(d) => self.push_digit(d),
            Button::Op(o) => self.set_operator(o),
            Button::Equals => self.evaluate(),
            Button::Clear => self.clear(),
            Button::Backspace => self.backspace(),
        }
    }

    fn push_digit(&mut self, digit: u8) {
        if self.just_evaluated {
            self.input_len = 0;
            self.just_evaluated = false;
        }
        if self.input_len == 1 && self.input[0] == b'0' {
            self.input[0] = digit;
            return;
        }
        if self.input_len < self.input.len() {
            self.input[self.input_len] = digit;
            self.input_len += 1;
        }
    }

    fn set_operator(&mut self, op: u8) {
        if self.op.is_some() {
            self.evaluate();
        }
        self.lhs = Some(self.current_value());
        self.op = Some(op);
        self.input_len = 1;
        self.input[0] = b'0';
        self.just_evaluated = false;
    }

// APPEND_MARKER4

    fn evaluate(&mut self) {
        let (Some(lhs), Some(op)) = (self.lhs, self.op) else {
            return;
        };
        let rhs = self.current_value();
        let result = match op {
            b'+' => Some(lhs + rhs),
            b'-' => Some(lhs - rhs),
            b'*' => Some(lhs * rhs),
            b'/' if rhs != 0 => Some(lhs / rhs),
            _ => None,
        };
        match result {
            Some(v) => self.set_input_i64(v),
            None => self.set_input_bytes(b"0"),
        }
        self.lhs = None;
        self.op = None;
        self.just_evaluated = true;
    }

    fn clear(&mut self) {
        self.input_len = 1;
        self.input[0] = b'0';
        self.lhs = None;
        self.op = None;
        self.just_evaluated = false;
    }

    fn backspace(&mut self) {
        if self.just_evaluated {
            self.clear();
            return;
        }
        if self.input_len > 1 {
            self.input_len -= 1;
        } else {
            self.input[0] = b'0';
            self.input_len = 1;
        }
    }

    fn current_value(&self) -> i64 {
        let mut value = 0i64;
        for &byte in &self.input[..self.input_len] {
            if byte.is_ascii_digit() {
                value = value * 10 + (byte - b'0') as i64;
            }
        }
        value
    }

    fn set_input_i64(&mut self, value: i64) {
        let mut out = [0u8; INPUT_CAP];
        let mut len = 0usize;
        append_i64(&mut out, &mut len, value);
        self.set_input_bytes(&out[..len]);
    }

    fn set_input_bytes(&mut self, value: &[u8]) {
        self.input_len = value.len().min(self.input.len()).max(1);
        self.input[..self.input_len].copy_from_slice(&value[..self.input_len]);
    }
}

/// Format `value` into `out[..len]` as decimal (with a leading '-' if negative).
fn append_i64(out: &mut [u8; INPUT_CAP], len: &mut usize, value: i64) {
    if value == 0 {
        out[0] = b'0';
        *len = 1;
        return;
    }
    let neg = value < 0;
    let mut n = if neg { (value as i128).unsigned_abs() as u128 } else { value as u128 };
    let mut tmp = [0u8; 24];
    let mut t = 0usize;
    while n > 0 {
        tmp[t] = b'0' + (n % 10) as u8;
        n /= 10;
        t += 1;
    }
    let mut i = 0usize;
    if neg && i < out.len() {
        out[i] = b'-';
        i += 1;
    }
    while t > 0 && i < out.len() {
        t -= 1;
        out[i] = tmp[t];
        i += 1;
    }
    *len = i;
}

// APPEND_MARKER5

/// Paint the whole calculator into the mapped ARGB8888 buffer.
fn render(px: *mut u8, calc: &Calculator) {
    let buf = unsafe { core::slice::from_raw_parts_mut(px as *mut u32, (W * H) as usize) };
    for p in buf.iter_mut() {
        *p = COLOR_BG;
    }
    // Display strip: current input, right-aligned, large.
    fill_rect(buf, 4, 4, W as i32 - 8, 48, COLOR_DISPLAY);
    let text = &calc.input[..calc.input_len];
    let scale = 4;
    let tw = text_width(text, scale);
    let tx = (W as i32 - 12 - tw).max(8);
    let ty = 4 + (48 - 5 * scale) / 2;
    draw_text(buf, tx, ty, scale, COLOR_TEXT, text);
    // Button grid.
    for row in 0..ROWS {
        for col in 0..COLS {
            let b = BUTTONS[row as usize][col as usize];
            let (x, y, w, h) = cell_rect(col, row);
            fill_rect(buf, x, y, w, h, button_color(b));
            let label = [button_label(b)];
            let s = 3;
            let lw = text_width(&label, s);
            let lx = x + (w - lw) / 2;
            let ly = y + (h - 5 * s) / 2;
            draw_glyph(buf, lx, ly, s, COLOR_TEXT, label[0]);
        }
    }
}

fn u64_at(p: &[u8], off: usize) -> u64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&p[off..off + 8]);
    u64::from_le_bytes(a)
}

// APPEND_MARKER6

#[no_mangle]
pub extern "C" fn _start() -> ! {
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
    let px = mapped as usize as *mut u8;
    let mut calc = Calculator::new();
    render(px, &calc);

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
        Request::CreateSurface { role: 1, width: W, height: H, format: FMT_XRGB8888 }.encode(SURFACE, 2);
    libdunit::ipc_send(compositor, &create);
    libdunit::ipc_recv_blocking(&mut rx, 0); // RESULT
    let n = libdunit::ipc_recv_blocking(&mut rx, 0); // CONFIGURE
    let token = if n >= 32 { u64_at(&rx, 24) } else { 0 };

// APPEND_MARKER7

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
        libdunit::println("gui_calc: FAIL cap transfer");
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
        libdunit::println("gui_calc: surface presented OK");
    } else {
        libdunit::println("gui_calc: FAIL no frame done");
        libdunit::handle_close(buf);
        libdunit::exit(6);
    }

    // 9) Interactive loop: the compositor forwards pointer events for the focused
    //    window as INPUT_MAGIC messages. Each button-down updates the calculator
    //    and repaints our shared buffer (the compositor blits it every tick).
    loop {
        let n = libdunit::ipc_recv_blocking(&mut rx, 0);
        if n < 8 || u32::from_le_bytes([rx[0], rx[1], rx[2], rx[3]]) != INPUT_MAGIC {
            continue;
        }
        let kind = rx[4];
        if kind == IN_QUIT {
            break;
        }
        if kind == IN_DOWN {
            let lx = i32::from_le_bytes([rx[8], rx[9], rx[10], rx[11]]);
            let ly = i32::from_le_bytes([rx[12], rx[13], rx[14], rx[15]]);
            if let Some(b) = button_at(lx, ly) {
                calc.press(b);
                render(px, &calc);
            }
        }
    }

    libdunit::handle_close(buf);
    libdunit::exit(0)
}







