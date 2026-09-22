use alloc::boxed::Box;
use alloc::vec::Vec;
use core::cell::UnsafeCell;

pub type ThreadFn = fn() -> !;

pub struct KernelThread {
    pub id: usize,
    pub name: &'static str,
    pub func: ThreadFn,
}

/// Таблица зарегистрированных потоков ядра. Раньше `static mut Option<Vec<..>>`;
/// теперь `UnsafeCell`-newtype без `static mut`. Заполняется на этапе загрузки и
/// используется кооперативно на одном CPU.
struct ThreadsCell(UnsafeCell<Option<Vec<KernelThread>>>);
unsafe impl Sync for ThreadsCell {}
static THREADS: ThreadsCell = ThreadsCell(UnsafeCell::new(None));

pub fn init() {
    unsafe {
        *THREADS.0.get() = Some(Vec::new());
    }
}

pub fn spawn(name: &'static str, func: ThreadFn) -> usize {
    unsafe {
        if let Some(threads) = &mut *THREADS.0.get() {
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
        if let Some(threads) = &*THREADS.0.get() {
            if !threads.is_empty() {
                let func = threads[0].func;
                func();
            }
        }
    }
    loop {}
}
