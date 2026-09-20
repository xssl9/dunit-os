#![no_std]
#![no_main]

use core::hint::black_box;
use core::panic::PanicInfo;

// Pure CPU-bound work: enough integer iterations that, at ~100 Hz, several
// timer ticks land while this loop runs. The child issues NO syscalls until it
// exits, so it can only make progress if the kernel timer preempts whoever was
// running and schedules it. `black_box` stops the optimizer from folding the
// loop away.
const SPIN_ITERS: u64 = 20_000_000;
const EXIT_CODE: i32 = 33;

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    loop {
        unsafe {
            core::arch::asm!("pause", options(nomem, nostack));
        }
    }
}

fn spin(iters: u64) {
    let mut acc: u64 = 0;
    let mut i: u64 = 0;
    while i < iters {
        acc = black_box(acc.wrapping_mul(6364136223846793005).wrapping_add(i));
        i += 1;
    }
    black_box(acc);
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    spin(SPIN_ITERS);
    libdunit::exit(EXIT_CODE);
}
