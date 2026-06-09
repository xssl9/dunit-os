#![no_std]
#![no_main]

use core::panic::PanicInfo;

const PATH: &str = "/tmp/fs_test.txt";
const DATA: &[u8] = b"hello from fs_test";
static mut READ_BUF: [u8; 32] = [0; 32];

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
    libdunit::println("fs_test: start");

    let fd = libdunit::open(
        PATH,
        libdunit::OPEN_CREATE | libdunit::OPEN_WRITE | libdunit::OPEN_TRUNC,
    );
    if fd < 0 {
        libdunit::println("fs_test: open write failed");
        libdunit::exit(1);
    }

    if libdunit::write(fd as usize, DATA) != DATA.len() as isize {
        libdunit::println("fs_test: write failed");
        libdunit::close(fd as usize);
        libdunit::exit(2);
    }

    if libdunit::close(fd as usize) != 0 {
        libdunit::println("fs_test: close write failed");
        libdunit::exit(3);
    }

    let fd = libdunit::open(PATH, libdunit::OPEN_READ);
    if fd < 0 {
        libdunit::println("fs_test: open read failed");
        libdunit::exit(4);
    }

    let buf = unsafe {
        core::slice::from_raw_parts_mut(core::ptr::addr_of_mut!(READ_BUF) as *mut u8, 32)
    };
    let read = libdunit::read(fd as usize, buf);
    if read != DATA.len() as isize {
        libdunit::println("fs_test: read failed");
        libdunit::close(fd as usize);
        libdunit::exit(5);
    }

    let mut idx = 0;
    while idx < DATA.len() {
        if buf[idx] != DATA[idx] {
            libdunit::println("fs_test: data mismatch");
            libdunit::close(fd as usize);
            libdunit::exit(6);
        }
        idx += 1;
    }

    if libdunit::close(fd as usize) != 0 {
        libdunit::println("fs_test: close read failed");
        libdunit::exit(7);
    }

    libdunit::println("fs_test: OK");
    libdunit::exit(0);
}
