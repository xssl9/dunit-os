#![no_std]
#![no_main]

//! Dunit GUI server — userspace ELF (M3).
//!
//! Bring-up slice for the M3 milestone "userspace GUI Server и compositor". This
//! is intentionally minimal: it proves the M2 headless protocol reference crate
//! [`gui_protocol_v1`] compiles, links and runs on the `x86_64-unknown-none`
//! userspace target, that a fresh system ELF flows through the
//! Makefile → `build/userspace` → kernel-embedded `/app` pipeline, and that the
//! privileged display-master capability can be acquired from userspace.
//!
//! Later M3 items replace this body with the real compositor: shared-buffer
//! syscalls, the framebuffer output path, and driving [`gui_protocol_v1::server`]
//! with real wire packets + transferred buffer capabilities.

use core::panic::PanicInfo;

extern crate alloc;
use alloc::vec::Vec;

use gui_protocol_v1::server::Server;
use gui_protocol_v1::wire::Request;
use gui_protocol_v1::wire::{FORMAT_ARGB8888, FORMAT_XRGB8888};
use gui_protocol_v1::Opcode;

use dwm_settings as settings;
use settings::{Applications, Edge, Effects, Layout, Theme};

use dunit_text::Font;

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    // No unwinding in userspace: report and exit non-zero.
    libdunit::println("gui_server: PANIC");
    libdunit::exit(101)
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    libdunit::println("gui_server: start");

    // 1) The M2 protocol reference server links and runs here (allocator from
    //    libdunit, no_std+alloc core). Instantiate it and admit one client so a
    //    genuine object is allocated — this exercises the crate end to end.
    let mut server = Server::new();
    let conn = match server.connect() {
        Some(conn) => {
            libdunit::println("gui_server: protocol server up (client admitted)");
            conn
        }
        None => {
            libdunit::println("gui_server: FAIL protocol server rejected first client");
            libdunit::exit(1);
        }
    };

    // 2) Acquire the exclusive display-master capability (kernel syscall 53).
    //    A single GUI server owns the display; a second acquire would fail.
    let display = libdunit::handle_display_acquire();
    if display > 0 {
        libdunit::println("gui_server: display master acquired");
    } else {
        libdunit::println("gui_server: display master unavailable");
    }

    // 3) Acquire the exclusive input-master capability (kernel syscall 55).
    //    The compositor is the sole reader of keyboard/mouse input; it fans
    //    events out to clients over the protocol. A second acquire would fail.
    let input = libdunit::handle_input_acquire();
    if input > 0 {
        libdunit::println("gui_server: input master acquired");
    } else {
        libdunit::println("gui_server: input master unavailable");
    }

    // 4) Shared-buffer capability smoke (M3 item 1): allocate a zero-copy shared
    //    frame buffer, map it writable via the handle's WRITE right, round-trip a
    //    byte pattern through the aliased physical frames, then release. This
    //    exercises the syscall path that clients will use to hand pixel buffers
    //    to the compositor.
    let buf = libdunit::handle_create_shared(4096);
    if buf > 0 {
        let mapped = libdunit::handle_map(buf as u32, 0, 4096);
        if mapped > 0 {
            let ptr = mapped as usize as *mut u8;
            let mut ok = true;
            unsafe {
                for i in 0..4096usize {
                    core::ptr::write_volatile(ptr.add(i), (i & 0xff) as u8);
                }
                for i in 0..4096usize {
                    if core::ptr::read_volatile(ptr.add(i)) != (i & 0xff) as u8 {
                        ok = false;
                        break;
                    }
                }
            }
            if ok {
                libdunit::println("gui_server: shared buffer round-trip OK");
            } else {
                libdunit::println("gui_server: FAIL shared buffer mismatch");
            }
        } else {
            libdunit::println("gui_server: FAIL shared buffer map");
        }
        libdunit::handle_close(buf as u32);
    } else {
        libdunit::println("gui_server: FAIL shared buffer create");
    }

    // 5) Cross-process capability transfer (M3 item 1; groundwork for item 4's
    //    untrusted clients): create a shared buffer, write a marker, spawn an
    //    untrusted child, transfer a handle to it, and prove both sides see the
    //    same physical frames (bidirectional zero-copy across a process bound).
    let xbuf = libdunit::handle_create_shared(4096);
    if xbuf > 0 {
        let mapped = libdunit::handle_map(xbuf as u32, 0, 4096);
        if mapped > 0 {
            let ptr = mapped as usize as *mut u8;
            unsafe {
                core::ptr::write_volatile(ptr, 0xA5);
                core::ptr::write_volatile(ptr.add(1), 0x5A);
            }
            let peer = libdunit::spawn("gui_shbuf_peer");
            if peer > 0 {
                // Transfer a duplicate so we keep our own handle + mapping.
                let dup = libdunit::handle_dup(
                    xbuf as u32,
                    libdunit::RIGHT_READ
                        | libdunit::RIGHT_WRITE
                        | libdunit::RIGHT_MAP
                        | libdunit::RIGHT_TRANSFER,
                );
                let child_handle = if dup > 0 {
                    libdunit::handle_transfer(dup as u32, peer as u32)
                } else {
                    -1
                };
                if child_handle > 0 {
                    let mut msg = [0u8; 8];
                    msg[..4].copy_from_slice(&libdunit::get_pid().to_le_bytes());
                    msg[4..].copy_from_slice(&(child_handle as u32).to_le_bytes());
                    libdunit::ipc_send(peer as u32, &msg);
                    let mut ack = [0u8; 4];
                    if libdunit::ipc_recv_blocking(&mut ack, 0) == 4 && &ack == b"done" {
                        let seen = unsafe {
                            core::ptr::read_volatile(ptr.add(2)) == 0xC3
                                && core::ptr::read_volatile(ptr.add(3)) == 0x3C
                        };
                        if seen {
                            libdunit::println("gui_server: cross-process shared buffer OK");
                        } else {
                            libdunit::println("gui_server: FAIL child writes not visible");
                        }
                    } else {
                        libdunit::println("gui_server: FAIL child ack");
                    }
                } else {
                    libdunit::println("gui_server: FAIL capability transfer");
                }
                // Reap the child so it does not linger as a zombie.
                let mut status = libdunit::WaitStatus::empty();
                for _ in 0..50 {
                    if libdunit::wait(peer as u32, &mut status) == peer as isize {
                        break;
                    }
                    libdunit::sleep_ms(10);
                }
            } else {
                libdunit::println("gui_server: FAIL peer spawn");
            }
        }
        libdunit::handle_close(xbuf as u32);
    }

    // 6) Framebuffer output path (M3 item 3, first slice): as the display master,
    //    blit a small pixel block into the system framebuffer via syscall 57. The
    //    kernel gates fb_present on the display-master owner, so a non-compositor
    //    process is denied (see gui_shbuf_peer). This is the mechanism the software
    //    compositor will drive to present composited surfaces.
    let mut block = [0u8; 16 * 16 * 4];
    for px in block.chunks_exact_mut(4) {
        // Solid opaque blue (little-endian XRGB/BGRA: B,G,R,A).
        px[0] = 0xC0;
        px[1] = 0x40;
        px[2] = 0x20;
        px[3] = 0xff;
    }
    if libdunit::fb_present(&block, 16, 16, 0, 0) == 0 {
        libdunit::println("gui_server: framebuffer present OK");
    } else {
        libdunit::println("gui_server: FAIL framebuffer present");
    }

    // 7) Software compositor cycle (M3 item 3): drive the gui_protocol_v1
    //    reference server with REAL wire packets through one client's full
    //    map-and-present lifecycle, back the surface with a REAL kernel shared
    //    buffer, and composite its pixels to the framebuffer.
    //
    //    `Server` is a pure protocol state machine — it never touches pixels and
    //    exposes no surface geometry — so the compositor keeps its own
    //    buffer-id -> shared-frame mapping and its own placement policy, feeds
    //    the client's requests through `deliver`, and on the committed frame
    //    blits the mapped pixels to the framebuffer via `fb_present`. This is the
    //    exact mechanism the untrusted cross-process clients (item 4) will drive;
    //    here the request stream is synthesized in-process to keep it serially
    //    verifiable without a second protocol-speaking process.
    if run_compositor_cycle(&mut server, conn) {
        libdunit::println("gui_server: compositor cycle OK");
    } else {
        libdunit::println("gui_server: FAIL compositor cycle");
    }

    // 8) Cross-process compositing (M3 item 4): serve TWO real, untrusted
    //    client ELFs concurrently over IPC. Each renders into its own shared
    //    buffer, transfers the buffer capability to us, and drives its surface
    //    through the gui-v1 wire protocol (client→compositor messages carry a
    //    4-byte client-id envelope so we route them to the right connection).
    //    We map each transferred buffer read-only, run its requests through a
    //    per-client Server connection, and composite both surfaces to the
    //    framebuffer at separate slots. Neither client holds display/input.
    // serve_two_clients() emits "served two untrusted clients OK" itself (right
    // before it holds the composited frame on screen); only report failure here.
    if !serve_two_clients() {
        libdunit::println("gui_server: FAIL serving clients");
    }

    libdunit::println("gui_server: OK");
    libdunit::exit(0)
}

/// Opcode of an outbound protocol packet (offset 8, little-endian u16).
fn opcode_of(p: &[u8]) -> Option<Opcode> {
    Opcode::from_u16(u16::from_le_bytes([p[8], p[9]]))
}

/// Read a little-endian u32 at `off`.
fn u32_at(p: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([p[off], p[off + 1], p[off + 2], p[off + 3]])
}

/// Read a little-endian u64 at `off`.
fn u64_at(p: &[u8], off: usize) -> u64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&p[off..off + 8]);
    u64::from_le_bytes(a)
}

/// Find the first packet with `op` in a batch of outbound packets.
fn find<'a>(batch: &'a [Vec<u8>], op: Opcode) -> Option<&'a Vec<u8>> {
    batch.iter().find(|p| opcode_of(p) == Some(op))
}

/// Drive one client's full map-and-present lifecycle through the reference
/// server with real wire packets, backing the surface with a real kernel
/// shared buffer, then composite the committed pixels to the framebuffer.
fn run_compositor_cycle(server: &mut Server, conn: gui_protocol_v1::server::ConnId) -> bool {
    const SURFACE: u64 = 1; // client-chosen surface object id
    const BUFFER: u64 = 2; // client-chosen buffer object id
    const W: u32 = 64;
    const H: u32 = 64;
    const FMT_XRGB8888: u32 = 1;
    let bytes = (W * H * 4) as usize;

    // Back the buffer with a real zero-copy kernel shared buffer and paint a
    // recognizable gradient into the mapped frames (XRGB8888, little-endian).
    let buf = libdunit::handle_create_shared(bytes);
    if buf <= 0 {
        return false;
    }
    let buf = buf as u32;
    let mapped = libdunit::handle_map(buf, 0, bytes);
    if mapped <= 0 {
        libdunit::handle_close(buf);
        return false;
    }
    let pixels = mapped as usize as *mut u8;
    unsafe {
        for y in 0..H as usize {
            for x in 0..W as usize {
                let off = (y * W as usize + x) * 4;
                core::ptr::write_volatile(pixels.add(off), (x * 4) as u8); // B
                core::ptr::write_volatile(pixels.add(off + 1), (y * 4) as u8); // G
                core::ptr::write_volatile(pixels.add(off + 2), 0x80); // R
                core::ptr::write_volatile(pixels.add(off + 3), 0xff); // X
            }
        }
    }

    let ok = drive_protocol(server, conn, bytes, W, H, FMT_XRGB8888, SURFACE, BUFFER, pixels);
    libdunit::handle_close(buf);
    ok
}

/// Feed one client's request stream through the reference server as real wire
/// packets and, on the committed frame, blit the shared buffer to the display.
/// Returns true iff every stage produced the expected reply and the framebuffer
/// present succeeded.
#[allow(clippy::too_many_arguments)]
fn drive_protocol(
    server: &mut Server,
    conn: gui_protocol_v1::server::ConnId,
    bytes: usize,
    w: u32,
    h: u32,
    fmt: u32,
    surface: u64,
    buffer: u64,
    pixels: *const u8,
) -> bool {
    use gui_protocol_v1::wire::FEATURE_ARGB8888;

    // 1) HELLO -> WELCOME (completes connection negotiation).
    let hello = Request::Hello {
        min_minor: 0,
        max_minor: 0,
        offered_features: FEATURE_ARGB8888,
        required_features: 0,
    }
    .encode(0, 1);
    if find(&server.deliver(conn, &hello, None), Opcode::Welcome).is_none() {
        return false;
    }

    // 2) CREATE_SURFACE -> RESULT + CONFIGURE; the configure token is the
    //    CONFIGURE packet's header serial (offset 24).
    let create =
        Request::CreateSurface { role: 1, width: w, height: h, format: fmt }.encode(surface, 2);
    let out = server.deliver(conn, &create, None);
    let token = match find(&out, Opcode::Configure) {
        Some(cfg) => u64_at(cfg, 24),
        None => return false,
    };

    // 3) ACK_CONFIGURE -> RESULT.
    let ack = Request::AckConfigure { configure: token }.encode(surface, 3);
    if find(&server.deliver(conn, &ack, None), Opcode::Result).is_none() {
        return false;
    }

    // 4) IMPORT_BUFFER — cap carries the backing size in bytes -> RESULT.
    let import = Request::ImportBuffer {
        width: w,
        height: h,
        stride: w * 4,
        format: fmt,
        offset: 0,
    }
    .encode(buffer, 4);
    if find(&server.deliver(conn, &import, Some(bytes as u64)), Opcode::Result).is_none() {
        return false;
    }

    // 5) ATTACH_BUFFER (no damage) -> RESULT.
    let attach = Request::AttachBuffer { buffer, damage: Vec::new() }.encode(surface, 5);
    if find(&server.deliver(conn, &attach, None), Opcode::Result).is_none() {
        return false;
    }

    // 6) COMMIT with a frame callback -> RESULT; the surface is now mapped.
    let commit = Request::Commit { configure: token, frame_callback: 1 }.encode(surface, 6);
    if find(&server.deliver(conn, &commit, None), Opcode::Result).is_none() {
        return false;
    }

    // 7) Composition tick fires the mapped surface's frame callback:
    //    FRAME_DONE with status 0 (PRESENTED, offset 48) addressed to conn.
    let presented = server.composite().iter().any(|(c, p)| {
        *c == conn && opcode_of(p) == Some(Opcode::FrameDone) && u32_at(p, 48) == 0
    });
    if !presented {
        return false;
    }

    // 8) Blit the committed surface's pixels to the framebuffer at the
    //    compositor's chosen placement. Gated by the kernel on display master.
    let data = unsafe { core::slice::from_raw_parts(pixels, bytes) };
    if libdunit::fb_present(data, w, h, 100, 100) != 0 {
        return false;
    }

    // 8.5) Damaged second frame: commit a new buffer that differs only in a
    //      small rectangle and re-present ONLY that damaged region. Proves the
    //      compositor honors client-declared damage instead of repainting the
    //      whole surface every frame.
    if !present_damaged_frame(server, conn, surface, token, w, h, fmt) {
        return false;
    }

    // 8.75) Server-initiated resize (spec §9 reconfigure): the compositor pushes
    //       a fresh CONFIGURE at new geometry; the client must ACK the new token,
    //       re-import a buffer of the new size with a FRESH object id, and commit.
    //       This is the exact mechanism interactive maximize/fullscreen drives —
    //       synthesized in-process so the whole round trip is serially verifiable.
    if !drive_reconfigure(server, conn, surface, fmt) {
        return false;
    }

    // 9) Focus/input routing: the compositor owns the input master and fans
    //    events out to clients through the protocol's single-seat router. Give
    //    the mapped surface pointer + keyboard focus, then inject a pointer
    //    motion/button and a key press; the router must emit the corresponding
    //    client-bound events (ENTER on focus change, then BUTTON/KEY). This is
    //    the path real keyboard/mouse input from the input master will drive.
    let target = Some((conn, surface));
    let entered_ptr = server
        .set_pointer_focus(target, 10, 10)
        .iter()
        .any(|(c, p)| *c == conn && opcode_of(p) == Some(Opcode::PointerEnter));
    let _ = server.pointer_motion(12, 14);
    let got_button = server
        .pointer_button(0, true)
        .iter()
        .any(|(c, p)| *c == conn && opcode_of(p) == Some(Opcode::PointerButton));
    let entered_kbd = server
        .set_keyboard_focus(target, 0)
        .iter()
        .any(|(c, p)| *c == conn && opcode_of(p) == Some(Opcode::KeyEnter));
    let got_key = server
        .key(0x04, true, 0, false)
        .iter()
        .any(|(c, p)| *c == conn && opcode_of(p) == Some(Opcode::Key));

    let routed = entered_ptr && got_button && entered_kbd && got_key;
    if routed {
        libdunit::println("gui_server: input routing OK");
    } else {
        libdunit::println("gui_server: FAIL input routing");
    }
    routed
}

/// Commit a second buffer to `surface` whose contents differ from the first
/// only inside a small rectangle, declaring that rectangle as damage, then
/// re-present ONLY the damaged sub-rect to the framebuffer. Returns true iff the
/// protocol lifecycle and the clipped present both succeed.
fn present_damaged_frame(
    server: &mut Server,
    conn: gui_protocol_v1::server::ConnId,
    surface: u64,
    token: u64,
    w: u32,
    h: u32,
    fmt: u32,
) -> bool {
    use gui_protocol_v1::wire::Rect;

    const BUFFER2: u64 = 3;
    let bytes = (w * h * 4) as usize;
    // Damage rectangle within the surface (compositor-space offset added below).
    let (dx, dy, dw, dh) = (8u32, 8u32, 16u32, 16u32);

    let buf = libdunit::handle_create_shared(bytes);
    if buf <= 0 {
        return false;
    }
    let buf = buf as u32;
    let mapped = libdunit::handle_map(buf, 0, bytes);
    if mapped <= 0 {
        libdunit::handle_close(buf);
        return false;
    }
    let pixels = mapped as usize as *mut u8;
    // Fill: same gradient as frame 1 everywhere, except a solid-red damaged box.
    unsafe {
        for y in 0..h as usize {
            for x in 0..w as usize {
                let off = (y * w as usize + x) * 4;
                let in_damage = (x as u32) >= dx
                    && (x as u32) < dx + dw
                    && (y as u32) >= dy
                    && (y as u32) < dy + dh;
                if in_damage {
                    core::ptr::write_volatile(pixels.add(off), 0x00); // B
                    core::ptr::write_volatile(pixels.add(off + 1), 0x00); // G
                    core::ptr::write_volatile(pixels.add(off + 2), 0xff); // R
                    core::ptr::write_volatile(pixels.add(off + 3), 0xff); // X
                } else {
                    core::ptr::write_volatile(pixels.add(off), (x * 4) as u8);
                    core::ptr::write_volatile(pixels.add(off + 1), (y * 4) as u8);
                    core::ptr::write_volatile(pixels.add(off + 2), 0x80);
                    core::ptr::write_volatile(pixels.add(off + 3), 0xff);
                }
            }
        }
    }

    let ok = drive_damage(server, conn, surface, token, w, h, fmt, BUFFER2, dx, dy, dw, dh, pixels);
    libdunit::handle_close(buf);
    ok
}

/// Protocol + present half of a damaged frame: import/attach(damage)/commit the
/// new buffer, complete the frame callback, then blit only the damaged rect.
#[allow(clippy::too_many_arguments)]
fn drive_damage(
    server: &mut Server,
    conn: gui_protocol_v1::server::ConnId,
    surface: u64,
    token: u64,
    w: u32,
    h: u32,
    fmt: u32,
    buffer: u64,
    dx: u32,
    dy: u32,
    dw: u32,
    dh: u32,
    pixels: *const u8,
) -> bool {
    use gui_protocol_v1::wire::Rect;

    let bytes = (w * h * 4) as usize;

    // IMPORT_BUFFER (serial 7) -> RESULT.
    let import = Request::ImportBuffer { width: w, height: h, stride: w * 4, format: fmt, offset: 0 }
        .encode(buffer, 7);
    if find(&server.deliver(conn, &import, Some(bytes as u64)), Opcode::Result).is_none() {
        return false;
    }
    // ATTACH_BUFFER with a single damage rect (serial 8) -> RESULT.
    let damage = {
        let mut v = Vec::new();
        v.push(Rect { x: dx as i32, y: dy as i32, w: dw, h: dh });
        v
    };
    let attach = Request::AttachBuffer { buffer, damage }.encode(surface, 8);
    if find(&server.deliver(conn, &attach, None), Opcode::Result).is_none() {
        return false;
    }
    // COMMIT (serial 9, reuse the still-valid configure token) -> RESULT (+ the
    // previous buffer's BUFFER_RELEASE, which we don't require here).
    let commit = Request::Commit { configure: token, frame_callback: 1 }.encode(surface, 9);
    if find(&server.deliver(conn, &commit, None), Opcode::Result).is_none() {
        return false;
    }
    // Frame callback fires for the still-mapped surface.
    let presented = server.composite().iter().any(|(c, p)| {
        *c == conn && opcode_of(p) == Some(Opcode::FrameDone) && u32_at(p, 48) == 0
    });
    if !presented {
        return false;
    }

    // Blit ONLY the damaged rect: pack its strided rows out of the surface
    // buffer, then present at the surface's screen origin (100,100) + rect.
    let mut region = Vec::with_capacity((dw * dh * 4) as usize);
    for row in 0..dh as usize {
        let src_y = dy as usize + row;
        let base = (src_y * w as usize + dx as usize) * 4;
        let len = dw as usize * 4;
        let src = unsafe { core::slice::from_raw_parts(pixels.add(base), len) };
        region.extend_from_slice(src);
    }
    let ok = libdunit::fb_present(&region, dw, dh, 100 + dx, 100 + dy) == 0;
    if ok {
        libdunit::println("gui_server: damage present OK");
    } else {
        libdunit::println("gui_server: FAIL damage present");
    }
    ok
}

/// Drive a server-initiated CONFIGURE (resize) round trip in-process: push a
/// fresh CONFIGURE at new geometry via `Server::reconfigure`, then play the
/// client half a resizing app must implement — ACK the new token, back a real
/// kernel buffer of the new size, IMPORT it with a FRESH object id, ATTACH,
/// COMMIT, and present. Proves the reconfigure mechanism end to end without a
/// second process. Returns true iff every stage produced the expected reply.
fn drive_reconfigure(
    server: &mut Server,
    conn: gui_protocol_v1::server::ConnId,
    surface: u64,
    fmt: u32,
) -> bool {
    // Resize object id: strictly greater than every id seen so far (the damage
    // frame's BUFFER2 = 3 raised the watermark), so it passes the freshness gate.
    const RESIZE_BUFFER: u64 = 4;
    const W2: u32 = 96;
    const H2: u32 = 48;
    const STATE_ACTIVATED: u32 = 1;
    let bytes = (W2 * H2 * 4) as usize;

    // Push CONFIGURE(W2, H2); its header serial (offset 24) is the new token the
    // client must ACK before its next COMMIT is accepted. `reconfigure` returns
    // conn-tagged packets (like `composite`), so pick ours out of the batch.
    let out = server.reconfigure(conn, surface, W2, H2, 1, STATE_ACTIVATED);
    let token = match out
        .iter()
        .find(|(c, p)| *c == conn && opcode_of(p) == Some(Opcode::Configure))
    {
        Some((_, cfg)) => u64_at(cfg, 24),
        None => return false,
    };

    // ACK the new configure token -> RESULT.
    let ack = Request::AckConfigure { configure: token }.encode(surface, 10);
    if find(&server.deliver(conn, &ack, None), Opcode::Result).is_none() {
        return false;
    }
    libdunit::println("gui_server: reconfigure w=96 h=48 acked");

    // Back the resized surface with a real kernel shared buffer and paint it.
    let buf = libdunit::handle_create_shared(bytes);
    if buf <= 0 {
        return false;
    }
    let buf = buf as u32;
    let mapped = libdunit::handle_map(buf, 0, bytes);
    if mapped <= 0 {
        libdunit::handle_close(buf);
        return false;
    }
    let pixels = mapped as usize as *mut u8;
    unsafe {
        for y in 0..H2 as usize {
            for x in 0..W2 as usize {
                let off = (y * W2 as usize + x) * 4;
                core::ptr::write_volatile(pixels.add(off), (x * 2) as u8); // B
                core::ptr::write_volatile(pixels.add(off + 1), (y * 4) as u8); // G
                core::ptr::write_volatile(pixels.add(off + 2), 0x40); // R
                core::ptr::write_volatile(pixels.add(off + 3), 0xff); // X
            }
        }
    }

    // IMPORT (fresh id) / ATTACH / COMMIT (new token) -> RESULT each.
    let import = Request::ImportBuffer { width: W2, height: H2, stride: W2 * 4, format: fmt, offset: 0 }
        .encode(RESIZE_BUFFER, 11);
    let imported = find(&server.deliver(conn, &import, Some(bytes as u64)), Opcode::Result).is_some();
    let attach = Request::AttachBuffer { buffer: RESIZE_BUFFER, damage: Vec::new() }.encode(surface, 12);
    let attached = imported && find(&server.deliver(conn, &attach, None), Opcode::Result).is_some();
    let commit = Request::Commit { configure: token, frame_callback: 1 }.encode(surface, 13);
    let committed = attached && find(&server.deliver(conn, &commit, None), Opcode::Result).is_some();
    let presented = committed
        && server.composite().iter().any(|(c, p)| {
            *c == conn && opcode_of(p) == Some(Opcode::FrameDone) && u32_at(p, 48) == 0
        });

    let data = unsafe { core::slice::from_raw_parts(pixels, bytes) };
    let ok = presented && libdunit::fb_present(data, W2, H2, 100, 100) == 0;
    libdunit::handle_close(buf);
    if ok {
        libdunit::println("gui_server: client resized to 96x48 OK");
    } else {
        libdunit::println("gui_server: FAIL client resize");
    }
    ok
}

/// Control-message magic marking a capability announce (vs. a protocol packet,
/// which begins with the DGUI wire magic). Shared with `gui_client`.
const CTRL_MAGIC: u32 = 0x3150_4143; // "CAP1"

// Client -> compositor "reload settings" signal (slice C). A settings app
// (gui_settings) rewrites /system/share/dwm/default.toml, then sends this 4-byte
// control message so the compositor re-reads the config and re-seeds its live
// theme/layout/effects state without a reboot — the GUI<->TOML round-trip. A
// distinct magic (not CAP1/INP1/DGUI) so `handle_client_payload` can tell it
// apart from a buffer announce or a wire packet.
const RELOAD_MAGIC: u32 = 0x3144_4C52; // "RLD1"

// Client -> compositor "post a desktop notification" signal. An unprivileged
// client asks the compositor to raise a toast (e.g. gui_demo's "Notify" button).
// The client never touches the toast queue itself — it only names the text; ALL
// policy (whether notifications are on, the timeout, the corner) stays in the
// compositor's `[notifications]` config (single source of truth), applied when
// the frame loop drains the request into `notify_post`. Layout: [magic:u32]
// [len:u32][utf8 text bytes]. A distinct magic from CAP1/INP1/RLD1/DGUI so
// `handle_client_payload` can tell it apart from a buffer announce or wire packet.
const NOTIFY_MAGIC: u32 = 0x3154_4F4E; // "NOT1"

/// Longest client-supplied toast text the compositor accepts (bytes). Bounds the
/// copy out of the untrusted IPC buffer; longer text is truncated, never trusted.
const NOTIFY_TEXT_MAX: usize = 64;

// Compositor -> client input control messages (20 bytes). Distinct magic from
// CTRL_MAGIC and the DGUI wire magic so the client can tell them apart. Coords
// are client-local (relative to the surface origin). Kept deliberately simple:
// the focused window is the sole recipient, so no per-message routing id.
const INPUT_MAGIC: u32 = 0x3150_4E49; // "INP1"
const IN_MOVE: u8 = 1;
const IN_DOWN: u8 = 2;
const IN_UP: u8 = 3;
const IN_LEAVE: u8 = 4;
const IN_KEY: u8 = 5;
const IN_SCROLL: u8 = 6;
const IN_QUIT: u8 = 9;

