#![no_std]
#![no_main]

//! Untrusted gui-v1 client (M3 item 4).
//!
//! A normal unprivileged ELF: no display/input master, no elevated rights. It
//! renders into its own kernel shared buffer, transfers that buffer capability
//! (read-only) to the compositor, and drives its surface through the gui-v1
//! wire protocol carried over IPC messages. Proves an untrusted process can be
//! composited to the screen purely via capabilities + the wire protocol, with
//! the compositor owning display/input.

use core::panic::PanicInfo;

extern crate alloc;

use gui_protocol_v1::wire::{Request, FEATURE_ARGB8888};

use dunit_render::{paint, Surface};
use dunit_style::cascade::{Cascade, NodeStyle};
use dunit_style::parse as parse_dss;
use dunit_text::Font;
use dunit_ui::layout::layout_measured;
use dunit_ui::parse as parse_dui;
use dunit_ui::tree::{Kind, NodeId};
use dunit_widgets::{intrinsic_size, FontMeasure, Widget};

/// Control-message magic distinguishing a capability announce from a protocol
/// packet (protocol packets begin with the DGUI wire magic instead). Shared
/// with the compositor.
const CTRL_MAGIC: u32 = 0x3150_4143; // "CAP1"

// Compositor -> client input control messages (mirrors gui_server). Coords are
// client-local. The client is only ever sent events while it holds focus.
const INPUT_MAGIC: u32 = 0x3150_4E49; // "INP1"
const IN_MOVE: u8 = 1;
const IN_DOWN: u8 = 2;
const IN_UP: u8 = 3;
const IN_LEAVE: u8 = 4;
const IN_KEY: u8 = 5;
const IN_QUIT: u8 = 9;

/// Interaction state driving the client's re-paint: whether the pointer is over
/// the OK button, whether it is being pressed, and how many times it was clicked.
#[derive(Clone, Copy, Default)]
struct UiState {
    hover: bool,
    press: bool,
    clicks: u32,
}

const SURFACE: u64 = 1;
const BUFFER: u64 = 2;
const W: u32 = 240;
const H: u32 = 120;
const FMT_XRGB8888: u32 = 1;

/// The window's text font, embedded in the ELF (M4 still ships assets in-image;
/// M5 moves them to the disk root).

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

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    libdunit::println("gui_client: PANIC");
    libdunit::exit(101)
}

fn u64_at(p: &[u8], off: usize) -> u64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&p[off..off + 8]);
    u64::from_le_bytes(a)
}

/// Re-negotiate the surface at a new size after a server-pushed CONFIGURE: back
/// a FRESH kernel buffer of `new_w*new_h`, render into it, then replay the
/// buffer half of the handshake with a NEW object id (the compositor's freshness
/// gate rejects a re-used id). Returns the new `(handle, mapped ptr, ok-button
/// rect)` on success; the OLD buffer handle is closed. `serial` advances so
/// every request keeps a unique, increasing client serial. This is the client
/// half of maximize/fullscreen/restore — identical for every resizing app.
#[allow(clippy::too_many_arguments)]
fn resize_surface(
    compositor: u32,
    id: u32,
    old_buf: u32,
    font: &Font,
    st: UiState,
    new_w: u32,
    new_h: u32,
    obj: u64,
    token: u64,
    serial: &mut u64,
) -> Option<(u32, *mut u8, (i32, i32, i32, i32))> {
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
    let btn = render_window(npx, font, id, st, new_w, new_h);

    // ACK the new configure token so the compositor accepts our next COMMIT.
    let ack = Request::AckConfigure { configure: token }.encode(SURFACE, *serial);
    *serial += 1;
    send_env(compositor, id, &ack);

    // Transfer the fresh buffer (read-only) and announce {handle, size, object}.
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
    send_env(compositor, id, &ann);

    // IMPORT (fresh object id) / ATTACH / COMMIT (new token).
    let import =
        Request::ImportBuffer { width: new_w, height: new_h, stride: new_w * 4, format: FMT_XRGB8888, offset: 0 }
            .encode(obj, *serial);
    *serial += 1;
    send_env(compositor, id, &import);
    let attach =
        Request::AttachBuffer { buffer: obj, damage: alloc::vec::Vec::new() }.encode(SURFACE, *serial);
    *serial += 1;
    send_env(compositor, id, &attach);
    let commit = Request::Commit { configure: token, frame_callback: 1 }.encode(SURFACE, *serial);
    *serial += 1;
    send_env(compositor, id, &commit);

    // Release the old backing buffer; the compositor now blits the new one.
    libdunit::handle_close(old_buf);
    Some((nb, npx, btn))
}

