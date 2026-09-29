#![no_std]
#![no_main]

//! DWM settings client for the M4 userspace DWM (Stack B), slice C.
//!
//! A gui-v1 client (same capability + wire path as `gui_files`) that edits the
//! live compositor config. It loads `dwm_settings::Settings`, paints clickable
//! color swatches, effect toggles and geometry steppers straight onto its shared
//! surface with the M4 text runtime, and on every change writes the config back
//! (`dwm_settings::save`) and pings the compositor to reload — the GUI<->TOML
//! round-trip the concept calls for. RAM-only this session (disk is DunitFS/M5).

use core::panic::PanicInfo;

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use gui_protocol_v1::wire::{Request, FEATURE_ARGB8888};

use dunit_render::Surface;
use dunit_style::value::Color;
use dunit_text::Font;

use dwm_settings::{Effects, Settings, Theme};

// Control-message magics shared with the compositor (see gui_server main.rs).
const CTRL_MAGIC: u32 = 0x3150_4143; // "CAP1" — capability announce
const INPUT_MAGIC: u32 = 0x3150_4E49; // "INP1" — pointer input
const RELOAD_MAGIC: u32 = 0x3144_4C52; // "RLD1" — "config changed, reload"
const IN_DOWN: u8 = 2;
const IN_QUIT: u8 = 9;

const SURFACE: u64 = 1;
const BUFFER: u64 = 2;
const W: u32 = 600;
const H: u32 = 600;
const FMT_XRGB8888: u32 = 1;

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

// UI geometry (client-local px).
const ROW_H: i32 = 24;
const Y0: i32 = 48;

// APPEND_MARKER

/// Preset color palette (ARGB8888) offered for every theme slot — the Green Tea
/// / Catppuccin family the baseline theme is built from, so any pick stays
/// coherent. Cycling `<`/`>` walks this list.
const PALETTE: [u32; 16] = [
    0xFF1E1E2E, 0xFF181825, 0xFF11111B, 0xFF313244, // base / mantle / crust / surface0
    0xFF45475A, 0xFFCDD6F4, 0xFFA6E3A1, 0xFF94E2D5, // surface1 / text / green / teal
    0xFF89B4FA, 0xFFCBA6F7, 0xFFF38BA8, 0xFFFAB387, // blue / mauve / red / peach
    0xFFF9E2AF, 0xFFF5C2E7, 0xFF89DCEB, 0xFFEBA0AC, // yellow / pink / sky / maroon
];

const NCOLORS: usize = 6;
const COLOR_LABELS: [&str; NCOLORS] =
    ["desktop", "panel", "title_focused", "border_focused", "taskbtn_focused", "menu"];

const NTOGGLES: usize = 3;
const TOGGLE_LABELS: [&str; NTOGGLES] = ["blur", "gradient", "animations"];

/// One geometry stepper: which field, the increment, and a sane clamp range.
struct IntSpec {
    label: &'static str,
    step: i32,
    min: i32,
    max: i32,
}
const NINTS: usize = 6;
const INT_SPECS: [IntSpec; NINTS] = [
    IntSpec { label: "panel_h", step: 2, min: 18, max: 64 },
    IntSpec { label: "title_h", step: 2, min: 16, max: 48 },
    IntSpec { label: "dock_w", step: 4, min: 32, max: 96 },
    IntSpec { label: "corner_radius", step: 2, min: 0, max: 32 },
    IntSpec { label: "shadow", step: 1, min: 0, max: 24 },
    IntSpec { label: "anim_ms", step: 20, min: 0, max: 600 },
];

/// Preset display modes offered by the resolution stepper. Index 0 is the
/// baseline `0/0` = "keep the boot resolution" (what the desktop does today); the
/// rest are common 16:9 modes the Bochs/DISPI mode-set backend accepts. Cycling
/// `<`/`>` walks this list; the CHOSEN mode is policy that lands in `[display]`
/// (single source of truth) — the compositor applies it on the next session
/// re-entry, and on a fixed GOP framebuffer the request degrades to a no-op.
const NRES: usize = 5;
const RES_PRESETS: [(u32, u32); NRES] = [
    (0, 0),
    (1280, 720),
    (1366, 768),
    (1600, 900),
    (1920, 1080),
];

