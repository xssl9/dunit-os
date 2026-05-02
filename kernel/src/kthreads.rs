use alloc::vec::Vec;
use alloc::boxed::Box;

pub type ThreadFn = fn() -> !;

pub struct KernelThread {
    pub id: usize,
    pub name: &'static str,
    pub func: ThreadFn,
}

static mut THREADS: Option<Vec<KernelThread>> = None;

pub fn init() {
    unsafe {
        THREADS = Some(Vec::new());
    }
}

pub fn spawn(name: &'static str, func: ThreadFn) -> usize {
    unsafe {
        if let Some(threads) = &mut THREADS {
            let id = threads.len();
            threads.push(KernelThread { id, name, func });
            id
        } else {
            0
        }
    }
}

pub fn run_all() -> ! {
    unsafe {
        if let Some(threads) = &THREADS {
            if !threads.is_empty() {
                let func = threads[0].func;
                func();
            }
        }
    }
    loop {}
}