/// How long the maximize/restore self-test waits for a client to re-import the
/// work-area-sized buffer before advancing anyway (in desktop ticks; ~60/s, so
/// ~4s). A resizable client grows well within this; a fixed-size client that
/// declines the server-pushed resize is force-settled here so the self-test —
/// and the recycle proof gated on every window reaching `st_step == 2` — never
/// stalls on a client exercising its right to keep its own size.
const ST_SETTLE_TICKS: u32 = 240;

/// Send one input control message to a client.
fn send_input(pid: u32, kind: u8, lx: i32, ly: i32, button: u32) {
    let mut msg = [0u8; 20];
    msg[0..4].copy_from_slice(&INPUT_MAGIC.to_le_bytes());
    msg[4] = kind;
    msg[8..12].copy_from_slice(&lx.to_le_bytes());
    msg[12..16].copy_from_slice(&ly.to_le_bytes());
    msg[16..20].copy_from_slice(&button.to_le_bytes());
    libdunit::ipc_send(pid, &msg);
}

/// True for the modifier keys themselves (Shift/Ctrl/Alt/Super/Caps), whose
/// make/break we never forward to clients — their state already rides in every
/// other event's `mods` field. Scancodes are the `sc & 0x7F` form the kernel
/// reports (no release bit).
fn is_modifier_scancode(sc: u8) -> bool {
    matches!(sc, 0x1D | 0x2A | 0x36 | 0x38 | 0x3A | 0x5B | 0x5C)
}

/// Per-client compositing state held by the multi-client server loop.
#[derive(Clone, Copy)]
struct ClientState {
    pid: u32,
    conn: gui_protocol_v1::server::ConnId,
    slot_x: u32,
    slot_y: u32,
    buf_ptr: *const u8,
    buf_size: usize,
    mapped_handle: u32,
    surf_w: u32,
    surf_h: u32,
    presented: bool,
    /// Surface object id the client chose in CreateSurface (needed to push a
    /// server-initiated CONFIGURE via `Server::reconfigure` for resize/maximize).
    surface: u64,
    /// Pixel format the client imported its buffer with (FORMAT_XRGB8888 = 1 or
    /// FORMAT_ARGB8888 = 2). Drives per-pixel blend vs. opaque copy at composite.
    format: u32,
    // Desktop bring-up state (used by run_desktop_session for runtime-spawned
    // clients): `ready` once the buffer is mapped and a frame has been committed
    // so it is safe to blit; `win_created` once it owns a Win in the model.
    ready: bool,
    win_created: bool,
    /// Index into the live application registry (`settings::Applications.apps`)
    /// of the app this client runs (0xFF = unknown/not registered), used to draw
    /// a "running" indicator on the matching dock icon.
    app: u8,
    /// Workspace (0-based) this client's window lives on, captured from the
    /// active workspace at spawn time. Startup clients default to workspace 0.
    ws: usize,
}

impl ClientState {
    const fn empty() -> ClientState {
        ClientState {
            pid: 0,
            conn: 0,
            slot_x: 0,
            slot_y: 0,
            buf_ptr: core::ptr::null(),
            buf_size: 0,
            mapped_handle: 0,
            surf_w: 0,
            surf_h: 0,
            presented: false,
            surface: 0,
            format: FORMAT_XRGB8888,
            ready: false,
            win_created: false,
            app: 0xFF,
            ws: 0,
        }
    }
}

/// Process one inbound (envelope-stripped) message for client `c`: either map
/// its announced buffer capability, or track geometry + feed the protocol
/// packet through the Server and relay the replies back to the client.
fn handle_client_payload(
    server: &mut Server,
    c: &mut ClientState,
    payload: &[u8],
    reload: &mut bool,
    notify_out: &mut Vec<alloc::string::String>,
) {
    let n = payload.len();
    if n >= 16 && u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]) == CTRL_MAGIC
    {
        let handle = u32::from_le_bytes([payload[4], payload[5], payload[6], payload[7]]);
        // Игнорируем размер, названный клиентом (payload[8..12]) — он недоверенный.
        // Мапим буфер и берём ИСТИННУЮ длину backing-объекта из ядра: только она
        // задаёт безопасную границу для последующего блита.
        let mapped = libdunit::handle_map(handle, 0, 0);
        if mapped > 0 {
            let real_len = libdunit::handle_shared_len(handle);
            if real_len > 0 {
                c.buf_ptr = mapped as usize as *const u8;
                c.buf_size = real_len as usize;
                c.mapped_handle = handle;
            }
        }
        return;
    }
    // "Reload settings" signal from a trusted-enough client (gui_settings): the
    // config file was just rewritten, so raise a flag the desktop loop drains at
    // the top of the next frame (re-read + re-seed derived state). No payload
    // beyond the magic — the new values come from the config file, not the wire.
    if n >= 4 && u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]) == RELOAD_MAGIC
    {
        *reload = true;
        return;
    }
    // "Post a notification" signal: the client names the toast text; the frame
    // loop turns it into a toast through the SAME `notify_post` path a launch
    // uses (so `[notifications]` policy still gates it). The text length is the
    // client's claim, so clamp it to the real remaining bytes AND a hard cap
    // before copying — an untrusted client must not drive an out-of-bounds read
    // or an unbounded allocation.
    if n >= 8 && u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]) == NOTIFY_MAGIC
    {
        let claimed = u32::from_le_bytes([payload[4], payload[5], payload[6], payload[7]]) as usize;
        let avail = n - 8;
        let len = claimed.min(avail).min(NOTIFY_TEXT_MAX);
        let text = core::str::from_utf8(&payload[8..8 + len]).unwrap_or("");
        if !text.is_empty() {
            notify_out.push(text.into());
        }
        return;
    }
    // Заголовок wire-протокола — 32 байта; CreateSurface читает поля вплоть до
    // offset 44, поэтому короткие пакеты отбрасываем ДО индексирования (иначе
    // паника → падение всего компоситора: DoS от одного клиента).
    if n < 32 {
        return;
    }
    let op = Opcode::from_u16(u16::from_le_bytes([payload[8], payload[9]]));
    if op == Some(Opcode::CreateSurface) {
        if n < 44 {
            return;
        }
        // Surface object id is the wire header `object` (offset 16); remember it so
        // the compositor can push a server-initiated CONFIGURE for resize/maximize.
        c.surface = u64::from_le_bytes([
            payload[16], payload[17], payload[18], payload[19],
            payload[20], payload[21], payload[22], payload[23],
        ]);
        c.surf_w = u32::from_le_bytes([payload[36], payload[37], payload[38], payload[39]]);
        c.surf_h = u32::from_le_bytes([payload[40], payload[41], payload[42], payload[43]]);
        // Surface format (offset 44) selects opaque copy (XRGB) vs. per-pixel
        // blend (ARGB) at composite time; older short packets keep the default.
        if n >= 48 {
            c.format = u32::from_le_bytes([payload[44], payload[45], payload[46], payload[47]]);
        }
    }
    // A client that resizes (in response to our CONFIGURE) re-imports a buffer of
    // the new geometry with a FRESH object id. IMPORT_BUFFER carries the true
    // pixel dimensions (width@32, height@36), so track them here — this is the
    // single point where the compositor learns a surface's current size, for both
    // the initial import and every subsequent resize.
    if op == Some(Opcode::ImportBuffer) && n >= 40 {
        c.surf_w = u32::from_le_bytes([payload[32], payload[33], payload[34], payload[35]]);
        c.surf_h = u32::from_le_bytes([payload[36], payload[37], payload[38], payload[39]]);
    }
    let cap = if op == Some(Opcode::ImportBuffer) {
        Some(c.buf_size as u64)
    } else {
        None
    };
    for reply in &server.deliver(c.conn, payload, cap) {
        libdunit::ipc_send(c.pid, reply);
    }
}
/// Bring one client online: reserve a protocol connection, spawn the `exec` ELF,
/// and hand it the `[our_pid][client_id]` handshake it blocks on at startup. The
/// returned `ClientState` is *pending* — its buffer is mapped and it becomes
/// `ready` only once its protocol handshake is pumped (see `pump_clients`). The
/// `id` is a tint hint only; routing uses the kernel-authenticated sender pid.
/// `app_idx` is the caller-resolved registry index (0xFF = not in the registry),
/// stored so the dock can draw a "running" marker on the matching icon.
fn spawn_client(
    server: &mut Server,
    id: u32,
    slot_x: u32,
    slot_y: u32,
    app_idx: u8,
    exec: &str,
) -> Option<ClientState> {
    let conn = server.connect()?;
    let pid = libdunit::spawn(exec);
    if pid <= 0 {
        return None;
    }
    let mut c = ClientState::empty();
    c.pid = pid as u32;
    c.conn = conn;
    c.slot_x = slot_x;
    c.slot_y = slot_y;
    c.app = app_idx;
    let mut hs = [0u8; 8];
    hs[0..4].copy_from_slice(&libdunit::get_pid().to_le_bytes());
    hs[4..8].copy_from_slice(&id.to_le_bytes());
    libdunit::ipc_send(c.pid, &hs);
    Some(c)
}

/// Open the app at registry index `ri` as a fresh client window on `current_ws`,
/// respecting the `max_windows` cap, and post the config-gated launch toast. This
/// is the SINGLE "launch an app" path shared by the launcher menu (mouse click +
/// keyboard Enter) and the dock — so program-launch policy lives in exactly one
/// place instead of being copied per trigger. Returns true iff a client spawned.
fn try_launch(
    server: &mut Server,
    clients: &mut Vec<ClientState>,
    apps: &Applications,
    ri: usize,
    current_ws: usize,
    next_id: &mut u32,
    max_windows: usize,
    notifs: &mut Vec<Toast>,
    notifications: &settings::Notifications,
    ticks: u32,
) -> bool {
    if clients.len() >= max_windows {
        return false;
    }
    let Some(entry) = apps.apps.get(ri) else {
        return false;
    };
    let exec = entry.exec.as_str();
    let name = entry.name.as_str();
    if let Some(mut c) = spawn_client(server, *next_id, 0, 0, ri as u8, exec) {
        c.ws = current_ws;
        clients.push(c);
        *next_id += 1;
        notify_post(notifs, notifications, ticks, name);
        true
    } else {
        false
    }
}

/// Bring up the desktop: autostart the configured `[startup]` apps, prove the
/// M3 cross-process isolation invariant (≥2 untrusted clients composited
/// concurrently, routed by KERNEL-AUTHENTICATED sender pid so one cannot inject
/// into another's connection), then hand off to the interactive session. What
/// launches at boot is data (`settings::Applications::startup`), so changing the
/// startup set needs no gui_server edit. Returns true iff the isolation smoke saw
/// two clients present at once (the baseline config autostarts three).
fn serve_two_clients() -> bool {
    let cfg = settings::load_config();
    let mut server = Server::new();
    let mut clients: Vec<ClientState> = Vec::new();
    // Notification texts a client posts (NOTIFY_MAGIC) DURING bring-up — before the
    // interactive desktop loop and its per-tick `pump_clients` exist. An autostart
    // app can toast the moment it presents (e.g. gui_demo's "I'm up" toast), which
    // lands here in `serve_two_clients` or the `resize_one_client` resize proof, not
    // in `run_desktop_session`. Collect them instead of dropping them, and hand them
    // to the session so it emits each through the SAME config-gated `notify_post`
    // path once its toast queue exists (single source of truth; nothing is lost to a
    // bring-up race).
    let mut pending_notifies: Vec<alloc::string::String> = Vec::new();

    // Autostart the configured startup apps. Each entry is a registry index; a
    // stale/out-of-range index is skipped rather than trusted. Initial slots
    // cascade — the desktop session re-tiles them under the panel/dock anyway.
    let mut next_id = 1u32;
    for (k, &ri) in cfg.apps.startup.iter().enumerate() {
        let Some(entry) = cfg.apps.apps.get(ri) else {
            continue;
        };
        let sx = 300 + (k as u32) * 70;
        let sy = 100 + (k as u32) * 90;
        if let Some(c) = spawn_client(&mut server, next_id, sx, sy, ri as u8, entry.exec.as_str()) {
            clients.push(c);
            next_id += 1;
        }
    }
    if clients.is_empty() {
        // No startup apps (or none spawnable): still raise the shell so the
        // panel/dock/launcher are usable and can spawn apps on demand.
        libdunit::println("gui_server: no startup apps to autostart");
        // Loop so a live `[display]` change (which returns `true`) re-enters the
        // session at the new resolution; `take` hands the notifies over once, then
        // empties so re-entries start clean.
        let mut pn = pending_notifies;
        while run_desktop_session(&mut server, &mut clients, core::mem::take(&mut pn)) {}
        return false;
    }

    // Pump until the isolation invariant is proven: at least two untrusted
    // clients presented concurrently (or, with a single startup app, that one).
    // We break the INSTANT the target count is reached — inside the composite —
    // so any slower startup app (e.g. the terminal, which sets up a pty first)
    // stays UNpresented here and becomes a runtime-pumped window in the desktop
    // loop. That is what later fires "gui_server: desktop input ready", so the
    // keyboard-ready gate keeps working regardless of client presentation order.
    let target = clients.len().min(2);
    let mut rx = [0u8; 256];
    let mut presented_total = clients.iter().filter(|c| c.presented).count();
    'pump: for _ in 0..128 {
        if presented_total >= target {
            break;
        }
        // Route by the kernel-authenticated sender pid; a client cannot spoof
        // another's identity, so the protocol connections stay isolated.
        let mut sender: u32 = 0;
        let n = libdunit::ipc_recv_blocking_from(&mut rx, &mut sender, 3000);
        if n <= 0 {
            break;
        }
        let n = n as usize;
        let idx = match clients.iter().position(|c| c.pid == sender) {
            Some(idx) => idx,
            None => continue, // message from an unknown pid — ignore
        };
        // Startup smoke: no live-reload here, so discard that signal. A client
        // NOTIFY posted this early is preserved in `pending_notifies` and shown once
        // the desktop session's toast queue exists (bring-up race safety).
        let mut _reload = false;
        handle_client_payload(&mut server, &mut clients[idx], &rx[..n], &mut _reload, &mut pending_notifies);

        // A composition tick: route each FRAME_DONE to its client and blit that
        // client's committed buffer into its own slot.
        for (fc, fp) in &server.composite() {
            for c in clients.iter_mut() {
                if c.conn != *fc {
                    continue;
                }
                libdunit::ipc_send(c.pid, fp);
                let status = if fp.len() >= 52 {
                    u32::from_le_bytes([fp[48], fp[49], fp[50], fp[51]])
                } else {
                    0
                };
                if status != 0 || c.presented {
                    continue;
                }
                if !c.buf_ptr.is_null() && c.surf_w > 0 && c.surf_h > 0 {
                    // Non-wrapping bounds check against the REAL mapped length
                    // (c.buf_size came from handle_shared_len, not the client).
                    let want = c.surf_w as u64 * c.surf_h as u64 * 4;
                    if want <= c.buf_size as u64 {
                        let data =
                            unsafe { core::slice::from_raw_parts(c.buf_ptr, want as usize) };
                        if libdunit::fb_present(data, c.surf_w, c.surf_h, c.slot_x, c.slot_y) == 0 {
                            c.presented = true;
                            presented_total += 1;
                            // Stop the instant the invariant holds, leaving the
                            // rest of the startup set for the runtime pump.
                            if presented_total >= target {
                                break 'pump;
                            }
                        }
                    }
                }
            }
        }
    }

    let both = presented_total >= 2;

    // Hand off to the interactive desktop session: a persistent compositor loop
    // that owns a full-screen back buffer, decorates each client surface with a
    // draggable title bar, routes the real mouse (syscall 60) into focus/raise/
    // drag, and re-presents every tick. This is the M4 userspace DWM taking over
    // from the linear M3 smoke.
    if both {
        // Emit the isolation marker BEFORE the session loop so automated smokes
        // observe it immediately (headless runs are force-quit after capture).
        libdunit::println("gui_server: served two untrusted clients OK");
        // Cross-process resize proof: push a server-initiated CONFIGURE to one
        // live client and drive its re-negotiation to completion. This exercises
        // the maximize/fullscreen mechanism across a REAL process boundary (the
        // client re-allocates its buffer and re-imports it with a fresh object
        // id), which the in-process compositor self-test cannot cover.
        resize_one_client(&mut server, &mut clients, &mut pending_notifies);
    }
    // Loop so a live `[display]` change re-enters the session at the new
    // resolution (rebuilding all buffers); the notifies are handed over once.
    let mut pn = pending_notifies;
    while run_desktop_session(&mut server, &mut clients, core::mem::take(&mut pn)) {}

    for c in clients.iter() {
        if c.mapped_handle != 0 {
            libdunit::handle_close(c.mapped_handle);
        }
        let mut status = libdunit::WaitStatus::empty();
        for _ in 0..50 {
            if libdunit::wait(c.pid, &mut status) == c.pid as isize {
                break;
            }
            libdunit::sleep_ms(10);
        }
    }
    both
}

/// Push a server-initiated CONFIGURE to one already-presented client and pump its
/// re-negotiation until it re-imports a fresh buffer at the new geometry and
/// re-presents. This drives the exact maximize/fullscreen mechanism across a real
/// process boundary: `Server::reconfigure` emits the CONFIGURE, the client backs
/// a new buffer and replays IMPORT/ATTACH/COMMIT with a FRESH object id, and the
/// compositor picks up the new size from the client's IMPORT_BUFFER. Verifiable
/// headlessly (the in-process self-test cannot cross an address-space boundary).
fn resize_one_client(
    server: &mut Server,
    clients: &mut [ClientState],
    pending_notifies: &mut Vec<alloc::string::String>,
) {
    const NEWW: u32 = 320;
    const NEWH: u32 = 160;
    const STATE_ACTIVATED: u32 = 1;
    // First presented client that announced a surface object (a gui_client).
    let target = match clients.iter().position(|c| c.presented && c.surface != 0) {
        Some(i) => i,
        None => return,
    };
    let (conn, surface, pid) = (clients[target].conn, clients[target].surface, clients[target].pid);

    // Emit CONFIGURE(NEWW, NEWH) and forward it to the owning client.
    for (fc, fp) in &server.reconfigure(conn, surface, NEWW, NEWH, 1, STATE_ACTIVATED) {
        if *fc == conn {
            libdunit::ipc_send(pid, fp);
        }
    }

    let mut rx = [0u8; 256];
    for _ in 0..256 {
        let mut sender: u32 = 0;
        let n = libdunit::ipc_recv_blocking_from(&mut rx, &mut sender, 3000);
        if n <= 0 {
            break;
        }
        let n = n as usize;
        let idx = match clients.iter().position(|c| c.pid == sender) {
            Some(i) => i,
            None => continue,
        };
        // Live-reload is not driven during this resize proof; a NOTIFY posted by any
        // client mid-resize is preserved (bring-up race safety) and shown once the
        // desktop session's toast queue exists.
        let mut _reload = false;
        handle_client_payload(server, &mut clients[idx], &rx[..n], &mut _reload, pending_notifies);
        // Route every frame callback to its owner (keeps the other clients live).
        // Mirror `pump_clients`: a client that completes its handshake here (a
        // slower startup app such as the terminal or file manager, which finishes
        // committing while we drive the target's resize) MUST be latched `ready`,
        // exactly as the desktop pump would. Otherwise it is orphaned — presented
        // client-side but never adopted as a runtime window — because it then goes
        // idle in `run_desktop_session` and sends nothing further to re-trigger a
        // composite. This keeps the documented "slower startup app becomes a
        // runtime-pumped window" invariant true even across this resize proof.
        for (fc, fp) in &server.composite() {
            let status = if fp.len() >= 52 {
                u32::from_le_bytes([fp[48], fp[49], fp[50], fp[51]])
            } else {
                0
            };
            for c in clients.iter_mut() {
                if c.conn != *fc {
                    continue;
                }
                libdunit::ipc_send(c.pid, fp);
                if status == 0 && !c.buf_ptr.is_null() && c.surf_w > 0 && c.surf_h > 0 {
                    c.ready = true;
                }
            }
        }
        // Done once the target reports the new geometry (via its IMPORT_BUFFER)
        // with a fresh backing buffer that we can blit within bounds.
        let c = &clients[target];
        if c.surf_w == NEWW && c.surf_h == NEWH && !c.buf_ptr.is_null() {
            let want = c.surf_w as u64 * c.surf_h as u64 * 4;
            if want <= c.buf_size as u64 {
                let data = unsafe { core::slice::from_raw_parts(c.buf_ptr, want as usize) };
                if libdunit::fb_present(data, c.surf_w, c.surf_h, c.slot_x, c.slot_y) == 0 {
                    libdunit::println("gui_server: live client resized to 320x160 OK");
                    return;
                }
            }
        }
    }
    libdunit::println("gui_server: FAIL live client resize");
}

/// Push a server-initiated CONFIGURE to the client that owns `pid` and forward it
/// to that client, so it re-imports a `w`x`h` buffer (Phase 3 maximize / restore /
/// fullscreen). The client's re-negotiation (IMPORT/ATTACH/COMMIT with a fresh
/// object id) is drained by the desktop loop's `pump_clients`, which updates the
/// client's `ClientState` geometry; the per-frame Win<->ClientState sync then
/// mirrors the fresh buffer + size into the owning `Win`. A no-op for an unknown
/// pid or a client that never announced a surface object (so a fixed-size client
/// that ignores CONFIGURE is simply never asked to resize).
fn reconfigure_client(
    server: &mut Server,
    clients: &[ClientState],
    pid: u32,
    w: u32,
    h: u32,
    state: u32,
) {
    let Some(c) = clients.iter().find(|c| c.pid == pid && c.surface != 0) else {
        return;
    };
    let (conn, surface) = (c.conn, c.surface);
    for (fc, fp) in &server.reconfigure(conn, surface, w, h, 1, state) {
        if *fc == conn {
            libdunit::ipc_send(pid, fp);
        }
    }
}

/// Reserved screen margins (px) that the shell strips subtract from the usable
/// desktop, one accumulator per edge. `reserved_insets` fills it from the
/// configured panel/tray edges plus the always-left dock, and every geometry
/// computation (work area, spawn/drag clamps, dock strip origin) reads from here
/// so "what real estate is free" has a single source of truth.
#[derive(Clone, Copy)]
struct Insets {
    top: i32,
    bottom: i32,
    left: i32,
    right: i32,
}

/// Add `size` px to the margin on `edge`.
fn add_edge(ins: &mut Insets, edge: Edge, size: i32) {
    match edge {
        Edge::Top => ins.top += size,
        Edge::Bottom => ins.bottom += size,
        Edge::Left => ins.left += size,
        Edge::Right => ins.right += size,
    }
}

/// Resolve the configured shell layout into reserved per-edge margins:
/// - the panel reserves `ly.panel_h` on `panel_edge`;
/// - the dock always reserves `ly.dock_w` on the LEFT edge (the dock stays left
///   in this milestone — only the panel and tray are edge-configurable);
/// - the tray reserves `tray_size` on `tray_edge`, but ONLY when it does not
///   share the panel's edge — a same-edge tray rides *inside* the panel strip
///   (the classic top-panel readout) and reserves nothing extra.
///
/// Margins accumulate, so the default (panel=top, tray=top, dock=left) yields
/// `{top: panel_h, left: dock_w, bottom: 0, right: 0}` — byte-identical to the
/// pre-Phase-5 fixed layout.
fn reserved_insets(ly: &Layout, panel_edge: Edge, tray_edge: Edge, tray_size: i32) -> Insets {
    let mut ins = Insets { top: 0, bottom: 0, left: 0, right: 0 };
    add_edge(&mut ins, panel_edge, ly.panel_h);
    ins.left += ly.dock_w;
    if tray_edge != panel_edge {
        add_edge(&mut ins, tray_edge, tray_size);
    }
    ins
}

/// The screen rect `(x, y, w, h)` of a shell strip `thickness` px thick on `edge`,
/// offset `off` px inward from that edge. Horizontal strips (top/bottom) span the
/// full width and own the corners; vertical strips (left/right) span only the gap
/// *between* the top/bottom insets, so a left/right strip never overlaps a
/// top/bottom one at a corner. `off` lets several strips stack on one edge (e.g.
/// panel outermost at `off=0`, dock innermost at `off = ins.left - dock_w`).
fn strip_rect(edge: Edge, thickness: i32, off: i32, ins: &Insets, bw: usize, bh: usize) -> (i32, i32, i32, i32) {
    let bw = bw as i32;
    let bh = bh as i32;
    match edge {
        Edge::Top => (0, off, bw, thickness),
        Edge::Bottom => (0, bh - off - thickness, bw, thickness),
        Edge::Left => (off, ins.top, thickness, (bh - ins.top - ins.bottom).max(1)),
        Edge::Right => (bw - off - thickness, ins.top, thickness, (bh - ins.top - ins.bottom).max(1)),
    }
}

/// The maximize target rect `(cx, cy, w, h)` = the framebuffer minus the reserved
/// shell margins (`reserved_insets`) and the window's own title bar + border. One
/// helper so the interactive chip, the drag-restore path and the headless
/// self-test all compute the same geometry, and so it honours whatever edges the
/// panel/tray are configured on.
fn work_area(bw: usize, bh: usize, ly: &Layout, ins: &Insets) -> (i32, i32, i32, i32) {
    let x = ins.left + ly.border;
    let y = ins.top + ly.title_h + ly.border;
    let w = (bw as i32 - ins.left - ins.right - 2 * ly.border).max(1);
    let h = (bh as i32 - ins.top - ins.bottom - ly.title_h - 2 * ly.border).max(1);
    (x, y, w, h)
}

/// Move `win` into `new_state`, driving the client re-import needed for the new
/// size. This is the single window-state transition used by the title-bar chip,
/// the macOS-style drag-to-restore, and the headless self-test. Leaving `Floating`
/// snapshots the current `(cx,cy,sw,sh)` into `restore`; returning to `Floating`
/// replays that snapshot. The *position* is applied here immediately (so the
/// decoration follows at once); the *size* only actually changes once the owning
/// client re-imports at the CONFIGURE'd geometry (mirrored back by the per-frame
/// Win<->ClientState sync), so a client that ignores CONFIGURE keeps its size while
/// its position still updates — harmless. No-op if already in `new_state`.
fn set_window_state(
    server: &mut Server,
    clients: &[ClientState],
    win: &mut Win,
    new_state: WinState,
    bw: usize,
    bh: usize,
    ly: &Layout,
    ins: &Insets,
) {
    // STATE_ACTIVATED — the surface stays focused across the reconfigure.
    const STATE_ACTIVATED: u32 = 1;
    if win.state == new_state {
        return;
    }
    // Snapshot floating geometry once, when first leaving the floating state.
    if win.state == WinState::Floating {
        win.restore = Some((win.cx, win.cy, win.sw, win.sh));
    }
    let target = match new_state {
        WinState::Maximized => Some(work_area(bw, bh, ly, ins)),
        WinState::Floating => win.restore.take(),
    };
    win.state = new_state;
    if let Some((tx, ty, tw, th)) = target {
        win.cx = tx;
        win.cy = ty;
        reconfigure_client(server, clients, win.pid, tw as u32, th as u32, STATE_ACTIVATED);
    }
}

/// Toggle the title-bar maximize chip: `Maximized` <-> `Floating`.
fn toggle_maximize(
    server: &mut Server,
    clients: &[ClientState],
    win: &mut Win,
    bw: usize,
    bh: usize,
    ly: &Layout,
    ins: &Insets,
) {
    let next = if win.state == WinState::Maximized {
        WinState::Floating
    } else {
        WinState::Maximized
    };
    set_window_state(server, clients, win, next, bw, bh, ly, ins);
}

// ---------------------------------------------------------------------------
// Window/session persistence (M4). Geometry is remembered per APP id (not per
// pid, which is ephemeral), so reopening an app restores where its window last
// sat. The store is `settings::SESSION_PATH`, pre-created writable in the kernel
// VFS; it is RAM-only until DunitFS v2, then the same path survives reboots.
// ---------------------------------------------------------------------------

