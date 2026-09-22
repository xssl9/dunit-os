//! Примитивы синхронизации ядра.
//!
//! Dunit сейчас исполняется на одном CPU с кооперативной планировкой userspace,
//! но часть глобального изменяемого состояния ядра также затрагивается из
//! обработчика таймера (путь преемпшна). Раньше это состояние жило в
//! `static mut ... : Option<T>` и раздавалось как `&'static mut` без какой-либо
//! синхронизации — это одновременно и потенциальный data race с IRQ, и
//! фундаментальный блокер для SMP.
//!
//! Этот модуль задаёт единую честную стратегию синхронизации, которая корректна
//! уже сегодня и готова к включению преемпшна/SMP:
//!
//! - [`SpinLock`] — простой test-and-set лок для данных, которые разделяются
//!   только между кооперативными путями ядра.
//! - [`IrqSafeSpinLock`] — тот же spinlock, но дополнительно запрещающий
//!   прерывания на время критической секции, чтобы обработчик прерывания не мог
//!   наблюдать или менять защищённые данные в середине обновления. Именно этот
//!   лок нужен для состояния, к которому обращается IRQ таймера.
//! - [`InterruptGuard`] — RAII-хелпер, который запрещает прерывания и
//!   восстанавливает предыдущее состояние флага IF при выходе (безопасен при
//!   вложенности).
//!
//! Всё это `no_std` и без внешних зависимостей, чтобы пережить запланированный
//! переход на userspace поверх libc.

use core::cell::UnsafeCell;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicBool, Ordering};

/// Возвращает `true`, если прерывания сейчас разрешены (RFLAGS.IF установлен).
#[inline]
pub fn interrupts_enabled() -> bool {
    let flags: u64;
    unsafe {
        core::arch::asm!("pushfq; pop {}", out(reg) flags, options(nomem));
    }
    (flags & (1 << 9)) != 0
}

/// Запрещает аппаратные прерывания (`cli`).
#[inline]
pub fn disable_interrupts() {
    unsafe {
        core::arch::asm!("cli", options(nomem, nostack));
    }
}

/// Разрешает аппаратные прерывания (`sti`).
#[inline]
pub fn enable_interrupts() {
    unsafe {
        core::arch::asm!("sti", options(nomem, nostack));
    }
}

/// RAII-страж, запрещающий прерывания на время своей жизни и восстанавливающий
/// предыдущее состояние IF при уничтожении. Безопасен при вложенности: каждый
/// страж запоминает своё исходное состояние, поэтому внутренний страж никогда
/// не включит прерывания раньше внешнего.
pub struct InterruptGuard {
    were_enabled: bool,
}

impl InterruptGuard {
    #[inline]
    pub fn new() -> Self {
        let were_enabled = interrupts_enabled();
        disable_interrupts();
        Self { were_enabled }
    }
}

impl Default for InterruptGuard {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for InterruptGuard {
    #[inline]
    fn drop(&mut self) {
        if self.were_enabled {
            enable_interrupts();
        }
    }
}

/// Простой test-and-set spinlock для данных, разделяемых только кооперативными
/// путями ядра (не из контекста прерывания).
pub struct SpinLock<T> {
    locked: AtomicBool,
    data: UnsafeCell<T>,
}

// Лок обеспечивает эксклюзивный доступ, поэтому его можно безопасно делить между
// потоками/ядрами, если сами данные допускают передачу между ними.
unsafe impl<T: Send> Sync for SpinLock<T> {}
unsafe impl<T: Send> Send for SpinLock<T> {}

impl<T> SpinLock<T> {
    pub const fn new(data: T) -> Self {
        Self {
            locked: AtomicBool::new(false),
            data: UnsafeCell::new(data),
        }
    }

    /// Захватывает лок, крутясь до освобождения.
    pub fn lock(&self) -> SpinLockGuard<'_, T> {
        while self
            .locked
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            while self.locked.load(Ordering::Relaxed) {
                core::hint::spin_loop();
            }
        }
        SpinLockGuard { lock: self }
    }

    /// Пытается захватить лок без ожидания. Возвращает `None`, если он занят.
    pub fn try_lock(&self) -> Option<SpinLockGuard<'_, T>> {
        if self
            .locked
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
        {
            Some(SpinLockGuard { lock: self })
        } else {
            None
        }
    }

    #[inline]
    fn release(&self) {
        self.locked.store(false, Ordering::Release);
    }
}

pub struct SpinLockGuard<'a, T> {
    lock: &'a SpinLock<T>,
}

impl<T> Deref for SpinLockGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*self.lock.data.get() }
    }
}

impl<T> DerefMut for SpinLockGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.lock.data.get() }
    }
}

