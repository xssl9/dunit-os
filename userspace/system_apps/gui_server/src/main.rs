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
use gui_protocol_v1::Opcode;

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

/// Control-message magic marking a capability announce (vs. a protocol packet,
/// which begins with the DGUI wire magic). Shared with `gui_client`.
const CTRL_MAGIC: u32 = 0x3150_4143; // "CAP1"

// Compositor -> client input control messages (20 bytes). Distinct magic from
// CTRL_MAGIC and the DGUI wire magic so the client can tell them apart. Coords
// are client-local (relative to the surface origin). Kept deliberately simple:
// the focused window is the sole recipient, so no per-message routing id.
const INPUT_MAGIC: u32 = 0x3150_4E49; // "INP1"
const IN_MOVE: u8 = 1;
const IN_DOWN: u8 = 2;
const IN_UP: u8 = 3;
const IN_LEAVE: u8 = 4;
const IN_QUIT: u8 = 9;

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
    // Desktop bring-up state (used by run_desktop_session for runtime-spawned
    // clients): `ready` once the buffer is mapped and a frame has been committed
    // so it is safe to blit; `win_created` once it owns a Win in the model.
    ready: bool,
    win_created: bool,
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
            ready: false,
            win_created: false,
        }
    }
}

/// Process one inbound (envelope-stripped) message for client `c`: either map
/// its announced buffer capability, or track geometry + feed the protocol
/// packet through the Server and relay the replies back to the client.
fn handle_client_payload(server: &mut Server, c: &mut ClientState, payload: &[u8]) {
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
        c.surf_w = u32::from_le_bytes([payload[36], payload[37], payload[38], payload[39]]);
        c.surf_h = u32::from_le_bytes([payload[40], payload[41], payload[42], payload[43]]);
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
/// Bring one `gui_client` online: reserve a protocol connection, spawn the ELF,
/// and hand it the `[our_pid][client_id]` handshake it blocks on at startup. The
/// returned `ClientState` is *pending* — its buffer is mapped and it becomes
/// `ready` only once its protocol handshake is pumped (see `pump_clients`). The
/// `id` is a tint hint only; routing uses the kernel-authenticated sender pid.
fn spawn_client(
    server: &mut Server,
    id: u32,
    slot_x: u32,
    slot_y: u32,
    app: &str,
) -> Option<ClientState> {
    let conn = server.connect()?;
    let pid = libdunit::spawn(app);
    if pid <= 0 {
        return None;
    }
    let mut c = ClientState::empty();
    c.pid = pid as u32;
    c.conn = conn;
    c.slot_x = slot_x;
    c.slot_y = slot_y;
    let mut hs = [0u8; 8];
    hs[0..4].copy_from_slice(&libdunit::get_pid().to_le_bytes());
    hs[4..8].copy_from_slice(&id.to_le_bytes());
    libdunit::ipc_send(c.pid, &hs);
    Some(c)
}

/// Composite two untrusted client ELFs to the screen at the same time. Spawns
/// both `gui_client` processes, hands each an 8-byte `[our_pid][client_id]`
/// handshake (the id is only a tint hint for the client), and runs one event
/// loop. Inbound messages are routed to a per-client `Server` connection by the
/// KERNEL-AUTHENTICATED sender pid (`ipc_recv_from`), NOT by any client-supplied
/// field — so one client cannot inject into another's protocol connection. On
/// every `composite()` we match each FRAME_DONE to its connection and blit that
/// client's buffer to its own slot. Returns true iff both surfaces present.
fn serve_two_clients() -> bool {
    let mut server = Server::new();
    let mut clients: Vec<ClientState> = Vec::new();
    let slots = [(300u32, 100u32), (400u32, 220u32)];
    for i in 0..2 {
        match spawn_client(&mut server, (i as u32) + 1, slots[i].0, slots[i].1, "gui_client") {
            Some(c) => clients.push(c),
            None => return false,
        }
    }

    // MARKER2
    let mut rx = [0u8; 256];
    for _ in 0..128 {
        if clients[0].presented && clients[1].presented {
            break;
        }
        // Route by the kernel-authenticated sender pid; a client cannot spoof
        // another's identity, so the two protocol connections stay isolated.
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
        handle_client_payload(&mut server, &mut clients[idx], &rx[..n]);

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
                        }
                    }
                }
            }
        }
    }

    let both = clients[0].presented && clients[1].presented;

    // Both untrusted windows have presented at least one frame. Hand off to the
    // interactive desktop session: a persistent compositor loop that owns a
    // full-screen back buffer, decorates each client surface with a draggable
    // title bar, routes the real mouse (syscall 60) into focus/raise/drag, and
    // re-presents every tick. This is the M4 userspace DWM taking over from the
    // linear M3 smoke.
    if both {
        // Emit the success marker BEFORE the session loop so automated smokes
        // observe it immediately (headless runs are force-quit after capture).
        libdunit::println("gui_server: served two untrusted clients OK");
        run_desktop_session(&mut server, &mut clients);
    }

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

