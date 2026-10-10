#![no_std]
#![no_main]

//! Demo / self-tests client for the userspace DWM (Stack B).
//!
//! A small showcase app that replaces the bare `gui_client` test window on the
//! dock. It renders a column of buttons; tapping one (pointer or keyboard 1..N)
//! posts a desktop notification to the compositor via the NOTIFY control message
//! ("NOT1"), so the toast policy ([notifications] timeout/corner/enabled) stays
//! entirely in the compositor — the client only names the text. Config-driven
//! palette (`apps/gui_demo.toml` via `dwm_settings::DemoCfg`), TrueType text,
//! and the same gui-v1 protocol path as the other clients, including the
//! server-pushed CONFIGURE resize (maximize / restore).

use core::panic::PanicInfo;

extern crate alloc;

use alloc::vec::Vec;

use gui_protocol_v1::wire::{Request, FEATURE_ARGB8888, MAGIC};

use dunit_render::Surface;
use dunit_style::value::Color;
use dunit_text::Font;

// Control-message magics shared with the compositor.
const CTRL_MAGIC: u32 = 0x3150_4143; // "CAP1" — capability announce
const INPUT_MAGIC: u32 = 0x3150_4E49; // "INP1" — pointer input
const NOTIFY_MAGIC: u32 = 0x3154_4F4E; // "NOT1" — post a desktop notification
const IN_DOWN: u8 = 2;
const IN_KEY: u8 = 5;
const IN_QUIT: u8 = 9;

/// Hard cap on notification text — matches the compositor's `NOTIFY_TEXT_MAX`.
/// The compositor re-clamps regardless (the length is the client's claim), so
/// this is only to keep the send buffer bounded on our side.
const NOTIFY_TEXT_MAX: usize = 64;

const SURFACE: u64 = 1;
const BUFFER: u64 = 2;
const W: u32 = 260;
const H: u32 = 300;
const FMT_XRGB8888: u32 = 1;
const FMT_ARGB8888: u32 = 2;

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    libdunit::println("gui_demo: PANIC");
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

/// Resolved demo palette, built once from `DemoCfg`. The window background
/// carries the configured `bg_alpha` in `clear_bg`; every text/button color is
/// forced opaque so labels stay crisp over a translucent backdrop.
#[derive(Clone, Copy)]
struct Palette {
    text: Color,
    muted: Color,
    btn: Color,
    accent: Color,
}

/// One demo button: a face label and the notification text it posts. This is the
/// app's own content (like the calculator's key faces), not desktop policy.
#[derive(Clone, Copy)]
struct DemoButton {
    label: &'static str,
    notify: &'static str,
}

/// The fixed button column. Row 0 is the primary (accent) action; the rest post
/// distinct toasts so the notification queue/stack is visibly exercised.
const BUTTONS: [DemoButton; 4] = [
    DemoButton { label: "Say hello",  notify: "Hello from gui_demo" },
    DemoButton { label: "Post info",  notify: "Info: everything nominal" },
    DemoButton { label: "Warn me",    notify: "Warning: this is a test" },
    DemoButton { label: "Report OK",  notify: "All systems go" },
];
const NBTN: i32 = BUTTONS.len() as i32;

/// Build the NOTIFY control message and send it to the compositor. The compositor
/// clamps the length regardless (untrusted), so we only keep our buffer bounded.
fn send_notify(compositor: u32, text: &str) {
    let bytes = text.as_bytes();
    let len = bytes.len().min(NOTIFY_TEXT_MAX);
    let mut msg = [0u8; 8 + NOTIFY_TEXT_MAX];
    msg[0..4].copy_from_slice(&NOTIFY_MAGIC.to_le_bytes());
    msg[4..8].copy_from_slice(&(len as u32).to_le_bytes());
    msg[8..8 + len].copy_from_slice(&bytes[..len]);
    libdunit::ipc_send(compositor, &msg[..8 + len]);
}

/// Geometry derived from the live surface size (the compositor can resize us):
/// a title band at the top, then an evenly spaced button column below it.
#[derive(Clone, Copy)]
struct Layout {
    pad: i32,
    title_h: i32,
    btn_h: i32,
    btn_w: i32,
    x0: i32,
}