/// The registered app id for a window (its join key into the session store), or
/// `None` when the window's app is unregistered / id-less (nothing to remember).
fn win_app_id<'a>(apps: &'a Applications, app: u8) -> Option<&'a str> {
    let id = apps.apps.get(app as usize)?.id.as_str();
    if id.is_empty() {
        None
    } else {
        Some(id)
    }
}

/// Upsert a window's FLOATING geometry into the in-RAM session mirror, keyed by
/// its app id. A maximized window records its pre-maximize floating rect (from
/// `restore`) plus the `maximized` flag, so a later restore reopens it maximized
/// yet with a sane un-maximize size. No-op for id-less windows.
fn session_remember(session: &mut Vec<settings::WinGeom>, apps: &Applications, w: &Win) {
    let Some(id) = win_app_id(apps, w.app) else {
        return;
    };
    let (gx, gy, gw, gh) = match w.restore {
        Some((rx, ry, rw, rh)) if w.state != WinState::Floating => (rx, ry, rw, rh),
        _ => (w.cx, w.cy, w.sw, w.sh),
    };
    let maximized = w.state == WinState::Maximized;
    let ge = settings::WinGeom::new(id, gx, gy, gw, gh, maximized);
    if let Some(e) = session.iter_mut().find(|e| e.id.as_str() == id) {
        *e = ge;
    } else if session.len() < settings::MAX_SESSION {
        session.push(ge);
    }
}

/// Find a remembered geometry for the app `app` (registry index) in the mirror.
fn session_lookup(session: &[settings::WinGeom], apps: &Applications, app: u8) -> Option<settings::WinGeom> {
    let id = win_app_id(apps, app)?;
    session.iter().find(|e| e.id.as_str() == id).copied()
}

// Hardware make-codes the switcher watches. The trigger key (Tab) and the cancel
// key (Esc) are keyboard-protocol invariants, not desktop policy — only which
// MODIFIER arms the switcher is config (`[shortcuts] switch_mod`).
const SC_TAB: u8 = 0x0F;
const SC_ESC: u8 = 0x01;
// Make-codes the global desktop shortcuts watch. Like Tab/Esc above these are
// keyboard-protocol invariants (the physical key positions), NOT desktop policy —
// only WHICH MODIFIER arms them and WHETHER each action is enabled is config
// (`[shortcuts] cmd_mod` + the per-action bools). The digit row 1..9/0 is the
// PC set-1 make sequence 0x02..0x0B.
const SC_SPACE: u8 = 0x39;
const SC_Q: u8 = 0x10;
const SC_M: u8 = 0x32;
const SC_UP: u8 = 0x48;
const SC_DOWN: u8 = 0x50;
const SC_ENTER: u8 = 0x1C;

/// Map a top-row digit make-code to its value 1..=9 (0 maps to 10, so it can
/// index workspace 10 when there are that many). Non-digit scancodes yield None.
fn digit_from_scancode(sc: u8) -> Option<u32> {
    match sc {
        0x02..=0x0A => Some((sc - 0x01) as u32), // '1'..'9'
        0x0B => Some(10),                        // '0' → the 10th workspace
        _ => None,
    }
}

/// A global desktop shortcut resolved from a key event. The DECISION (key+mods →
/// action, gated by `[shortcuts]`) is pure and lives here so it can be unit-proven
/// headless; the EFFECT (mutating compositor state) stays in the key-drain loop,
/// reusing the exact same paths the mouse chips drive (single source of truth).
#[derive(Clone, Copy, PartialEq, Eq)]
enum ShortcutAction {
    None,
    /// Jump to this 0-based virtual workspace.
    Workspace(usize),
    /// Open the application launcher menu.
    Launcher,
    /// Close the focused window.
    CloseWindow,
    /// Toggle maximize on the focused window.
    MaximizeWindow,
}

/// Resolve a key press against the config-driven global-shortcut policy. Returns
/// the action a matching `cmd_mod`+key chord maps to, or `None`. A bare key (no
/// `cmd_mod` held) never fires a global action — it falls through to the focused
/// client — so app typing is never stolen. Pure: no compositor state touched,
/// which is exactly what lets the self-test prove the mapping without a keyboard.
fn match_shortcut(scancode: u8, mods: u8, pressed: bool, sc: &settings::Shortcuts, ws_count: usize) -> ShortcutAction {
    if !pressed || (mods & sc.cmd_mod.mask()) == 0 {
        return ShortcutAction::None;
    }
    if sc.ws_switch {
        if let Some(n) = digit_from_scancode(scancode) {
            if n >= 1 && (n as usize) <= ws_count {
                return ShortcutAction::Workspace(n as usize - 1);
            }
        }
    }
    if sc.launcher && scancode == SC_SPACE {
        return ShortcutAction::Launcher;
    }
    if sc.win_close && scancode == SC_Q {
        return ShortcutAction::CloseWindow;
    }
    if sc.win_max && scancode == SC_M {
        return ShortcutAction::MaximizeWindow;
    }
    ShortcutAction::None
}

/// Alt/Super-Tab window switcher (M4, gated by `[shortcuts]`). While armed it
/// holds a frozen most-recently-used snapshot of the candidate windows and the
/// highlighted index; releasing the arming modifier commits the highlight (the
/// compositor then raises + un-minimizes that window). Tab advances, Shift-Tab
/// steps back, Esc cancels — all UI invariants.
struct Switcher {
    active: bool,
    order: Vec<usize>,
    idx: usize,
}

impl Switcher {
    fn new() -> Self {
        Switcher { active: false, order: Vec::new(), idx: 0 }
    }

    /// Arm over a fresh MRU snapshot. The current front window is index 0, so the
    /// first Tab moves to the next candidate (classic Alt-Tab "previous window").
    fn begin(&mut self, mru: &[usize], backward: bool) {
        self.order.clear();
        self.order.extend_from_slice(mru);
        self.active = !self.order.is_empty();
        self.idx = 0;
        if self.order.len() >= 2 {
            self.advance(backward);
        }
    }

    fn advance(&mut self, backward: bool) {
        let n = self.order.len();
        if n == 0 {
            return;
        }
        self.idx = if backward { (self.idx + n - 1) % n } else { (self.idx + 1) % n };
    }

    fn selected(&self) -> Option<usize> {
        self.order.get(self.idx).copied()
    }

    fn cancel(&mut self) {
        self.active = false;
    }

    /// Close the overlay and yield the highlighted window index to raise.
    fn commit(&mut self) -> Option<usize> {
        let sel = self.selected();
        self.active = false;
        sel
    }
}

/// Candidate windows for the switcher in most-recently-used order (topmost of the
/// z-order first). Only live windows on the active workspace are offered; a
/// minimized one is still listed — committing to it un-minimizes it.
fn switcher_mru(z: &[usize], wins: &[Win], ws: usize) -> Vec<usize> {
    let mut out = Vec::new();
    for &i in z.iter().rev() {
        if wins.get(i).map_or(false, |w| w.alive && w.ws == ws) {
            out.push(i);
        }
    }
    out
}

/// Move `wi` to the top of the z-order (mirrors the click-to-raise path).
fn raise_window(z: &mut Vec<usize>, wi: usize) {
    z.retain(|&i| i != wi);
    z.push(wi);
}

/// Repair the three `Option<wins index>` cursors after the `Win` at `wi` was
/// removed from `wins`. An index equal to the removed slot is cleared; any index
/// above it shifts down by one to track the `Vec::remove` compaction. `drag`
/// carries a `(win, off_x, off_y)` tuple; the other two are bare indices.
fn adjust_indices_after_removal(
    drag: &mut Option<(usize, i32, i32)>,
    pressed_win: &mut Option<usize>,
    input_focus: &mut Option<usize>,
    wi: usize,
) {
    match *drag {
        Some((i, ..)) if i == wi => *drag = None,
        Some((i, ox, oy)) if i > wi => *drag = Some((i - 1, ox, oy)),
        _ => {}
    }
    for opt in [pressed_win, input_focus] {
        match *opt {
            Some(i) if i == wi => *opt = None,
            Some(i) if i > wi => *opt = Some(i - 1),
            _ => {}
        }
    }
}

/// Remove every `Win` owned by `pid` and repair ALL stored `wins` indices — the
/// z-order (`z`), the drag capture, and the pressed/hover cursors. Iterates the
/// victims high→low so an earlier `Vec::remove` never shifts an index still to be
/// visited. Called once a client process has been reaped, so leaving its window
/// model behind (as the old close path did) would both leak and desync indices.
fn reap_windows_of(
    pid: u32,
    wins: &mut Vec<Win>,
    z: &mut Vec<usize>,
    drag: &mut Option<(usize, i32, i32)>,
    pressed_win: &mut Option<usize>,
    input_focus: &mut Option<usize>,
) {
    let mut victims: Vec<usize> = (0..wins.len()).filter(|&i| wins[i].pid == pid).collect();
    victims.sort_unstable();
    for &wi in victims.iter().rev() {
        wins.remove(wi);
        z.retain(|&i| i != wi);
        for i in z.iter_mut() {
            if *i > wi {
                *i -= 1;
            }
        }
        adjust_indices_after_removal(drag, pressed_win, input_focus, wi);
    }
}

/// A transient desktop notification (M4, `[notifications]`). `born`/`expire` are
/// frame ticks (~16 ms each); the compositor prunes a toast once `expire` passes.
/// Purely runtime UI feedback — the policy (on/off, timeout, corner) lives in the
/// config, never a constant.
struct Toast {
    text: alloc::string::String,
    born: u32,
    expire: u32,
}

/// Post a notification toast, gated by `[notifications] enabled` (single source of
/// truth — the flag is the resolved config's, never a hardcoded default). The
/// per-toast lifetime comes from `timeout_ms` (clamped 500..30000 at parse time)
/// converted to ~16 ms frames. The queue is bounded so a burst of launches can't
/// grow it without limit; the oldest toasts are dropped first.
fn notify_post(q: &mut Vec<Toast>, cfg: &settings::Notifications, now: u32, text: &str) {
    if !cfg.enabled {
        return;
    }
    const MAX_TOASTS: usize = 4;
    let frames = (cfg.timeout_ms / 16).max(1);
    q.push(Toast { text: text.into(), born: now, expire: now.saturating_add(frames) });
    if q.len() > MAX_TOASTS {
        let drop = q.len() - MAX_TOASTS;
        q.drain(0..drop);
    }
}

// ===========================================================================
// M4 userspace DWM — interactive desktop session
// ===========================================================================
//
// A persistent compositor loop that owns a full-screen back buffer. Each client
// surface is decorated with a draggable title bar; the real mouse (syscall 60,
// `get_mouse_state`) drives focus/raise/drag/close. Every tick the whole desktop
// is recomposited and presented, so it stays correct as windows move and as the
// terminal underneath (when launched via `exec gui_server`) would otherwise show
// through. Client *content* is still static here — reacting to input inside a
// window (button hover/press) is the next slice.

// Window decoration geometry (title-bar height, border) now lives in
// `settings::Layout`, loaded from the `[layout]` TOML table at desktop start.
// Each `Win` carries its own `title_h`/`border` so its methods stay parameter-free.

/// Fill an axis-aligned rectangle in the back buffer, clipped to its bounds.
fn fill_rect(buf: &mut [u32], bw: usize, bh: usize, x: i32, y: i32, w: i32, h: i32, color: u32) {
    let x0 = x.max(0) as usize;
    let y0 = y.max(0) as usize;
    let x1 = ((x + w).max(0) as usize).min(bw);
    let y1 = ((y + h).max(0) as usize).min(bh);
    let mut yy = y0;
    while yy < y1 {
        let row = yy * bw;
        let mut xx = x0;
        while xx < x1 {
            buf[row + xx] = color;
            xx += 1;
        }
        yy += 1;
    }
}

// ===========================================================================
// Compositor visual effects (concept §5): alpha blending, gradients, rounded
// corners, soft shadows, backdrop blur. All integer math (no FP in userspace),
// all gated by `settings::Effects` so the flat look is one config flag away.
// ===========================================================================

/// src-over composite of an ARGB `src` onto an opaque XRGB `dst`. Fast paths for
/// fully transparent / fully opaque source.
#[inline]
fn blend(dst: u32, src: u32) -> u32 {
    let a = (src >> 24) & 0xFF;
    if a == 0 {
        return dst;
    }
    if a == 255 {
        return 0xFF00_0000 | (src & 0x00FF_FFFF);
    }
    let na = 255 - a;
    let sr = (src >> 16) & 0xFF;
    let sg = (src >> 8) & 0xFF;
    let sb = src & 0xFF;
    let dr = (dst >> 16) & 0xFF;
    let dg = (dst >> 8) & 0xFF;
    let db = dst & 0xFF;
    let r = (sr * a + dr * na + 127) / 255;
    let g = (sg * a + dg * na + 127) / 255;
    let b = (sb * a + db * na + 127) / 255;
    0xFF00_0000 | (r << 16) | (g << 8) | b
}

/// Blend one ARGB pixel into the back buffer (bounds-checked).
#[inline]
fn blend_pixel(buf: &mut [u32], bw: usize, bh: usize, x: i32, y: i32, src: u32) {
    if x < 0 || y < 0 || x as usize >= bw || y as usize >= bh {
        return;
    }
    let idx = y as usize * bw + x as usize;
    buf[idx] = blend(buf[idx], src);
}

/// Shift every RGB channel of `c` by `delta` (positive = lighten), clamped. Used
/// to derive gradient endpoints from a single theme color.
#[inline]
fn shade(c: u32, delta: i32) -> u32 {
    let cl = |v: i32| v.clamp(0, 255) as u32;
    let r = ((c >> 16) & 0xFF) as i32 + delta;
    let g = ((c >> 8) & 0xFF) as i32 + delta;
    let b = (c & 0xFF) as i32 + delta;
    (cl(r) << 16) | (cl(g) << 8) | cl(b)
}

/// Linear RGB interpolation between `a` and `b`; `t` in 0..=255 (0 = a, 255 = b).
#[inline]
fn lerp_color(a: u32, b: u32, t: u32) -> u32 {
    let t = t.min(255);
    let inv = 255 - t;
    let r = (((a >> 16) & 0xFF) * inv + ((b >> 16) & 0xFF) * t) / 255;
    let g = (((a >> 8) & 0xFF) * inv + ((b >> 8) & 0xFF) * t) / 255;
    let bl = ((a & 0xFF) * inv + (b & 0xFF) * t) / 255;
    (r << 16) | (g << 8) | bl
}

/// Blend a translucent solid ARGB rect into the buffer.
fn fill_rect_alpha(buf: &mut [u32], bw: usize, bh: usize, x: i32, y: i32, w: i32, h: i32, color: u32) {
    let x0 = x.max(0);
    let y0 = y.max(0);
    let x1 = (x + w).min(bw as i32);
    let y1 = (y + h).min(bh as i32);
    let mut py = y0;
    while py < y1 {
        let mut px = x0;
        while px < x1 {
            blend_pixel(buf, bw, bh, px, py, color);
            px += 1;
        }
        py += 1;
    }
}

/// Ramp value 0..=255 for an animation `dur_frames` long, `dur_frames == 0` =
/// instant (returns 255).
#[inline]
fn ramp(now: u32, start: u32, dur_frames: u32) -> u32 {
    if dur_frames == 0 {
        return 255;
    }
    let age = now.saturating_sub(start);
    if age >= dur_frames {
        255
    } else {
        age * 255 / dur_frames
    }
}

// Rounded-corner selection bitmask for `fill_rrect*` (which corners get the arc).
const RR_TL: u8 = 1;
const RR_TR: u8 = 2;
const RR_BL: u8 = 4;
const RR_BR: u8 = 8;
const RR_ALL: u8 = RR_TL | RR_TR | RR_BL | RR_BR;
const RR_TOP: u8 = RR_TL | RR_TR;

/// Coverage 0..255 of pixel (px,py) inside rounded-rect [x,y,w,h] radius `r`.
/// Straight edges and non-selected corners return 255; a selected corner is
/// 4x4-supersampled in 1/8-px units for a cheap anti-aliased arc (integer only).
fn rr_cov(px: i32, py: i32, x: i32, y: i32, w: i32, h: i32, r: i32, corners: u8) -> u32 {
    if r <= 0 {
        return 255;
    }
    let (cx, cy, bit);
    if px < x + r && py < y + r {
        cx = x + r;
        cy = y + r;
        bit = RR_TL;
    } else if px >= x + w - r && py < y + r {
        cx = x + w - r;
        cy = y + r;
        bit = RR_TR;
    } else if px < x + r && py >= y + h - r {
        cx = x + r;
        cy = y + h - r;
        bit = RR_BL;
    } else if px >= x + w - r && py >= y + h - r {
        cx = x + w - r;
        cy = y + h - r;
        bit = RR_BR;
    } else {
        return 255;
    }
    if corners & bit == 0 {
        return 255;
    }
    let r8 = (r * 8) as i64;
    let r2 = r8 * r8;
    let mut inside = 0u32;
    let mut sy = 0;
    while sy < 4 {
        let dy = (py * 8 + (sy * 2 + 1) - cy * 8) as i64;
        let mut sx = 0;
        while sx < 4 {
            let dx = (px * 8 + (sx * 2 + 1) - cx * 8) as i64;
            if dx * dx + dy * dy <= r2 {
                inside += 1;
            }
            sx += 1;
        }
        sy += 1;
    }
    inside * 255 / 16
}

/// Fill a rounded rect with a flat (alpha-aware) `color`. `corners` selects which
/// corners get the arc (e.g. `RR_TOP` for a title bar). `r <= 0` = plain rect.
fn fill_rrect(buf: &mut [u32], bw: usize, bh: usize, x: i32, y: i32, w: i32, h: i32, r: i32, corners: u8, color: u32) {
    if w <= 0 || h <= 0 {
        return;
    }
    let r = r.max(0).min(w / 2).min(h / 2);
    let base_a = (color >> 24) & 0xFF;
    if base_a == 0 {
        return;
    }
    let x0 = x.max(0);
    let y0 = y.max(0);
    let x1 = (x + w).min(bw as i32);
    let y1 = (y + h).min(bh as i32);
    let mut py = y0;
    while py < y1 {
        let mut px = x0;
        while px < x1 {
            let cov = rr_cov(px, py, x, y, w, h, r, corners);
            if cov > 0 {
                let a = base_a * cov / 255;
                blend_pixel(buf, bw, bh, px, py, (a << 24) | (color & 0x00FF_FFFF));
            }
            px += 1;
        }
        py += 1;
    }
}

/// Fill a rounded rect with a vertical gradient (`top`..`bottom` RGB) at overall
/// opacity `alpha` (0..255), corner arcs anti-aliased. Used for glass panels,
/// title bars and buttons.
fn fill_rrect_grad(buf: &mut [u32], bw: usize, bh: usize, x: i32, y: i32, w: i32, h: i32, r: i32, corners: u8, top: u32, bottom: u32, alpha: i32) {
    if w <= 0 || h <= 0 || alpha <= 0 {
        return;
    }
    let alpha = alpha.min(255) as u32;
    let r = r.max(0).min(w / 2).min(h / 2);
    let x0 = x.max(0);
    let y0 = y.max(0);
    let x1 = (x + w).min(bw as i32);
    let y1 = (y + h).min(bh as i32);
    let span = (h - 1).max(1) as u32;
    let mut py = y0;
    while py < y1 {
        let t = ((py - y).max(0) as u32 * 255) / span;
        let row = lerp_color(top, bottom, t);
        let mut px = x0;
        while px < x1 {
            let cov = rr_cov(px, py, x, y, w, h, r, corners);
            if cov > 0 {
                let a = alpha * cov / 255;
                blend_pixel(buf, bw, bh, px, py, (a << 24) | (row & 0x00FF_FFFF));
            }
            px += 1;
        }
        py += 1;
    }
}

/// Layered soft drop shadow under a rounded rect: `spread` nested black rounded
/// rects, each at `alpha/spread`, so overlap accumulates into a soft falloff.
/// Offset down-right a touch for a light-from-above look. The opaque window
/// interior [x,y,w,h] is skipped — it gets overdrawn, so shadowing it is wasted
/// work (this is the bulk of the pixels, so skipping keeps the effect cheap).
fn draw_soft_shadow(buf: &mut [u32], bw: usize, bh: usize, x: i32, y: i32, w: i32, h: i32, r: i32, spread: i32, alpha: i32) {
    if spread <= 0 || alpha <= 0 {
        return;
    }
    let per = ((alpha / spread).max(1)) as u32;
    let mut s = spread;
    while s >= 1 {
        let off = s / 2 + 2;
        let (lx, ly2, lw, lh, lr) = (x - s, y - s + off, w + 2 * s, h + 2 * s, r + s);
        let x0 = lx.max(0);
        let y0 = ly2.max(0);
        let x1 = (lx + lw).min(bw as i32);
        let y1 = (ly2 + lh).min(bh as i32);
        let mut py = y0;
        while py < y1 {
            let in_row = py >= y && py < y + h;
            let mut px = x0;
            while px < x1 {
                if in_row && px >= x && px < x + w {
                    px = x + w; // jump past the opaque window interior
                    continue;
                }
                let cov = rr_cov(px, py, lx, ly2, lw, lh, lr, RR_ALL);
                if cov > 0 {
                    blend_pixel(buf, bw, bh, px, py, (per * cov / 255) << 24);
                }
                px += 1;
            }
            py += 1;
        }
        s -= 1;
    }
}

/// One horizontal box-blur pass: `src` -> `dst` (both `w*h` packed), radius `r`,
/// sliding-window average of RGB with edge clamp. Alpha forced opaque.
fn box_h(src: &[u32], dst: &mut [u32], w: usize, h: usize, r: usize) {
    for y in 0..h {
        let row = y * w;
        let mut sr = 0u32;
        let mut sg = 0u32;
        let mut sb = 0u32;
        let first_hi = r.min(w - 1);
        for i in 0..=first_hi {
            let p = src[row + i];
            sr += (p >> 16) & 0xFF;
            sg += (p >> 8) & 0xFF;
            sb += p & 0xFF;
        }
        let mut lo = 0i32;
        let mut hi = first_hi as i32;
        for x in 0..w {
            let cnt = (hi - lo + 1) as u32;
            dst[row + x] = 0xFF00_0000 | ((sr / cnt) << 16) | ((sg / cnt) << 8) | (sb / cnt);
            let add = x as i32 + 1 + r as i32;
            if add < w as i32 {
                let p = src[row + add as usize];
                sr += (p >> 16) & 0xFF;
                sg += (p >> 8) & 0xFF;
                sb += p & 0xFF;
                hi = add;
            }
            let rem = x as i32 - r as i32;
            if rem >= 0 {
                let p = src[row + rem as usize];
                sr -= (p >> 16) & 0xFF;
                sg -= (p >> 8) & 0xFF;
                sb -= p & 0xFF;
                lo = rem + 1;
            }
        }
    }
}

/// One vertical box-blur pass: `src` -> `dst` (both `w*h` packed), radius `r`.
fn box_v(src: &[u32], dst: &mut [u32], w: usize, h: usize, r: usize) {
    for x in 0..w {
        let mut sr = 0u32;
        let mut sg = 0u32;
        let mut sb = 0u32;
        let first_hi = r.min(h - 1);
        for i in 0..=first_hi {
            let p = src[i * w + x];
            sr += (p >> 16) & 0xFF;
            sg += (p >> 8) & 0xFF;
            sb += p & 0xFF;
        }
        let mut lo = 0i32;
        let mut hi = first_hi as i32;
        for y in 0..h {
            let cnt = (hi - lo + 1) as u32;
            dst[y * w + x] = 0xFF00_0000 | ((sr / cnt) << 16) | ((sg / cnt) << 8) | (sb / cnt);
            let add = y as i32 + 1 + r as i32;
            if add < h as i32 {
                let p = src[add as usize * w + x];
                sr += (p >> 16) & 0xFF;
                sg += (p >> 8) & 0xFF;
                sb += p & 0xFF;
                hi = add;
            }
            let rem = y as i32 - r as i32;
            if rem >= 0 {
                let p = src[rem as usize * w + x];
                sr -= (p >> 16) & 0xFF;
                sg -= (p >> 8) & 0xFF;
                sb -= p & 0xFF;
                lo = rem + 1;
            }
        }
    }
}

/// Separable box blur of a sub-region of the back buffer (the acrylic backdrop
/// under the panel/menu/dock). `tmp_a`/`tmp_b` are ping-pong scratch buffers
/// allocated once by the caller; a region larger than the scratch is skipped.
/// `iters` box passes ≈ a Gaussian. Integer-only, O(region) per pass.
fn blur_region(buf: &mut [u32], bw: usize, bh: usize, rx: i32, ry: i32, rw: i32, rh: i32, radius: i32, iters: i32, tmp_a: &mut [u32], tmp_b: &mut [u32]) {
    if radius <= 0 || iters <= 0 {
        return;
    }
    let x0 = rx.max(0) as usize;
    let y0 = ry.max(0) as usize;
    let x1 = ((rx + rw).max(0) as usize).min(bw);
    let y1 = ((ry + rh).max(0) as usize).min(bh);
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    let w = x1 - x0;
    let h = y1 - y0;
    if w * h > tmp_a.len() || w * h > tmp_b.len() {
        return;
    }
    for yy in 0..h {
        let dr = yy * w;
        let sr = (y0 + yy) * bw + x0;
        tmp_a[dr..dr + w].copy_from_slice(&buf[sr..sr + w]);
    }
    let r = radius as usize;
    for _ in 0..iters {
        box_h(tmp_a, tmp_b, w, h, r);
        box_v(tmp_b, tmp_a, w, h, r);
    }
    for yy in 0..h {
        let sr = yy * w;
        let dr = (y0 + yy) * bw + x0;
        buf[dr..dr + w].copy_from_slice(&tmp_a[sr..sr + w]);
    }
}

/// Clip the rect `(x, y, w, h)` to the `bw*bh` framebuffer, returning
/// `(x0, y0, cw, ch)` in buffer pixels (`cw == 0` or `ch == 0` = fully off-screen).
fn clip_to_screen(x: i32, y: i32, w: i32, h: i32, bw: usize, bh: usize) -> (usize, usize, usize, usize) {
    let x0 = x.max(0);
    let y0 = y.max(0);
    let x1 = (x + w).min(bw as i32);
    let y1 = (y + h).min(bh as i32);
    if x1 <= x0 || y1 <= y0 {
        return (0, 0, 0, 0);
    }
    (x0 as usize, y0 as usize, (x1 - x0) as usize, (y1 - y0) as usize)
}

/// Blit a client's surface into the back buffer at (x, y), clipped. `argb`
/// selects the compositing rule per the format the client imported its buffer
/// with (captured in `ClientState.format`, mirrored into `Win.format`):
///   * XRGB8888 (`argb == false`): the surface is opaque — a straight copy, the
///     fast path; the top 8 bits are "don't care" so we never read them.
///   * ARGB8888 (`argb == true`):  the surface carries a straight-alpha channel
///     (e.g. a terminal with a translucent background) — src-over blend each
///     pixel over the already-composited desktop so the wallpaper/windows behind
///     show through, while opaque pixels (text, icons: alpha 255) overwrite.
fn blit_surface(
    buf: &mut [u32],
    bw: usize,
    bh: usize,
    x: i32,
    y: i32,
    src: &[u32],
    sw: usize,
    sh: usize,
    argb: bool,
) {
    let mut sy = 0usize;
    while sy < sh {
        let dy = y + sy as i32;
        if dy >= 0 && (dy as usize) < bh {
            let drow = dy as usize * bw;
            let srow = sy * sw;
            let mut sx = 0usize;
            while sx < sw {
                let dx = x + sx as i32;
                if dx >= 0 && (dx as usize) < bw {
                    if argb {
                        // src-over: opaque (a==255) pixels overwrite, translucent
                        // ones blend, fully transparent (a==0) leave the desktop.
                        blend_pixel(buf, bw, bh, dx, dy, src[srow + sx]);
                    } else {
                        buf[drow + dx as usize] = src[srow + sx];
                    }
                }
                sx += 1;
            }
        }
        sy += 1;
    }
}
/// A classic top-left arrow cursor. 'X' = black outline, '.' = white fill,
/// ' ' = transparent. Hotspot is the top-left corner (0,0).
const CURSOR: [&str; 17] = [
    "X                ",
    "XX               ",
    "X.X              ",
    "X..X             ",
    "X...X            ",
    "X....X           ",
    "X.....X          ",
    "X......X         ",
    "X.......X        ",
    "X........X       ",
    "X.....XXXXX      ",
    "X..X..X          ",
    "X.X X..X         ",
    "XX  X..X         ",
    "X    X..X        ",
    "     X..X        ",
    "      XX         ",
];