// --- Settings field accessors (index -> field), read + mutable variants ------

fn color_ref(t: &mut Theme, i: usize) -> &mut u32 {
    match i {
        0 => &mut t.desktop,
        1 => &mut t.panel,
        2 => &mut t.title_focused,
        3 => &mut t.border_focused,
        4 => &mut t.taskbtn_focused,
        _ => &mut t.menu,
    }
}
fn color_get(t: &Theme, i: usize) -> u32 {
    match i {
        0 => t.desktop,
        1 => t.panel,
        2 => t.title_focused,
        3 => t.border_focused,
        4 => t.taskbtn_focused,
        _ => t.menu,
    }
}

// APPEND_MARKER2

fn toggle_get(fx: &Effects, i: usize) -> bool {
    match i {
        0 => fx.blur,
        1 => fx.gradient,
        _ => fx.anim,
    }
}
fn toggle_flip(fx: &mut Effects, i: usize) {
    match i {
        0 => fx.blur = !fx.blur,
        1 => fx.gradient = !fx.gradient,
        _ => fx.anim = !fx.anim,
    }
}

fn int_ref(s: &mut Settings, i: usize) -> &mut i32 {
    match i {
        0 => &mut s.layout.panel_h,
        1 => &mut s.layout.title_h,
        2 => &mut s.layout.dock_w,
        3 => &mut s.effects.corner_radius,
        4 => &mut s.effects.shadow,
        _ => &mut s.effects.anim_ms,
    }
}
fn int_get(s: &Settings, i: usize) -> i32 {
    match i {
        0 => s.layout.panel_h,
        1 => s.layout.title_h,
        2 => s.layout.dock_w,
        3 => s.effects.corner_radius,
        4 => s.effects.shadow,
        _ => s.effects.anim_ms,
    }
}

/// A click action bound to a screen rectangle.
#[derive(Clone, Copy)]
enum Action {
    ColorPrev(usize),
    ColorNext(usize),
    Toggle(usize),
    IntDelta(usize, i32),
    ResPrev,
    ResNext,
}

/// A clickable region and the action it fires.
struct Hotspot {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    act: Action,
}
impl Hotspot {
    fn hit(&self, px: i32, py: i32) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }
}

/// Editor model: the working settings plus the palette index chosen per color.
struct App {
    s: Settings,
    color_idx: [usize; NCOLORS],
    res_idx: usize,
}

// APPEND_MARKER3

impl App {
    fn new() -> App {
        let s = dwm_settings::load();
        let mut color_idx = [0usize; NCOLORS];
        for (i, slot) in color_idx.iter_mut().enumerate() {
            let cur = color_get(&s.theme, i);
            *slot = PALETTE.iter().position(|&p| p == cur).unwrap_or(0);
        }
        let res_idx = RES_PRESETS
            .iter()
            .position(|&(w, h)| w == s.display.width && h == s.display.height)
            .unwrap_or(0);
        App { s, color_idx, res_idx }
    }

    fn apply(&mut self, act: Action) {
        match act {
            Action::ColorPrev(i) => {
                self.color_idx[i] = (self.color_idx[i] + PALETTE.len() - 1) % PALETTE.len();
                *color_ref(&mut self.s.theme, i) = PALETTE[self.color_idx[i]];
            }
            Action::ColorNext(i) => {
                self.color_idx[i] = (self.color_idx[i] + 1) % PALETTE.len();
                *color_ref(&mut self.s.theme, i) = PALETTE[self.color_idx[i]];
            }
            Action::Toggle(i) => toggle_flip(&mut self.s.effects, i),
            Action::IntDelta(i, d) => {
                let (min, max) = (INT_SPECS[i].min, INT_SPECS[i].max);
                let r = int_ref(&mut self.s, i);
                *r = (*r + d).clamp(min, max);
            }
            Action::ResPrev => {
                self.res_idx = (self.res_idx + NRES - 1) % NRES;
                let (w, h) = RES_PRESETS[self.res_idx];
                self.s.display.width = w;
                self.s.display.height = h;
            }
            Action::ResNext => {
                self.res_idx = (self.res_idx + 1) % NRES;
                let (w, h) = RES_PRESETS[self.res_idx];
                self.s.display.width = w;
                self.s.display.height = h;
            }
        }
    }
}

