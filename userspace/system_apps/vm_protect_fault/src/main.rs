#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_: &PanicInfo) -> ! { loop { core::hint::spin_loop(); } }

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mapped = libdunit::vm_map(
        0, 4096, libdunit::VM_PROT_READ | libdunit::VM_PROT_WRITE,
        libdunit::VM_MAP_PRIVATE | libdunit::VM_MAP_ANONYMOUS,
    );
    if mapped < 0 { libdunit::exit(2); }
    if libdunit::vm_protect(mapped as usize, 4096, libdunit::VM_PROT_READ) != 0 {
        libdunit::exit(3);
    }
    unsafe { (mapped as *mut u8).write_volatile(1); }
    libdunit::exit(4)
}