impl<T> Drop for SpinLockGuard<'_, T> {
    fn drop(&mut self) {
        self.lock.release();
    }
}

/// Spinlock, который дополнительно запрещает прерывания на время удержания.
///
/// Нужен для данных, к которым обращается и кооперативный путь ядра, и
/// обработчик прерывания (например, таблица процессов и путь преемпшна из IRQ
/// таймера). Прерывания запрещаются ДО захвата лока и восстанавливаются ПОСЛЕ
/// его освобождения, поэтому на одном ядре обработчик прерывания физически не
/// может вклиниться в критическую секцию, а на SMP лок продолжает крутиться.
pub struct IrqSafeSpinLock<T> {
    inner: SpinLock<T>,
}

unsafe impl<T: Send> Sync for IrqSafeSpinLock<T> {}
unsafe impl<T: Send> Send for IrqSafeSpinLock<T> {}

impl<T> IrqSafeSpinLock<T> {
    pub const fn new(data: T) -> Self {
        Self {
            inner: SpinLock::new(data),
        }
    }

    /// Запрещает прерывания и захватывает лок.
    pub fn lock(&self) -> IrqSafeSpinLockGuard<'_, T> {
        let irq = InterruptGuard::new();
        let guard = self.inner.lock();
        IrqSafeSpinLockGuard { guard, _irq: irq }
    }

    /// Пытается захватить лок без ожидания, предварительно запретив прерывания.
    /// Если лок занят, прерывания восстанавливаются и возвращается `None`.
    pub fn try_lock(&self) -> Option<IrqSafeSpinLockGuard<'_, T>> {
        let irq = InterruptGuard::new();
        match self.inner.try_lock() {
            Some(guard) => Some(IrqSafeSpinLockGuard { guard, _irq: irq }),
            None => None,
        }
    }
}

/// Порядок полей важен: `Drop` для структуры сначала освобождает лок (`guard`),
/// и лишь затем уничтожается `_irq`, восстанавливая IF. Так прерывания снова
/// включаются только после того, как лок отпущен.
pub struct IrqSafeSpinLockGuard<'a, T> {
    guard: SpinLockGuard<'a, T>,
    _irq: InterruptGuard,
}

impl<T> Deref for IrqSafeSpinLockGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.guard
    }
}

impl<T> DerefMut for IrqSafeSpinLockGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.guard
    }
}

/// Write-once cell for `static` singletons that are initialised exactly once
/// during early boot and then only read. Replaces the `static mut ... : Option<T>`
/// + `unsafe { X = Some(..) }` / `unsafe { X.as_ref() }` pattern: `set` succeeds
/// only for the first caller (serialised by an atomic), and `get` hands out a
/// shared `&T` without `unsafe` at the call site. The stored `T` must provide its
/// own interior synchronisation (e.g. atomics or a `SpinLock`) if it is mutated
/// after publication — `OnceCell` guarantees single initialisation, not interior
/// mutability.
pub struct OnceCell<T> {
    initialized: AtomicBool,
    // `true` only while `set` is writing; readers spin until it clears so a
    // concurrent `get` never observes a half-written value.
    writing: AtomicBool,
    value: UnsafeCell<Option<T>>,
}

unsafe impl<T: Send + Sync> Sync for OnceCell<T> {}
unsafe impl<T: Send> Send for OnceCell<T> {}

impl<T> OnceCell<T> {
    pub const fn new() -> Self {
        Self {
            initialized: AtomicBool::new(false),
            writing: AtomicBool::new(false),
            value: UnsafeCell::new(None),
        }
    }

    /// Initialise the cell. Returns `Ok(())` for the first caller and
    /// `Err(value)` (handing the value back) if it was already initialised or a
    /// concurrent `set` won the race.
    pub fn set(&self, value: T) -> Result<(), T> {
        if self
            .writing
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            return Err(value);
        }
        if self.initialized.load(Ordering::Acquire) {
            self.writing.store(false, Ordering::Release);
            return Err(value);
        }
        unsafe {
            *self.value.get() = Some(value);
        }
        self.initialized.store(true, Ordering::Release);
        self.writing.store(false, Ordering::Release);
        Ok(())
    }

    /// Shared reference to the value, or `None` before initialisation.
    pub fn get(&self) -> Option<&T> {
        if !self.initialized.load(Ordering::Acquire) {
            return None;
        }
        // A `set` may still be clearing its flag; the store to `value` is already
        // visible (initialized was released after it), so the reference is sound.
        unsafe { (*self.value.get()).as_ref() }
    }

    pub fn is_initialized(&self) -> bool {
        self.initialized.load(Ordering::Acquire)
    }
}

impl<T> Default for OnceCell<T> {
    fn default() -> Self {
        Self::new()
    }
}