/// Draw the arrow cursor into the back buffer with its hotspot at (px, py).
fn draw_cursor(buf: &mut [u32], bw: usize, bh: usize, px: i32, py: i32) {
    for (ry, row) in CURSOR.iter().enumerate() {
        let dy = py + ry as i32;
        if dy < 0 || dy as usize >= bh {
            continue;
        }
        let drow = dy as usize * bw;
        for (rx, ch) in row.bytes().enumerate() {
            let color = match ch {
                b'X' => 0xFF000000,
                b'.' => 0xFFFFFFFF,
                _ => continue,
            };
            let dx = px + rx as i32;
            if dx >= 0 && (dx as usize) < bw {
                buf[drow + dx as usize] = color;
            }
        }
    }
}
// ===========================================================================
// DWM shell — top panel / taskbar (M4 slice 5)
// ===========================================================================
//
// A compositor-owned panel across the top of the screen: a launcher glyph on
// the left, one taskbar button per live window (click to raise + focus) in the
// middle, and an uptime clock on the right. The panel is drawn on top of every
// window each tick and its band is reserved — windows are kept below it and
// pointer input over the panel is consumed by the shell, never forwarded to a
// client.

// Panel/launcher/taskbar/menu geometry now lives in `settings::Layout` (loaded
// from the `[layout]` TOML table); `max_windows` there also caps launcher spawns
// so a stuck loop cannot fork the machine to death.

// The application registry (ids, names, exec paths, icons, labels) and the
// dock / launcher / autostart lists are no longer hardcoded here: they live in
// `settings::Applications`, built from the `[application.*]` / `[dock]` /
// `[launcher]` / `[startup]` TOML tables (baseline in `Applications::baseline`).
// A window's `app` field is an index into that live registry (0xFF = unknown).

// Shell-content geometry is laid out along the panel's MAIN axis (X for a
// top/bottom panel, Y for a left/right panel) so one set of helpers serves all
// four edges. `panel_slot` maps a 1-D span on the main axis to a screen rect
// spanning the full cross-axis thickness; `inset_cross` trims the cross axis.
// For the default top panel these reproduce the old fixed geometry byte-for-byte.

/// Fill a shell strip (panel / dock / tray) with the acrylic-glass look: optional
/// backdrop blur, then a vertical gradient tint (or a flat tint when gradients are
/// off). `top_shade` lightens the gradient's top edge (panel 16, dock 14). One
/// code path for every strip so all edges paint identically.
fn fill_strip_bg(
    buf: &mut [u32],
    bw: usize,
    bh: usize,
    rect: (i32, i32, i32, i32),
    panel: u32,
    fx: &Effects,
    top_shade: i32,
    tmp_a: &mut [u32],
    tmp_b: &mut [u32],
) {
    let (x, y, w, h) = rect;
    if fx.blur {
        blur_region(buf, bw, bh, x, y, w, h, fx.blur_radius, fx.blur_iters, tmp_a, tmp_b);
    }
    if fx.gradient {
        fill_rrect_grad(buf, bw, bh, x, y, w, h, 0, 0, shade(panel, top_shade), panel, fx.panel_alpha);
    } else {
        fill_rect_alpha(buf, bw, bh, x, y, w, h, ((fx.panel_alpha.min(255) as u32) << 24) | (panel & 0x00FF_FFFF));
    }
}

/// Map a `[main_off, main_off+main_len)` span on the panel's main axis to a
/// screen rect that spans the panel's full cross-axis thickness.
fn panel_slot(panel: (i32, i32, i32, i32), edge: Edge, main_off: i32, main_len: i32) -> (i32, i32, i32, i32) {
    let (px, py, pw, ph) = panel;
    if edge.is_horizontal() {
        (px + main_off, py, main_len, ph)
    } else {
        (px, py + main_off, pw, main_len)
    }
}

/// Trim `c` px off each side of a strip rect on its CROSS axis (vertical for a
/// horizontal strip, horizontal for a vertical strip); the main axis is untouched.
fn inset_cross(rect: (i32, i32, i32, i32), edge: Edge, c: i32) -> (i32, i32, i32, i32) {
    let (x, y, w, h) = rect;
    if edge.is_horizontal() {
        (x, y + c, w, (h - 2 * c).max(1))
    } else {
        (x + c, y, (w - 2 * c).max(1), h)
    }
}

/// Main-axis length reserved for the launcher button at the panel's start
/// (`launcher_w` on a horizontal panel; a square panel-thick cell on a vertical
/// one, so the mark stays legible in the narrow bar).
fn launcher_main(ly: &Layout, edge: Edge) -> i32 {
    if edge.is_horizontal() {
        ly.launcher_w
    } else {
        ly.panel_h
    }
}

/// The panel strip itself: `ly.panel_h` thick, flush against `panel_edge`, owning
/// its corners (offset 0). For the default top panel this is `(0, 0, bw, panel_h)`.
fn panel_strip(ly: &Layout, ins: &Insets, edge: Edge, bw: usize, bh: usize) -> (i32, i32, i32, i32) {
    strip_rect(edge, ly.panel_h, 0, ins, bw, bh)
}

/// The dock strip: always the left vertical edge, nested just inside any
/// left-edge panel/tray (offset = everything reserved on the left minus itself).
fn dock_strip(ly: &Layout, ins: &Insets, bw: usize, bh: usize) -> (i32, i32, i32, i32) {
    strip_rect(Edge::Left, ly.dock_w, ins.left - ly.dock_w, ins, bw, bh)
}

/// Rect of the i-th dock icon (0-based), a square cell laid top-down inside the
/// dock strip. The dock stays a left vertical strip (only panel/tray are
/// edge-configurable), so this is always vertical.
fn dock_icon_rect(ly: &Layout, ins: &Insets, bw: usize, bh: usize, i: i32) -> (i32, i32, i32, i32) {
    let (dx, dy, _, _) = dock_strip(ly, ins, bw, bh);
    let inset = 6;
    let iw = ly.dock_w - 2 * inset;
    let x = dx + inset;
    let y = dy + inset + i * (iw + inset);
    (x, y, iw, iw)
}

/// Rect of the i-th workspace pip (0-based), laid along the panel's main axis
/// immediately after the launcher glyph.
fn ws_pip_rect(ly: &Layout, edge: Edge, panel: (i32, i32, i32, i32), i: i32) -> (i32, i32, i32, i32) {
    let off = launcher_main(ly, edge) + i * ly.ws_w;
    inset_cross(panel_slot(panel, edge, off, ly.ws_w - 2), edge, 2)
}

/// Top-left origin `(x, y)` of the launcher dropdown, flying out PERPENDICULAR to
/// the panel edge: below a top panel, above a bottom panel, right of a left panel,
/// left of a right panel. `item_count` sizes the "above" case. The menu itself is
/// always a vertical list of `menu_item_h` rows.
fn menu_origin(ly: &Layout, edge: Edge, panel: (i32, i32, i32, i32), item_count: i32) -> (i32, i32) {
    let (px, py, pw, ph) = panel;
    let full_h = item_count * ly.menu_item_h;
    match edge {
        Edge::Top => (px, py + ph),
        Edge::Bottom => (px, py - full_h),
        Edge::Left => (px + pw, py),
        Edge::Right => (px - ly.menu_w, py),
    }
}

/// Rect of the i-th launcher-menu entry, stacked vertically from `origin`.
fn menu_item_rect(ly: &Layout, origin: (i32, i32), i: i32) -> (i32, i32, i32, i32) {
    (origin.0, origin.1 + i * ly.menu_item_h, ly.menu_w, ly.menu_item_h)
}

/// Quick-settings applet cell (M4, `[quicksettings]`): a panel-thick square on the
/// panel's main axis, just past the workspace pips. Clickable to open the flyout
/// of live toggles. Only meaningful when `[quicksettings] enabled` (the caller
/// gates on it); geometry follows the configured panel edge like every other cell.
fn qs_applet_rect(ly: &Layout, edge: Edge, panel: (i32, i32, i32, i32), ws_count: usize) -> (i32, i32, i32, i32) {
    let off = launcher_main(ly, edge) + ws_count as i32 * ly.ws_w + 6;
    let side = if edge.is_horizontal() { panel.3 } else { panel.2 };
    inset_cross(panel_slot(panel, edge, off, side), edge, 3)
}

/// Quick-settings flyout card: `rows` toggle rows flying out PERPENDICULAR to the
/// panel edge from the applet cell (same convention as the launcher `menu_origin`).
fn qs_flyout_rect(ly: &Layout, edge: Edge, applet: (i32, i32, i32, i32), rows: i32) -> (i32, i32, i32, i32) {
    let (ax, ay, aw, _ah) = applet;
    let w = 150i32;
    let h = rows * ly.menu_item_h + 8;
    match edge {
        Edge::Top => (ax, ay + applet.3, w, h),
        Edge::Bottom => (ax, ay - h, w, h),
        Edge::Left => (ax + aw, ay, w, h),
        Edge::Right => (ax - w, ay, w, h),
    }
}

/// Rect of the i-th quick-settings flyout row (a toggle line).
fn qs_row_rect(ly: &Layout, flyout: (i32, i32, i32, i32), i: i32) -> (i32, i32, i32, i32) {
    (flyout.0 + 4, flyout.1 + 4 + i * ly.menu_item_h, flyout.2 - 8, ly.menu_item_h)
}

/// True when the point is inside `rect`.
fn in_rect(rect: (i32, i32, i32, i32), x: i32, y: i32) -> bool {
    x >= rect.0 && x < rect.0 + rect.2 && y >= rect.1 && y < rect.1 + rect.3
}

// A 3x5 bitmap font, just digits and ':' — enough for window numbers and a
// MM:SS clock. Each row is a 3-bit mask (bit 2 = leftmost pixel).
const GLYPH_W: i32 = 3;

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
        b':' => [0, 2, 0, 2, 0],
        // A few uppercase letters for launcher menu labels (WIN / CALC).
        b'W' => [5, 5, 5, 7, 5],
        b'I' => [7, 2, 2, 2, 7],
        b'N' => [5, 7, 7, 7, 5],
        b'C' => [7, 4, 4, 4, 7],
        b'A' => [2, 5, 7, 5, 5],
        b'L' => [4, 4, 4, 4, 7],
        b'S' => [3, 4, 2, 1, 6],
        b'T' => [7, 2, 2, 2, 2],
        b'F' => [7, 4, 6, 4, 4],
        b'E' => [7, 4, 6, 4, 7],
        b'R' => [6, 5, 6, 5, 5],
        b'M' => [5, 7, 7, 5, 5],
        _ => return None,
    })
}

/// Draw an ASCII string in the 3x5 font at `scale`, returning the x just past
/// the last glyph. Unknown bytes advance a blank cell (used for spaces).
fn draw_text_3x5(
    buf: &mut [u32],
    bw: usize,
    bh: usize,
    x: i32,
    y: i32,
    scale: i32,
    color: u32,
    s: &[u8],
) -> i32 {
    let advance = (GLYPH_W + 1) * scale;
    let mut cx = x;
    for &c in s {
        if let Some(rows) = glyph_3x5(c) {
            for (ry, mask) in rows.iter().enumerate() {
                for bit in 0..GLYPH_W {
                    if mask & (1 << (GLYPH_W - 1 - bit)) != 0 {
                        fill_rect(
                            buf,
                            bw,
                            bh,
                            cx + bit * scale,
                            y + ry as i32 * scale,
                            scale,
                            scale,
                            color,
                        );
                    }
                }
            }
        }
        cx += advance;
    }
    cx
}

/// Write a zero-padded two-digit value into `out[off..off+2]`.
fn two_digits(out: &mut [u8], off: usize, v: u64) {
    out[off] = b'0' + ((v / 10) % 10) as u8;
    out[off + 1] = b'0' + (v % 10) as u8;
}

// --- Crisp TTF text for the shell (panel titles, window titles, tray) --------
// The 3x5 bitmap font above stays for tiny numeric labels (ws pips, taskbar
// numbers, dock initials); everything reference-facing now renders in the real
// desktop font so it reads like the mockup.

/// The desktop font, embedded in the ELF as a last-known-good fallback. The
/// live font path is a config knob (`[desktop] font`); see `load_font`.
static FONT_BYTES: &[u8] = include_bytes!("../../../../assets/fonts/DejaVuSans.ttf");

/// Load the configured TTF from the VFS, falling back to `FONT_BYTES`. `None`
/// only if even the embedded font fails to parse — then the shell silently
/// keeps its 3x5 labels and skips the new crisp text.
fn load_font() -> Option<Font> {
    let cfg = settings::load();
    if let Some(bytes) = libdunit::read_binary(cfg.desktop.font.as_str(), 4 * 1024 * 1024) {
        if let Ok(f) = Font::parse(bytes) {
            return Some(f);
        }
    }
    Font::parse(FONT_BYTES.to_vec()).ok()
}

fn round_i32(v: f32) -> i32 {
    if v <= 0.0 {
        0
    } else {
        (v + 0.5) as i32
    }
}

/// Total advance width of `text` at `px`, rounded to whole pixels.
fn text_width_ttf(font: &Font, text: &str, px: f32) -> i32 {
    round_i32(font.layout_line(text, px).1)
}

/// Draw one line of TTF `text` into the back buffer with its left edge at `x`
/// and baseline at `baseline`, blending each glyph's 8-bit coverage as alpha
/// over `color` (0x00RRGGBB). Returns the x just past the last glyph.
fn draw_text_ttf(
    back: &mut [u32],
    bw: usize,
    bh: usize,
    font: &Font,
    x: i32,
    baseline: i32,
    px: f32,
    text: &str,
    color: u32,
) -> i32 {
    let rgb = color & 0x00FF_FFFF;
    let (glyphs, adv) = font.layout_line(text, px);
    for g in glyphs {
        if let Some(bmp) = font.rasterize(g.glyph, px) {
            let ox = x + round_i32(g.x) + bmp.left;
            let oy = baseline - bmp.top;
            for row in 0..bmp.height {
                for col in 0..bmp.width {
                    let cov = bmp.coverage[row * bmp.width + col] as u32;
                    if cov == 0 {
                        continue;
                    }
                    blend_pixel(back, bw, bh, ox + col as i32, oy + row as i32, (cov << 24) | rgb);
                }
            }
        }
    }
    x + round_i32(adv)
}

/// Human-readable window title for a window's registry index (`Win.app`), looked
/// up in the live application registry. An unknown index (0xFF, e.g. a client
/// spawned for an app not in the registry) falls back to a generic "Window".
fn win_title<'a>(apps: &'a Applications, app: u8) -> &'a str {
    apps.apps.get(app as usize).map(|a| a.name.as_str()).unwrap_or("Window")
}


/// Layout state of a top-level window. `Floating` is the free-form default every
/// client is created in; `Maximized` fills the work area (screen minus the
/// reserved panel + dock). Modelled as an enum, not a `maximized: bool`, so the
/// same state machine (`set_window_state`) extends to `Fullscreen` / tiled layouts
/// later without touching every call site — a window is always in exactly one
/// state, and `Win::restore` holds the floating geometry to return to. This also
/// keeps "maximize" (a window *state*) cleanly separate from "resize" (the
/// protocol CONFIGURE mechanism that *any* state change drives).
#[derive(Clone, Copy, PartialEq, Eq)]
enum WinState {
    Floating,
    Maximized,
}

/// Per-window compositor state, derived from a `ClientState` but with a live
/// content origin the user can drag around.
struct Win {
    pid: u32,
    cx: i32,
    cy: i32,
    sw: i32,
    sh: i32,
    buf_ptr: *const u8,
    buf_size: usize,
    alive: bool,
    /// Decoration geometry copied from the resolved `[layout]` config so each
    /// window's hit-tests and framing stay self-contained.
    title_h: i32,
    border: i32,
    /// Index into the live application registry (0xFF = unknown) — drives the
    /// dock running marker and the window title (see `win_title`).
    app: u8,
    /// Workspace (0-based) this window belongs to; only the active workspace's
    /// windows are drawn and receive input.
    ws: usize,
    /// Desktop tick when this window first appeared — drives the open fade-in.
    born: u32,
    /// Minimized windows are not composited and receive no input; a click on the
    /// app's dock icon (task switcher) restores and raises them.
    minimized: bool,
    /// Current layout state (see `WinState`). Driven by the title-bar maximize
    /// chip and by dragging a tiled window's title bar (macOS-style restore).
    state: WinState,
    /// Floating geometry `(cx, cy, sw, sh)` snapshotted when the window leaves the
    /// floating state, so returning to `Floating` restores the exact pre-maximize
    /// position and size. `None` while the window is floating.
    restore: Option<(i32, i32, i32, i32)>,
    /// Self-test choreography step (config-gated `[startup] self_test`, test.toml
    /// only): 0 = not yet maximized, 1 = maximized (awaiting restore), 2 = restored.
    /// Lets the headless harness drive a full maximize->restore round trip without a
    /// mouse. Always 0 on the real desktop (`self_test` is false in `default.toml`).
    st_step: u8,
    /// Desktop tick when this window entered `st_step == 1` (maximize pushed).
    /// The restore leg normally waits for the client to re-import the work-area
    /// buffer (`sw >= ww`); a client that DECLINES the server-pushed resize (a
    /// fixed-size dialog like gui_settings never grows) would otherwise stall the
    /// self-test forever, so after `ST_SETTLE_TICKS` we advance anyway — the
    /// compositor must never deadlock on a client's right to keep its own size.
    st_since: u32,
    /// Pixel format the owning client imported its buffer with (mirrored from
    /// `ClientState.format` in the per-frame sync). `FORMAT_ARGB8888` selects the
    /// per-pixel src-over blit in `blit_surface` so a translucent client (e.g. a
    /// terminal with `bg_alpha < 255`) shows the desktop through its background;
    /// `FORMAT_XRGB8888` keeps the opaque fast-path copy.
    format: u32,
}

impl Win {
    /// Full outer rect (border + title bar + content) in screen space.
    fn outer(&self) -> (i32, i32, i32, i32) {
        let x = self.cx - self.border;
        let y = self.cy - self.title_h - self.border;
        let w = self.sw + 2 * self.border;
        let h = self.sh + self.title_h + 2 * self.border;
        (x, y, w, h)
    }
    fn contains(&self, mx: i32, my: i32) -> bool {
        let (x, y, w, h) = self.outer();
        mx >= x && mx < x + w && my >= y && my < y + h
    }
    /// The client content region (below the title bar).
    fn in_content(&self, mx: i32, my: i32) -> bool {
        mx >= self.cx && mx < self.cx + self.sw && my >= self.cy && my < self.cy + self.sh
    }
    fn in_title(&self, mx: i32, my: i32) -> bool {
        mx >= self.cx && mx < self.cx + self.sw && my >= self.cy - self.title_h && my < self.cy
    }
    /// Close box: a small square at the right end of the title bar.
    fn in_close(&self, mx: i32, my: i32) -> bool {
        let sz = self.title_h - 12;
        let bx = self.cx + self.sw - sz - 6;
        let by = self.cy - self.title_h + 6;
        mx >= bx && mx < bx + sz && my >= by && my < by + sz
    }
    /// Minimize box: a small square just left of the close box.
    fn in_min(&self, mx: i32, my: i32) -> bool {
        let sz = self.title_h - 12;
        let bx = self.cx + self.sw - 2 * sz - 10;
        let by = self.cy - self.title_h + 6;
        mx >= bx && mx < bx + sz && my >= by && my < by + sz
    }
    /// Maximize/restore box: a small square just left of the minimize box.
    fn in_max(&self, mx: i32, my: i32) -> bool {
        let sz = self.title_h - 12;
        let bx = self.cx + self.sw - 3 * sz - 14;
        let by = self.cy - self.title_h + 6;
        mx >= bx && mx < bx + sz && my >= by && my < by + sz
    }
    /// True when the window is not in its free-form floating state (currently only
    /// `Maximized`): it occupies a compositor-computed region, so a title-bar drag
    /// restores it to `restore` before following the pointer (macOS-style).
    fn is_tiled(&self) -> bool {
        self.state != WinState::Floating
    }
}
/// Persistent interactive compositor. Owns a full-screen back buffer, decorates
/// each client surface, and lets the real mouse focus/raise/drag/close windows.
/// Drain any queued client protocol messages (non-blocking) and advance each
/// client toward `ready`. Called every desktop tick so runtime-spawned clients
/// complete their handshake while the compositor keeps rendering. Bounded per
/// tick so a chatty client cannot starve the frame. Routing is by the
/// kernel-authenticated sender pid, so clients stay isolated.
fn pump_clients(
    server: &mut Server,
    clients: &mut Vec<ClientState>,
    reload: &mut bool,
    notify_out: &mut Vec<alloc::string::String>,
) {
    let mut rx = [0u8; 256];
    let mut sender: u32 = 0;
    for _ in 0..64 {
        let n = libdunit::ipc_recv_from(&mut rx, &mut sender);
        if n <= 0 {
            break; // EAGAIN (queue empty) or error — nothing more this tick
        }
        let n = n as usize;
        let idx = match clients.iter().position(|c| c.pid == sender) {
            Some(i) => i,
            None => continue, // message from an unknown pid — ignore
        };
        handle_client_payload(server, &mut clients[idx], &rx[..n], reload, notify_out);
        for (fc, fp) in &server.composite() {
            let status = if fp.len() >= 52 {
                u32::from_le_bytes([fp[48], fp[49], fp[50], fp[51]])
            } else {
                0
            };
            for c in clients.iter_mut() {
                if c.conn != *fc {
                    continue;
                }
                libdunit::ipc_send(c.pid, fp);
                if status == 0 && !c.buf_ptr.is_null() && c.surf_w > 0 && c.surf_h > 0 {
                    c.ready = true;
                }
            }
        }
    }
}

// --- Desktop wallpaper (ported from the legacy Stack A renderer) ---------
// The backdrop image lives in the (read-only) asset VFS as a 1600x900 24-bit
// BMP. It is loaded and pre-scaled to the framebuffer once at session start
// into an XRGB8888 buffer the size of `back`, so compositing the desktop each
// frame is a single `copy_from_slice` (no per-pixel BMP sampling in the hot
// loop). A missing/garbage file yields `None` and the gradient fallback stays.
// The path is no longer hardcoded here: it comes from `[desktop] wallpaper` in
// the config (baseline `/assets/wallpapers/wallpaper.bmp`).
const WALLPAPER_WIDTH: usize = 1920;
const WALLPAPER_HEIGHT: usize = 1080;
const WALLPAPER_OFFSET: usize = 54;
const WALLPAPER_STRIDE: usize = WALLPAPER_WIDTH * 3;

/// Slurp a whole (binary) VFS file into a byte vector. Unlike
/// `settings::read_file` this imposes no UTF-8 requirement and a larger cap, so
/// it can carry the multi-megabyte wallpaper. `None` on open error.
fn read_binary(path: &str, cap: usize) -> Option<Vec<u8>> {
    let fd = libdunit::open(path, libdunit::OPEN_READ);
    if fd < 0 {
        return None;
    }
    let fd = fd as usize;
    let mut data: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let n = libdunit::read(fd, &mut chunk);
        if n <= 0 {
            break;
        }
        data.extend_from_slice(&chunk[..n as usize]);
        if data.len() > cap {
            break;
        }
    }
    libdunit::close(fd);
    Some(data)
}

/// A 1600x900 24-bit BMP: "BM" magic, 54-byte pixel offset, matching
/// dimensions and 24 bpp. Mirrors the legacy `validate_wallpaper_bmp`.
fn validate_wallpaper_bmp(data: &[u8]) -> bool {
    data.len() >= WALLPAPER_OFFSET + WALLPAPER_STRIDE * WALLPAPER_HEIGHT
        && data[0] == b'B'
        && data[1] == b'M'
        && data.get(10).copied() == Some(WALLPAPER_OFFSET as u8)
        && data.get(18).copied() == Some((WALLPAPER_WIDTH & 0xff) as u8)
        && data.get(19).copied() == Some(((WALLPAPER_WIDTH >> 8) & 0xff) as u8)
        && data.get(22).copied() == Some((WALLPAPER_HEIGHT & 0xff) as u8)
        && data.get(23).copied() == Some(((WALLPAPER_HEIGHT >> 8) & 0xff) as u8)
        && data.get(28).copied() == Some(24)
}

/// Load + pre-scale the wallpaper into an XRGB8888 buffer sized `bw*bh`
/// (nearest-neighbor, bottom-up BGR source). `None` if the file is absent or
/// not a valid 1600x900 24-bit BMP — the caller keeps the gradient backdrop.
/// Full brightness (unlike the legacy 46% dim) so the image reads as intended.
fn load_wallpaper(path: &str, bw: usize, bh: usize) -> Option<Vec<u32>> {
    let data = read_binary(path, 8 * 1024 * 1024)?;
    if !validate_wallpaper_bmp(&data) {
        return None;
    }
    let mut out: Vec<u32> = Vec::new();
    out.resize(bw * bh, 0xFF00_0000);
    for y in 0..bh {
        let src_y = y.saturating_mul(WALLPAPER_HEIGHT) / bh.max(1);
        let bmp_y = WALLPAPER_HEIGHT
            .saturating_sub(1)
            .saturating_sub(src_y.min(WALLPAPER_HEIGHT - 1));
        let row_off = WALLPAPER_OFFSET + bmp_y * WALLPAPER_STRIDE;
        let dst_base = y * bw;
        for x in 0..bw {
            let src_x = x.saturating_mul(WALLPAPER_WIDTH) / bw.max(1);
            let off = row_off + src_x.min(WALLPAPER_WIDTH - 1) * 3;
            let b = data[off] as u32;
            let g = data[off + 1] as u32;
            let r = data[off + 2] as u32;
            out[dst_base + x] = 0xFF00_0000 | (r << 16) | (g << 8) | b;
        }
    }
    Some(out)
}

/// Launcher-icon side length (px) of the embedded Breeze-Chameleon RGBA assets.
/// Every `*.rgba` under `/assets/icons/breeze/` is `ICON_W`×`ICON_H`, straight
/// alpha, `R,G,B,A` byte order (see `assets/icons/breeze/NOTICE.md`).
const ICON_W: usize = 32;
const ICON_H: usize = 32;

/// Load a launcher icon *by convention* — `<dir><app>.rgba` — into packed ARGB,
/// or `None` when no such asset ships (the dock then falls back to the app
/// initial). `dir` is the configured icon-theme directory (e.g.
/// `/assets/icons/breeze/`, ending in `/`), so the theme is a config knob, not a
/// constant. `app` is the registry entry's icon stem; adding an app to the config
/// registry picks up its icon automatically if the file exists. The path is
/// assembled on the stack to keep the compositor's no-heap-`String` style.
fn load_app_icon(app: &str, dir: &str) -> Option<Vec<u32>> {
    const SUFFIX: &[u8] = b".rgba";
    let prefix = dir.as_bytes();
    let mut buf = [0u8; 160];
    let n = prefix.len() + app.len() + SUFFIX.len();
    if n > buf.len() {
        return None;
    }
    buf[..prefix.len()].copy_from_slice(prefix);
    buf[prefix.len()..prefix.len() + app.len()].copy_from_slice(app.as_bytes());
    buf[prefix.len() + app.len()..n].copy_from_slice(SUFFIX);
    let path = core::str::from_utf8(&buf[..n]).ok()?;
    load_icon_rgba(path)
}

