#![no_std]
#![no_main]

use core::panic::PanicInfo;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

const STACK_BYTES: usize = 64 * 1024;
#[repr(align(16))]
struct Stack([u8; STACK_BYTES]);
static mut STACK_W: Stack = Stack([0; STACK_BYTES]);

/// Shared futex word the worker parks on. Starts at OLD, main flips it to NEW.
const OLD: u32 = 0;
const NEW: u32 = 0xC0FFEE;
static FUTEX_WORD: AtomicU32 = AtomicU32::new(OLD);
/// Worker signals it is about to park / has finished here.
static WORKER_READY: AtomicU64 = AtomicU64::new(0);

#[panic_handler]
fn panic(_: &PanicInfo) -> ! { loop { core::hint::spin_loop(); } }

fn fail(reason: &str) -> ! {
    libdunit::print("futex_test: FAIL ");
    libdunit::println(reason);
    libdunit::exit(1)
}

/// Worker parks until the shared word becomes NEW, then confirms the value.
/// Exit code 0 = success; other codes flag a specific failure.
extern "C" fn worker(_arg: usize) -> ! {
    WORKER_READY.store(1, Ordering::SeqCst);
    // Loop because a wake is only a hint: recheck the word each time. The
    // compare-and-block is atomic, so if main already flipped the word before
    // we park, futex_wait returns EAGAIN and we observe NEW without blocking.
    let mut spins = 0u64;
    while FUTEX_WORD.load(Ordering::SeqCst) != NEW {
        let r = libdunit::futex_wait(&FUTEX_WORD, OLD, 0);
        // 0 = woken, EAGAIN = value already changed. Anything else is a bug.
        if r != 0 && r != libdunit::EAGAIN {
            libdunit::thread_exit(2);
        }
        spins += 1;
        if spins > 1_000_000 {
            libdunit::thread_exit(3);
        }
    }
    if FUTEX_WORD.load(Ordering::SeqCst) != NEW {
        libdunit::thread_exit(4);
    }
    libdunit::thread_exit(0)
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    libdunit::println("futex_test: start");

    // 1) EAGAIN: word value != expected returns immediately.
    let mismatch = AtomicU32::new(5);
    if libdunit::futex_wait(&mismatch, 7, 0) != libdunit::EAGAIN {
        fail("expected EAGAIN on value mismatch");
    }

    // 2) Timeout: a matching word with a small timeout must return, not hang.
    let timed = AtomicU32::new(42);
    let r = libdunit::futex_wait(&timed, 42, 50);
    if r < 0 {
        fail("timeout wait returned error");
    }

    // 3) Wake: worker parks on FUTEX_WORD == OLD; main flips it and wakes.
    let top_w = unsafe { (core::ptr::addr_of_mut!(STACK_W) as *mut u8).add(STACK_BYTES) };
    let w = libdunit::thread_create(worker, top_w, 0);
    if w <= 0 { fail("thread create"); }
    while WORKER_READY.load(Ordering::SeqCst) != 1 { libdunit::yield_now(); }
    // Give the worker time to actually park in futex_wait.
    for _ in 0..64 { libdunit::yield_now(); }
    FUTEX_WORD.store(NEW, Ordering::SeqCst);
    let woken = libdunit::futex_wake(&FUTEX_WORD, 0);
    if woken < 0 { fail("futex_wake error"); }

    // Join the worker and confirm it observed NEW (exit code 0).
    let mut status = libdunit::WaitStatus::empty();
    let mut joined = false;
    for _ in 0..10000 {
        let result = libdunit::thread_join(w as u32, &mut status);
        if result == w as isize {
            if !status.exited() || status.code != 0 { fail("worker status"); }
            joined = true;
            break;
        }
        if result != libdunit::EAGAIN { fail("thread join"); }
        libdunit::yield_now();
    }
    if !joined { fail("worker timeout"); }

    libdunit::println("futex_test: OK");
    libdunit::exit(0)
}