impl Layout {
    fn new(w: i32, h: i32) -> Layout {
        let pad = (w / 24).max(8);
        let title_h = (h * 22 / 100).max(40);
        let avail = h - title_h - pad * (NBTN + 1);
        let btn_h = (avail / NBTN).max(1);
        let btn_w = (w - 2 * pad).max(1);
        Layout { pad, title_h, btn_h, btn_w, x0: pad }
    }

    /// Screen-local rect of button `i`.
    fn button(&self, i: i32) -> (i32, i32, i32, i32) {
        let y = self.title_h + self.pad + i * (self.btn_h + self.pad);
        (self.x0, y, self.btn_w, self.btn_h)
    }

    /// Map a client-local pointer position to a button index, if any.
    fn button_at(&self, px: i32, py: i32) -> Option<usize> {
        for i in 0..NBTN {
            let (x, y, w, h) = self.button(i);
            if px >= x && px < x + w && py >= y && py < y + h {
                return Some(i as usize);
            }
        }
        None
    }
}

/// Paint the whole demo surface into the mapped buffer at the given geometry.
fn render(px: *mut u8, w: u32, h: u32, font: &Font, pal: &Palette, clear_bg: Color) {
    let pixels = unsafe { core::slice::from_raw_parts_mut(px as *mut u32, (w * h) as usize) };
    let mut s = Surface::new(pixels, w as usize, h as usize);
    // Base fill carries the configured alpha (translucent when bg_alpha < 255).
    s.clear(clear_bg);

    let ly = Layout::new(w as i32, h as i32);

    // Title + subtitle in the top band.
    let title = "Dunit Demo";
    let tpx = (ly.title_h as f32 * 0.34).max(16.0);
    draw_text(&mut s, font, ly.x0, (ly.title_h as f32 * 0.46) as i32, tpx, title, pal.text);
    let sub = "Tap a button to post a toast";
    let spx = (ly.title_h as f32 * 0.20).max(10.0);
    draw_text(&mut s, font, ly.x0, (ly.title_h as f32 * 0.80) as i32, spx, sub, pal.muted);
    // Accent rule under the title.
    s.fill_rect(ly.x0 as f32, (ly.title_h - 2) as f32, ly.btn_w as f32, 2.0, pal.accent);

    // Button column: row 0 is the accent (primary) action.
    for i in 0..NBTN {
        let (x, y, bw, bh) = ly.button(i);
        let fill = if i == 0 { pal.accent } else { pal.btn };
        s.fill_rect(x as f32, y as f32, bw as f32, bh as f32, fill);
        let label = BUTTONS[i as usize].label;
        let lpx = (bh as f32 * 0.40).max(12.0);
        let lw = text_width(font, label, lpx) as i32;
        let lx = x + (bw - lw) / 2;
        let lbase = y + bh / 2 + (lpx * 0.34) as i32;
        draw_text(&mut s, font, lx, lbase, lpx, label, pal.text);
    }
}

