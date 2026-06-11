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

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut buf = [0u8; 64];

    libdunit::print("cwd_test: start cwd=");
    print_cwd(&mut buf);

    if libdunit::chdir("/tmp") != 0 {
        libdunit::println("cwd_test: chdir failed");
        libdunit::exit(1);
    }

    libdunit::print("cwd_test: after chdir cwd=");
    let len = libdunit::getcwd(&mut buf);
    if len < 0 {
        libdunit::println("cwd_test: getcwd failed");
        libdunit::exit(2);
    }
    let cwd = unsafe { core::str::from_utf8_unchecked(&buf[..len as usize]) };
    libdunit::println(cwd);

    if cwd != "/tmp" {
        libdunit::println("cwd_test: cwd mismatch");
        libdunit::exit(3);
    }

    libdunit::println("cwd_test: OK");
    libdunit::exit(0);
}

fn print_cwd(buf: &mut [u8]) {
    let len = libdunit::getcwd(buf);
    if len < 0 {
        libdunit::println("<error>");
        return;
    }

    let cwd = unsafe { core::str::from_utf8_unchecked(&buf[..len as usize]) };
    libdunit::println(cwd);
}
