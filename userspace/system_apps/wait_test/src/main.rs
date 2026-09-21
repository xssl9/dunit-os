#![no_std]
#![no_main]

use core::panic::PanicInfo;
use core::sync::atomic::{AtomicU64, Ordering};

const STACK_BYTES: usize = 64 * 1024;
#[repr(align(16))]
struct Stack([u8; STACK_BYTES]);
static mut STACK: Stack = Stack([0; STACK_BYTES]);
static WORKER_TID: AtomicU64 = AtomicU64::new(0);

#[panic_handler]
fn panic(_: &PanicInfo) -> ! { loop { core::hint::spin_loop(); } }

fn fail(reason: &str) -> ! {
    libdunit::print("wait_test: FAIL ");
    libdunit::println(reason);
    libdunit::exit(1);
}

extern "C" fn worker(_: usize) -> ! {
    WORKER_TID.store(libdunit::get_tid() as u64, Ordering::SeqCst);
    libdunit::sleep_ms(50);
    let mut stats = libdunit::SystemStats::default();
    if libdunit::get_system_stats(&mut stats) != 0 || stats.process_blocked == 0 {
        let _ = libdunit::ipc_send(libdunit::get_pid(), b"bad!");
        libdunit::thread_exit(3);
    }
    if libdunit::ipc_send(libdunit::get_pid(), b"wake") != 4 {
        libdunit::thread_exit(2);
    }
    libdunit::thread_exit(0);
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    libdunit::println("wait_test: start");
    let mut empty = [0u8; 16];
    if libdunit::ipc_recv(&mut empty) != libdunit::EAGAIN { fail("empty queue"); }
    if libdunit::wait_ipc_event(20) != 0 { fail("timeout wait"); }
    if libdunit::ipc_recv(&mut empty) != libdunit::EAGAIN { fail("timeout received data"); }
    let top = unsafe { (core::ptr::addr_of_mut!(STACK) as *mut u8).add(STACK_BYTES) };
    let tid = libdunit::thread_create(worker, top, 0);
    if tid <= 0 { fail("thread create"); }
    let mut message = [0u8; 16];
    let received = libdunit::ipc_recv_blocking(&mut message, 0);
    if received != 4 || &message[..4] != b"wake" { fail("blocked receive"); }
    if WORKER_TID.load(Ordering::SeqCst) != tid as u64 { fail("thread identity"); }
    let mut status = libdunit::WaitStatus::empty();
    for _ in 0..100 {
        if libdunit::thread_join(tid as u32, &mut status) == tid {
            if !status.exited() || status.code != 0 { fail("thread status"); }
            libdunit::println("wait_test: OK");
            libdunit::exit(0);
        }
        libdunit::yield_now();
    }
    fail("join timeout");
}
