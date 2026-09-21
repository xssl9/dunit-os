#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_: &PanicInfo) -> ! { loop { core::hint::spin_loop(); } }

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mapped = libdunit::vm_map(
        0, 4096, libdunit::VM_PROT_READ | libdunit::VM_PROT_WRITE,
        libdunit::VM_MAP_PRIVATE | libdunit::VM_MAP_ANONYMOUS | libdunit::VM_MAP_GUARD,
    );
    if mapped < 0 { libdunit::exit(2); }
    unsafe { ((mapped as usize - 4096) as *mut u8).write_volatile(1); }
    libdunit::exit(3)
}
