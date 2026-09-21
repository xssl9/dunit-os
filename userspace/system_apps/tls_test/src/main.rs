#![no_std]
#![no_main]
#![feature(thread_local)]

use core::panic::PanicInfo;
use core::sync::atomic::{AtomicU64, Ordering};

const STACK_BYTES: usize = 64 * 1024;
#[repr(align(16))]
struct Stack([u8; STACK_BYTES]);
static mut STACK_A: Stack = Stack([0; STACK_BYTES]);
static mut STACK_B: Stack = Stack([0; STACK_BYTES]);
static READY: AtomicU64 = AtomicU64::new(0);

#[thread_local]
static mut TLS_INITIALIZED: u64 = 0x4455_6677_8899_aabb;
#[thread_local]
static mut TLS_ZERO: u64 = 0;

#[panic_handler]
fn panic(_: &PanicInfo) -> ! { loop { core::hint::spin_loop(); } }

fn fail(reason: &str) -> ! {
    libdunit::print("tls_test: FAIL ");
    libdunit::println(reason);
    libdunit::exit(1)
}

extern "C" fn worker(arg: usize) -> ! {
    unsafe {
        if TLS_INITIALIZED != 0x4455_6677_8899_aabb || TLS_ZERO != 0 {
            libdunit::thread_exit(2);
        }
        TLS_INITIALIZED = arg as u64;
        TLS_ZERO = (arg as u64) << 32;
    }
    READY.fetch_add(1, Ordering::SeqCst);
    for _ in 0..16 { libdunit::yield_now(); }
    unsafe {
        if TLS_INITIALIZED != arg as u64 || TLS_ZERO != (arg as u64) << 32 {
            libdunit::thread_exit(3);
        }
    }
    libdunit::thread_exit(arg as i32)
}

fn join(tid: u32, expected: i32) {
    let mut status = libdunit::WaitStatus::empty();
    for _ in 0..100 {
        let result = libdunit::thread_join(tid, &mut status);
        if result == tid as isize {
            if !status.exited() || status.code != expected { fail("thread status"); }
            return;
        }
        if result != libdunit::EAGAIN { fail("thread join"); }
        libdunit::yield_now();
    }
    fail("thread timeout")
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    libdunit::println("tls_test: start");
    let initial_tp = libdunit::get_thread_pointer();
    if initial_tp == 0 { fail("missing initial TP"); }
    unsafe {
        if TLS_INITIALIZED != 0x4455_6677_8899_aabb || TLS_ZERO != 0 {
            fail("initial TLS image");
        }
        TLS_INITIALIZED = 0x1111;
        TLS_ZERO = 0x2222;
    }
    let manual = libdunit::vm_map(
        0, 4096, libdunit::VM_PROT_READ | libdunit::VM_PROT_WRITE,
        libdunit::VM_MAP_PRIVATE | libdunit::VM_MAP_ANONYMOUS,
    );
    if manual < 0 || libdunit::set_thread_pointer(manual as usize) != 0
        || libdunit::get_thread_pointer() != manual as usize
        || libdunit::set_thread_pointer(initial_tp) != 0 {
        fail("set/get TP");
    }
    if libdunit::set_thread_pointer(usize::MAX) != libdunit::EINVAL { fail("invalid TP"); }
    let top_a = unsafe { (core::ptr::addr_of_mut!(STACK_A) as *mut u8).add(STACK_BYTES) };
    let top_b = unsafe { (core::ptr::addr_of_mut!(STACK_B) as *mut u8).add(STACK_BYTES) };
    let a = libdunit::thread_create(worker, top_a, 7);
    let b = libdunit::thread_create(worker, top_b, 9);
    if a <= 0 || b <= 0 { fail("thread create"); }
    while READY.load(Ordering::SeqCst) != 2 { libdunit::yield_now(); }
    unsafe {
        if TLS_INITIALIZED != 0x1111 || TLS_ZERO != 0x2222 { fail("main TLS isolation"); }
    }
    join(a as u32, 7);
    join(b as u32, 9);
    if libdunit::vm_unmap(manual as usize, 4096) != 0 { fail("manual TP unmap"); }
    libdunit::println("tls_test: OK");
    libdunit::exit(0)
}
