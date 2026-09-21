#![no_std]
#![no_main]

use core::panic::PanicInfo;
use core::sync::atomic::{AtomicU64, Ordering};

const STACK_BYTES: usize = 64 * 1024;
const SPIN: u64 = 40_000_000;

#[repr(align(16))]
struct Stack([u8; STACK_BYTES]);
static mut STACK_A: Stack = Stack([0; STACK_BYTES]);
static mut STACK_B: Stack = Stack([0; STACK_BYTES]);
static mut STACK_C: Stack = Stack([0; STACK_BYTES]);
static mut STACK_D: Stack = Stack([0; STACK_BYTES]);
static OWNER: AtomicU64 = AtomicU64::new(0);
static SHARED: AtomicU64 = AtomicU64::new(0);
static QUICK_COUNT: AtomicU64 = AtomicU64::new(0);

#[panic_handler]
fn panic(_: &PanicInfo) -> ! { loop { core::hint::spin_loop(); } }

fn fail(message: &str) -> ! {
    libdunit::print("thread_test: FAIL ");
    libdunit::println(message);
    libdunit::exit(1)
}

extern "C" fn worker(arg: usize) -> ! {
    let pid = libdunit::get_pid() as u64;
    let tid = libdunit::get_tid() as u64;
    if pid != OWNER.load(Ordering::SeqCst) || tid == pid {
        libdunit::thread_exit(2);
    }
    let pattern = [arg as u8; 16];
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
            pattern = in(reg) pattern.as_ptr(), count = in(reg) SPIN,
            lateout("eax") mask, out("rcx") _, out("rdx") _, out("xmm0") _,
            options(nostack),
        );
    }
    if mask != 0xffff { libdunit::thread_exit(3); }
    SHARED.fetch_add(1, Ordering::SeqCst);
    libdunit::thread_exit(arg as i32);
}

extern "C" fn unjoined_worker(_: usize) -> ! {
    loop { core::hint::spin_loop(); }
}

extern "C" fn returns_without_exit(_: usize) {}

extern "C" fn quick_worker(arg: usize) -> ! {
    QUICK_COUNT.fetch_add(1, Ordering::SeqCst);
    libdunit::thread_exit(arg as i32);
}

fn join(tid: u32, expected: i32) {
    let mut status = libdunit::WaitStatus::empty();
    for _ in 0..128 {
        let result = libdunit::thread_join(tid, &mut status);
        if result == tid as isize {
            if !status.exited() || status.code != expected { fail("status"); }
            return;
        }
        if result != libdunit::EAGAIN { fail("join"); }
        let _ = libdunit::yield_now();
    }
    fail("timeout");
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    libdunit::println("thread_test: start");
    let pid = libdunit::get_pid();
    if libdunit::get_tid() != pid { fail("main TID"); }
    OWNER.store(pid as u64, Ordering::SeqCst);
    let top_a = unsafe { (core::ptr::addr_of_mut!(STACK_A) as *mut u8).add(STACK_BYTES) };
    let top_b = unsafe { (core::ptr::addr_of_mut!(STACK_B) as *mut u8).add(STACK_BYTES) };
    let a = libdunit::thread_create(worker, top_a, 0x5a);
    let b = libdunit::thread_create(worker, top_b, 0xa5);
    if a <= 0 || b <= 0 || a == b || a == pid as isize || b == pid as isize {
        fail("create");
    }
    join(a as u32, 0x5a);
    join(b as u32, 0xa5);
    if SHARED.load(Ordering::SeqCst) != 2 { fail("shared address space"); }
    if libdunit::thread_join(pid, &mut libdunit::WaitStatus::empty()) != libdunit::ECHILD {
        fail("join main thread");
    }
    let top_d = unsafe { (core::ptr::addr_of_mut!(STACK_D) as *mut u8).add(STACK_BYTES) };
    if libdunit::syscall3(libdunit::SYSCALL_THREAD_CREATE, 0, top_d as usize, 0) != libdunit::EFAULT {
        fail("unmapped entry");
    }
    if libdunit::syscall3(libdunit::SYSCALL_THREAD_CREATE, worker as usize, 0, 0) != libdunit::EINVAL {
        fail("unmapped stack");
    }
    let faulty = libdunit::syscall3(
        libdunit::SYSCALL_THREAD_CREATE, returns_without_exit as usize, top_d as usize, 0,
    );
    if faulty <= 0 { fail("fault thread create"); }
    let mut fault_status = libdunit::WaitStatus::empty();
    let mut fault_joined = false;
    for _ in 0..128 {
        let result = libdunit::thread_join(faulty as u32, &mut fault_status);
        if result == faulty { fault_joined = true; break; }
        if result != libdunit::EAGAIN { fail("fault thread join"); }
        let _ = libdunit::yield_now();
    }
    if !fault_joined || !fault_status.faulted() || fault_status.code != libdunit::EFAULT as i32 {
        fail("thread fault isolation");
    }
    for _ in 0..24 {
        let tid = libdunit::thread_create(quick_worker, top_a, 7);
        if tid <= 0 { fail("repeated create"); }
        join(tid as u32, 7);
    }
    if QUICK_COUNT.load(Ordering::SeqCst) != 24 { fail("repeated lifecycle"); }
    let top_c = unsafe { (core::ptr::addr_of_mut!(STACK_C) as *mut u8).add(STACK_BYTES) };
    if libdunit::thread_create(unjoined_worker, top_c, 0) <= 0 { fail("orphan create"); }
    libdunit::println("thread_test: OK shared=2 xmm=isolated");
    libdunit::exit(0);
}
