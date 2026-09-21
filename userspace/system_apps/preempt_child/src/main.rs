#![no_std]
#![no_main]

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
    // Keep a distinct XMM value live across timer interrupts and switches.
    let pattern = [0x5au8; 16];
    let mask: u32;
    unsafe {
        core::arch::asm!(
            "movdqu xmm0, [{pattern}]",
            "mov rcx, {count}",
            "xor rdx, rdx",
            "2: imul rdx, rdx, 33",
            "add rdx, rcx",
            "dec rcx",
            "jnz 2b",
            "pcmpeqb xmm0, [{pattern}]",
            "pmovmskb eax, xmm0",
            pattern = in(reg) pattern.as_ptr(), count = in(reg) iters,
            lateout("eax") mask, out("rcx") _, out("rdx") _, out("xmm0") _,
            options(nostack),
        );
    }
    if mask != 0xffff { libdunit::exit(1); }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    spin(SPIN_ITERS);
    libdunit::exit(EXIT_CODE);
}