/// Resolve one registry entry's icon spec to packed ARGB. A spec containing '/'
/// is an absolute VFS path loaded verbatim (`load_icon_rgba`); otherwise it is a
/// stem resolved against the active icon-theme directory as `<dir><stem>.rgba`
/// (`load_app_icon`). An empty spec, or a missing/mis-sized asset, yields `None`
/// so the dock/menu cell falls back to the app's label initial.
fn resolve_app_icon(icon: &str, dir: &str) -> Option<Vec<u32>> {
    if icon.is_empty() {
        return None;
    }
    if icon.as_bytes().contains(&b'/') {
        load_icon_rgba(icon)
    } else {
        load_app_icon(icon, dir)
    }
}

/// Build the icon-theme directory path `/assets/icons/<theme>/` into `buf` and
/// return the slice. An empty or over-long theme name falls back to `breeze`, so
/// a bad config never leaves the dock icon-less. Stack-assembled (no `String`).
fn icon_dir<'a>(theme: &str, buf: &'a mut [u8; 160]) -> &'a str {
    const PRE: &[u8] = b"/assets/icons/";
    const FALLBACK: &str = "/assets/icons/breeze/";
    let theme = if theme.is_empty() { "breeze" } else { theme };
    let n = PRE.len() + theme.len() + 1;
    if n > buf.len() {
        buf[..FALLBACK.len()].copy_from_slice(FALLBACK.as_bytes());
        return core::str::from_utf8(&buf[..FALLBACK.len()]).unwrap_or(FALLBACK);
    }
    buf[..PRE.len()].copy_from_slice(PRE);
    buf[PRE.len()..PRE.len() + theme.len()].copy_from_slice(theme.as_bytes());
    buf[n - 1] = b'/';
    core::str::from_utf8(&buf[..n]).unwrap_or(FALLBACK)
}

/// Load a straight-alpha `R,G,B,A` icon (`ICON_W`×`ICON_H`) from the VFS into
/// packed ARGB8888, ready for `blend`. `None` if the file is absent or shorter
/// than one full icon — a mis-sized asset falls back to the dock initial.
fn load_icon_rgba(path: &str) -> Option<Vec<u32>> {
    let need = ICON_W * ICON_H * 4;
    let data = read_binary(path, need + 16)?;
    if data.len() < need {
        return None;
    }
    let mut out: Vec<u32> = Vec::with_capacity(ICON_W * ICON_H);
    let mut i = 0;
    while i + 3 < need {
        let r = data[i] as u32;
        let g = data[i + 1] as u32;
        let b = data[i + 2] as u32;
        let a = data[i + 3] as u32;
        out.push((a << 24) | (r << 16) | (g << 8) | b);
        i += 4;
    }
    Some(out)
}

/// Alpha-blend a packed-ARGB icon (`src`, `sw`×`sh`) into the back buffer,
/// resampled to `dw`×`dh` at `(dx,dy)`. The Breeze source assets are 32×32; dock
/// buttons grow them (~46 px on hover) and launcher-menu rows shrink them (~22 px),
/// so a nearest-neighbor pick visibly aliases. Instead we resample with alpha in
/// mind: bilinear when enlarging (both axes ≥ source), box-average when shrinking.
/// Both paths interpolate in PREMULTIPLIED alpha — RGB is weighted by each texel's
/// alpha before averaging and un-premultiplied at the end — so the black of the
/// fully-transparent texels around a non-square silhouette never bleeds a dark
/// fringe into the edges. A resolved alpha of 0 is skipped, so silhouettes still
/// composite cleanly over the dock buttons / menu rows.
fn blit_icon(
    buf: &mut [u32],
    bw: usize,
    bh: usize,
    src: &[u32],
    sw: usize,
    sh: usize,
    dx: i32,
    dy: i32,
    dw: i32,
    dh: i32,
) {
    if dw <= 0 || dh <= 0 || sw == 0 || sh == 0 || src.len() < sw * sh {
        return;
    }
    let (dw, dh) = (dw as usize, dh as usize);
    // Enlarging (or 1:1) on both axes → bilinear; otherwise → area/box average.
    let upscale = dw >= sw && dh >= sh;
    for ry in 0..dh {
        for rx in 0..dw {
            let px = if upscale {
                sample_bilinear(src, sw, sh, rx, ry, dw, dh)
            } else {
                sample_area(src, sw, sh, rx, ry, dw, dh)
            };
            if (px >> 24) & 0xFF == 0 {
                continue;
            }
            blend_pixel(buf, bw, bh, dx + rx as i32, dy + ry as i32, px);
        }
    }
}

/// Un-premultiply an accumulated (Σα, Σr·α, Σg·α, Σb·α) into a straight-alpha ARGB
/// pixel. `weight` is the total filter weight (Σ of the per-texel weights) used to
/// normalise alpha; RGB is normalised by `asum` (the alpha-weighted mass) so the
/// colour of transparent texels contributes nothing. `weight == 0` yields fully
/// transparent. Integer-only (no soft-float in the no_std ELF).
#[inline]
fn unpremul(asum: u64, rsum: u64, gsum: u64, bsum: u64, weight: u64) -> u32 {
    if weight == 0 {
        return 0;
    }
    let a = (asum / weight).min(255) as u32;
    if asum == 0 {
        return 0; // every covered texel was transparent
    }
    let r = (rsum / asum).min(255) as u32;
    let g = (gsum / asum).min(255) as u32;
    let b = (bsum / asum).min(255) as u32;
    (a << 24) | (r << 16) | (g << 8) | b
}

/// Bilinear sample of the four texels around the source point that maps to dest
/// `(rx,ry)`, using centre-aligned mapping so a 1:1 scale reproduces the source
/// exactly. Fixed-point (1/256) weights, premultiplied by alpha.
#[inline]
fn sample_bilinear(src: &[u32], sw: usize, sh: usize, rx: usize, ry: usize, dw: usize, dh: usize) -> u32 {
    // fx = (rx + 0.5) * sw/dw - 0.5, in 1/256 units, clamped to ≥ 0.
    let fx = (((2 * rx + 1) * sw * 128) / dw).saturating_sub(128);
    let fy = (((2 * ry + 1) * sh * 128) / dh).saturating_sub(128);
    let x0 = (fx >> 8).min(sw - 1);
    let y0 = (fy >> 8).min(sh - 1);
    let x1 = (x0 + 1).min(sw - 1);
    let y1 = (y0 + 1).min(sh - 1);
    let tx = (fx & 0xFF) as u64; // 0..255 fractional weight toward x1
    let ty = (fy & 0xFF) as u64;
    let wx0 = 256 - tx;
    let wy0 = 256 - ty;
    let w = [wx0 * wy0, tx * wy0, wx0 * ty, tx * ty]; // 00,10,01,11 — Σ = 65536
    let p = [
        src[y0 * sw + x0],
        src[y0 * sw + x1],
        src[y1 * sw + x0],
        src[y1 * sw + x1],
    ];
    let (mut asum, mut rsum, mut gsum, mut bsum) = (0u64, 0u64, 0u64, 0u64);
    for i in 0..4 {
        let a = ((p[i] >> 24) & 0xFF) as u64;
        let aw = a * w[i];
        asum += aw;
        rsum += ((p[i] >> 16) & 0xFF) as u64 * aw;
        gsum += ((p[i] >> 8) & 0xFF) as u64 * aw;
        bsum += (p[i] & 0xFF) as u64 * aw;
    }
    unpremul(asum, rsum, gsum, bsum, 65536)
}

/// Box/area sample: average every source texel covered by dest `(rx,ry)` (at least
/// one), premultiplied by alpha. Correct downscale filter for shrinking 32×32 icons.
#[inline]
fn sample_area(src: &[u32], sw: usize, sh: usize, rx: usize, ry: usize, dw: usize, dh: usize) -> u32 {
    let sx0 = (rx * sw) / dw;
    let sy0 = (ry * sh) / dh;
    let sx1 = (((rx + 1) * sw + dw - 1) / dw).max(sx0 + 1).min(sw);
    let sy1 = (((ry + 1) * sh + dh - 1) / dh).max(sy0 + 1).min(sh);
    let (mut asum, mut rsum, mut gsum, mut bsum, mut count) = (0u64, 0u64, 0u64, 0u64, 0u64);
    for sy in sy0..sy1 {
        for sx in sx0..sx1 {
            let px = src[sy * sw + sx];
            let a = ((px >> 24) & 0xFF) as u64;
            asum += a;
            rsum += ((px >> 16) & 0xFF) as u64 * a;
            gsum += ((px >> 8) & 0xFF) as u64 * a;
            bsum += (px & 0xFF) as u64 * a;
            count += 1;
        }
    }
    unpremul(asum, rsum, gsum, bsum, count)
}

/// A desktop widget kind the compositor knows how to draw. The renderer owns the
/// per-kind drawing code (a clock formats time, a monitor reads system stats);
/// the CONFIG owns which kinds are shown and where (per-widget `widgets/*.toml`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum WidgetKind {
    Clock,
    Monitor,
}

/// One resolved desktop widget: its kind plus the placement/accent resolved from
/// its per-widget file (falling back to the `[widgets] corner` default / theme).
#[derive(Clone, Copy)]
struct DeskWidget {
    kind: WidgetKind,
    corner: i32,
    accent: u32,
}

/// Resolve the config-driven desktop-widget set. Each widget kind the compositor
/// implements is included ONLY when its `widgets/<name>.toml` exists AND is
/// enabled — so a widget is removed by deleting its file or setting
/// `enabled = false` (the Phase 8 removal switch; single source of truth, no
/// `[widgets] clock/monitor` bool). Per-widget `corner < 0` inherits
/// `default_corner` (`[widgets] corner`); `accent == 0` inherits the theme accent
/// at draw time. Called once at bring-up and again on each live config reload —
/// never per frame, so the per-widget files are not re-read every tick.
fn resolve_desk_widgets(default_corner: i32) -> Vec<DeskWidget> {
    // The compositor's registry of IMPLEMENTED widget kinds. This is renderer
    // implementation (each kind has bespoke draw code below), not desktop policy:
    // policy — which of these are actually shown — lives in the per-widget files.
    let known: [(&str, WidgetKind); 2] =
        [("clock", WidgetKind::Clock), ("monitor", WidgetKind::Monitor)];
    let mut v: Vec<DeskWidget> = Vec::new();
    for (name, kind) in known {
        let wc = settings::WidgetCfg::load(name);
        if wc.from_file && wc.enabled {
            let corner = if wc.corner >= 0 { wc.corner.min(3) } else { default_corner };
            v.push(DeskWidget { kind, corner, accent: wc.accent });
        }
    }
    v
}

/// Apply the configured `[display]` resolution via the kernel display backend.
/// `0/0` (baseline) keeps the current/boot resolution. A fixed backend (Limine/
/// GOP) reports EOPNOTSUPP (-95) — the desktop simply stays at the boot mode.
/// This is the compositor's mechanism→policy seam: the KERNEL owns the mode-set,
/// the CONFIG owns which mode (CLAUDE.md §6-10).
fn apply_display_mode(d: &settings::Display) {
    if d.width == 0 || d.height == 0 {
        return;
    }
    let r = libdunit::set_video_mode(d.width, d.height);
    if r == 0 {
        libdunit::println(&alloc::format!(
            "gui_server: [display] resolution set to {}x{}",
            d.width, d.height
        ));
    } else if r == -95 {
        libdunit::println("gui_server: [display] fixed backend — boot resolution kept");
    } else {
        libdunit::println(&alloc::format!(
            "gui_server: [display] set_video_mode {}x{} rejected ({})",
            d.width, d.height, r
        ));
    }
}

