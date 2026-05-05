#![no_std]
#![no_main]

use core::panic::PanicInfo;

const SYSCALL_WRITE: usize = 1;
const SYSCALL_EXIT: usize = 60;

fn syscall3(num: usize, arg1: usize, arg2: usize, arg3: usize) -> isize {
    let ret: isize;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") num,
            in("rdi") arg1,
            in("rsi") arg2,
            in("rdx") arg3,
            lateout("rax") ret,
            options(nostack)
        );
    }
    ret
}

fn write_str(s: &str) {
    syscall3(SYSCALL_WRITE, 1, s.as_ptr() as usize, s.len());
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    write_str("Dunit OS System Monitor v1.0\n");
    write_str("CPU: 0% | RAM: 0MB/0MB\n");
    syscall3(SYSCALL_EXIT, 0, 0, 0);
    loop {}
}
