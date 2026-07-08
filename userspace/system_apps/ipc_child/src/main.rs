#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    loop {
        unsafe {
            core::arch::asm!("pause", options(nomem, nostack));
        }
    }
}

fn parse_parent_pid(buf: &[u8]) -> Option<u32> {
    if buf.len() < 6 || &buf[..5] != b"ping:" {
        return None;
    }
    let mut pid = 0u32;
    for byte in &buf[5..] {
        if *byte < b'0' || *byte > b'9' {
            return None;
        }
        pid = pid.checked_mul(10)?.checked_add((*byte - b'0') as u32)?;
    }
    Some(pid)
}

fn recv_ping(buf: &mut [u8]) -> isize {
    let mut attempts = 0usize;
    while attempts < 8 {
        let len = libdunit::ipc_recv(buf);
        if len != libdunit::EAGAIN {
            return len;
        }
        let yielded = libdunit::yield_now();
        if yielded < 0 && yielded != libdunit::EAGAIN {
            return yielded;
        }
        attempts += 1;
    }
    libdunit::EAGAIN
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut buf = [0u8; 32];
    let len = recv_ping(&mut buf);
    if len < 0 {
        libdunit::println("ipc_child: recv failed");
        libdunit::exit(1);
    }

    let Some(parent_pid) = parse_parent_pid(&buf[..len as usize]) else {
        libdunit::println("ipc_child: expected ping");
        libdunit::exit(2);
    };

    libdunit::println("ipc_child: got ping");
    if libdunit::ipc_send(parent_pid, b"pong") != 4 {
        libdunit::println("ipc_child: send failed");
        libdunit::exit(3);
    }
    libdunit::println("ipc_child: sent pong");
    libdunit::exit(0);
}
