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
const SURFACE: u64 = 1;
const BUFFER: u64 = 2;
const W: u32 = 240;
const H: u32 = 120;
const FMT_XRGB8888: u32 = 1;

/// The window's text font, embedded in the ELF (M4 still ships assets in-image;
/// M5 moves them to the disk root).
static FONT_BYTES: &[u8] = include_bytes!("../../../../assets/fonts/DejaVuSans.ttf");

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

/// Send `payload` to the compositor. The compositor routes inbound messages by
/// the kernel-authenticated sender pid, so no client-side envelope is needed (a
/// client-supplied id could be spoofed and is therefore never trusted). `_id`
/// is kept only so the call sites read symmetrically with the handshake.
fn send_env(dst: u32, _id: u32, payload: &[u8]) {
    libdunit::ipc_send(dst, payload);
}

// APPEND_MARKER

/// Paint a small DUI window (title + button) into the mapped ARGB8888 buffer at
/// `px`, using the full UI Runtime: DUI tree -> DSS cascade -> content-measured
/// layout -> dunit-render painter, with real TTF text. `id` tints the accent so
/// the two concurrent client windows read as distinct on screen.
fn render_window(px: *mut u8, id: u32) {
    let font = match Font::parse(FONT_BYTES.to_vec()) {
        Ok(f) => f,
        Err(_) => return,
    };

    let dui = if id == 1 {
        r#"Column#win { Text#title "Dunit" Text#sub "green tea" Button#ok "OK" }"#
    } else {
        r#"Column#win { Text#title "Window 2" Text#sub "runtime demo" Button#ok "Close" }"#
    };
    let tree = match parse_dui(dui) {
        Ok(t) => t,
        Err(_) => return,
    };

    // Green Tea dark (mono) reference theme; client 2 shifts its accent hue.
    let accent = if id == 1 { "#a6e3a1" } else { "#94e2d5" };
    let mut dss = alloc::string::String::new();
    dss.push_str("Column#win { background: #1b1b1b; padding: 12; }\n");
    dss.push_str("Text { color: #cdd6f4; font-size: 18; padding: 2; }\n");
    dss.push_str("Text#sub { color: ");
    dss.push_str(accent);
    dss.push_str("; font-size: 13; }\n");
    dss.push_str("Button { background: #2e2e2e; color: ");
    dss.push_str(accent);
    dss.push_str("; border-width: 1; border-color: #45475a; font-size: 15; padding: 6; }\n");
    let sheet = match parse_dss(&dss) {
        Ok(s) => s,
        Err(_) => return,
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
    let fm = FontMeasure { font: &font };
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
    paint(&tree, &lay, &style_of, &font, &mut surface);
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
    let mapped = libdunit::handle_map(buf, 0, bytes);
    if mapped <= 0 {
        libdunit::println("gui_client: FAIL map buffer");
        libdunit::exit(3);
    }
    let px = mapped as usize as *mut u8;
    render_window(px, id);

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
    }

    libdunit::handle_close(buf);
    libdunit::exit(if ok { 0 } else { 6 })
}