// --- Low-level drawing (straight onto the shared surface) --------------------

/// ARGB8888 -> render Color.
fn col(argb: u32) -> Color {
    Color::rgba(
        ((argb >> 16) & 0xFF) as u8,
        ((argb >> 8) & 0xFF) as u8,
        (argb & 0xFF) as u8,
        ((argb >> 24) & 0xFF) as u8,
    )
}

/// Draw one line of text with its top-left at (x, y_top). Mirrors the runtime
/// painter's baseline math so it looks identical to DUI-rendered text.
fn draw_text(surf: &mut Surface, font: &Font, x: i32, y_top: i32, px: f32, color: Color, text: &str) {
    let baseline = y_top as f32 + font.line_metrics(px).ascent;
    let (glyphs, _adv) = font.layout_line(text, px);
    for g in glyphs {
        if let Some(bmp) = font.rasterize(g.glyph, px) {
            let ox = (x as f32 + g.x + 0.5) as i32 + bmp.left;
            let oy = (baseline + 0.5) as i32 - bmp.top;
            surf.blit_glyph(&bmp, ox, oy, color);
        }
    }
}

/// Draw a small labelled button; returns nothing (hotspot is registered by the
/// caller so hit rects stay in one place).
fn button(surf: &mut Surface, font: &Font, x: i32, y: i32, w: i32, label: &str) {
    let h = ROW_H - 6;
    surf.fill_rect(x as f32, y as f32, w as f32, h as f32, col(0xFF45475A));
    surf.stroke_rect(x as f32, y as f32, w as f32, h as f32, 1.0, col(0xFF585B70));
    draw_text(surf, font, x + 6, y + 2, 13.0, col(0xFFCDD6F4), label);
}

// APPEND_MARKER4

