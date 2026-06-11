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
    libdunit::println("fault_pf: triggering page fault");
    unsafe {
        let ptr = core::ptr::null::<u64>();
        core::ptr::read_volatile(ptr);
    }
    libdunit::exit(99);
}
