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
    let mut clients = [ClientState::empty(); 2];
    let slots = [(300u32, 100u32), (400u32, 220u32)];
    for i in 0..2 {
        let conn = match server.connect() {
            Some(c) => c,
            None => return false,
        };
        let pid = libdunit::spawn("gui_client");
        if pid <= 0 {
            return false;
        }
        clients[i].pid = pid as u32;
        clients[i].conn = conn;
        clients[i].slot_x = slots[i].0;
        clients[i].slot_y = slots[i].1;
        // Handshake: our pid (IPC + cap-transfer target) + this client's id.
        let mut hs = [0u8; 8];
        hs[0..4].copy_from_slice(&libdunit::get_pid().to_le_bytes());
        hs[4..8].copy_from_slice(&((i as u32) + 1).to_le_bytes());
        libdunit::ipc_send(clients[i].pid, &hs);
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

    // Both untrusted windows are now blitted to the framebuffer at their slots,
    // and those blits are the last thing written to the display. Linger here —
    // before any further terminal text repaints over them — so the composited
    // frame is actually visible on screen (and can be screenshotted). A real
    // compositor loops forever; this demo just holds the frame, then tears down.
    if both {
        // Emit the success marker BEFORE holding the frame, so automated smokes
        // observe it without waiting out the hold. Then re-present both windows
        // repeatedly for a few seconds: the kernel terminal draws to the same
        // framebuffer, so a one-shot blit gets clobbered; refreshing keeps the
        // composited frame on screen long enough to actually see (and capture).
        libdunit::println("gui_server: served two untrusted clients OK");
        // Hold the composited frame: re-present continuously (the kernel terminal
        // shares this framebuffer, so a one-shot blit gets clobbered) for the whole
        // session. We deliberately do NOT break on a keypress: the composited
        // windows must stay on screen while the user pokes the system — an
        // interactive session ends by closing the QEMU window, not a keystroke. The
        // cap only bounds headless smokes so they can't wedge (they force-quit after
        // the screenshot long before it is reached).
        for _ in 0..6000 {
            for c in clients.iter() {
                if c.buf_ptr.is_null() || c.surf_w == 0 || c.surf_h == 0 {
                    continue;
                }
                let want = c.surf_w as u64 * c.surf_h as u64 * 4;
                if want <= c.buf_size as u64 {
                    let data = unsafe { core::slice::from_raw_parts(c.buf_ptr, want as usize) };
                    libdunit::fb_present(data, c.surf_w, c.surf_h, c.slot_x, c.slot_y);
                }
            }
            libdunit::sleep_ms(100);
        }
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