fn push_hex_byte(s: &mut String, v: u32) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    s.push(HEX[((v >> 4) & 0xF) as usize] as char);
    s.push(HEX[(v & 0xF) as usize] as char);
}
fn hex_argb(argb: u32) -> String {
    let mut s = String::from("#");
    push_hex_byte(&mut s, (argb >> 24) & 0xFF);
    push_hex_byte(&mut s, (argb >> 16) & 0xFF);
    push_hex_byte(&mut s, (argb >> 8) & 0xFF);
    push_hex_byte(&mut s, argb & 0xFF);
    s
}
fn push_int(s: &mut String, mut v: i32) {
    if v < 0 {
        s.push('-');
        v = -v;
    }
    if v == 0 {
        s.push('0');
        return;
    }
    let mut tmp = [0u8; 12];
    let mut n = 0;
    while v > 0 {
        tmp[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
    }
    while n > 0 {
        n -= 1;
        s.push(tmp[n] as char);
    }
}

// APPEND_MARKER5

/// Paint the whole editor into the shared surface and return the hit rects.
fn render(px_ptr: *mut u8, font: &Font, app: &App) -> Vec<Hotspot> {
    let pixels = unsafe { core::slice::from_raw_parts_mut(px_ptr as *mut u32, (W * H) as usize) };
    let mut surf = Surface::new(pixels, W as usize, H as usize);
    let mut spots: Vec<Hotspot> = Vec::new();

    // Backdrop + header.
    surf.fill_rect(0.0, 0.0, W as f32, H as f32, col(0xFF1E1E2E));
    draw_text(&mut surf, font, 16, 12, 17.0, col(0xFFA6E3A1), "DWM Settings — live config");
    draw_text(
        &mut surf,
        font,
        16,
        32,
        12.0,
        col(0xFF9399B2),
        "click < / > to recolor, [x] to toggle, - / + to adjust — saved to config & applied live",
    );

    let mut y = Y0;

    // ---- Theme colors --------------------------------------------------
    draw_text(&mut surf, font, 16, y, 14.0, col(0xFF89DCEB), "Theme colors");
    y += ROW_H;
    for i in 0..NCOLORS {
        button(&mut surf, font, 16, y, 22, "<");
        spots.push(Hotspot { x: 16, y, w: 22, h: ROW_H - 4, act: Action::ColorPrev(i) });
        let c = color_get(&app.s.theme, i);
        surf.fill_rect(44.0, y as f32, 44.0, (ROW_H - 6) as f32, col(c));
        surf.stroke_rect(44.0, y as f32, 44.0, (ROW_H - 6) as f32, 1.0, col(0xFF585B70));
        button(&mut surf, font, 92, y, 22, ">");
        spots.push(Hotspot { x: 92, y, w: 22, h: ROW_H - 4, act: Action::ColorNext(i) });
        let mut lbl = String::from(COLOR_LABELS[i]);
        lbl.push_str("   ");
        lbl.push_str(&hex_argb(c));
        draw_text(&mut surf, font, 124, y + 2, 14.0, col(0xFFCDD6F4), &lbl);
        y += ROW_H;
    }

    // APPEND_MARKER6

    // ---- Effects toggles ----------------------------------------------
    y += 4;
    draw_text(&mut surf, font, 16, y, 14.0, col(0xFF89DCEB), "Effects");
    y += ROW_H;
    for i in 0..NTOGGLES {
        let on = toggle_get(&app.s.effects, i);
        button(&mut surf, font, 16, y, 54, if on { "[x] on" } else { "[ ] off" });
        spots.push(Hotspot { x: 16, y, w: 54, h: ROW_H - 4, act: Action::Toggle(i) });
        draw_text(&mut surf, font, 124, y + 2, 14.0, col(0xFFCDD6F4), TOGGLE_LABELS[i]);
        y += ROW_H;
    }

    // ---- Geometry steppers --------------------------------------------
    y += 4;
    draw_text(&mut surf, font, 16, y, 14.0, col(0xFF89DCEB), "Layout & effect sizes");
    y += ROW_H;
    for i in 0..NINTS {
        let step = INT_SPECS[i].step;
        button(&mut surf, font, 16, y, 22, "-");
        spots.push(Hotspot { x: 16, y, w: 22, h: ROW_H - 4, act: Action::IntDelta(i, -step) });
        let mut val = String::new();
        push_int(&mut val, int_get(&app.s, i));
        draw_text(&mut surf, font, 48, y + 2, 14.0, col(0xFFF9E2AF), &val);
        button(&mut surf, font, 92, y, 22, "+");
        spots.push(Hotspot { x: 92, y, w: 22, h: ROW_H - 4, act: Action::IntDelta(i, step) });
        draw_text(&mut surf, font, 124, y + 2, 14.0, col(0xFFCDD6F4), INT_SPECS[i].label);
        y += ROW_H;
    }

    // ---- Display resolution -------------------------------------------
    y += 4;
    draw_text(&mut surf, font, 16, y, 14.0, col(0xFF89DCEB), "Display resolution");
    y += ROW_H;
    button(&mut surf, font, 16, y, 22, "<");
    spots.push(Hotspot { x: 16, y, w: 22, h: ROW_H - 4, act: Action::ResPrev });
    button(&mut surf, font, 92, y, 22, ">");
    spots.push(Hotspot { x: 92, y, w: 22, h: ROW_H - 4, act: Action::ResNext });
    let (rw, rh) = RES_PRESETS[app.res_idx];
    let mut rlbl = String::new();
    if rw == 0 || rh == 0 {
        rlbl.push_str("boot (keep)");
    } else {
        push_int(&mut rlbl, rw as i32);
        rlbl.push('x');
        push_int(&mut rlbl, rh as i32);
    }
    rlbl.push_str("   (applied on desktop reload)");
    draw_text(&mut surf, font, 124, y + 2, 14.0, col(0xFFF9E2AF), &rlbl);
    y += ROW_H;

    // ---- Status -------------------------------------------------------
    y += 6;
    draw_text(
        &mut surf,
        font,
        16,
        y,
        12.0,
        col(0xFF94E2D5),
        "changes written to /system/share/dwm/default.toml (RAM this session)",
    );

    spots
}

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    libdunit::println("gui_settings: PANIC");
    libdunit::exit(101)
}

fn u64_at(p: &[u8], off: usize) -> u64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&p[off..off + 8]);
    u64::from_le_bytes(a)
}

// APPEND_START

