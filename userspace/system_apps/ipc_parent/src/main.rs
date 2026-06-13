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

fn append_u32_decimal(out: &mut [u8], mut len: usize, value: u32) -> usize {
    let mut digits = [0u8; 10];
    let mut count = 0usize;
    let mut n = value;
    if n == 0 {
        digits[0] = b'0';
        count = 1;
    } else {
        while n > 0 {
            digits[count] = b'0' + (n % 10) as u8;
            count += 1;
            n /= 10;
        }
    }
    while count > 0 {
        count -= 1;
        out[len] = digits[count];
        len += 1;
    }
    len
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    libdunit::println("ipc_parent: start");

    let mut empty = [0u8; 4];
    if libdunit::ipc_recv(&mut empty) != libdunit::EAGAIN {
        libdunit::println("ipc_parent: empty recv should be EAGAIN");
        libdunit::exit(7);
    }

    let child = libdunit::spawn("ipc_child");
    if child < 0 {
        libdunit::println("ipc_parent: spawn failed");
        libdunit::exit(1);
    }

    let mut ping = [0u8; 32];
    ping[..5].copy_from_slice(b"ping:");
    let len = append_u32_decimal(&mut ping, 5, libdunit::get_pid());
    if libdunit::ipc_send(child as u32, &ping[..len]) != len as isize {
        libdunit::println("ipc_parent: send ping failed");
        libdunit::exit(2);
    }
    libdunit::println("ipc_parent: sent ping");

    if libdunit::yield_now() != 0 {
        libdunit::println("ipc_parent: yield failed");
        libdunit::exit(3);
    }

    let mut pong = [0u8; 16];
    let pong_len = libdunit::ipc_recv(&mut pong);
    if pong_len != 4 || &pong[..4] != b"pong" {
        libdunit::println("ipc_parent: expected pong");
        libdunit::exit(4);
    }
    libdunit::println("ipc_parent: got pong");

    let mut status = libdunit::WaitStatus::empty();
    let waited = libdunit::wait(child as u32, &mut status);
    if waited != child {
        libdunit::println("ipc_parent: wait failed");
        libdunit::exit(5);
    }
    if !status.exited() || status.code != 0 {
        libdunit::println("ipc_parent: child status mismatch");
        libdunit::exit(6);
    }

    libdunit::println("ipc_parent: OK");
    libdunit::exit(0);
}