/// Send `payload` to the compositor. The compositor routes inbound messages by
/// the kernel-authenticated sender pid, so no client-side envelope is needed (a
/// client-supplied id could be spoofed and is therefore never trusted). `_id`
/// is kept only so the call sites read symmetrically with the handshake.
fn send_env(dst: u32, _id: u32, payload: &[u8]) {
    libdunit::ipc_send(dst, payload);
}

// APPEND_MARKER

/// Paint the DUI window into the mapped ARGB8888 buffer using the full UI
/// Runtime (DUI tree -> DSS cascade -> content-measured layout -> dunit-render),
/// reflecting the current `UiState` (button hover/press + a live click counter).
/// Returns the OK button's rect in client-local pixels so the input loop can
/// hit-test the pointer against it. `w`/`h` are the surface's CURRENT size — the
/// compositor can grow/shrink us via a server-pushed CONFIGURE (resize/maximize),
/// so the layout must be measured against the live geometry, not a fixed const.
fn render_window(px: *mut u8, font: &Font, id: u32, st: UiState, w: u32, h: u32) -> (i32, i32, i32, i32) {
    let accent = if id == 1 { "#a6e3a1" } else { "#94e2d5" };
    let title = if id == 1 { "Dunit" } else { "Window 2" };
    // Live subtext: click count so a press is visibly acknowledged even without
    // text-caret focus. `format!` is available via alloc.
    let sub = alloc::format!("clicks: {}", st.clicks);
    let mut dui = alloc::string::String::new();
    dui.push_str("Column#win { Text#title \"");
    dui.push_str(title);
    dui.push_str("\" Text#sub \"");
    dui.push_str(&sub);
    dui.push_str("\" Button#ok \"OK\" }");
    let tree = match parse_dui(&dui) {
        Ok(t) => t,
        Err(_) => return (0, 0, 0, 0),
    };

    // Button background reflects interaction: pressed -> accent, hover -> lighter.
    let btn_bg = if st.press {
        accent
    } else if st.hover {
        "#3a3a3a"
    } else {
        "#2e2e2e"
    };
    let mut dss = alloc::string::String::new();
    dss.push_str("Column#win { background: #1b1b1b; padding: 12; }\n");
    dss.push_str("Text { color: #cdd6f4; font-size: 18; padding: 2; }\n");
    dss.push_str("Text#sub { color: ");
    dss.push_str(accent);
    dss.push_str("; font-size: 13; }\n");
    dss.push_str("Button { background: ");
    dss.push_str(btn_bg);
    dss.push_str("; color: #1b1b1b; border-width: 1; border-color: ");
    dss.push_str(accent);
    dss.push_str("; font-size: 15; padding: 6; }\n");
    let sheet = match parse_dss(&dss) {
        Ok(s) => s,
        Err(_) => return (0, 0, 0, 0),
    };
    let mut cas = Cascade::new();
    cas.push(sheet);

    // Resolve a node's cascaded style from its element tag (+ id).
    let style_of = |nid: NodeId| {
        let node = tree.node(nid);
        let tag = node.kind.tag();
        let ns = match node.name.as_deref() {
            Some(name) => NodeStyle { element: tag, id: Some(name), classes: &[], states: &[] },
            None => NodeStyle::element(tag),
        };
        cas.resolve(&ns)
    };

    // Measure element leaves against their real content (widget intrinsic size).
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
    let lay = layout_measured(&tree, w as f32, h as f32, &measure_fn);

    let pixels = unsafe { core::slice::from_raw_parts_mut(px as *mut u32, (w * h) as usize) };
    let mut surface = Surface::new(pixels, w as usize, h as usize);
    paint(&tree, &lay, &style_of, font, &mut surface);

    // Report the OK button's rect (client-local) for pointer hit-testing.
    match tree.by_name("ok") {
        Some(nid) => {
            let r = lay.rect(nid);
            (r.x as i32, r.y as i32, r.w as i32, r.h as i32)
        }
        None => (0, 0, 0, 0),
    }
}