/// Re-negotiate a fresh buffer at a new geometry in response to a server-pushed
/// CONFIGURE (maximize/restore), mirroring the calculator's resize path.
#[allow(clippy::too_many_arguments)]
fn resize_surface(
    compositor: u32,
    old_buf: u32,
    font: &Font,
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
    render(npx, new_w, new_h, font, pal, clear_bg);

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
    // 0) Load this app's config (apps/gui_demo.toml). A missing/garbage file
    //    yields the built-in baseline; the resolved values drive the palette and
    //    background transparency (config→behavior, single source of truth).
    let dcfg = dwm_settings::DemoCfg::load("gui_demo");
    let fmt = if dcfg.bg_alpha < 255 { FMT_ARGB8888 } else { FMT_XRGB8888 };
    libdunit::println(&alloc::format!(
        "gui_demo: cfg from_file={} bg={:#010X} accent={:#010X} bg_alpha={} fmt={}",
        dcfg.from_file as u32,
        dcfg.bg,
        dcfg.accent,
        dcfg.bg_alpha,
        if fmt == FMT_ARGB8888 { "argb8888" } else { "xrgb8888" },
    ));

    let pal = Palette {
        text: col(dcfg.text | 0xFF00_0000),
        muted: col(dcfg.muted | 0xFF00_0000),
        btn: col(dcfg.btn | 0xFF00_0000),
        accent: col(dcfg.accent | 0xFF00_0000),
    };
    let clear_bg = col((dcfg.bg_alpha << 24) | (dcfg.bg & 0x00FF_FFFF));

    // A per-app `[demo] font` override wins over the desktop font; empty inherits.
    let font = if !dcfg.font.as_str().is_empty() {
        match libdunit::read_binary(dcfg.font.as_str(), 4 * 1024 * 1024)
            .and_then(|b| Font::parse(b).ok())
        {
            Some(f) => f,
            None => match load_font() {
                Ok(f) => f,
                Err(_) => {
                    libdunit::println("gui_demo: FAIL font parse");
                    libdunit::exit(7);
                }
            },
        }
    } else {
        match load_font() {
            Ok(f) => f,
            Err(_) => {
                libdunit::println("gui_demo: FAIL font parse");
                libdunit::exit(7);
            }
        }
    };

    // 1) Handshake: compositor pid + our client id (id is a tint hint only).
    let mut m = [0u8; 8];
    if libdunit::ipc_recv_blocking(&mut m, 0) < 8 {
        libdunit::println("gui_demo: FAIL recv handshake");
        libdunit::exit(1);
    }
    let compositor = u32::from_le_bytes([m[0], m[1], m[2], m[3]]);

    // 2) Create + map our own shared buffer and paint the initial frame.
    let bytes = (W * H * 4) as usize;
    let buf = libdunit::handle_create_shared(bytes);
    if buf <= 0 {
        libdunit::println("gui_demo: FAIL create buffer");
        libdunit::exit(2);
    }
    let buf = buf as u32;
    let mapped = libdunit::handle_map(buf, 0, bytes);
    if mapped <= 0 {
        libdunit::println("gui_demo: FAIL map buffer");
        libdunit::exit(3);
    }
    let px = mapped as usize as *mut u8;
    let mut cur_buf = buf;
    let mut cur_w = W;
    let mut cur_h = H;
    render(px, cur_w, cur_h, &font, &pal, clear_bg);

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
        libdunit::println("gui_demo: FAIL welcome");
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
        libdunit::println("gui_demo: FAIL cap transfer");
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
        libdunit::println("gui_demo: surface presented OK");
    } else {
        libdunit::println("gui_demo: FAIL no frame done");
        libdunit::handle_close(buf);
        libdunit::exit(6);
    }

    // 8b) Startup notification: post one toast right away. Headless this proves
    //     the whole client→compositor NOTIFY path end to end (the compositor
    //     prints `client notify … queued=…`); on a real desktop it is the app's
    //     "I'm up" toast. The compositor still gates it by [notifications].
    send_notify(compositor, "gui_demo started");
    libdunit::println("gui_demo: notify sent \"gui_demo started\"");

    // 9) Interactive loop. Pointer button-down on a button posts its toast; keys
    //    1..N post the same buttons (full keyboard control); a server CONFIGURE
    //    re-negotiates the buffer at the new geometry.
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
            } else if let Some((nb, _npx)) = resize_surface(
                compositor, cur_buf, &font, &pal, clear_bg, fmt, nw, nh, next_obj, tok,
                &mut serial,
            ) {
                // The demo surface is static content (the toast is the feedback),
                // so we never re-render after the resize — resize_surface already
                // painted the fresh buffer; we only track the new cap + geometry.
                cur_buf = nb;
                cur_w = nw;
                cur_h = nh;
                next_obj += 1;
                libdunit::println("gui_demo: reconfigured OK");
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
        let mut fire: Option<usize> = None;
        if kind == IN_DOWN {
            let lx = i32::from_le_bytes([rx[8], rx[9], rx[10], rx[11]]);
            let ly = i32::from_le_bytes([rx[12], rx[13], rx[14], rx[15]]);
            fire = Layout::new(cur_w as i32, cur_h as i32).button_at(lx, ly);
        } else if kind == IN_KEY {
            // ASCII '1'..'N' fire the matching button — keyboard control.
            let a = rx[16];
            if a >= b'1' && (a - b'0') as i32 <= NBTN {
                fire = Some((a - b'1') as usize);
            }
        }
        if let Some(i) = fire {
            send_notify(compositor, BUTTONS[i].notify);
            libdunit::println(&alloc::format!("gui_demo: notify sent \"{}\"", BUTTONS[i].notify));
        }
    }

    libdunit::handle_close(cur_buf);
    libdunit::exit(0)
}




