#![no_std]
#![no_main]

//! Untrusted peer for the M3 shared-buffer capability-transfer test.
//!
//! `gui_server` allocates a zero-copy shared buffer, writes a marker pattern,
//! transfers a capability handle to this child process, and IPC-sends the
//! child's local handle number. This peer maps that handle, verifies it sees
//! the parent's bytes (proving the transferred capability aliases the same
//! physical frames), writes its own marker back, and acks. The parent then
//! reads the child's bytes — confirming bidirectional zero-copy sharing across
//! a process boundary via a transferred capability.

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    libdunit::println("gui_shbuf_peer: PANIC");
    libdunit::exit(101)
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    // Message layout: [parent_pid: u32 LE][handle: u32 LE].
    let mut msg = [0u8; 8];
    if libdunit::ipc_recv_blocking(&mut msg, 0) != 8 {
        libdunit::println("gui_shbuf_peer: FAIL recv");
        libdunit::exit(1);
    }
    let parent = u32::from_le_bytes([msg[0], msg[1], msg[2], msg[3]]);
    let handle = u32::from_le_bytes([msg[4], msg[5], msg[6], msg[7]]);

    let mapped = libdunit::handle_map(handle, 0, 4096);
    if mapped <= 0 {
        libdunit::println("gui_shbuf_peer: FAIL map");
        libdunit::exit(2);
    }
    let ptr = mapped as usize as *mut u8;

    // Parent wrote 0xA5,0x5A at offsets 0,1 before the transfer.
    let seen_parent = unsafe {
        core::ptr::read_volatile(ptr) == 0xA5 && core::ptr::read_volatile(ptr.add(1)) == 0x5A
    };
    if !seen_parent {
        libdunit::println("gui_shbuf_peer: FAIL parent pattern");
        libdunit::exit(3);
    }

    // Write our own marker back for the parent to observe.
    unsafe {
        core::ptr::write_volatile(ptr.add(2), 0xC3);
        core::ptr::write_volatile(ptr.add(3), 0x3C);
    }
    libdunit::println("gui_shbuf_peer: shared capability round-trip OK");

    libdunit::ipc_send(parent, b"done");
    libdunit::exit(0)
}