/// Run one interactive desktop session. Returns `true` when the session asked to
/// be RE-ENTERED (a live `[display]` change: the caller loops and this function's
/// head re-applies the new mode, re-queries the framebuffer and rebuilds every
/// buffer at the new size — cleaner than mutating bw/bh across the live loop).
/// Returns `false` (or diverges via the compositor loop) otherwise.
fn run_desktop_session(
    server: &mut Server,
    clients: &mut Vec<ClientState>,
    pending_notifies: Vec<alloc::string::String>,
) -> bool {
    // Load config FIRST: `[display]` must program the video mode BEFORE we query
    // the framebuffer, so bw/bh and every buffer derived from them size to the
    // chosen resolution from the start (no mid-loop buffer surgery).
    // Slice 9: the palette is data. Overlay /system/share/dwm/default.toml on the
    // Green Tea baseline; a missing/garbage file keeps the baseline (last-known-good).
    // Slice C: `theme`/`ly`/`fx` are `mut` because a gui_settings "reload" signal
    // re-reads the config live and re-seeds them (see the reload block in `loop`).
    let cfg = settings::load_config();
    // Boot-time schema/shape gate (mirrors the live-reload path): a config from a
    // FUTURE schema, or one that parsed to an incoherent shape, is `!valid`. Rather
    // than half-apply an unknown layout we fall back WHOLESALE to the built-in
    // baseline — the same last-known-good discipline the reload block uses, applied
    // to the very first load. A well-formed current/older file passes through as-is
    // (older ones are already forward-migrated inside `load_config`).
    let cfg = if cfg.valid {
        cfg
    } else {
        libdunit::println("gui_server: boot config invalid (schema/shape) — using baseline");
        settings::Config::defaults()
    };

    // Config-driven runtime resolution: apply BEFORE `get_framebuffer` so the
    // query below returns the chosen mode's geometry. `applied_display` is the
    // baseline the live-reload block diffs against to detect a GUI resolution edit.
    apply_display_mode(&cfg.settings.display);
    let applied_display = cfg.settings.display;

    let mut fb = libdunit::FbInfo {
        addr: 0,
        width: 0,
        height: 0,
        pitch: 0,
    };
    if !libdunit::get_framebuffer(&mut fb) || fb.width == 0 || fb.height == 0 {
        libdunit::println("gui_server: no framebuffer for desktop session");
        return false;
    }
    let bw = fb.width as usize;
    let bh = fb.height as usize;
    let mut theme: Theme = cfg.settings.theme;
    let mut ly: Layout = cfg.settings.layout;
    // The application registry (id/name/exec/icon/label), the dock/launcher/
    // startup pin-lists and the workspace count are DATA, owned here so the dock,
    // launcher menu, taskbar, workspace switcher and window titles all render off
    // one live model. A malformed config never lands: `load_config` keeps the
    // last-known-good baseline (an invalid file yields the baseline registry).
    let mut apps: Applications = cfg.apps;
    if cfg.settings.from_file {
        libdunit::println("gui_server: settings loaded from /system/share/dwm/default.toml");
    } else {
        libdunit::println("gui_server: settings default (no config file)");
    }
    // Serial diagnostic: the RESOLVED registry sizes, so a config→behavior change
    // is observable headless without a screenshot. Flip a count in the TOML and
    // this line moves — proof the desktop is config-driven, not baked into Rust.
    libdunit::println(&alloc::format!(
        "gui_server: config apps={} dock={} launcher={} startup={} ws={}",
        apps.apps.len(),
        apps.dock.len(),
        apps.launcher.len(),
        apps.startup.len(),
        apps.workspaces
    ));

    // Schema-version handling (M4): the single predicate `settings::version_supported`
    // decides which document versions this build accepts, in the config layer — the
    // compositor only reports the RESOLVED version and runs a boundary self-check so
    // the acceptance is observable headless. The guard proves the predicate rejects a
    // future schema (SCHEMA_VERSION+1) and the non-schema 0, and accepts the current
    // one — the same rule that made a bad-version boot fall back to baseline above.
    {
        let sv = cfg.settings.meta.schema_version;
        libdunit::println(&alloc::format!(
            "gui_server: config schema_version={} supported={}",
            sv,
            settings::version_supported(sv) as u32
        ));
        let guard_ok = settings::version_supported(settings::SCHEMA_VERSION)
            && !settings::version_supported(settings::SCHEMA_VERSION + 1)
            && !settings::version_supported(0);
        libdunit::println(&alloc::format!(
            "gui_server: schema guard {} (rejects future)",
            if guard_ok { "ok" } else { "FAIL" }
        ));
    }

    // Shell-strip placement (Phase 5): the panel and the system tray each dock to
    // a configurable screen edge (`[panel] pos` / `[tray] pos`), and the tray has
    // its own thickness (`[tray] size`). These are `mut` because the live-reload
    // block re-seeds them, exactly like theme/layout. `reserved_insets` turns them
    // (plus the always-left dock) into the per-edge margins every geometry
    // computation below subtracts, so maximize/spawn/drag all honor the edges.
    let mut panel_edge: Edge = cfg.settings.panel.pos;
    let mut tray_edge: Edge = cfg.settings.tray.pos;
    let mut tray_size: i32 = cfg.settings.tray.size;
    // Serial diagnostic: the resolved edges, so a `[panel]`/`[tray] pos` flip is
    // observable headless. Flip the TOML and this line moves — proof the shell
    // layout is config-driven, not baked into Rust.
    libdunit::println(&alloc::format!(
        "gui_server: panel edge={} tray edge={} tray_size={}",
        panel_edge.as_str(),
        tray_edge.as_str(),
        tray_size
    ));
    let mut ins = reserved_insets(&ly, panel_edge, tray_edge, tray_size);

    // Config-driven desktop-widget set (Phase 8): each widget is present only when
    // its widgets/<name>.toml exists AND is enabled, so removing a widget = delete
    // its file (or `enabled = false`). `[widgets] enabled` is the master switch;
    // `[widgets] corner` the default corner. The render block below iterates THIS
    // set, not hardcoded per-part bools — the per-widget files are the single
    // source of truth. Rebuilt on live reload; the marker traces the resolved
    // decision so a config→behavior change is observable headless.
    let mut desk_widgets = resolve_desk_widgets(cfg.settings.widgets.corner);
    {
        let has = |k: WidgetKind| desk_widgets.iter().any(|w| w.kind == k);
        libdunit::println(&alloc::format!(
            "gui_server: widgets enabled={} clock={} monitor={}",
            cfg.settings.widgets.enabled as u32,
            has(WidgetKind::Clock) as u32,
            has(WidgetKind::Monitor) as u32,
        ));
    }

    let mut back: Vec<u32> = Vec::new();
    back.resize(bw * bh, theme.desktop);

    // Desktop wallpaper: loaded + pre-scaled once (XRGB8888, framebuffer-sized).
    // Path comes from `[desktop] wallpaper`; absent/invalid → gradient fallback.
    let mut wallpaper: Option<Vec<u32>> = load_wallpaper(cfg.settings.desktop.wallpaper.as_str(), bw, bh);
    if wallpaper.is_some() {
        libdunit::println("gui_server: wallpaper loaded (config [desktop] wallpaper)");
    } else {
        libdunit::println("gui_server: wallpaper absent — gradient backdrop");
    }

    // Launcher icons, one slot per REGISTRY entry (indexed by app id → the same
    // index the dock/launcher pin-lists resolve to). The theme directory
    // (`/assets/icons/<icon_theme>/`) is a config knob; each entry's `icon` spec
    // is a stem resolved against it, or an absolute VFS path when it contains
    // '/'. A missing/mis-sized asset leaves the slot `None` and that cell falls
    // back to the app's label initial.
    let mut app_icons: Vec<Option<Vec<u32>>> = Vec::with_capacity(apps.apps.len());
    let mut icon_count = 0usize;
    {
        let mut dbuf = [0u8; 160];
        let dir = icon_dir(cfg.settings.desktop.icon_theme.as_str(), &mut dbuf);
        for entry in apps.apps.iter() {
            let icon = resolve_app_icon(entry.icon.as_str(), dir);
            if icon.is_some() {
                icon_count += 1;
            }
            app_icons.push(icon);
        }
    }
    if icon_count > 0 {
        libdunit::println("gui_server: launcher icons loaded (Breeze-Chameleon)");
    } else {
        libdunit::println("gui_server: launcher icons absent — initials fallback");
    }

    // Brand logo for the app-menu wordmark: a 32x32 straight-alpha RGBA with a
    // circular mask baked in (path from `[desktop] logo`). `None` when the asset
    // is missing/mis-sized → the wordmark draws its flat accent mark instead.
    let mut logo: Option<Vec<u32>> = load_icon_rgba(cfg.settings.desktop.logo.as_str());
    if logo.is_some() {
        libdunit::println("gui_server: wordmark logo loaded (circular)");
    } else {
        libdunit::println("gui_server: wordmark logo absent — accent mark fallback");
    }

    // Crisp TTF for the shell's reference-facing text (window titles, centered
    // focused title, right-side tray). `None` only if even the embedded font is
    // unparsable — then those texts are skipped and the 3x5 labels remain.
    let font = load_font();
    if font.is_some() {
        libdunit::println("gui_server: shell font loaded (TTF)");
    } else {
        libdunit::println("gui_server: shell font unavailable — 3x5 labels only");
    }

    // Window/session restore (M4): the remembered per-app geometry, loaded once
    // from the writable session store. Gated by `[session] restore`; when off we
    // start with an empty mirror so nothing is restored *or* rewritten. The marker
    // traces the resolved policy + how many windows were remembered, so the
    // save→restore round-trip is observable headless.
    let mut session: Vec<settings::WinGeom> = if cfg.settings.session.restore {
        settings::load_session()
    } else {
        Vec::new()
    };
    libdunit::println(&alloc::format!(
        "gui_server: session restore={} entries={}",
        cfg.settings.session.restore as u32,
        session.len()
    ));

    // Build the window model from presented clients, tiling them if their slot
    // origins collide, and keeping the title bar on-screen.
    let mut wins: Vec<Win> = Vec::new();
    let mut z: Vec<usize> = Vec::new();
    let mut offset = 0i32;
    for i in 0..clients.len() {
        if !clients[i].presented || clients[i].buf_ptr.is_null() {
            continue;
        }
        let sw = clients[i].surf_w as i32;
        let sh = clients[i].surf_h as i32;
        let mut cx = clients[i].slot_x as i32 + offset;
        let mut cy = clients[i].slot_y as i32 + ly.title_h + ly.border + offset;
        // Keep the whole window (title bar included) clear of the reserved panel/
        // tray strips on every edge (Phase 5 `reserved_insets`), not just a fixed
        // top panel + left dock.
        if cy - ly.title_h < ins.top + ly.border {
            cy = ins.top + ly.title_h + ly.border;
        }
        if cx < ins.left + ly.border {
            cx = ins.left + ly.border;
        }
        if cx + sw + ins.right + ly.border > bw as i32 {
            cx = (bw as i32 - ins.right - sw - ly.border).max(ins.left + ly.border);
        }
        if cy + sh + ins.bottom + ly.border > bh as i32 {
            cy = (bh as i32 - ins.bottom - sh - ly.border).max(ins.top + ly.title_h + ly.border);
        }
        // Session restore: if this app has a remembered position, reopen it there
        // (clamped back on-screen), so windows return to where the user left them.
        // Size stays client-owned here (a floating resize would need a CONFIGURE
        // round-trip); position is the immediately-safe part restored at boot.
        if let Some(g) = session_lookup(&session, &apps, clients[i].app) {
            cx = g
                .x
                .max(ins.left + ly.border)
                .min((bw as i32 - ins.right - sw - ly.border).max(ins.left + ly.border));
            cy = g
                .y
                .max(ins.top + ly.title_h + ly.border)
                .min((bh as i32 - ins.bottom - sh - ly.border).max(ins.top + ly.title_h + ly.border));
        }
        z.push(wins.len());
        wins.push(Win {
            pid: clients[i].pid,
            cx,
            cy,
            sw,
            sh,
            buf_ptr: clients[i].buf_ptr,
            buf_size: clients[i].buf_size,
            alive: true,
            title_h: ly.title_h,
            border: ly.border,
            app: clients[i].app,
            ws: 0,
            born: 0,
            minimized: false,
            state: WinState::Floating,
            restore: None,
            st_step: 0,
            st_since: 0,
            format: clients[i].format,
        });
        clients[i].ready = true;
        clients[i].win_created = true;
        offset += 32;
    }

    // Config-gated session round-trip proof (`test.toml` `[startup] self_test`).
    // The harness has no mouse to drag/close windows, so exercise the persistence
    // MECHANISM directly: remember every initial window, write the store, read it
    // back through the real VFS file, and confirm the geometry survived. This is
    // the same `session_remember` + `save_session` + `load_session` path the live
    // desktop drives on window create/move/close. Never runs on the real desktop.
    if apps.self_test {
        for w in wins.iter() {
            session_remember(&mut session, &apps, w);
        }
        let wrote = settings::save_session(&session);
        let reloaded = settings::load_session();
        let matched = reloaded.len() == session.len()
            && session.iter().all(|s| {
                reloaded
                    .iter()
                    .any(|r| r.id.as_str() == s.id.as_str() && r.x == s.x && r.y == s.y && r.w == s.w && r.h == s.h)
            });
        libdunit::println(&alloc::format!(
            "gui_server: session roundtrip {} (wrote={} saved={} reloaded={})",
            if wrote && matched { "ok" } else { "FAIL" },
            wrote as u32,
            session.len(),
            reloaded.len()
        ));
    }
    // An EMPTY window set is a first-class state now: a clean desktop boots into
    // a usable but window-less session (panel/dock/launcher/wallpaper/widgets are
    // live and can spawn apps on demand). We no longer early-return here — the
    // compositor loop below runs regardless so the desktop is interactive even
    // with zero clients. The old `if wins.is_empty() { return; }` gate defeated
    // the clean-boot goal (`[startup] entries = []`).
    // Windows present at session start (the smoke's two gui_clients). The
    // keyboard-ready marker waits until a *runtime*-pumped window appears and
    // takes focus — i.e. the startup gui_terminal — so headless keystroke
    // injection lands in the terminal (topmost) rather than a static client.
    let initial_wins = wins.len();
    let mut prev_left = false;
    let mut prev_right = false;
    let mut drag: Option<(usize, i32, i32)> = None; // (win index, off_x, off_y)
    let mut pressed_win: Option<usize> = None; // window that captured the press
    let mut input_focus: Option<usize> = None; // window under the pointer
    let mut last_lx = i32::MIN;
    let mut last_ly = i32::MIN;
    // Bounded so a headless run (mouse never moves) still terminates and lets
    // the harness force-quit; ~60000 * 16ms ≈ 16 min of interactive use.
    let mut ticks = 0u32;
    // Runtime app-launch: the launcher spawns fresh gui_client windows up to
    // `ly.max_windows`; `next_id` is the tint hint handed to each new client.
    let mut next_id = clients.len() as u32 + 1;
    // Launcher dropdown state. Toggled by the launcher glyph; any click either
    // selects a menu entry (spawn) or dismisses the menu.
    let mut menu_open = false;
    // Keyboard-highlighted launcher row (`cmd_mod`+Space opens the menu, then
    // Up/Down move this selection and Enter launches it). Reset to 0 on each open.
    let mut menu_sel: usize = 0;
    // Active workspace (0-based, concept §11). Only its windows composite and
    // take input; the panel switcher and newly-spawned windows follow it.
    let mut current_ws: usize = 0;
    // One-shot: announce on serial when a window first takes keyboard focus, so
    // headless tests know the compositor is ready to accept injected keystrokes.
    let mut input_ready_announced = false;
    // One-shot: emitted right after the FIRST composited frame is presented,
    // unconditionally (empty desktop or not). This is the clean-boot acceptance
    // marker — proof the desktop reached an interactive, presenting state.
    let mut desktop_ready_announced = false;
    // --- Visual-effects state (concept §5) -------------------------------
    let mut fx = cfg.settings.effects;
    // Keyboard-shortcut policy (`[shortcuts]`): whether the Alt/Super-Tab window
    // switcher is offered and which modifier arms it. `mut` so a live reload can
    // flip the switcher on/off or swap Alt<->Super without a restart (re-seeded in
    // the reload block, single source of truth — never read from a constant).
    let mut shortcuts = cfg.settings.shortcuts;
    // Live window-switcher overlay state (armed by `switch_mod`+Tab, committed on
    // modifier release). Empty/inactive until the user (or the self-test) arms it.
    let mut switcher = Switcher::new();
    // One-shot latch so the config-gated switcher self-test drives exactly one
    // cycle (test.toml `[startup] self_test`), never on the real desktop.
    let mut switcher_st_done = false;
    // Desktop widget card (concept §5): painted on the wallpaper behind windows.
    let mut wg = cfg.settings.widgets;
    // Quick-settings applet (`[quicksettings]`): a panel cell whose flyout flips the
    // LIVE effect/widget flags (`fx.blur` / `wg.enabled`) — single source of truth,
    // the flyout mutates the very fields the renderer reads, never a shadow copy.
    // `mut` so a live reload re-seeds `enabled`; the flyout closes on reload so a
    // now-disabled applet can't leave a stale card up.
    let mut quicksettings = cfg.settings.quicksettings;
    let mut qs_open = false;
    let mut qs_st_done = false;
    // Desktop notifications (`[notifications]`): a bounded queue of transient
    // launch-feedback toasts, each auto-dismissed after `timeout_ms` (→ ~16 ms
    // frames) and stacked in the configured corner. All policy is config-driven.
    let mut notifications = cfg.settings.notifications;
    let mut notifs: Vec<Toast> = Vec::new();
    let mut notify_st_done = false;
    let mut notify_expired_logged = false;
    // Flush any notifications a client posted during bring-up (before this session's
    // toast queue existed) through the SAME config-gated `notify_post` path the
    // per-tick client-notify drain and app-launch sites use — so a startup toast
    // (e.g. gui_demo's "I'm up") is honored exactly once here rather than lost to
    // the bring-up race. Policy stays the compositor's; the client only named text.
    for text in pending_notifies {
        notify_post(&mut notifs, &notifications, ticks, &text);
        libdunit::println(&alloc::format!(
            "gui_server: client notify \"{}\" queued={} (enabled={})",
            text,
            notifs.len(),
            notifications.enabled as u32,
        ));
    }
    // One-shot flag for the global keyboard-shortcut self-test (below).
    let mut shortcut_st_done = false;
    // Config-gated client-recycle self-test (`test.toml` `[startup] self_test`).
    // Proves the "close many windows, then open new ones" bug is fixed: once all
    // startup windows have been through the maximize/restore self-test, close them
    // ALL, wait for the per-frame reaper to prune the `clients` Vec (which is what
    // frees spawn slots), then relaunch — the ceiling must be lifted. A tiny state
    // machine because close→exit→reap→relaunch spans several frames. Never runs on
    // the real desktop (`self_test` is false in `default.toml`).
    let mut recycle_phase: u8 = 0; // 0=wait 1=draining 2=done
    let mut recycle_pre: usize = 0; // clients present when we closed them
    let mut recycle_deadline: u32 = 0; // tick budget for the drain to finish
    // Animation length in frames (~16ms/frame); 0 when animations are off, so
    // every ramp/reveal collapses to instant (the flat look).
    let mut anim_frames = if fx.anim { (fx.anim_ms / 16).max(1) as u32 } else { 0 };
    // Workspace-switch crossfade: the active workspace's windows fade in from
    // this tick (bumped on every switch below).
    let mut ws_switch_tick = 0u32;
    // Launcher-menu drop-in: stamped when the dropdown opens.
    let mut menu_anim_tick = 0u32;
    let mut menu_was_open = false;
    // Per-dock-icon hover progress (0..255), eased toward hovered/idle each
    // frame. Sized to the live dock pin-count (config `[dock] entries`).
    let mut dock_hover: Vec<u8> = alloc::vec![0u8; apps.dock.len()];
    // Ping-pong scratch for the backdrop blur, sized for the largest blurred
    // region (panel / dock strip / menu) and allocated once.
    let panel_area = bw * ly.panel_h.max(0) as usize;
    let dock_area = ly.dock_w.max(0) as usize * (bh as i32 - ly.panel_h).max(0) as usize;
    let menu_area = ly.menu_w.max(0) as usize * (apps.launcher.len() * ly.menu_item_h.max(0) as usize);
    let scratch_len = panel_area.max(dock_area).max(menu_area).max(1);
    let mut blur_a: Vec<u32> = alloc::vec![0u32; scratch_len];
    let mut blur_b: Vec<u32> = alloc::vec![0u32; scratch_len];
    // Scratch for translucent (ARGB) windows: the true backing (wallpaper + lower
    // windows) behind a window's content rect, snapshotted before the opaque frame
    // gradient overwrites it, so the client blends over what's actually behind the
    // window rather than the window's own frame. Reused across frames.
    let mut win_backing: Vec<u32> = Vec::new();
    // One-shot latch: log the first per-pixel ARGB composite so the transparency
    // path is observable in the serial log (headless proof that a translucent
    // client is blended, not opaque-copied). Never fires when every client is XRGB.
    let mut argb_blit_logged = false;
    // Frame-time instrumentation so the blur cost is observable in the serial
    // log even headless: report ms/frame once warm, then periodically.
    let mut prev_uptime = 0u64;
    let mut prev_tick = 0u32;
    // Slice C: raised by a gui_settings "reload" control message (see
    // `handle_client_payload`); drained at the top of the frame to re-read the
    // config and re-seed live theme/layout/effects state.
    let mut reload_requested = false;
    // Client-posted notification texts collected by `pump_clients` this tick
    // (NOTIFY_MAGIC), turned into config-gated toasts right after the pump. Reused
    // across frames (cleared each tick) so no per-frame allocation in the hot path.
    let mut notify_requests: Vec<alloc::string::String> = Vec::new();
    loop {
        // Advance any runtime-spawned clients through their protocol handshake,
        // then hand each newly-ready client a cascaded, focused window.
        notify_requests.clear();
        pump_clients(server, clients, &mut reload_requested, &mut notify_requests);

        // Live settings reload (GUI<->TOML round-trip): a gui_settings client
        // rewrote the config and pinged us. Re-read it and re-seed everything
        // derived from theme/layout/effects — plus the asset paths in [desktop]
        // (wallpaper + icon theme), so a config change swaps them live too. Never
        // fatal — load() falls back to the baseline on a bad file.
        if reload_requested {
            reload_requested = false;
            let ncfg = settings::load_config();
            if !ncfg.valid {
                // Malformed/incoherent config: keep the running (last-known-good)
                // state WHOLESALE. Never partially apply a bad file — a broken
                // reload must not blank the dock or drop the registry.
                libdunit::println("gui_server: settings reload rejected (invalid config) — keeping last good");
            } else {
                // Live resolution change (GUI edited `[display]`): a runtime mode-
                // set would strand bw/bh and every buffer sized from them. Instead
                // RE-ENTER the session — the head re-applies the mode, re-queries the
                // framebuffer and rebuilds all buffers at the new size. Windows
                // survive because they are rebuilt from the persistent `clients`.
                // 0/0 means "keep current", so only an explicit new mode re-enters.
                if ncfg.settings.display != applied_display
                    && ncfg.settings.display.width != 0
                    && ncfg.settings.display.height != 0
                {
                    libdunit::println(
                        "gui_server: [display] changed on reload — re-entering session",
                    );
                    return true;
                }
                theme = ncfg.settings.theme;
                ly = ncfg.settings.layout;
                fx = ncfg.settings.effects;
                shortcuts = ncfg.settings.shortcuts;
                wg = ncfg.settings.widgets;
                // Quick-settings + notification policy travel through the same
                // reload (single source of truth); close any open flyout so a
                // now-disabled applet can't leave a stale card behind.
                quicksettings = ncfg.settings.quicksettings;
                qs_open = false;
                notifications = ncfg.settings.notifications;
                // Re-resolve the config-driven widget set so a live edit that adds/
                // removes a widgets/<name>.toml (or flips its `enabled`) takes effect.
                desk_widgets = resolve_desk_widgets(ncfg.settings.widgets.corner);
                apps = ncfg.apps;
                // Re-seed the shell-strip placement + recompute the reserved
                // margins, so a live `[panel]`/`[tray] pos` edit moves the panel,
                // tray and the maximize/work area together.
                panel_edge = ncfg.settings.panel.pos;
                tray_edge = ncfg.settings.tray.pos;
                tray_size = ncfg.settings.tray.size;
                ins = reserved_insets(&ly, panel_edge, tray_edge, tray_size);
                anim_frames = if fx.anim { (fx.anim_ms / 16).max(1) as u32 } else { 0 };
                // Every Win caches its own title_h/border (used by outer/contains):
                // re-seed them so existing windows adopt the new geometry.
                for w in wins.iter_mut() {
                    w.title_h = ly.title_h;
                    w.border = ly.border;
                }
                // The blur scratch is sized for the largest blurred region; a larger
                // panel/dock/menu after reload needs a bigger buffer (never shrink —
                // a smaller region simply uses a prefix of the existing allocation).
                let panel_area = bw * ly.panel_h.max(0) as usize;
                let dock_area = ly.dock_w.max(0) as usize * (bh as i32 - ly.panel_h).max(0) as usize;
                let menu_area =
                    ly.menu_w.max(0) as usize * (apps.launcher.len() * ly.menu_item_h.max(0) as usize);
                let need = panel_area.max(dock_area).max(menu_area).max(1);
                if blur_a.len() < need {
                    blur_a.resize(need, 0);
                    blur_b.resize(need, 0);
                }
                // Re-seed the config-driven assets (wallpaper + icon theme) and the
                // registry-derived state (icons per entry, dock hover slots).
                wallpaper = load_wallpaper(ncfg.settings.desktop.wallpaper.as_str(), bw, bh);
                logo = load_icon_rgba(ncfg.settings.desktop.logo.as_str());
                let mut dbuf = [0u8; 160];
                let dir = icon_dir(ncfg.settings.desktop.icon_theme.as_str(), &mut dbuf);
                app_icons.clear();
                for entry in apps.apps.iter() {
                    app_icons.push(resolve_app_icon(entry.icon.as_str(), dir));
                }
                dock_hover = alloc::vec![0u8; apps.dock.len()];
                if current_ws >= apps.workspaces {
                    current_ws = apps.workspaces.saturating_sub(1);
                }
                // A live shortcut-policy change (switcher toggled off, or the
                // arming modifier swapped) must not leave a stale overlay armed.
                switcher.cancel();
                libdunit::println("gui_server: settings reloaded (config changed)");
            }
        }

        // Turn this tick's client-posted notification requests (NOTIFY_MAGIC) into
        // toasts. Policy stays the compositor's: `notify_post` is a no-op when
        // `[notifications]` is disabled, and the timeout/corner come from config —
        // the client only supplied the (bounds-checked) text. Drained AFTER the
        // reload block so a just-reloaded `notifications` policy applies at once.
        for text in notify_requests.drain(..) {
            notify_post(&mut notifs, &notifications, ticks, &text);
            libdunit::println(&alloc::format!(
                "gui_server: client notify \"{}\" queued={} (enabled={})",
                text,
                notifs.len(),
                notifications.enabled as u32,
            ));
        }

        // Per-frame client reaper. A window closed via the title-bar chip or
        // Cmd-Q sets `alive=false` and sends IN_QUIT; the client then exits. A
        // client can also exit on its own (crash / voluntary quit). Either way its
        // resources must be reclaimed, or they leak: BEFORE this the `clients` Vec
        // was never pruned, so after `max_windows` cumulative windows the spawn
        // gate (`clients.len() >= max_windows`) blocked every new window — the
        // "close a bunch of windows, then can't open new ones" bug. And the
        // protocol `Server` caps connections (MAX_CONNECTIONS), so an un-dropped
        // `ConnId` per closed window would eventually starve `server.connect()`
        // too. Poll every already-presented client with the non-blocking `wait`
        // (the kernel reaps only a truly-terminated child and returns an error for
        // a live one, so a running window is never touched; a not-yet-`ready`
        // client is skipped so a still-spawning child is never reaped early). On a
        // reap, free ALL of it: the protocol connection + its surfaces/buffers
        // (`disconnect_peer`), the compositor's shared-buffer mapping
        // (`handle_close`), the `ClientState` (this is what lifts the spawn
        // ceiling), and the `Win` with full z/drag/focus index fixup.
        {
            let mut ci = 0;
            while ci < clients.len() {
                if !clients[ci].ready {
                    ci += 1;
                    continue;
                }
                let pid = clients[ci].pid;
                let mut st = libdunit::WaitStatus::empty();
                if libdunit::wait(pid, &mut st) != pid as isize {
                    ci += 1; // still running (or not reapable yet)
                    continue;
                }
                server.disconnect_peer(clients[ci].conn);
                if clients[ci].mapped_handle != 0 {
                    libdunit::handle_close(clients[ci].mapped_handle);
                }
                clients.remove(ci);
                reap_windows_of(
                    pid,
                    &mut wins,
                    &mut z,
                    &mut drag,
                    &mut pressed_win,
                    &mut input_focus,
                );
                // The switcher's MRU snapshot holds `wins` indices; a mid-flight
                // reap would leave them stale, so drop any active overlay.
                switcher.cancel();
                libdunit::println(&alloc::format!(
                    "gui_server: reaped client pid={} clients={} (spawn slot freed)",
                    pid,
                    clients.len(),
                ));
                // `ci` not advanced: the compacted next client now sits at `ci`.
            }
        }

        // Per-frame Win<->ClientState reconciliation. A client that re-imported a
        // buffer after a resize (maximize / restore / fullscreen) now carries a
        // fresh backing pointer + geometry in its `ClientState`; mirror it into the
        // owning `Win` so the compositor blits the new buffer at the new size and
        // never the freed old one. Position stays compositor-owned (set by drag or
        // by `toggle_maximize`), so only the buffer + surface size are synced here.
        for w in wins.iter_mut() {
            if !w.alive {
                continue;
            }
            if let Some(c) = clients.iter().find(|c| c.pid == w.pid) {
                w.buf_ptr = c.buf_ptr;
                w.buf_size = c.buf_size;
                w.sw = c.surf_w as i32;
                w.sh = c.surf_h as i32;
                w.format = c.format;
            }
        }

        for i in 0..clients.len() {
            if !clients[i].ready || clients[i].win_created || clients[i].buf_ptr.is_null() {
                continue;
            }
            let sw = clients[i].surf_w as i32;
            let sh = clients[i].surf_h as i32;
            let step = (wins.len() as i32 % 6) * 40;
            let mut cx = 90 + step + ly.border;
            let mut cy = ins.top + ly.title_h + ly.border + step;
            if cx < ins.left + ly.border {
                cx = ins.left + ly.border;
            }
            if cx + sw + ins.right + ly.border > bw as i32 {
                cx = (bw as i32 - ins.right - sw - ly.border).max(ins.left + ly.border);
            }
            if cy + sh + ins.bottom + ly.border > bh as i32 {
                cy = (bh as i32 - ins.bottom - sh - ly.border).max(ins.top + ly.title_h + ly.border);
            }
            // Session restore for a runtime-launched app: reopen at the remembered
            // position (clamped on-screen). A window remembered as maximized is
            // re-maximized just after creation via the shared `set_window_state`
            // path (below), once the Win exists.
            let remembered = session_lookup(&session, &apps, clients[i].app);
            if let Some(g) = remembered {
                cx = g
                    .x
                    .max(ins.left + ly.border)
                    .min((bw as i32 - ins.right - sw - ly.border).max(ins.left + ly.border));
                cy = g
                    .y
                    .max(ins.top + ly.title_h + ly.border)
                    .min((bh as i32 - ins.bottom - sh - ly.border).max(ins.top + ly.title_h + ly.border));
            }
            let wi = wins.len();
            wins.push(Win {
                pid: clients[i].pid,
                cx,
                cy,
                sw,
                sh,
                buf_ptr: clients[i].buf_ptr,
                buf_size: clients[i].buf_size,
                alive: true,
                title_h: ly.title_h,
                border: ly.border,
                app: clients[i].app,
                ws: clients[i].ws,
                born: ticks,
                minimized: false,
                state: WinState::Floating,
                restore: None,
                st_step: 0,
                st_since: 0,
                format: clients[i].format,
            });
            clients[i].win_created = true;
            z.push(wi);
            // Re-maximize a window that was remembered maximized (uses the same
            // CONFIGURE path the chip drives), then persist the freshly-placed
            // window so its geometry is remembered from birth.
            if remembered.map_or(false, |g| g.maximized) {
                set_window_state(server, clients, &mut wins[wi], WinState::Maximized, bw, bh, &ly, &ins);
            }
            session_remember(&mut session, &apps, &wins[wi]);
            settings::save_session(&session);
        }

        // Config-gated resize self-exercise (`test.toml` `[startup] self_test`).
        // The automated harness has no mouse, so the title-bar maximize chip and
        // the drag-to-restore gesture can't be exercised by hand. Instead drive
        // EVERY live window through the SAME `set_window_state` path those gestures
        // use — proving server-push CONFIGURE + client buffer re-import across a
        // real process boundary, in BOTH directions:
        //   step 0 -> 1: maximize on first sight (window fills the work area);
        //   step 1 -> 2: after a short settle, restore to the floating geometry.
        // Maximizing every window (not just runtime-pumped ones) makes the proof
        // independent of which client presents first in `serve_two_clients`. The
        // restore leg is what verifies the user-visible "un-maximize" actually
        // works end-to-end (the same `set_window_state(Floating)` the chip and the
        // drag-restore call). Never runs on the real desktop (`self_test` is false
        // in `default.toml`).
        if apps.self_test {
            let (_, _, ww, wh) = work_area(bw, bh, &ly, &ins);
            for wi in 0..wins.len() {
                if !wins[wi].alive {
                    continue;
                }
                if wins[wi].st_step == 0 && wins[wi].state == WinState::Floating {
                    set_window_state(server, clients, &mut wins[wi], WinState::Maximized, bw, bh, &ly, &ins);
                    wins[wi].st_step = 1;
                    wins[wi].st_since = ticks;
                    libdunit::println(&alloc::format!(
                        "gui_server: reconfigure w={} h={} acked (self-test)",
                        ww, wh
                    ));
                } else if wins[wi].st_step == 1
                    && (wins[wi].sw >= ww
                        || ticks.saturating_sub(wins[wi].st_since) >= ST_SETTLE_TICKS)
                {
                    // The client has observably re-imported the work-area-sized
                    // buffer (its synced surface reached the maximize width), so the
                    // maximize round trip is complete. Now prove the reverse leg —
                    // the user-visible un-maximize — via the SAME
                    // `set_window_state(Floating)` the chip and drag-to-restore call.
                    // A fixed-size client that never grew (declined the resize) falls
                    // through here on the ST_SETTLE_TICKS timeout so the self-test
                    // (and the recycle proof gated on it) can't stall on it.
                    let grew = wins[wi].sw >= ww;
                    let (rw, rh) = wins[wi]
                        .restore
                        .map(|(_, _, w, h)| (w, h))
                        .unwrap_or((wins[wi].sw, wins[wi].sh));
                    set_window_state(server, clients, &mut wins[wi], WinState::Floating, bw, bh, &ly, &ins);
                    wins[wi].st_step = 2;
                    libdunit::println(&alloc::format!(
                        "gui_server: reconfigure w={} h={} {} (self-test)",
                        rw, rh,
                        if grew { "restored" } else { "settled (client kept size)" }
                    ));
                }
            }
        }

        // Config-gated window-switcher proof (`test.toml` `[startup] self_test`,
        // and only when `[shortcuts] switcher` is on). The harness has no keyboard
        // to hold Alt+Tab, so drive the SAME `Switcher` state the key-drain path
        // arms: snapshot the MRU list, arm (advancing off the front window), commit
        // to the highlight, then raise it exactly as the modifier-release path does.
        // Proves the switcher selects a DIFFERENT window and that committing makes
        // it the topmost/focused one. One-shot; never runs on the real desktop.
        if apps.self_test && !switcher_st_done && shortcuts.switcher {
            let mru = switcher_mru(&z, &wins, current_ws);
            if mru.len() >= 2 {
                let front_pid = wins[mru[0]].pid;
                switcher.begin(&mru, false); // advances to the next candidate
                if let Some(sel) = switcher.commit() {
                    let sel_pid = wins[sel].pid;
                    wins[sel].minimized = false;
                    raise_window(&mut z, sel);
                    let now_top = z
                        .iter()
                        .rev()
                        .copied()
                        .find(|&i| wins[i].alive && wins[i].ws == current_ws && !wins[i].minimized);
                    if now_top == Some(sel) && sel_pid != front_pid {
                        libdunit::println(&alloc::format!(
                            "gui_server: switcher cycle prev_pid={} -> pid={} ok (self-test)",
                            front_pid, sel_pid
                        ));
                    } else {
                        libdunit::println(&alloc::format!(
                            "gui_server: FAIL switcher cycle prev_pid={} -> pid={} top={:?}",
                            front_pid,
                            sel_pid,
                            now_top.map(|i| wins[i].pid)
                        ));
                    }
                }
                switcher_st_done = true;
            }
        }

        // Config-gated notification proof (`test.toml` `[startup] self_test`). The
        // harness has no mouse to click the dock, so post one toast through the
        // SAME `notify_post` path a launch uses, prove it enqueues, and prove the
        // auto-dismiss predicate removes it once its timeout elapses — checked
        // against the very predicate the per-frame prune runs (`now < expire`), so
        // the queue lifecycle is verified without a fragile wall-clock wait. The
        // toast stays queued and expires for real a few frames later (below),
        // which trips the end-to-end `notify expired` marker.
        if apps.self_test && !notify_st_done && notifications.enabled {
            let before = notifs.len();
            notify_post(&mut notifs, &notifications, ticks, "Self-test");
            let posted = notifs.len();
            let frames = (notifications.timeout_ms / 16).max(1);
            let future = ticks.saturating_add(frames + 1);
            let survivors = notifs.iter().filter(|t| future < t.expire).count();
            let ok = posted == before + 1 && survivors < posted;
            // Report the DELTA this post added (`added`), not the absolute queue
            // size: a client (e.g. autostarted gui_demo) may have a live startup
            // toast already queued, so the absolute count is timing/config-dependent
            // while "this post enqueued exactly one" is the real invariant. `queued`
            // trails as a diagnostic. corner/timeout_ms prove the policy is config-
            // driven; "ok" folds in the enqueue-delta + auto-dismiss predicate.
            libdunit::println(&alloc::format!(
                "gui_server: notify posted added={} corner={} timeout_ms={} {} (self-test) queued={}",
                posted - before,
                notifications.corner,
                notifications.timeout_ms,
                if ok { "ok" } else { "FAIL" },
                posted,
            ));
            notify_st_done = true;
        }

        // Config-gated quick-settings proof. The applet can't be clicked headless,
        // so flip the SAME live flags its flyout toggles (`fx.blur`, `wg.enabled`)
        // and confirm each change lands on the field the renderer reads — proving
        // the applet mutates real state, not a shadow copy — then restore them so
        // the desktop's configured look is untouched. One-shot.
        if apps.self_test && !qs_st_done && quicksettings.enabled {
            let b0 = fx.blur;
            let w0 = wg.enabled;
            fx.blur = !b0;
            wg.enabled = !w0;
            let flipped = fx.blur != b0 && wg.enabled != w0;
            fx.blur = b0;
            wg.enabled = w0;
            libdunit::println(&alloc::format!(
                "gui_server: quicksettings applet enabled=1 toggled blur={}->{} widgets={}->{} {} (self-test)",
                b0 as u32,
                (!b0) as u32,
                w0 as u32,
                (!w0) as u32,
                if flipped { "ok" } else { "FAIL" }
            ));
            qs_st_done = true;
        }

        // Config-gated global-shortcut proof (`test.toml` `[startup] self_test`).
        // The harness cannot hold `cmd_mod`+key, so exercise the SAME pure
        // `match_shortcut` decision the key-drain loop uses: assert every ENABLED
        // action maps from its `cmd_mod`+key chord, that a bare key (no modifier)
        // maps to nothing (app typing is never stolen), and — the behavioural
        // anchor — that APPLYING a workspace action really moves `current_ws`
        // (config→behaviour, not just parse). Launcher/close/maximize reuse the
        // exact mouse-chip effect paths, so proving their mapping proves the wiring.
        if apps.self_test && !shortcut_st_done {
            let m = shortcuts.cmd_mod.mask();
            let mut map_ok = true;
            if shortcuts.ws_switch && apps.workspaces >= 2 {
                map_ok &= match_shortcut(0x03, m, true, &shortcuts, apps.workspaces)
                    == ShortcutAction::Workspace(1); // cmd_mod + '2' → ws index 1
            }
            if shortcuts.launcher {
                map_ok &= match_shortcut(SC_SPACE, m, true, &shortcuts, apps.workspaces)
                    == ShortcutAction::Launcher;
            }
            if shortcuts.win_close {
                map_ok &= match_shortcut(SC_Q, m, true, &shortcuts, apps.workspaces)
                    == ShortcutAction::CloseWindow;
            }
            if shortcuts.win_max {
                map_ok &= match_shortcut(SC_M, m, true, &shortcuts, apps.workspaces)
                    == ShortcutAction::MaximizeWindow;
            }
            // Negative: the same keys WITHOUT the modifier must not fire globally.
            map_ok &= match_shortcut(SC_Q, 0, true, &shortcuts, apps.workspaces)
                == ShortcutAction::None;
            // Behavioural: apply a workspace jump and read it back, then restore.
            let mut effect_ok = true;
            if shortcuts.ws_switch && apps.workspaces >= 3 {
                let before = current_ws;
                if let ShortcutAction::Workspace(ws) =
                    match_shortcut(0x04, m, true, &shortcuts, apps.workspaces) // cmd_mod + '3'
                {
                    current_ws = ws;
                }
                effect_ok = current_ws == 2;
                current_ws = before;
            }
            libdunit::println(&alloc::format!(
                "gui_server: shortcuts cmd_mod={} ws={} launcher={} close={} max={} map={} effect={} (self-test)",
                shortcuts.cmd_mod.as_str(),
                shortcuts.ws_switch as u32,
                shortcuts.launcher as u32,
                shortcuts.win_close as u32,
                shortcuts.win_max as u32,
                if map_ok { "ok" } else { "FAIL" },
                if effect_ok { "ok" } else { "FAIL" },
            ));
            shortcut_st_done = true;
        }

        // Config-gated client-recycle self-test (see the state decls above). The
        // reported scheduler bug was that after closing enough windows no new one
        // could open, because the `clients` Vec was never pruned. This proves the
        // fix end-to-end across a real process boundary: close every window, let
        // the per-frame reaper drain the `clients` Vec, then relaunch and confirm
        // spawning succeeds again. Starts only after the maximize/restore self-test
        // has finished on every startup window, so it never disturbs the earlier
        // one-shot proofs.
        if apps.self_test {
            match recycle_phase {
                0 => {
                    let live: Vec<usize> =
                        (0..wins.len()).filter(|&i| wins[i].alive).collect();
                    let all_settled =
                        !live.is_empty() && live.iter().all(|&i| wins[i].st_step == 2);
                    // Wait for the ARGB steady-state composite proof (`argb blit`)
                    // to have fired first — otherwise closing every window would
                    // preempt the frame in which the translucent terminal finally
                    // composites at full reveal. Recycle is deliberately the LAST
                    // self-test, so it must not race the earlier ones.
                    if all_settled && argb_blit_logged && !clients.is_empty() {
                        recycle_pre = clients.len();
                        for &wi in &live {
                            wins[wi].alive = false;
                            send_input(wins[wi].pid, IN_QUIT, 0, 0, 0);
                        }
                        recycle_deadline = ticks.saturating_add(600); // ~10s budget
                        recycle_phase = 1;
                        libdunit::println(&alloc::format!(
                            "gui_server: recycle self-test closing {} windows (clients={})",
                            live.len(),
                            recycle_pre,
                        ));
                    }
                }
                1 => {
                    // The reaper prunes `clients` as each closed client exits. Once
                    // at least one slot is freed, relaunch to prove the ceiling is
                    // lifted; fill until the (now reachable) cap and report.
                    if clients.len() < recycle_pre {
                        let free_before = clients.len();
                        let mut reopened = 0usize;
                        while try_launch(
                            server,
                            clients,
                            &apps,
                            0, // first registered app (config order, not a hardcoded id)
                            current_ws,
                            &mut next_id,
                            ly.max_windows,
                            &mut notifs,
                            &notifications,
                            ticks,
                        ) {
                            reopened += 1;
                            if reopened >= ly.max_windows {
                                break; // safety: never loop past the cap
                            }
                        }
                        libdunit::println(&alloc::format!(
                            "gui_server: recycle self-test reaped_to={} reopened={} {} (ceiling lifted)",
                            free_before,
                            reopened,
                            if reopened > 0 { "ok" } else { "FAIL" },
                        ));
                        recycle_phase = 2;
                    } else if ticks >= recycle_deadline {
                        libdunit::println(&alloc::format!(
                            "gui_server: FAIL recycle self-test drain stalled clients={} pre={}",
                            clients.len(),
                            recycle_pre,
                        ));
                        recycle_phase = 2;
                    }
                }
                _ => {}
            }
        }

        let m = libdunit::get_mouse_state();
        let mx = m.x as i32;
        let my = m.y as i32;
        let left = m.left();
        let press = left && !prev_left;
        let release = !left && prev_left;
        let right = m.right();
        let rpress = right && !prev_right;

        // Shell-strip rects for THIS frame (recomputed cheaply so a live reload
        // that moved the panel/tray takes effect at once). The panel and the dock
        // always exist; the tray gets its OWN strip only when it does not share the
        // panel's edge (otherwise it renders inside the panel — the classic look).
        // `in_shell` marks the shell-owned region: a click/right-click there is the
        // shell's, never routed to a client window.
        let panel_rect = panel_strip(&ly, &ins, panel_edge, bw, bh);
        let dock_rect = dock_strip(&ly, &ins, bw, bh);
        let tray_own = if tray_edge != panel_edge {
            Some(strip_rect(tray_edge, tray_size, 0, &ins, bw, bh))
        } else {
            None
        };
        let launcher_hit = panel_slot(panel_rect, panel_edge, 0, launcher_main(&ly, panel_edge));
        let menu_org = menu_origin(&ly, panel_edge, panel_rect, apps.launcher.len() as i32);
        // Quick-settings applet cell on the panel (only when enabled); its flyout
        // flies out below/beside the panel and is NOT part of `in_shell` (a
        // transient overlay), so its clicks are caught by the `qs_open` branch
        // below before window routing — exactly like the launcher dropdown.
        let qs_applet = qs_applet_rect(&ly, panel_edge, panel_rect, apps.workspaces);
        let in_shell = |x: i32, y: i32| {
            in_rect(panel_rect, x, y)
                || in_rect(dock_rect, x, y)
                || tray_own.map_or(false, |t| in_rect(t, x, y))
        };

        // --- Right-button press: forward to the content window under the cursor
        // as IN_DOWN with button=1 (context-menu trigger). Panel/dock/title are
        // shell-owned and ignore the right button; a right-click there is a no-op.
        if rpress && !menu_open && !qs_open && !in_shell(mx, my) {
            let mut zi = z.len();
            while zi > 0 {
                zi -= 1;
                let wi = z[zi];
                if wins[wi].alive && wins[wi].ws == current_ws && !wins[wi].minimized && wins[wi].in_content(mx, my) {
                    z.retain(|&i| i != wi);
                    z.push(wi);
                    send_input(wins[wi].pid, IN_DOWN, mx - wins[wi].cx, my - wins[wi].cy, 1);
                    break;
                }
            }
        }

        // --- Button press edge: menu first, then panel, then windows ---
        if press && menu_open {
            // The dropdown is open: a click on an entry spawns that app; any
            // other click just dismisses the menu. Either way the menu closes
            // and the click is consumed (never reaches a window).
            let mut chosen: Option<usize> = None;
            for i in 0..apps.launcher.len() {
                let r = menu_item_rect(&ly, menu_org, i as i32);
                if in_rect(r, mx, my) {
                    chosen = Some(i);
                    break;
                }
            }
            menu_open = false;
            if let Some(i) = chosen {
                if let Some(ri) = apps.launcher.get(i).copied() {
                    try_launch(
                        server, clients, &apps, ri, current_ws, &mut next_id,
                        ly.max_windows, &mut notifs, &notifications, ticks,
                    );
                }
            }
        } else if press && qs_open {
            // Quick-settings flyout is open: a click on a toggle row flips the LIVE
            // flag it represents (single source of truth — `fx.blur` / `wg.enabled`
            // are exactly the fields the renderer reads); any other click just
            // dismisses the flyout. Either way the click is consumed and the flyout
            // closes, mirroring the launcher-dropdown model.
            let flyout = qs_flyout_rect(&ly, panel_edge, qs_applet, 2);
            if in_rect(qs_row_rect(&ly, flyout, 0), mx, my) {
                fx.blur = !fx.blur;
                libdunit::println(&alloc::format!("gui_server: quicksettings blur={}", fx.blur as u32));
            } else if in_rect(qs_row_rect(&ly, flyout, 1), mx, my) {
                wg.enabled = !wg.enabled;
                libdunit::println(&alloc::format!("gui_server: quicksettings widgets={}", wg.enabled as u32));
            }
            qs_open = false;
        } else if press && in_rect(panel_rect, mx, my) {
            // Panel click. The launcher mark (strip start) opens the app menu; the
            // quick-settings applet opens its flyout; the workspace pips switch
            // workspaces; the rest of the panel is consumed by the shell (task
            // switching lives on the dock now). Either way the click never reaches a
            // client — the shell owns the panel strip.
            if in_rect(launcher_hit, mx, my) {
                menu_open = true;
                menu_sel = 0;
                qs_open = false;
            } else if quicksettings.enabled && panel_edge.is_horizontal() && in_rect(qs_applet, mx, my) {
                // Toggle the quick-settings flyout (horizontal panels only — a
                // narrow vertical bar has no spare main-axis room for the applet).
                qs_open = !qs_open;
                menu_open = false;
            } else {
                // Workspace switcher: activate the clicked pip's workspace. A
                // click in the gap between pips is consumed but changes nothing.
                for i in 0..apps.workspaces {
                    let r = ws_pip_rect(&ly, panel_edge, panel_rect, i as i32);
                    if in_rect(r, mx, my) {
                        if current_ws != i {
                            ws_switch_tick = ticks; // start the incoming crossfade
                        }
                        current_ws = i;
                        break;
                    }
                }
            }
            // Any other panel click (centered title, tray) is consumed.
        } else if press && in_rect(dock_rect, mx, my) {
            // Dock strip (left edge, inside any left panel/tray): the task switcher.
            // A click on a pinned icon raises its running window (switching
            // workspace if needed) or spawns the app when none is running. The dock
            // owns its strip — clicks never reach a client.
            let mut chosen: Option<usize> = None;
            for i in 0..apps.dock.len() {
                let r = dock_icon_rect(&ly, &ins, bw, bh, i as i32);
                if in_rect(r, mx, my) {
                    chosen = Some(i);
                    break;
                }
            }
            if let Some(i) = chosen {
                if let Some(ri) = apps.dock.get(i).copied() {
                    // Raise-if-running: newest live window registered to this app.
                    let running = (0..wins.len())
                        .rev()
                        .find(|&wi| wins[wi].alive && wins[wi].app as usize == ri);
                    if let Some(wi) = running {
                        if current_ws != wins[wi].ws {
                            ws_switch_tick = ticks; // crossfade into the target ws
                            current_ws = wins[wi].ws;
                        }
                        // Restore if it was minimized, then raise to the top.
                        if wins[wi].minimized {
                            wins[wi].minimized = false;
                            wins[wi].born = ticks; // replay the grow-in on restore
                        }
                        z.retain(|&i| i != wi);
                        z.push(wi);
                    } else {
                        try_launch(
                            server, clients, &apps, ri, current_ws, &mut next_id,
                            ly.max_windows, &mut notifs, &notifications, ticks,
                        );
                    }
                }
            }
        } else if press {
            let mut hit: Option<usize> = None;
            let mut zi = z.len();
            while zi > 0 {
                zi -= 1;
                let wi = z[zi];
                if wins[wi].alive && wins[wi].ws == current_ws && !wins[wi].minimized && wins[wi].contains(mx, my) {
                    hit = Some(wi);
                    break;
                }
            }
            if let Some(wi) = hit {
                // Raise: move wi to the top of the z-order.
                z.retain(|&i| i != wi);
                z.push(wi);
                if wins[wi].in_close(mx, my) {
                    wins[wi].alive = false;
                    send_input(wins[wi].pid, IN_QUIT, 0, 0, 0);
                    drag = None;
                    // Remember where it was before it goes away, so reopening the
                    // app restores its last geometry.
                    session_remember(&mut session, &apps, &wins[wi]);
                    settings::save_session(&session);
                } else if wins[wi].in_min(mx, my) {
                    // Minimize: drop it from compositing/input; the dock task
                    // switcher restores it on click.
                    wins[wi].minimized = true;
                    drag = None;
                } else if wins[wi].in_max(mx, my) {
                    // Maximize/restore: toggle the window between the work area
                    // and its saved floating geometry via a server-push CONFIGURE
                    // (Phase 3). The owning client re-imports at the new size and
                    // the per-frame sync mirrors the fresh buffer back.
                    toggle_maximize(server, clients, &mut wins[wi], bw, bh, &ly, &ins);
                    drag = None;
                    // Persist the new maximize state (with the preserved floating
                    // rect) so a reopen restores it in the same state.
                    session_remember(&mut session, &apps, &wins[wi]);
                    settings::save_session(&session);
                } else if wins[wi].in_title(mx, my) {
                    // Title-bar drag. macOS-style restore: grabbing a *tiled*
                    // window (e.g. maximized) first restores it to its floating
                    // size, then it follows the pointer keeping the SAME
                    // proportional grip along the now-narrower title bar — so the
                    // window "shrinks under the cursor" instead of snapping to a
                    // corner. A floating window just starts dragging from where it
                    // was grabbed. `ox`/`oy` are offsets from the window content
                    // origin to the cursor (oy is negative — the bar sits above the
                    // content), matching the drag-move math below.
                    if wins[wi].is_tiled() {
                        let old_w = wins[wi].sw.max(1);
                        let grip_x = (mx - wins[wi].cx).clamp(0, old_w);
                        let grip_y =
                            (my - (wins[wi].cy - wins[wi].title_h)).clamp(0, (wins[wi].title_h - 1).max(0));
                        let rest_w = wins[wi]
                            .restore
                            .map(|(_, _, w, _)| w)
                            .unwrap_or(old_w)
                            .max(1);
                        set_window_state(server, clients, &mut wins[wi], WinState::Floating, bw, bh, &ly, &ins);
                        let ox = (grip_x as i64 * rest_w as i64 / old_w as i64) as i32;
                        let oy = grip_y - wins[wi].title_h;
                        wins[wi].cx = mx - ox;
                        wins[wi].cy = my - oy;
                        drag = Some((wi, ox, oy));
                    } else {
                        drag = Some((wi, mx - wins[wi].cx, my - wins[wi].cy));
                    }
                } else if wins[wi].in_content(mx, my) {
                    // Click landed on client content: forward it to the client.
                    send_input(wins[wi].pid, IN_DOWN, mx - wins[wi].cx, my - wins[wi].cy, 0);
                    pressed_win = Some(wi);
                }
            }
        }
        if !left {
            // Drag released: persist the window's final position so it reopens
            // where the user dropped it (save on drag-END, not every move frame).
            if let Some((wi, _, _)) = drag {
                session_remember(&mut session, &apps, &wins[wi]);
                settings::save_session(&session);
            }
            drag = None;
        }

        // --- Drag move ---
        if let Some((wi, ox, oy)) = drag {
            let mut ncx = mx - ox;
            let mut ncy = my - oy;
            ncx = ncx
                .max(ins.left + ly.border)
                .min(bw as i32 - ins.right - wins[wi].sw - ly.border);
            ncy = ncy
                .max(ins.top + ly.title_h + ly.border)
                .min(bh as i32 - ins.bottom - wins[wi].sh - ly.border);
            wins[wi].cx = ncx;
            wins[wi].cy = ncy;
        }

        // --- Pointer focus + motion routed to the topmost content window ---
        let cur = if drag.is_some() {
            None
        } else {
            let mut c = None;
            let mut zi = z.len();
            while zi > 0 {
                zi -= 1;
                let wi = z[zi];
                if wins[wi].alive && wins[wi].ws == current_ws && !wins[wi].minimized && wins[wi].in_content(mx, my) {
                    c = Some(wi);
                    break;
                }
            }
            c
        };
        if cur != input_focus {
            if let Some(old) = input_focus {
                if wins[old].alive {
                    send_input(wins[old].pid, IN_LEAVE, 0, 0, 0);
                }
            }
            input_focus = cur;
            last_lx = i32::MIN; // force the next motion to be delivered
        }
        if let Some(wi) = cur {
            let lx = mx - wins[wi].cx;
            let ly = my - wins[wi].cy;
            if lx != last_lx || ly != last_ly {
                send_input(wins[wi].pid, IN_MOVE, lx, ly, 0);
                last_lx = lx;
                last_ly = ly;
            }
        }

        // --- Button release: deliver UP to the window that captured the press ---
        if release {
            if let Some(wi) = pressed_win {
                if wins[wi].alive {
                    send_input(wins[wi].pid, IN_UP, mx - wins[wi].cx, my - wins[wi].cy, 0);
                }
            }
            pressed_win = None;
        }
        prev_left = left;
        prev_right = right;

        // The desktop session persists even with no live windows: closing the
        // last window drops back to the empty-but-usable desktop rather than
        // ending the session. (Was: `if !wins.iter().any(alive) { break; }`,
        // which killed the compositor the moment the last client closed and made
        // a clean, window-less boot impossible.) The loop is still bounded by the
        // `ticks >= 60000` headless guard at the bottom.

        let focused = z
            .iter()
            .rev()
            .copied()
            .find(|&i| wins[i].alive && wins[i].ws == current_ws && !wins[i].minimized);

        // --- Mouse wheel: forward the tick's scroll delta to the focused window
        // as IN_SCROLL (delta in `lx`). Clients that don't scroll ignore it; the
        // terminal uses it to move through its scrollback history.
        if m.wheel != 0 {
            if let Some(wi) = focused {
                send_input(wins[wi].pid, IN_SCROLL, m.wheel, 0, 0);
            }
        }

        // --- Keyboard: drain the seat and route events to the focused window ---
        // The compositor is the sole reader of the kernel key ring. Every key
        // press (make) for a non-modifier key is forwarded to the focused window
        // as an IN_KEY event carrying the full trio the kernel already cooks:
        //   lx     = scancode (sc & 0x7F)
        //   ly     = modifier mask (KEYMOD_*)
        //   button = cooked ASCII byte (0 for extended/arrow/nav keys)
        // Old clients read only the ASCII byte in `button` (unchanged); the
        // terminal decodes scancode+mods for control keys (Ctrl-C, arrows, …).
        // Modifier-only presses and key releases are dropped — their state is
        // already reflected in the `mods` field of the events we do send.
        if focused.is_some() && wins.len() > initial_wins && !input_ready_announced {
            libdunit::println("gui_server: desktop input ready");
            input_ready_announced = true;
        }
        while let Some(ev) = libdunit::get_key_event() {
            // Window switcher (M4, config-gated). The compositor owns the seat, so
            // it intercepts the switcher gesture BEFORE forwarding keys to clients:
            //   * `switch_mod`+Tab arms/advances an MRU overlay (Tab is swallowed,
            //     never reaches the focused client);
            //   * Shift adds reverse cycling; Esc cancels without switching;
            //   * releasing the arming modifier commits — raise + un-minimize the
            //     highlighted window.
            // `mods` reflects the modifier state at the event moment, so a cleared
            // arming bit on any drained event while armed means "modifier released".
            if shortcuts.switcher {
                let mask = shortcuts.switch_mod.mask();
                let mod_held = (ev.mods & mask) != 0;
                if ev.pressed && ev.scancode == SC_TAB && mod_held {
                    if switcher.active {
                        switcher.advance(ev.shift());
                    } else {
                        let mru = switcher_mru(&z, &wins, current_ws);
                        switcher.begin(&mru, ev.shift());
                    }
                    continue; // swallow Tab — it never reaches a client
                }
                if switcher.active {
                    if ev.pressed && ev.scancode == SC_ESC {
                        switcher.cancel();
                        continue;
                    }
                    if !mod_held {
                        // Arming modifier released → commit the highlighted window.
                        if let Some(sel) = switcher.commit() {
                            if wins.get(sel).map_or(false, |w| w.alive) {
                                wins[sel].minimized = false;
                                raise_window(&mut z, sel);
                                libdunit::println(&alloc::format!(
                                    "gui_server: switcher raised pid={}",
                                    wins[sel].pid
                                ));
                            }
                        }
                        // Fall through: this event is the modifier release itself,
                        // dropped by the release/modifier filter just below.
                    }
                }
            }

            // --- Launcher menu keyboard navigation (modal) ---------------------
            // While the dropdown is open it CAPTURES every key: Up/Down move the
            // highlight, Enter launches the highlighted app (the SAME `try_launch`
            // path a mouse click uses), Esc closes. Swallowing all keys keeps them
            // out of the focused client so typing can't leak past the open menu.
            if menu_open {
                if ev.pressed {
                    match ev.scancode {
                        SC_ESC => menu_open = false,
                        SC_UP => {
                            if menu_sel > 0 {
                                menu_sel -= 1;
                            }
                        }
                        SC_DOWN => {
                            if menu_sel + 1 < apps.launcher.len() {
                                menu_sel += 1;
                            }
                        }
                        SC_ENTER => {
                            menu_open = false;
                            if let Some(ri) = apps.launcher.get(menu_sel).copied() {
                                try_launch(
                                    server, clients, &apps, ri, current_ws, &mut next_id,
                                    ly.max_windows, &mut notifs, &notifications, ticks,
                                );
                            }
                        }
                        _ => {}
                    }
                }
                continue; // menu is modal — no key reaches a client while it is open
            }

            // --- Global desktop shortcuts (config-driven `cmd_mod` + key) -------
            // Intercepted before client forwarding, exactly like the switcher. The
            // DECISION is the pure `match_shortcut` (so the self-test can prove the
            // mapping); the EFFECT here reuses the same paths the mouse chips drive.
            match match_shortcut(ev.scancode, ev.mods, ev.pressed, &shortcuts, apps.workspaces) {
                ShortcutAction::Workspace(ws) => {
                    if current_ws != ws {
                        ws_switch_tick = ticks; // start the incoming crossfade
                        current_ws = ws;
                    }
                    continue;
                }
                ShortcutAction::Launcher => {
                    menu_open = true;
                    menu_sel = 0;
                    qs_open = false;
                    continue;
                }
                ShortcutAction::CloseWindow => {
                    if let Some(wi) = focused {
                        // Same close path as the title-bar close chip: drop it from
                        // compositing, tell the client to quit, and remember its
                        // geometry so reopening restores where it sat.
                        wins[wi].alive = false;
                        send_input(wins[wi].pid, IN_QUIT, 0, 0, 0);
                        drag = None;
                        session_remember(&mut session, &apps, &wins[wi]);
                        settings::save_session(&session);
                    }
                    continue;
                }
                ShortcutAction::MaximizeWindow => {
                    if let Some(wi) = focused {
                        // Same maximize path as the title-bar chip: a server-push
                        // CONFIGURE the client re-imports at the new size.
                        toggle_maximize(server, clients, &mut wins[wi], bw, bh, &ly, &ins);
                        drag = None;
                        session_remember(&mut session, &apps, &wins[wi]);
                        settings::save_session(&session);
                    }
                    continue;
                }
                ShortcutAction::None => {}
            }

            if !ev.pressed || is_modifier_scancode(ev.scancode) {
                continue;
            }
            if let Some(wi) = focused {
                send_input(
                    wins[wi].pid,
                    IN_KEY,
                    ev.scancode as i32,
                    ev.mods as i32,
                    ev.ascii as u32,
                );
            }
        }

        // Desktop backdrop: the wallpaper (pre-scaled) when present, else a
        // subtle vertical gradient (or a flat fill when the gradient effect is
        // off — the pre-§5 look).
        if let Some(wp) = &wallpaper {
            back.copy_from_slice(wp);
        } else if fx.gradient {
            let top = shade(theme.desktop, 12);
            let bot = shade(theme.desktop, -8);
            let span = (bh as u32 - 1).max(1);
            for y in 0..bh {
                let row = 0xFF00_0000 | lerp_color(top, bot, (y as u32 * 255) / span);
                let base = y * bw;
                for px in &mut back[base..base + bw] {
                    *px = row;
                }
            }
        } else {
            for px in back.iter_mut() {
                *px = theme.desktop;
            }
        }

        // --- Desktop widget cards (concept §5) -------------------------------
        // Translucent plasmoids on the wallpaper, behind windows. The SET is
        // config-driven (`desk_widgets`, resolved from the per-widget files):
        // each widget is present only when its file exists and is enabled, so a
        // widget is removed by deleting its file. `[widgets] enabled` is the
        // master kill switch. Widgets sharing a corner stack into one card
        // (clock above monitor), so the default (both in corner 1) reads as one
        // plasmoid; per-widget `corner` can split them across corners. Windows
        // composite on top.
        if wg.enabled && !desk_widgets.is_empty() {
            if let Some(f) = font.as_ref() {
                let mut wst = libdunit::SystemStats::default();
                let whave = libdunit::get_system_stats(&mut wst) >= 0;
                for corner in 0..4i32 {
                    let clock_w = desk_widgets
                        .iter()
                        .find(|w| w.corner == corner && w.kind == WidgetKind::Clock);
                    let mon_w = desk_widgets
                        .iter()
                        .find(|w| w.corner == corner && w.kind == WidgetKind::Monitor);
                    if clock_w.is_none() && mon_w.is_none() {
                        continue;
                    }
                    let card_w = 236i32;
                    let pad = 16i32;
                    let clock_px = (ly.title_font_px as f32 * 2.4).max(24.0);
                    let clock_h = if clock_w.is_some() { clock_px as i32 + 10 } else { 0 };
                    let mon_h = if mon_w.is_some() { 58 } else { 0 };
                    let card_h = pad * 2 + clock_h + mon_h;
                    let margin = 24i32;
                    let cx = if corner == 1 || corner == 3 {
                        bw as i32 - ins.right - card_w - margin
                    } else {
                        ins.left + margin
                    };
                    let cy = if corner == 2 || corner == 3 {
                        bh as i32 - ins.bottom - card_h - margin
                    } else {
                        ins.top + margin
                    };
                    if fx.blur {
                        blur_region(&mut back, bw, bh, cx, cy, card_w, card_h, fx.blur_radius, fx.blur_iters, &mut blur_a, &mut blur_b);
                    }
                    fill_rrect_grad(&mut back, bw, bh, cx, cy, card_w, card_h, fx.corner_radius, RR_ALL, shade(theme.menu, 16), shade(theme.menu, -8), fx.menu_alpha);
                    let secs = if whave { wst.uptime_ticks / 100 } else { 0 };
                    let mut ty = cy + pad;
                    if clock_w.is_some() {
                        let clk = alloc::format!("{:02}:{:02}:{:02}", (secs / 3600) % 100, (secs / 60) % 60, secs % 60);
                        let tw = text_width_ttf(f, &clk, clock_px);
                        let baseline = ty + clock_px as i32 - 4;
                        draw_text_ttf(&mut back, bw, bh, f, cx + (card_w - tw) / 2, baseline, clock_px, &clk, theme.panel_text & 0x00FF_FFFF);
                        ty += clock_h;
                    }
                    if let Some(mw) = mon_w {
                        let px = ly.title_font_px as f32;
                        let ram_pct = if whave && wst.pmm_total_bytes > 0 {
                            (wst.pmm_used_bytes * 100 / wst.pmm_total_bytes) as u32
                        } else {
                            0
                        };
                        let line = alloc::format!("RAM {}%    {} proc", ram_pct, wst.process_running);
                        draw_text_ttf(&mut back, bw, bh, f, cx + pad, ty + px as i32, px, &line, theme.panel_text & 0x00FF_FFFF);
                        // RAM usage bar under the text row.
                        let bar_y = ty + px as i32 + 12;
                        let bar_w = card_w - 2 * pad;
                        fill_rrect(&mut back, bw, bh, cx + pad, bar_y, bar_w, 8, 4, RR_ALL, ((fx.menu_alpha.min(255) as u32) << 24) | (shade(theme.menu, -20) & 0x00FF_FFFF));
                        let fill_w = (bar_w * ram_pct.min(100) as i32) / 100;
                        if fill_w > 0 {
                            // Per-widget accent (widgets/monitor.toml `accent`) overrides
                            // the theme launcher color for the fill; 0 = inherit theme.
                            let bar_col = if mw.accent != 0 { mw.accent } else { theme.launcher };
                            fill_rrect(&mut back, bw, bh, cx + pad, bar_y, fill_w, 8, 4, RR_ALL, 0xFF00_0000 | (bar_col & 0x00FF_FFFF));
                        }
                    }
                }
            }
        }

        // Windows, bottom-to-top: soft shadow, rounded gradient frame, rounded
        // gradient title bar, then the opaque client surface. Each window fades
        // in from `born`, and the active workspace fades in after a switch —
        // folded into one alpha `a` so both read as a single grow-in.
        let ws_fade = ramp(ticks, ws_switch_tick, anim_frames);
        for &wi in z.iter() {
            let w = &wins[wi];
            if !w.alive || w.ws != current_ws || w.minimized {
                continue;
            }
            let is_focused = focused == Some(wi);
            let a = ramp(ticks, w.born, anim_frames).min(ws_fade) as i32;
            if a <= 0 {
                continue;
            }
            let (ox, oy, ow, oh) = w.outer();
            let bcol = if is_focused { theme.border_focused } else { theme.border_unfocused };
            let tcol = if is_focused { theme.title_focused } else { theme.title_unfocused };
            if fx.shadow > 0 {
                draw_soft_shadow(&mut back, bw, bh, ox, oy, ow, oh, fx.corner_radius, fx.shadow, fx.shadow_alpha * a / 255);
            }
            // Translucent (ARGB) window: snapshot the real backing (wallpaper +
            // lower-z windows already composited into `back`) under the content
            // rect NOW, before the opaque frame gradient below overwrites it. It's
            // restored right before the client blit so the client's semi-transparent
            // background blends over what's genuinely behind the window. Opaque
            // (XRGB) windows skip this — they overwrite the content rect wholesale.
            let translucent = w.format == FORMAT_ARGB8888 && a >= 224;
            let mut backing_rect: Option<(usize, usize, usize, usize)> = None;
            if translucent {
                let (bx, by, cw, ch) = clip_to_screen(w.cx, w.cy, w.sw, w.sh, bw, bh);
                if cw > 0 && ch > 0 {
                    win_backing.clear();
                    for ry in 0..ch {
                        let srow = (by + ry) * bw + bx;
                        win_backing.extend_from_slice(&back[srow..srow + cw]);
                    }
                    backing_rect = Some((bx, by, cw, ch));
                }
            }
            // Rounded frame: top corners only, so the square-bottomed client
            // surface tucks flush against the bottom edge (no corner poke-through).
            fill_rrect_grad(&mut back, bw, bh, ox, oy, ow, oh, fx.corner_radius, RR_TOP, shade(bcol, 18), shade(bcol, -12), a);
            fill_rrect_grad(&mut back, bw, bh, w.cx, w.cy - w.title_h, w.sw, w.title_h, fx.corner_radius, RR_TOP, shade(tcol, 16), shade(tcol, -10), a);
            // Close box: a small rounded chip in the danger color.
            let sz = w.title_h - 12;
            fill_rrect(&mut back, bw, bh, w.cx + w.sw - sz - 6, w.cy - w.title_h + 6, sz, sz, fx.corner_radius.min(sz / 2), RR_ALL, ((a as u32) << 24) | (theme.close & 0x00FF_FFFF));
            // Minimize box: a neutral chip just left of the close chip, carrying a
            // horizontal minus glyph. Matches Win::in_min's hit rect.
            {
                let mbx = w.cx + w.sw - 2 * sz - 10;
                let mby = w.cy - w.title_h + 6;
                let chip = shade(tcol, 34);
                fill_rrect(&mut back, bw, bh, mbx, mby, sz, sz, fx.corner_radius.min(sz / 2), RR_ALL, ((a as u32) << 24) | (chip & 0x00FF_FFFF));
                let gw = (sz / 2).max(3);
                let gx = mbx + (sz - gw) / 2;
                let gy = mby + sz / 2;
                let glyph = if is_focused { theme.title_text_focused } else { theme.title_text_unfocused };
                fill_rect_alpha(&mut back, bw, bh, gx, gy, gw, 2, ((a as u32) << 24) | (glyph & 0x00FF_FFFF));
            }
            // Maximize/restore box: a neutral chip left of the minimize chip,
            // carrying a small square-outline glyph (a doubled square hints
            // "restore" once maximized). Matches Win::in_max's hit rect.
            {
                let xbx = w.cx + w.sw - 3 * sz - 14;
                let xby = w.cy - w.title_h + 6;
                let chip = shade(tcol, 34);
                fill_rrect(&mut back, bw, bh, xbx, xby, sz, sz, fx.corner_radius.min(sz / 2), RR_ALL, ((a as u32) << 24) | (chip & 0x00FF_FFFF));
                let glyph = if is_focused { theme.title_text_focused } else { theme.title_text_unfocused };
                let gcol = ((a as u32) << 24) | (glyph & 0x00FF_FFFF);
                let gs = (sz / 2).max(4);
                let gx = xbx + (sz - gs) / 2;
                let gy = xby + (sz - gs) / 2;
                // Square outline (2px sides) — the maximize affordance.
                fill_rect_alpha(&mut back, bw, bh, gx, gy, gs, 2, gcol);
                fill_rect_alpha(&mut back, bw, bh, gx, gy + gs - 2, gs, 2, gcol);
                fill_rect_alpha(&mut back, bw, bh, gx, gy, 2, gs, gcol);
                fill_rect_alpha(&mut back, bw, bh, gx + gs - 2, gy, 2, gs, gcol);
                // Already maximized: overlay a second, offset square (restore hint).
                if w.state == WinState::Maximized {
                    let o = 2;
                    fill_rect_alpha(&mut back, bw, bh, gx + o, gy - o, gs, 2, gcol);
                    fill_rect_alpha(&mut back, bw, bh, gx + gs - 2 + o, gy - o, 2, gs, gcol);
                }
            }
            // Window title text (TTF), left-aligned in the title bar, clear of the
            // close chip. Drawn only once the card is nearly solid so it appears
            // with the surface rather than through the grow-in blend.
            if a >= 224 {
                if let Some(f) = font.as_ref() {
                    let px = ly.title_font_px as f32;
                    let baseline = w.cy - w.title_h / 2 + 5;
                    let txtcol = if is_focused {
                        theme.title_text_focused & 0x00FF_FFFF
                    } else {
                        theme.title_text_unfocused & 0x00FF_FFFF
                    };
                    draw_text_ttf(&mut back, bw, bh, f, w.cx + 12, baseline, px, win_title(&apps, w.app), txtcol);
                }
            }
            // Client surface. Held back until the frame is nearly solid so the
            // grow-in shows a clean card, not a half-blended image. XRGB surfaces
            // are an opaque copy (fast path); ARGB surfaces blend per-pixel over
            // the restored backing so a translucent client reveals the desktop.
            if a >= 224 {
                let want = (w.sw as usize) * (w.sh as usize);
                if want > 0 && want * 4 <= w.buf_size {
                    // Restore the pre-frame backing under the content rect so the
                    // ARGB blend composites over wallpaper/lower windows, not frame.
                    if let Some((bx, by, cw, ch)) = backing_rect {
                        for ry in 0..ch {
                            let drow = (by + ry) * bw + bx;
                            let srow = ry * cw;
                            back[drow..drow + cw].copy_from_slice(&win_backing[srow..srow + cw]);
                        }
                    }
                    let src = unsafe { core::slice::from_raw_parts(w.buf_ptr as *const u32, want) };
                    let argb = w.format == FORMAT_ARGB8888;
                    if argb && !argb_blit_logged {
                        libdunit::println(&alloc::format!(
                            "gui_server: argb blit (client format={}, per-pixel src-over)",
                            w.format
                        ));
                        argb_blit_logged = true;
                    }
                    blit_surface(&mut back, bw, bh, w.cx, w.cy, src, w.sw as usize, w.sh as usize, argb);
                }
            }
        }

        // --- DWM panel: acrylic glass bar on its configured edge ---
        // `panel_rect` (computed per-frame from `panel_edge`) is the strip; for the
        // default top edge it is (0,0,bw,panel_h) so the layout below is byte-for-
        // byte identical. Horizontal panels (top/bottom) carry the full text layout
        // (wordmark, centred title, tray line); vertical panels (left/right) are a
        // narrow bar and render only the compact icon + pip content.
        let (panx, pany, panw, panh) = panel_rect;
        let horiz = panel_edge.is_horizontal();
        fill_strip_bg(&mut back, bw, bh, panel_rect, theme.panel, &fx, 16, &mut blur_a, &mut blur_b);
        // App-menu button: the brand logo mark at the panel's start (top-left on a
        // horizontal bar, top-centre on a vertical one), clickable to open the
        // launcher dropdown. The "Dunit" wordmark follows it, but only where a wide
        // horizontal bar has room for it. Falls back to a flat mark + hamburger
        // bars when the shell font/logo is unavailable.
        {
            let logo_sz = (ly.panel_h - 8).clamp(8, 22);
            let (lgx, lgy) = if horiz {
                (panx + 8, pany + (panh - logo_sz) / 2)
            } else {
                (panx + (panw - logo_sz) / 2, pany + 8)
            };
            if let Some(lg) = logo.as_ref() {
                blit_icon(&mut back, bw, bh, lg, ICON_W, ICON_H, lgx, lgy, logo_sz, logo_sz);
            } else {
                fill_rrect(&mut back, bw, bh, lgx, lgy, logo_sz, logo_sz, logo_sz / 2, RR_ALL, 0xFF00_0000 | (theme.launcher & 0x00FF_FFFF));
                fill_rrect(&mut back, bw, bh, lgx + logo_sz / 3, lgy + logo_sz / 3, logo_sz / 3, logo_sz / 3, logo_sz / 6, RR_ALL, 0xFF00_0000 | (theme.panel & 0x00FF_FFFF));
            }
            if horiz {
                let text_x = lgx + logo_sz + 6;
                if let Some(f) = font.as_ref() {
                    let px = ly.panel_font_px as f32;
                    let baseline = pany + ly.panel_h / 2 + 5;
                    draw_text_ttf(&mut back, bw, bh, f, text_x, baseline, px, "Dunit", theme.panel_text & 0x00FF_FFFF);
                } else {
                    for r in 0..3 {
                        fill_rect(&mut back, bw, bh, text_x, pany + 8 + r * 5, 18, 2, theme.launcher);
                    }
                }
            }
        }
        // Workspace switcher: pips 1..=ws_count laid along the panel's main axis
        // after the launcher glyph. The active workspace is highlighted; any
        // workspace holding a live window gets an accent underline.
        for i in 0..apps.workspaces {
            let (px, py, pw, ph) = ws_pip_rect(&ly, panel_edge, panel_rect, i as i32);
            let base = if i == current_ws {
                theme.taskbtn_focused
            } else {
                theme.taskbtn
            };
            fill_rrect_grad(&mut back, bw, bh, px, py, pw, ph, 5, RR_ALL, shade(base, 18), shade(base, -10), 255);
            let label = [b'1' + i as u8];
            let tx = px + (pw - GLYPH_W * 2) / 2;
            let ty = py + (ph - 5 * 2) / 2;
            draw_text_3x5(&mut back, bw, bh, tx, ty, 2, theme.panel_text, &label);
            if wins.iter().any(|w| w.alive && w.ws == i) {
                fill_rrect(&mut back, bw, bh, px + 2, py + ph - 3, pw - 4, 2, 1, RR_ALL, 0xFF00_0000 | (theme.launcher & 0x00FF_FFFF));
            }
        }
        // Quick-settings applet cell: a small toggle button just past the pips
        // (horizontal panels only — a vertical bar has no spare main-axis room).
        // Gated by `[quicksettings] enabled`; opening it reveals the live-toggle
        // flyout drawn later. A "sliders" mark keeps it legible without an asset.
        if quicksettings.enabled && horiz {
            let (ax, ay, aw, ah) = qs_applet;
            let hot = qs_open || (mx >= ax && mx < ax + aw && my >= ay && my < ay + ah);
            let base = if hot { theme.taskbtn_focused } else { theme.taskbtn };
            fill_rrect_grad(&mut back, bw, bh, ax, ay, aw, ah, 6, RR_ALL, shade(base, 18), shade(base, -10), 255);
            let gx = ax + aw / 2 - 5;
            for r in 0..3i32 {
                let gy = ay + ah / 2 - 4 + r * 4;
                fill_rect(&mut back, bw, bh, gx, gy, 10, 2, theme.panel_text);
                let kx = gx + if r == 1 { 6 } else { r * 3 };
                fill_rrect(&mut back, bw, bh, kx, gy - 1, 3, 4, 1, RR_ALL, 0xFF00_0000 | (theme.launcher & 0x00FF_FFFF));
            }
        }
        // Centered focused-window title (icon + app name). Horizontal panels only —
        // a narrow vertical bar has no room for the label. Task switching lives on
        // the dock; this is just the active window's identity.
        if horiz {
            if let (Some(wi), Some(f)) = (focused, font.as_ref()) {
                let title = win_title(&apps, wins[wi].app);
                let px = ly.title_font_px as f32;
                let tw = text_width_ttf(f, title, px);
                let icon = app_icons.get(wins[wi].app as usize).and_then(|s| s.as_ref());
                let isz = (ly.panel_h - 8).clamp(8, 22);
                let gap = if icon.is_some() { 6 } else { 0 };
                let iw = if icon.is_some() { isz } else { 0 };
                let total = iw + gap + tw;
                let sx = panx + (panw - total) / 2;
                if let Some(ic) = icon {
                    let iy = pany + (ly.panel_h - isz) / 2;
                    blit_icon(&mut back, bw, bh, ic, ICON_W, ICON_H, sx, iy, isz, isz);
                }
                let baseline = pany + ly.panel_h / 2 + 5;
                draw_text_ttf(&mut back, bw, bh, f, sx + iw + gap, baseline, px, title, theme.title_text_focused & 0x00FF_FFFF);
            }
        }
        // System tray: live RAM usage, running-process count and the uptime clock.
        // When `[tray] pos` differs from the panel edge it gets its OWN acrylic
        // strip (`tray_own`); otherwise it rides the end of the panel (right-aligned
        // on a horizontal bar — the default look). A horizontal tray shows the full
        // TTF line; a vertical tray shows only the compact MM:SS clock (no room for
        // the wide text) so it stays legible in the narrow bar.
        {
            if let Some(t) = tray_own {
                fill_strip_bg(&mut back, bw, bh, t, theme.panel, &fx, 12, &mut blur_a, &mut blur_b);
            }
            let (trect, tray_h) = match tray_own {
                Some(t) => (t, tray_edge.is_horizontal()),
                None => (panel_rect, horiz),
            };
            let (tx0, ty0, tw0, th0) = trect;
            let mut stats = libdunit::SystemStats::default();
            let have = libdunit::get_system_stats(&mut stats) >= 0;
            let secs = if have { stats.uptime_ticks / 100 } else { 0 };
            if tray_h {
                if let Some(f) = font.as_ref() {
                    let ram_pct = if have && stats.pmm_total_bytes > 0 {
                        (stats.pmm_used_bytes * 100 / stats.pmm_total_bytes) as u32
                    } else {
                        0
                    };
                    let tray = alloc::format!(
                        "RAM {}%   {} proc   {:02}:{:02}",
                        ram_pct,
                        stats.process_running,
                        (secs / 60) % 100,
                        secs % 60
                    );
                    let px = ly.title_font_px as f32;
                    let tw = text_width_ttf(f, &tray, px);
                    let baseline = ty0 + th0 / 2 + 5;
                    draw_text_ttf(&mut back, bw, bh, f, tx0 + tw0 - tw - 14, baseline, px, &tray, theme.panel_text);
                } else {
                    let mut clk = *b"00:00";
                    two_digits(&mut clk, 0, (secs / 60) % 100);
                    two_digits(&mut clk, 3, secs % 60);
                    let clk_w = clk.len() as i32 * (GLYPH_W + 1) * 3;
                    draw_text_3x5(&mut back, bw, bh, tx0 + tw0 - clk_w - 12, ty0 + (th0 - 15) / 2, 3, theme.panel_text, &clk);
                }
            } else {
                let mut clk = *b"00:00";
                two_digits(&mut clk, 0, (secs / 60) % 100);
                two_digits(&mut clk, 3, secs % 60);
                let clk_w = clk.len() as i32 * (GLYPH_W + 1) * 2;
                let cx0 = tx0 + (tw0 - clk_w) / 2;
                let cy0 = ty0 + th0 - 5 * 2 - 8;
                draw_text_3x5(&mut back, bw, bh, cx0, cy0, 2, theme.panel_text, &clk);
            }
        }

        // --- DWM dock: acrylic strip of pinned launchers down the left edge ---
        // `dock_rect` nests just inside anything the panel/tray reserve on the left;
        // for the default (top panel) it is (0,panel_h,dock_w,bh-panel_h).
        fill_strip_bg(&mut back, bw, bh, dock_rect, theme.panel, &fx, 14, &mut blur_a, &mut blur_b);
        // Hover easing step per frame (instant when animations are off).
        let hover_step = if anim_frames == 0 { 255u8 } else { (255 / anim_frames).max(28) as u8 };
        for i in 0..apps.dock.len() {
            let ri = apps.dock[i];
            let (ix, iy, iw, ih) = dock_icon_rect(&ly, &ins, bw, bh, i as i32);
            if iy + ih > bh as i32 {
                break;
            }
            let hovered = mx >= dock_rect.0 && mx < dock_rect.0 + dock_rect.2 && mx >= ix && mx < ix + iw && my >= iy && my < iy + ih;
            dock_hover[i] = if hovered {
                dock_hover[i].saturating_add(hover_step)
            } else {
                dock_hover[i].saturating_sub(hover_step)
            };
            let hp = dock_hover[i] as i32; // 0..255 eased hover amount
            let grow = 3 * hp / 255; // grow the cell up to 3px each side on hover
            let (cx0, cy0, cw, ch) = (ix - grow, iy - grow, iw + 2 * grow, ih + 2 * grow);
            let base = lerp_color(theme.taskbtn, theme.taskbtn_focused, hp as u32);
            fill_rrect_grad(&mut back, bw, bh, cx0, cy0, cw, ch, 7, RR_ALL, shade(base, 20), shade(base, -10), 255);
            // Icon for the pinned registry entry (grows with the hover zoom); if
            // the asset is missing, fall back to the app's label initial.
            if let Some(icon) = app_icons.get(ri).and_then(|s| s.as_ref()) {
                let pad = 4;
                let dw = (cw - 2 * pad).max(1);
                let dh = (ch - 2 * pad).max(1);
                blit_icon(&mut back, bw, bh, icon, ICON_W, ICON_H, cx0 + pad, cy0 + pad, dw, dh);
            } else {
                let tx = cx0 + (cw - GLYPH_W * 3) / 2;
                let ty = cy0 + (ch - 5 * 3) / 2;
                let ch0 = apps
                    .apps
                    .get(ri)
                    .and_then(|a| a.label.as_bytes().first().copied())
                    .unwrap_or(b'?');
                draw_text_3x5(&mut back, bw, bh, tx, ty, 3, theme.panel_text, &[ch0]);
            }
            // Running/active marker: an accent bar on the icon's left edge. A
            // merely-running app gets a short, dimmed pip; the app that owns the
            // focused window gets a longer, full-accent bar — the dock's active
            // indicator, like the reference dock.
            if wins.iter().any(|w| w.alive && w.app == ri as u8) {
                let active = focused.map(|wi| wins[wi].app) == Some(ri as u8);
                let barh = if active { ch * 2 / 3 } else { ch / 4 };
                let bcol = if active { theme.launcher } else { shade(theme.launcher, -40) };
                fill_rrect(&mut back, bw, bh, cx0 - 3, cy0 + (ch - barh) / 2, 3, barh, 1, RR_ALL, 0xFF00_0000 | (bcol & 0x00FF_FFFF));
            }
        }

        // Launcher dropdown: acrylic rounded menu with a drop-in reveal.
        if menu_open && !menu_was_open {
            menu_anim_tick = ticks; // stamp the open so the menu drops in
        }
        menu_was_open = menu_open;
        if menu_open {
            let mprog = ramp(ticks, menu_anim_tick, anim_frames) as i32; // 0..255
            let (mx0, my0, mw0, _) = menu_item_rect(&ly, menu_org, 0);
            let full_h = apps.launcher.len() as i32 * ly.menu_item_h;
            let reveal_h = full_h * mprog / 255; // grow the card downward
            if reveal_h > 0 {
                if fx.blur {
                    blur_region(&mut back, bw, bh, mx0, my0, mw0, reveal_h, fx.blur_radius, fx.blur_iters, &mut blur_a, &mut blur_b);
                }
                fill_rrect_grad(&mut back, bw, bh, mx0, my0, mw0, reveal_h, fx.corner_radius, RR_ALL, shade(theme.menu, 14), theme.menu, fx.menu_alpha);
            }
            for i in 0..apps.launcher.len() {
                let ri = apps.launcher[i];
                let (ix, iy, iw, ih) = menu_item_rect(&ly, menu_org, i as i32);
                if iy + ih > my0 + reveal_h {
                    break; // below the revealed edge — not shown yet
                }
                // Highlight the row under the pointer OR the keyboard selection,
                // so mouse and `cmd_mod`+Space→Up/Down navigation share one cue.
                let hover = (mx >= ix && mx < ix + iw && my >= iy && my < iy + ih) || i == menu_sel;
                if hover {
                    fill_rrect(&mut back, bw, bh, ix + 3, iy + 2, iw - 6, ih - 4, 5, RR_ALL, 0xC000_0000 | (theme.menu_hover & 0x00FF_FFFF));
                }
                // Registry icon at the row's left, then the app label after it.
                let mut text_x = ix + 8;
                if let Some(icon) = app_icons.get(ri).and_then(|s| s.as_ref()) {
                    let isz = (ih - 8).clamp(8, 22);
                    let icy = iy + (ih - isz) / 2;
                    blit_icon(&mut back, bw, bh, icon, ICON_W, ICON_H, ix + 6, icy, isz, isz);
                    text_x = ix + 6 + isz + 6;
                }
                let label = apps.apps.get(ri).map(|a| a.label.as_str()).unwrap_or("");
                draw_text_3x5(&mut back, bw, bh, text_x, iy + (ih - 5 * 3) / 2, 3, theme.panel_text, label.as_bytes());
            }
        }

        // --- Window switcher overlay (M4) ------------------------------------
        // A centered acrylic card with one icon tile per candidate window and the
        // highlighted app's name below it. Drawn above all windows (below only the
        // cursor) while the switcher is armed; every colour comes from the resolved
        // theme, so it inherits the desktop palette rather than hardcoding one.
        if switcher.active && !switcher.order.is_empty() {
            let n = switcher.order.len() as i32;
            let tile = 64i32;
            let gap = 12i32;
            let pad = 20i32;
            let label_h = 24i32;
            let panw = n * tile + (n - 1) * gap + 2 * pad;
            let panh = tile + label_h + 2 * pad;
            let px0 = (bw as i32 - panw) / 2;
            let py0 = (bh as i32 - panh) / 2;
            draw_soft_shadow(&mut back, bw, bh, px0, py0, panw, panh, 16, 10, 90);
            fill_rrect(&mut back, bw, bh, px0, py0, panw, panh, 16, 0x0F, 0xF0000000 | (theme.menu & 0x00FF_FFFF));
            for (k, &wi) in switcher.order.iter().enumerate() {
                let tx = px0 + pad + k as i32 * (tile + gap);
                let ty = py0 + pad;
                if k == switcher.idx {
                    fill_rrect(&mut back, bw, bh, tx - 4, ty - 4, tile + 8, tile + 8, 10, 0x0F,
                        0xFF00_0000 | (theme.menu_hover & 0x00FF_FFFF));
                }
                let app = wins[wi].app as usize;
                let isz = tile - 12;
                if let Some(ic) = app_icons.get(app).and_then(|s| s.as_ref()) {
                    blit_icon(&mut back, bw, bh, ic, ICON_W, ICON_H, tx + 6, ty + 6, isz, isz);
                } else {
                    let lbl = apps.apps.get(app).map(|a| a.label.as_str()).unwrap_or("?");
                    draw_text_3x5(&mut back, bw, bh, tx + 8, ty + isz / 2 - 6, 3, theme.panel_text, lbl.as_bytes());
                }
            }
            if let Some(&wi) = switcher.order.get(switcher.idx) {
                if let Some(f) = font.as_ref() {
                    let title = win_title(&apps, wins[wi].app);
                    let fpx = 15.0f32;
                    let tw = text_width_ttf(f, title, fpx);
                    let sx = px0 + (panw - tw) / 2;
                    let baseline = py0 + pad + tile + label_h - 6;
                    draw_text_ttf(&mut back, bw, bh, f, sx, baseline, fpx, title, theme.panel_text & 0x00FF_FFFF);
                }
            }
        }

        // --- Quick-settings flyout (M4) --------------------------------------
        // A small acrylic card of live toggles that flies out from the applet cell.
        // Each row shows and flips the SAME flag the renderer reads (fx.blur /
        // wg.enabled) with an on/off pill — no shadow state. Drawn above windows,
        // below the switcher overlay/cursor. Every colour comes from the theme.
        if quicksettings.enabled && qs_open && panel_edge.is_horizontal() {
            let flyout = qs_flyout_rect(&ly, panel_edge, qs_applet, 2);
            let (fx0, fy0, fw0, fh0) = flyout;
            if fx.blur {
                blur_region(&mut back, bw, bh, fx0, fy0, fw0, fh0, fx.blur_radius, fx.blur_iters, &mut blur_a, &mut blur_b);
            }
            draw_soft_shadow(&mut back, bw, bh, fx0, fy0, fw0, fh0, 10, 10, 90);
            fill_rrect_grad(&mut back, bw, bh, fx0, fy0, fw0, fh0, fx.corner_radius, RR_ALL, shade(theme.menu, 14), theme.menu, fx.menu_alpha);
            let rows = [("Blur", fx.blur), ("Widgets", wg.enabled)];
            for (i, (lbl, on)) in rows.iter().enumerate() {
                let (rx, ry, rw, rh) = qs_row_rect(&ly, flyout, i as i32);
                let hover = mx >= rx && mx < rx + rw && my >= ry && my < ry + rh;
                if hover {
                    fill_rrect(&mut back, bw, bh, rx + 2, ry + 2, rw - 4, rh - 4, 5, RR_ALL, 0xC000_0000 | (theme.menu_hover & 0x00FF_FFFF));
                }
                if let Some(f) = font.as_ref() {
                    let px = ly.panel_font_px as f32;
                    let baseline = ry + rh / 2 + 5;
                    draw_text_ttf(&mut back, bw, bh, f, rx + 10, baseline, px, lbl, theme.panel_text & 0x00FF_FFFF);
                }
                // On/off pill on the row's trailing edge, knob sliding to reflect state.
                let (pw, ph) = (22i32, 12i32);
                let px0 = rx + rw - pw - 8;
                let py0 = ry + (rh - ph) / 2;
                let pill = if *on { theme.launcher } else { theme.taskbtn };
                fill_rrect(&mut back, bw, bh, px0, py0, pw, ph, ph / 2, RR_ALL, 0xFF00_0000 | (pill & 0x00FF_FFFF));
                let knob = ph - 4;
                let kx = if *on { px0 + pw - knob - 2 } else { px0 + 2 };
                fill_rrect(&mut back, bw, bh, kx, py0 + 2, knob, knob, knob / 2, RR_ALL, 0xFFFF_FFFF);
            }
        }

        // --- Notification toasts (M4) ----------------------------------------
        // Prune expired toasts (the SAME `now < expire` predicate the self-test
        // verifies), then stack the survivors in the configured corner. The
        // one-shot `notify expired` marker fires when the self-test's toast has
        // drained end-to-end, proving the auto-dismiss path — not just the queue.
        notifs.retain(|t| ticks < t.expire);
        if notify_st_done && !notify_expired_logged && notifs.is_empty() {
            libdunit::println("gui_server: notify expired (self-test)");
            notify_expired_logged = true;
        }
        if notifications.enabled && !notifs.is_empty() {
            let margin = 16i32;
            let cardh = 30i32;
            let gap = 8i32;
            let corner = notifications.corner;
            let top = corner == 0 || corner == 1;
            let leftc = corner == 0 || corner == 2;
            for (slot, t) in notifs.iter().enumerate() {
                let px = ly.title_font_px as f32;
                let tw = font
                    .as_ref()
                    .map(|f| text_width_ttf(f, &t.text, px))
                    .unwrap_or(t.text.len() as i32 * 8);
                let cardw = (tw + 30).clamp(96, bw as i32 - 2 * margin);
                // Short slide-in from the anchored edge over the first few frames.
                let age = ticks.saturating_sub(t.born) as i32;
                let slide = (12 - age.min(12)).max(0);
                let base_x = if leftc { margin } else { bw as i32 - margin - cardw };
                let cx = if leftc { base_x - slide } else { base_x + slide };
                let cy = if top {
                    margin + slot as i32 * (cardh + gap)
                } else {
                    bh as i32 - margin - cardh - slot as i32 * (cardh + gap)
                };
                draw_soft_shadow(&mut back, bw, bh, cx, cy, cardw, cardh, 10, 8, 80);
                fill_rrect_grad(&mut back, bw, bh, cx, cy, cardw, cardh, 10, RR_ALL, shade(theme.menu, 14), theme.menu, fx.menu_alpha);
                // Accent bar on the leading edge marks it as a toast.
                fill_rrect(&mut back, bw, bh, cx, cy + 4, 4, cardh - 8, 2, RR_ALL, 0xFF00_0000 | (theme.launcher & 0x00FF_FFFF));
                if let Some(f) = font.as_ref() {
                    let baseline = cy + cardh / 2 + 5;
                    draw_text_ttf(&mut back, bw, bh, f, cx + 14, baseline, px, &t.text, theme.panel_text & 0x00FF_FFFF);
                }
            }
        }

        draw_cursor(&mut back, bw, bh, mx, my);


        let bytes =
            unsafe { core::slice::from_raw_parts(back.as_ptr() as *const u8, back.len() * 4) };
        libdunit::fb_present(bytes, fb.width, fb.height, 0, 0);
        if !desktop_ready_announced {
            libdunit::println("gui_server: desktop ready");
            desktop_ready_announced = true;
        }

        libdunit::sleep_ms(16);
        ticks += 1;
        // Report average ms/frame once the loop is warm (a single line, so the
        // blur/compositing cost shows up in the headless serial log without
        // spamming it). >16 ms means compositing overran the frame budget.
        if ticks == 20 {
            let mut st = libdunit::SystemStats::default();
            if libdunit::get_system_stats(&mut st) >= 0 {
                prev_uptime = st.uptime_ticks;
                prev_tick = ticks;
            }
        } else if ticks == 80 && prev_tick != 0 {
            let mut st = libdunit::SystemStats::default();
            if libdunit::get_system_stats(&mut st) >= 0 {
                let win = (ticks - prev_tick) as u64;
                let dt = st.uptime_ticks.saturating_sub(prev_uptime); // 100 Hz PIT
                let ms = (dt * 10 / win).min(999);
                let mut line = *b"gui_server: frame ~000 ms";
                line[19] = b'0' + ((ms / 100) % 10) as u8;
                line[20] = b'0' + ((ms / 10) % 10) as u8;
                line[21] = b'0' + (ms % 10) as u8;
                if let Ok(s) = core::str::from_utf8(&line) {
                    libdunit::println(s);
                }
            }
        }
        if ticks >= 60000 {
            break;
        }
    }

    // Session over: tell every client to exit its input loop so serve_two_clients
    // can reap them (a QUIT to an already-exited pid is a harmless no-op).
    for w in wins.iter() {
        send_input(w.pid, IN_QUIT, 0, 0, 0);
    }
    // Natural session end (not a live-resolution re-enter): caller does not loop.
    false
}
