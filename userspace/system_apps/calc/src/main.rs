#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;
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
    envp: libdunit::RawEnvp,
) -> ! {
    libdunit::init_runtime(argc, argv, envp);
    libdunit::println("calc: enter expressions like 2+2 or 7 * 3");
    libdunit::println("calc: type exit to quit, Ctrl+C interrupts while waiting");

    loop {
        libdunit::print("calc> ");
        match libdunit::read_line() {
            Ok(line) => {
                let input = trim(&line);
                if input == "exit" || input == "quit" {
                    libdunit::println("calc: bye");
                    libdunit::exit(0);
                }
                match eval(input) {
                    Ok(value) => print_i64(value),
                    Err(msg) => libdunit::println(msg),
                }
            }
            Err(code) => {
                libdunit::print("calc: read failed ");
                libdunit::println(libdunit::error_name(code));
                libdunit::exit(1);
            }
        }
    }
}

fn eval(input: &str) -> Result<i64, &'static str> {
    let bytes = input.as_bytes();
    let mut index = 0usize;
    skip_spaces(bytes, &mut index);
    let left = parse_i64(bytes, &mut index)?;
    skip_spaces(bytes, &mut index);
    if index >= bytes.len() {
        return Ok(left);
    }
    let op = bytes[index];
    index += 1;
    skip_spaces(bytes, &mut index);
    let right = parse_i64(bytes, &mut index)?;
    skip_spaces(bytes, &mut index);
    if index != bytes.len() {
        return Err("calc: expected end of input");
    }

    match op {
        b'+' => Ok(left + right),
        b'-' => Ok(left - right),
        b'*' => Ok(left * right),
        b'/' => {
            if right == 0 {
                Err("calc: division by zero")
            } else {
                Ok(left / right)
            }
        }
        _ => Err("calc: expected operator + - * /"),
    }
}

fn parse_i64(bytes: &[u8], index: &mut usize) -> Result<i64, &'static str> {
    let mut sign = 1i64;
    if *index < bytes.len() && bytes[*index] == b'-' {
        sign = -1;
        *index += 1;
    }
    if *index >= bytes.len() || !bytes[*index].is_ascii_digit() {
        return Err("calc: expected number");
    }

    let mut value = 0i64;
    while *index < bytes.len() && bytes[*index].is_ascii_digit() {
        value = value * 10 + (bytes[*index] - b'0') as i64;
        *index += 1;
    }
    Ok(value * sign)
}

fn skip_spaces(bytes: &[u8], index: &mut usize) {
    while *index < bytes.len() && bytes[*index] == b' ' {
        *index += 1;
    }
}

fn trim(input: &String) -> &str {
    let bytes = input.as_bytes();
    let mut start = 0usize;
    let mut end = bytes.len();
    while start < end && bytes[start] == b' ' {
        start += 1;
    }
    while end > start && bytes[end - 1] == b' ' {
        end -= 1;
    }
    unsafe { core::str::from_utf8_unchecked(&bytes[start..end]) }
}

fn print_i64(value: i64) {
    if value < 0 {
        libdunit::print("-");
        libdunit::print_usize((-value) as usize);
    } else {
        libdunit::print_usize(value as usize);
    }
    libdunit::print("\n");
}