#[no_mangle]
pub extern "C" fn _start() -> ! {
    // 1) Handshake: compositor pid + our client id (id is a tint hint only).
    let mut m = [0u8; 8];
    if libdunit::ipc_recv_blocking(&mut m, 0) < 8 {
        libdunit::println("gui_settings: FAIL recv handshake");
        libdunit::exit(1);
    }
    let compositor = u32::from_le_bytes([m[0], m[1], m[2], m[3]]);

    // 2) Create + map our own shared buffer and paint the initial editor.
    let bytes = (W * H * 4) as usize;
    let buf = libdunit::handle_create_shared(bytes);
    if buf <= 0 {
        libdunit::println("gui_settings: FAIL create buffer");
        libdunit::exit(2);
    }
    let buf = buf as u32;
    let mapped = libdunit::handle_map(buf, 0, bytes);
    if mapped <= 0 {
        libdunit::println("gui_settings: FAIL map buffer");
        libdunit::exit(3);
    }
    let px = mapped as usize as *mut u8;
    let font = match load_font() {
        Ok(f) => f,
        Err(_) => {
            libdunit::println("gui_settings: FAIL font parse");
            libdunit::exit(7);
        }
    };
    let mut app = App::new();
    let mut spots = render(px, &font, &app);

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
        libdunit::println("gui_settings: FAIL welcome");
        libdunit::exit(4);
    }
    // APPEND_START2

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
        libdunit::println("gui_settings: FAIL cap transfer");
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
    // APPEND_START3

    // 8) FRAME_DONE from the compositor's composition tick.
    let n = libdunit::ipc_recv_blocking(&mut rx, 0);
    let ok = n >= 32 && u16::from_le_bytes([rx[8], rx[9]]) == 0x8021;
    if ok {
        libdunit::println("gui_settings: surface presented OK");
    } else {
        libdunit::println("gui_settings: FAIL no frame done");
        libdunit::handle_close(buf);
        libdunit::exit(6);
    }

    // 8.5) Headless resolution-picker proof (config-gated `[startup] self_test`,
    //      true only in test.toml — NEVER on the real desktop). The automated
    //      harness has no mouse, so we actuate the SAME `apply(ResNext)` path a
    //      `>` click drives, walking the picker to a concrete non-boot mode, then
    //      persist it (`dwm_settings::save`) and ping the compositor to reload —
    //      exactly what a click does. This drives the full GUI -> [display] ->
    //      live-resolution round trip: the compositor diffs the new `[display]`,
    //      re-enters its session and asks the kernel backend to mode-set. One-shot
    //      (fires once before the input loop), so the compositor re-enters exactly
    //      once — no reload storm.
    if dwm_settings::load_config().apps.self_test {
        let target = (1600u32, 900u32);
        let mut guard = 0;
        while RES_PRESETS[app.res_idx] != target && guard < NRES {
            app.apply(Action::ResNext);
            guard += 1;
        }
        dwm_settings::save(&app.s);
        let mut sig = [0u8; 8];
        sig[0..4].copy_from_slice(&RELOAD_MAGIC.to_le_bytes());
        libdunit::ipc_send(compositor, &sig);
        spots = render(px, &font, &app);
        let (tw, th) = RES_PRESETS[app.res_idx];
        let mut msg = String::from("gui_settings: res picker self-test picked ");
        push_int(&mut msg, tw as i32);
        msg.push('x');
        push_int(&mut msg, th as i32);
        msg.push_str(" (saved, reload pinged)");
        libdunit::println(&msg);
    }

    // 9) Interactive loop: a pointer-down inside a hotspot mutates the working
    //    Settings, writes them back to the config (RAM this session) and pings
    //    the compositor to reload — the GUI<->TOML round-trip applied live. We
    //    then repaint our shared buffer so the swatch/value reflects the change.
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
            let mut act = None;
            for hs in spots.iter() {
                if hs.hit(lx, ly) {
                    act = Some(hs.act);
                    break;
                }
            }
            if let Some(a) = act {
                app.apply(a);
                // Persist to the live config and ask the compositor to reload,
                // so the desktop reflects the new theme/geometry immediately.
                dwm_settings::save(&app.s);
                let mut sig = [0u8; 8];
                sig[0..4].copy_from_slice(&RELOAD_MAGIC.to_le_bytes());
                libdunit::ipc_send(compositor, &sig);
                spots = render(px, &font, &app);
            }
        }
    }

    libdunit::handle_close(buf);
    libdunit::exit(0)
}