#[no_mangle]
pub extern "C" fn _start() -> ! {
    // 1) Learn the compositor's pid and our client id (sent right after spawn).
    let mut m = [0u8; 8];
    if libdunit::ipc_recv_blocking(&mut m, 0) < 8 {
        libdunit::println("gui_client: FAIL recv handshake");
        libdunit::exit(1);
    }
    let compositor = u32::from_le_bytes([m[0], m[1], m[2], m[3]]);
    let id = u32::from_le_bytes([m[4], m[5], m[6], m[7]]);

    // 2) Render our window into our own kernel shared buffer via the M4 UI
    //    Runtime (DUI + DSS + TTF text), painted by dunit-render. The buffer is
    //    XRGB8888 little-endian, i.e. 0xAARRGGBB per u32 — the painter's format.
    let bytes = (W * H * 4) as usize;
    let buf = libdunit::handle_create_shared(bytes);
    if buf <= 0 {
        libdunit::println("gui_client: FAIL create buffer");
        libdunit::exit(2);
    }
    let buf = buf as u32;
    let mut cur_buf = buf;
    let mapped = libdunit::handle_map(buf, 0, bytes);
    if mapped <= 0 {
        libdunit::println("gui_client: FAIL map buffer");
        libdunit::exit(3);
    }
    let mut px = mapped as usize as *mut u8;
    let font = match load_font() {
        Ok(f) => f,
        Err(_) => {
            libdunit::println("gui_client: FAIL font parse");
            libdunit::exit(7);
        }
    };
    let mut st = UiState::default();
    // The surface's live geometry. The compositor may resize us via a
    // server-pushed CONFIGURE, so track it rather than reusing the W/H consts.
    let mut cur_w = W;
    let mut cur_h = H;
    let mut btn = render_window(px, &font, id, st, cur_w, cur_h);

    let mut rx = [0u8; 256];

    // 3) HELLO -> WELCOME.
    let hello = Request::Hello {
        min_minor: 0,
        max_minor: 0,
        offered_features: FEATURE_ARGB8888,
        required_features: 0,
    }
    .encode(0, 1);
    send_env(compositor, id, &hello);
    if libdunit::ipc_recv_blocking(&mut rx, 0) <= 0 {
        libdunit::println("gui_client: FAIL welcome");
        libdunit::exit(4);
    }

    // 4) CREATE_SURFACE -> RESULT + CONFIGURE (capture the configure token).
    let create =
        Request::CreateSurface { role: 1, width: W, height: H, format: FMT_XRGB8888 }.encode(SURFACE, 2);
    send_env(compositor, id, &create);
    libdunit::ipc_recv_blocking(&mut rx, 0); // RESULT
    let n = libdunit::ipc_recv_blocking(&mut rx, 0); // CONFIGURE
    let token = if n >= 32 { u64_at(&rx, 24) } else { 0 };

    // 5) ACK_CONFIGURE -> RESULT.
    let ack = Request::AckConfigure { configure: token }.encode(SURFACE, 3);
    send_env(compositor, id, &ack);
    libdunit::ipc_recv_blocking(&mut rx, 0);

    // 6) Transfer the buffer capability (read-only) to the compositor and
    //    announce {handle, size, object} so it can back our IMPORT_BUFFER.
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
        libdunit::println("gui_client: FAIL cap transfer");
        libdunit::exit(5);
    }
    let mut ann = [0u8; 16];
    ann[0..4].copy_from_slice(&CTRL_MAGIC.to_le_bytes());
    ann[4..8].copy_from_slice(&(ch as u32).to_le_bytes());
    ann[8..12].copy_from_slice(&(bytes as u32).to_le_bytes());
    ann[12..16].copy_from_slice(&(BUFFER as u32).to_le_bytes());
    send_env(compositor, id, &ann);

    // 7) IMPORT / ATTACH / COMMIT (each -> RESULT).
    let import =
        Request::ImportBuffer { width: W, height: H, stride: W * 4, format: FMT_XRGB8888, offset: 0 }
            .encode(BUFFER, 4);
    send_env(compositor, id, &import);
    libdunit::ipc_recv_blocking(&mut rx, 0);
    let attach = Request::AttachBuffer { buffer: BUFFER, damage: alloc::vec::Vec::new() }.encode(SURFACE, 5);
    send_env(compositor, id, &attach);
    libdunit::ipc_recv_blocking(&mut rx, 0);
    let commit = Request::Commit { configure: token, frame_callback: 1 }.encode(SURFACE, 6);
    send_env(compositor, id, &commit);
    libdunit::ipc_recv_blocking(&mut rx, 0);

    // 8) FRAME_DONE (opcode 0x8021) from the compositor's composition tick.
    let n = libdunit::ipc_recv_blocking(&mut rx, 0);
    let ok = n >= 32 && u16::from_le_bytes([rx[8], rx[9]]) == 0x8021;
    if ok {
        libdunit::println("gui_client: surface presented OK");
    } else {
        libdunit::println("gui_client: FAIL no frame done");
        libdunit::handle_close(buf);
        libdunit::exit(6);
    }

    // 9) Interactive event loop. The compositor owns the display and forwards
    //    pointer input for the focused window as INPUT_MAGIC control messages;
    //    we react by re-painting into our shared buffer (the compositor blits it
    //    every tick, so repaints appear without us presenting). It can also push
    //    a server-initiated CONFIGURE (resize/maximize/fullscreen); we re-buffer
    //    at the new size. Exit on QUIT.
    let mut next_obj: u64 = 3; // fresh buffer object id per resize (> import id 2)
    let mut serial: u64 = 7; // client request serial, continues past the handshake
    loop {
        let n = libdunit::ipc_recv_blocking(&mut rx, 0);
        if n < 8 {
            continue;
        }
        let magic = u32::from_le_bytes([rx[0], rx[1], rx[2], rx[3]]);
        // Server-pushed CONFIGURE (opcode 0x8010): the compositor resized us
        // (maximize/fullscreen/restore). Re-negotiate a fresh buffer at the new
        // geometry, then keep rendering into it at the new size.
        if magic == gui_protocol_v1::wire::MAGIC
            && n >= 48
            && u16::from_le_bytes([rx[8], rx[9]]) == 0x8010
        {
            let tok = u64_at(&rx, 24);
            let new_w = u32::from_le_bytes([rx[32], rx[33], rx[34], rx[35]]);
            let new_h = u32::from_le_bytes([rx[36], rx[37], rx[38], rx[39]]);
            if new_w == 0 || new_h == 0 || (new_w == cur_w && new_h == cur_h) {
                // No geometry change: just re-ack so a later COMMIT stays valid.
                let ack = Request::AckConfigure { configure: tok }.encode(SURFACE, serial);
                serial += 1;
                send_env(compositor, id, &ack);
                continue;
            }
            if let Some((nb, npx, nbtn)) = resize_surface(
                compositor, id, cur_buf, &font, st, new_w, new_h, next_obj, tok, &mut serial,
            ) {
                cur_buf = nb;
                px = npx;
                cur_w = new_w;
                cur_h = new_h;
                next_obj += 1;
                btn = nbtn;
                libdunit::println("gui_client: reconfigured OK");
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
        if kind == IN_KEY {
            // The compositor forwards each typed byte in the button field. Echo
            // it to serial so the keyboard-routing path is headlessly verifiable.
            let byte = rx[16];
            let line = [b'k', b'e', b'y', b'=', byte];
            libdunit::write(1, &line);
            libdunit::write(1, b"\n");
            continue;
        }
        let lx = i32::from_le_bytes([rx[8], rx[9], rx[10], rx[11]]);
        let ly = i32::from_le_bytes([rx[12], rx[13], rx[14], rx[15]]);
        let inside =
            lx >= btn.0 && lx < btn.0 + btn.2 && ly >= btn.1 && ly < btn.1 + btn.3;
        let before = st;
        match kind {
            IN_MOVE => st.hover = inside,
            IN_DOWN => {
                st.press = inside;
                if inside {
                    st.clicks = st.clicks.wrapping_add(1);
                }
            }
            IN_UP => st.press = false,
            IN_LEAVE => {
                st.hover = false;
                st.press = false;
            }
            _ => {}
        }
        // Repaint only when the visible state actually changed.
        if st.hover != before.hover || st.press != before.press || st.clicks != before.clicks {
            btn = render_window(px, &font, id, st, cur_w, cur_h);
        }
    }

    libdunit::handle_close(cur_buf);
    libdunit::exit(0)
}

