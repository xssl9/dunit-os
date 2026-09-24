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

/// Control-message magic distinguishing a capability announce from a protocol
/// packet (protocol packets begin with the DGUI wire magic instead). Shared
/// with the compositor.
const CTRL_MAGIC: u32 = 0x3150_4143; // "CAP1"
const SURFACE: u64 = 1;
const BUFFER: u64 = 2;
const W: u32 = 48;
const H: u32 = 48;
const FMT_XRGB8888: u32 = 1;

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

/// Send `payload` to the compositor wrapped in a 4-byte client-id envelope, so
/// it can route messages from several concurrent clients to the right protocol
/// connection. Replies come back un-enveloped on our own queue.
fn send_env(dst: u32, id: u32, payload: &[u8]) {
    let mut m = [0u8; 256];
    m[0..4].copy_from_slice(&id.to_le_bytes());
    let len = payload.len().min(m.len() - 4);
    m[4..4 + len].copy_from_slice(&payload[..len]);
    libdunit::ipc_send(dst, &m[..4 + len]);
}

// APPEND_MARKER

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

    // 2) Render into our own kernel shared buffer (a solid surface tinted by id
    //    so the two concurrent clients are visually distinct on screen).
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
    let (r, g, b) = if id == 1 { (0x30, 0xC0, 0x20) } else { (0xE0, 0x60, 0x20) };
    unsafe {
        for i in 0..(W * H) as usize {
            let o = i * 4;
            core::ptr::write_volatile(px.add(o), b);
            core::ptr::write_volatile(px.add(o + 1), g);
            core::ptr::write_volatile(px.add(o + 2), r);
            core::ptr::write_volatile(px.add(o + 3), 0xff);
        }
    }

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

