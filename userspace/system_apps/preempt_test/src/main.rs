#![no_std]
#![no_main]

use core::panic::PanicInfo;

// M1 preemption proof (userspace side).
//
// This process spawns a CPU-bound child (`preempt_child`) and then does its own
// CPU-bound work WITHOUT ever calling `yield_now`. In the cooperative model a
// spawned child stays Ready but never runs until the parent explicitly yields,
// and `wait` is non-scheduling (it only reports status, it does not hand off the
// CPU). So the ONLY way the child can run to completion here is if the kernel's
// timer forcibly preempts this process. Observing the child's exit code is the
// proof that preemption fired.

const CHILD_EXIT_CODE: i32 = 33;
// Big enough that, at ~100 Hz, many ticks land while we spin — under TCG too.
const BIG_SPIN: u64 = 60_000_000;
// Bounded backstop so a preemption regression fails fast instead of hanging.
const POLL_ROUNDS: usize = 64;
const POLL_SPIN: u64 = 3_000_000;

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    loop {
        unsafe {
            core::arch::asm!("pause", options(nomem, nostack));
        }
    }
}

fn fail(message: &str) -> ! {
    libdunit::print("preempt_test: FAIL ");
    libdunit::println(message);
    libdunit::exit(1);
}

fn spin(iters: u64) {
    let pattern = [0xa5u8; 16];
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
    if mask != 0xffff { fail("SSE state corrupted"); }
}

/// Non-blocking wait: returns true once the child has exited with `code`.
/// Never yields.
fn child_exited(pid: u32) -> bool {
    let mut status = libdunit::WaitStatus::empty();
    let waited = libdunit::wait(pid, &mut status);
    if waited == libdunit::EAGAIN {
        return false;
    }
    if waited != pid as isize {
        fail("wait returned unexpected pid");
    }
    if !status.exited() || status.code != CHILD_EXIT_CODE {
        fail("child exited with wrong status");
    }
    true
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    libdunit::println("preempt_test: start");

    let child = libdunit::spawn("preempt_child");
    if child < 0 {
        fail("spawn preempt_child");
    }
    let child = child as u32;

    // One large spin guarantees several timer ticks land here. With preemption
    // on, the timer switches to the child during this window even though we
    // never yield.
    spin(BIG_SPIN);

    // Backstop: keep doing CPU work (still no yield) and poll for the child.
    let mut round = 0usize;
    while round < POLL_ROUNDS {
        if child_exited(child) {
            libdunit::println("preempt_test: OK");
            libdunit::exit(0);
        }
        spin(POLL_SPIN);
        round += 1;
    }

    fail("no preemption");
}
