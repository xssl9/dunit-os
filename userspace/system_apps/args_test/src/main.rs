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
pub extern "C" fn _start(
    argc: usize,
    argv: libdunit::RawArgv,
    _envp: libdunit::RawEnvp,
) -> ! {
    let mut line = [0u8; 160];
    write_count_line(&mut line, argc);

    let mut index = 0;
    while index < argc {
        let value = unsafe { libdunit::argv_get(argc, argv, index) }.unwrap_or("<invalid>");
        write_arg_line(&mut line, index, value);
        index += 1;
    }

    libdunit::exit(0);
}

fn write_count_line(buf: &mut [u8], argc: usize) {
    let mut len = 0;
    append_bytes(buf, &mut len, b"argc=");
    append_usize(buf, &mut len, argc);
    append_bytes(buf, &mut len, b"\n");
    libdunit::write(1, &buf[..len]);
}

fn write_arg_line(buf: &mut [u8], index: usize, value: &str) {
    let mut len = 0;
    append_bytes(buf, &mut len, b"argv[");
    append_usize(buf, &mut len, index);
    append_bytes(buf, &mut len, b"]=");
    append_bytes(buf, &mut len, value.as_bytes());
    append_bytes(buf, &mut len, b"\n");
    libdunit::write(1, &buf[..len]);
}

fn append_bytes(buf: &mut [u8], len: &mut usize, bytes: &[u8]) {
    for byte in bytes {
        if *len < buf.len() {
            buf[*len] = *byte;
            *len += 1;
        }
    }
}

fn append_usize(buf: &mut [u8], len: &mut usize, mut value: usize) {
    let mut digits = [0u8; 20];
    let mut index = digits.len();

    if value == 0 {
        append_bytes(buf, len, b"0");
        return;
    }

    while value > 0 {
        index -= 1;
        digits[index] = b'0' + (value % 10) as u8;
        value /= 10;
    }

    append_bytes(buf, len, &digits[index..]);
}
