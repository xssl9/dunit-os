#![no_std]
#![no_main]

use core::panic::PanicInfo;

const PATH: &str = "/tmp/file_api_test.txt";

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
    libdunit::println("file_api_test: start");

    if let Err(code) = libdunit::write_string(PATH, "alpha\n") {
        fail("write_string", code);
    }
    if let Err(code) = libdunit::append_string(PATH, "beta\n") {
        fail("append_string", code);
    }
    if !libdunit::file_exists(PATH) {
        libdunit::println("file_api_test: file_exists failed");
        libdunit::exit(3);
    }

    match libdunit::read_to_string(PATH) {
        Ok(contents) => {
            if contents.as_bytes() != b"alpha\nbeta\n" {
                libdunit::println("file_api_test: data mismatch");
                libdunit::exit(4);
            }
        }
        Err(code) => fail("read_to_string", code),
    }

    libdunit::println("file_api_test: OK");
    libdunit::exit(0);
}

fn fail(op: &str, code: isize) -> ! {
    libdunit::print("file_api_test: ");
    libdunit::print(op);
    libdunit::print(" failed ");
    libdunit::println(libdunit::error_name(code));
    libdunit::exit(1);
}