const TITLE_H: i32 = 26;
const BORDER: i32 = 2;

const COLOR_DESKTOP: u32 = 0xFF1E1E2E;
const COLOR_TITLE_FOCUSED: u32 = 0xFF313244;
const COLOR_TITLE_UNFOCUSED: u32 = 0xFF232331;
const COLOR_BORDER_FOCUSED: u32 = 0xFFA6E3A1;
const COLOR_BORDER_UNFOCUSED: u32 = 0xFF45475A;
const COLOR_CLOSE: u32 = 0xFFF38BA8;

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

/// Blit a client's XRGB8888 surface into the back buffer at (x, y), clipped.
fn blit_surface(
    buf: &mut [u32],
    bw: usize,
    bh: usize,
    x: i32,
    y: i32,
    src: &[u32],
    sw: usize,
    sh: usize,
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
                    buf[drow + dx as usize] = src[srow + sx];
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

const PANEL_H: i32 = 28;
const COLOR_PANEL: u32 = 0xFF181825;
const COLOR_PANEL_TEXT: u32 = 0xFFCDD6F4;
const COLOR_LAUNCHER: u32 = 0xFFA6E3A1;
const COLOR_TASKBTN: u32 = 0xFF313244;
const COLOR_TASKBTN_FOCUSED: u32 = 0xFF45475A;

const LAUNCHER_W: i32 = 40;
const TASKBTN_W: i32 = 120;
const TASKBTN_GAP: i32 = 4;

/// Upper bound on concurrently composited client windows. Caps launcher spawns
/// so a stuck loop cannot fork the machine to death.
const MAX_WINDOWS: usize = 8;

/// Launcher menu: label shown in the dropdown paired with the ELF to spawn.
const LAUNCH_APPS: [(&[u8], &str); 2] =
    [(b"WIN", "gui_client"), (b"CALC", "gui_calc")];

const MENU_W: i32 = 130;
const MENU_ITEM_H: i32 = 26;
const COLOR_MENU: u32 = 0xFF11111B;
const COLOR_MENU_HOVER: u32 = 0xFF45475A;

/// Rect of the i-th launcher-menu entry (0-based), dropped below the launcher.
fn menu_item_rect(i: i32) -> (i32, i32, i32, i32) {
    (0, PANEL_H + i * MENU_ITEM_H, MENU_W, MENU_ITEM_H)
}

/// Rect of the i-th taskbar button (0-based), laid out left-to-right after the
/// launcher. Independent of window state so hit-testing and drawing agree.
fn taskbtn_rect(i: i32) -> (i32, i32, i32, i32) {
    let x = LAUNCHER_W + TASKBTN_GAP + i * (TASKBTN_W + TASKBTN_GAP);
    let y = 3;
    (x, y, TASKBTN_W, PANEL_H - 6)
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
}

impl Win {
    /// Full outer rect (border + title bar + content) in screen space.
    fn outer(&self) -> (i32, i32, i32, i32) {
        let x = self.cx - BORDER;
        let y = self.cy - TITLE_H - BORDER;
        let w = self.sw + 2 * BORDER;
        let h = self.sh + TITLE_H + 2 * BORDER;
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
        mx >= self.cx && mx < self.cx + self.sw && my >= self.cy - TITLE_H && my < self.cy
    }
    /// Close box: a small square at the right end of the title bar.
    fn in_close(&self, mx: i32, my: i32) -> bool {
        let sz = TITLE_H - 12;
        let bx = self.cx + self.sw - sz - 6;
        let by = self.cy - TITLE_H + 6;
        mx >= bx && mx < bx + sz && my >= by && my < by + sz
    }
}
/// Persistent interactive compositor. Owns a full-screen back buffer, decorates
/// each client surface, and lets the real mouse focus/raise/drag/close windows.
/// Drain any queued client protocol messages (non-blocking) and advance each
/// client toward `ready`. Called every desktop tick so runtime-spawned clients
/// complete their handshake while the compositor keeps rendering. Bounded per
/// tick so a chatty client cannot starve the frame. Routing is by the
/// kernel-authenticated sender pid, so clients stay isolated.
fn pump_clients(server: &mut Server, clients: &mut Vec<ClientState>) {
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
        handle_client_payload(server, &mut clients[idx], &rx[..n]);
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

fn run_desktop_session(server: &mut Server, clients: &mut Vec<ClientState>) {
    let mut fb = libdunit::FbInfo {
        addr: 0,
        width: 0,
        height: 0,
        pitch: 0,
    };
    if !libdunit::get_framebuffer(&mut fb) || fb.width == 0 || fb.height == 0 {
        libdunit::println("gui_server: no framebuffer for desktop session");
        return;
    }
    let bw = fb.width as usize;
    let bh = fb.height as usize;

    let mut back: Vec<u32> = Vec::new();
    back.resize(bw * bh, COLOR_DESKTOP);

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
        let mut cy = clients[i].slot_y as i32 + TITLE_H + BORDER + offset;
        // Keep the whole window (title bar included) below the reserved panel.
        if cy - TITLE_H < PANEL_H + BORDER {
            cy = PANEL_H + TITLE_H + BORDER;
        }
        if cx + sw + BORDER > bw as i32 {
            cx = (bw as i32 - sw - BORDER).max(BORDER);
        }
        if cy + sh + BORDER > bh as i32 {
            cy = (bh as i32 - sh - BORDER).max(PANEL_H + TITLE_H + BORDER);
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
        });
        clients[i].ready = true;
        clients[i].win_created = true;
        offset += 32;
    }
    if wins.is_empty() {
        return;
    }
    let mut prev_left = false;
    let mut drag: Option<(usize, i32, i32)> = None; // (win index, off_x, off_y)
    let mut pressed_win: Option<usize> = None; // window that captured the press
    let mut input_focus: Option<usize> = None; // window under the pointer
    let mut last_lx = i32::MIN;
    let mut last_ly = i32::MIN;
    // Bounded so a headless run (mouse never moves) still terminates and lets
    // the harness force-quit; ~60000 * 16ms ≈ 16 min of interactive use.
    let mut ticks = 0u32;
    // Runtime app-launch: the launcher spawns fresh gui_client windows up to
    // MAX_WINDOWS; `next_id` is the tint hint handed to each new client.
    let mut next_id = clients.len() as u32 + 1;
    // Launcher dropdown state. Toggled by the launcher glyph; any click either
    // selects a menu entry (spawn) or dismisses the menu.
    let mut menu_open = false;
    loop {
        // Advance any runtime-spawned clients through their protocol handshake,
        // then hand each newly-ready client a cascaded, focused window.
        pump_clients(server, clients);
        for i in 0..clients.len() {
            if !clients[i].ready || clients[i].win_created || clients[i].buf_ptr.is_null() {
                continue;
            }
            let sw = clients[i].surf_w as i32;
            let sh = clients[i].surf_h as i32;
            let step = (wins.len() as i32 % 6) * 40;
            let mut cx = 90 + step + BORDER;
            let mut cy = PANEL_H + TITLE_H + BORDER + step;
            if cx + sw + BORDER > bw as i32 {
                cx = (bw as i32 - sw - BORDER).max(BORDER);
            }
            if cy + sh + BORDER > bh as i32 {
                cy = (bh as i32 - sh - BORDER).max(PANEL_H + TITLE_H + BORDER);
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
            });
            clients[i].win_created = true;
            z.push(wi);
        }

        let m = libdunit::get_mouse_state();
        let mx = m.x as i32;
        let my = m.y as i32;
        let left = m.left();
        let press = left && !prev_left;
        let release = !left && prev_left;

        // --- Button press edge: menu first, then panel, then windows ---
        if press && menu_open {
            // The dropdown is open: a click on an entry spawns that app; any
            // other click just dismisses the menu. Either way the menu closes
            // and the click is consumed (never reaches a window).
            let mut chosen: Option<usize> = None;
            for i in 0..LAUNCH_APPS.len() {
                let (ix, iy, iw, ih) = menu_item_rect(i as i32);
                if mx >= ix && mx < ix + iw && my >= iy && my < iy + ih {
                    chosen = Some(i);
                    break;
                }
            }
            menu_open = false;
            if let Some(i) = chosen {
                if clients.len() < MAX_WINDOWS {
                    if let Some(c) = spawn_client(server, next_id, 0, 0, LAUNCH_APPS[i].1) {
                        clients.push(c);
                        next_id += 1;
                    }
                }
            }
        } else if press && my < PANEL_H {
            // Panel click. The launcher glyph (leftmost cell) opens the app menu;
            // the taskbar buttons raise + focus their window. Either way the click
            // never reaches a client — the shell owns the panel band.
            if mx < LAUNCHER_W {
                menu_open = true;
            } else {
                let mut slot = 0i32;
                for wi in 0..wins.len() {
                    if !wins[wi].alive {
                        continue;
                    }
                    let (bx, by, bw2, bh2) = taskbtn_rect(slot);
                    if mx >= bx && mx < bx + bw2 && my >= by && my < by + bh2 {
                        z.retain(|&i| i != wi);
                        z.push(wi);
                        break;
                    }
                    slot += 1;
                }
            }
        } else if press {
            let mut hit: Option<usize> = None;
            let mut zi = z.len();
            while zi > 0 {
                zi -= 1;
                let wi = z[zi];
                if wins[wi].alive && wins[wi].contains(mx, my) {
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
                } else if wins[wi].in_title(mx, my) {
                    drag = Some((wi, mx - wins[wi].cx, my - wins[wi].cy));
                } else if wins[wi].in_content(mx, my) {
                    // Click landed on client content: forward it to the client.
                    send_input(wins[wi].pid, IN_DOWN, mx - wins[wi].cx, my - wins[wi].cy, 0);
                    pressed_win = Some(wi);
                }
            }
        }
        if !left {
            drag = None;
        }

        // --- Drag move ---
        if let Some((wi, ox, oy)) = drag {
            let mut ncx = mx - ox;
            let mut ncy = my - oy;
            ncx = ncx.max(BORDER).min(bw as i32 - wins[wi].sw - BORDER);
            ncy = ncy.max(PANEL_H + TITLE_H + BORDER).min(bh as i32 - wins[wi].sh - BORDER);
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
                if wins[wi].alive && wins[wi].in_content(mx, my) {
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

        // Stop if every window was closed.
        if !wins.iter().any(|w| w.alive) {
            break;
        }

        let focused = z.iter().rev().copied().find(|&i| wins[i].alive);

        // Clear desktop.
        for px in back.iter_mut() {
            *px = COLOR_DESKTOP;
        }

        // Draw windows bottom-to-top.
        for &wi in z.iter() {
            let w = &wins[wi];
            if !w.alive {
                continue;
            }
            let is_focused = focused == Some(wi);
            let border = if is_focused {
                COLOR_BORDER_FOCUSED
            } else {
                COLOR_BORDER_UNFOCUSED
            };
            let title = if is_focused {
                COLOR_TITLE_FOCUSED
            } else {
                COLOR_TITLE_UNFOCUSED
            };
            let (ox, oy, ow, oh) = w.outer();
            fill_rect(&mut back, bw, bh, ox, oy, ow, oh, border);
            fill_rect(&mut back, bw, bh, w.cx, w.cy - TITLE_H, w.sw, TITLE_H, title);
            // Close box.
            let sz = TITLE_H - 12;
            fill_rect(
                &mut back,
                bw,
                bh,
                w.cx + w.sw - sz - 6,
                w.cy - TITLE_H + 6,
                sz,
                sz,
                COLOR_CLOSE,
            );
            // Client surface.
            let want = (w.sw as usize) * (w.sh as usize);
            if want > 0 && want * 4 <= w.buf_size {
                let src = unsafe { core::slice::from_raw_parts(w.buf_ptr as *const u32, want) };
                blit_surface(
                    &mut back,
                    bw,
                    bh,
                    w.cx,
                    w.cy,
                    src,
                    w.sw as usize,
                    w.sh as usize,
                );
            }
        }

        // --- DWM panel on top of every window ---
        fill_rect(&mut back, bw, bh, 0, 0, bw as i32, PANEL_H, COLOR_PANEL);
        // Launcher glyph: three stacked bars (hamburger) in the accent color.
        for r in 0..3 {
            fill_rect(&mut back, bw, bh, 10, 8 + r * 5, 20, 2, COLOR_LAUNCHER);
        }
        // One taskbar button per live window, in creation order; the focused
        // window's button is highlighted. Label = 1-based window number.
        {
            let mut slot = 0i32;
            for wi in 0..wins.len() {
                if !wins[wi].alive {
                    continue;
                }
                let (bx, by, bw2, bh2) = taskbtn_rect(slot);
                if bx + bw2 > bw as i32 {
                    break;
                }
                let bg = if focused == Some(wi) {
                    COLOR_TASKBTN_FOCUSED
                } else {
                    COLOR_TASKBTN
                };
                fill_rect(&mut back, bw, bh, bx, by, bw2, bh2, bg);
                let mut label = [0u8; 2];
                two_digits(&mut label, 0, (wi as u64) + 1);
                draw_text_3x5(&mut back, bw, bh, bx + 8, by + 5, 2, COLOR_PANEL_TEXT, &label);
                slot += 1;
            }
        }
        // Uptime clock (MM:SS) on the right.
        {
            let mut stats = libdunit::SystemStats::default();
            let secs = if libdunit::get_system_stats(&mut stats) >= 0 {
                stats.uptime_ticks / 100
            } else {
                0
            };
            let mut clk = *b"00:00";
            two_digits(&mut clk, 0, (secs / 60) % 100);
            two_digits(&mut clk, 3, secs % 60);
            let clk_w = clk.len() as i32 * (GLYPH_W + 1) * 3;
            draw_text_3x5(
                &mut back,
                bw,
                bh,
                bw as i32 - clk_w - 12,
                6,
                3,
                COLOR_PANEL_TEXT,
                &clk,
            );
        }

        // Launcher dropdown, painted on top of the panel and every window.
        if menu_open {
            for i in 0..LAUNCH_APPS.len() {
                let (ix, iy, iw, ih) = menu_item_rect(i as i32);
                let hover = mx >= ix && mx < ix + iw && my >= iy && my < iy + ih;
                let bg = if hover { COLOR_MENU_HOVER } else { COLOR_MENU };
                fill_rect(&mut back, bw, bh, ix, iy, iw, ih, bg);
                draw_text_3x5(
                    &mut back,
                    bw,
                    bh,
                    ix + 10,
                    iy + (ih - 5 * 3) / 2,
                    3,
                    COLOR_PANEL_TEXT,
                    LAUNCH_APPS[i].0,
                );
            }
        }

        draw_cursor(&mut back, bw, bh, mx, my);

        let bytes =
            unsafe { core::slice::from_raw_parts(back.as_ptr() as *const u8, back.len() * 4) };
        libdunit::fb_present(bytes, fb.width, fb.height, 0, 0);

        libdunit::sleep_ms(16);
        ticks += 1;
        if ticks >= 60000 {
            break;
        }
    }

    // Session over: tell every client to exit its input loop so serve_two_clients
    // can reap them (a QUIT to an already-exited pid is a harmless no-op).
    for w in wins.iter() {
        send_input(w.pid, IN_QUIT, 0, 0, 0);
    }
}
