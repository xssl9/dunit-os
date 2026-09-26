#![no_std]
#![no_main]

//! Untrusted system-monitor client for the M4 userspace DWM (Stack B).
//!
//! The legacy kernel-GUI `gui_stats` reborn as a gui-v1 client. Same capability
//! + wire-protocol path as `gui_client`, and it paints through the same M4 UI
//! Runtime (DUI + DSS + TTF text). The difference is the event loop: instead of
//! only reacting to pointer input, it blocks on IPC with a timeout and repaints
//! fresh `get_system_stats` counters each time it wakes, so the window is a live
//! monitor. The compositor blits our shared buffer every tick, so repaints show
//! up without us ever presenting (display master stays with the server).

use core::panic::PanicInfo;

extern crate alloc;

use alloc::string::String;

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
const INPUT_MAGIC: u32 = 0x3150_4E49; // "INP1" — pointer input
const IN_QUIT: u8 = 9;

const SURFACE: u64 = 1;
const BUFFER: u64 = 2;
const W: u32 = 460;
const H: u32 = 300;
const FMT_XRGB8888: u32 = 1;

/// Poll cadence: block on IPC at most this long, then repaint fresh counters.
const POLL_MS: u64 = 500;

/// The window's text font, embedded in the ELF (M4 still ships assets in-image).
static FONT_BYTES: &[u8] = include_bytes!("../../../../assets/fonts/DejaVuSans.ttf");

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    libdunit::println("gui_stat: PANIC");
    libdunit::exit(101)
}

fn u64_at(p: &[u8], off: usize) -> u64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&p[off..off + 8]);
    u64::from_le_bytes(a)
}

/// Human-readable byte size (B / KiB / MiB), matching the legacy stats window.
fn fmt_size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        alloc::format!("{}MiB", bytes / (1024 * 1024))
    } else if bytes >= 1024 {
        alloc::format!("{}KiB", bytes / 1024)
    } else {
        alloc::format!("{}B", bytes)
    }
}

/// Append a `Text "…"` child, stripping characters that would break DUI parsing.
fn push_line(d: &mut String, text: &str) {
    d.push_str("Text \"");
    for c in text.chars() {
        if c != '"' && c != '{' && c != '}' {
            d.push(c);
        }
    }
    d.push_str("\" ");
}

// APPEND_MARKER

/// Build the DUI document for the current stats snapshot. Each row is one Text
/// child of a Column so the runtime stacks them vertically.
fn build_dui(s: &libdunit::SystemStats) -> String {
    let mut d = String::new();
    d.push_str("Column#win { Text#title \"System Stats\" ");
    push_line(
        &mut d,
        &alloc::format!(
            "proc: total {} run {} ready {} blocked {}",
            s.process_total, s.process_running, s.process_ready, s.process_blocked
        ),
    );
    push_line(&mut d, &alloc::format!("dead {} reaped {}", s.process_dead, s.process_reaped));
    push_line(
        &mut d,
        &alloc::format!("pmm: {} / {}", fmt_size(s.pmm_used_bytes), fmt_size(s.pmm_total_bytes)),
    );
    push_line(
        &mut d,
        &alloc::format!("heap: {} used  {} free-blocks", fmt_size(s.heap_used_bytes), s.heap_free_blocks),
    );
    push_line(
        &mut d,
        &alloc::format!(
            "ipc: queues {} msgs {} shared {}",
            s.ipc_queue_count, s.ipc_queued_messages, s.ipc_shared_regions
        ),
    );
    push_line(
        &mut d,
        &alloc::format!("fs: files {} dirs {} {}", s.fs_files, s.fs_directories, fmt_size(s.fs_bytes)),
    );
    push_line(
        &mut d,
        &alloc::format!("net: nics {} supported {}", s.net_total_nics, s.net_supported_nics),
    );
    push_line(&mut d, &alloc::format!("uptime: {}s", s.uptime_ticks / 100));
    d.push('}');
    d
}

// APPEND_MARKER2

/// Paint the current stats snapshot into the mapped ARGB8888 buffer using the
/// M4 UI Runtime (DUI tree -> DSS cascade -> content-measured layout -> render).
fn render_stats(px: *mut u8, font: &Font, s: &libdunit::SystemStats) {
    let dui = build_dui(s);
    let tree = match parse_dui(&dui) {
        Ok(t) => t,
        Err(_) => return,
    };
    let mut dss = String::new();
    dss.push_str("Column#win { background: #121820; padding: 10; }\n");
    dss.push_str("Text { color: #cdd6f4; font-size: 13; padding: 2; }\n");
    dss.push_str("Text#title { color: #2f8fbd; font-size: 18; padding: 4; }\n");
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

// APPEND_MARKER3

#[no_mangle]
pub extern "C" fn _start() -> ! {
    // 1) Handshake: compositor pid + our client id (id is a tint hint only).
    let mut m = [0u8; 8];
    if libdunit::ipc_recv_blocking(&mut m, 0) < 8 {
        libdunit::println("gui_stat: FAIL recv handshake");
        libdunit::exit(1);
    }
    let compositor = u32::from_le_bytes([m[0], m[1], m[2], m[3]]);

    // 2) Create + map our own shared buffer and paint the initial frame.
    let bytes = (W * H * 4) as usize;
    let buf = libdunit::handle_create_shared(bytes);
    if buf <= 0 {
        libdunit::println("gui_stat: FAIL create buffer");
        libdunit::exit(2);
    }
    let buf = buf as u32;
    let mapped = libdunit::handle_map(buf, 0, bytes);
    if mapped <= 0 {
        libdunit::println("gui_stat: FAIL map buffer");
        libdunit::exit(3);
    }
    let px = mapped as usize as *mut u8;
    let font = match Font::parse(FONT_BYTES.to_vec()) {
        Ok(f) => f,
        Err(_) => {
            libdunit::println("gui_stat: FAIL font parse");
            libdunit::exit(7);
        }
    };
    let mut stats = libdunit::SystemStats::default();
    libdunit::get_system_stats(&mut stats);
    render_stats(px, &font, &stats);

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
        libdunit::println("gui_stat: FAIL welcome");
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
        libdunit::println("gui_stat: FAIL cap transfer");
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
        libdunit::println("gui_stat: surface presented OK");
    } else {
        libdunit::println("gui_stat: FAIL no frame done");
        libdunit::handle_close(buf);
        libdunit::exit(6);
    }

    // 9) Live loop: block on IPC with a timeout. On QUIT we exit; on any input
    //    we ignore it (nothing to hit-test); on timeout (EAGAIN) we re-poll the
    //    counters and repaint. The compositor blits our buffer every tick.
    loop {
        let n = libdunit::ipc_recv_blocking(&mut rx, POLL_MS);
        if n >= 8 && u32::from_le_bytes([rx[0], rx[1], rx[2], rx[3]]) == INPUT_MAGIC && rx[4] == IN_QUIT
        {
            break;
        }
        // Whether woken by a stray event or the poll timeout, refresh the view.
        libdunit::get_system_stats(&mut stats);
        render_stats(px, &font, &stats);
    }

    libdunit::handle_close(buf);
    libdunit::exit(0)
}

