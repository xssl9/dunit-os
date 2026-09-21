//! Monotonic clock and deadline interface. PIT is the first clock source;
//! callers depend on this interface rather than PIT registers or IRQ0.
use core::sync::atomic::{AtomicU64, Ordering};

pub const TICKS_PER_SECOND: u64 = 100;
const PIT_DIVISOR: u16 = 11_931;
static MONOTONIC_TICKS: AtomicU64 = AtomicU64::new(0);

pub fn init_pit() {
    unsafe {
        crate::hal::hal_outb(0x43, 0x36);
        crate::hal::hal_outb(0x40, PIT_DIVISOR as u8);
        crate::hal::hal_outb(0x40, (PIT_DIVISOR >> 8) as u8);
    }
}

/// Called exactly once for each IRQ0, regardless of whether a task is active.
#[inline]
pub fn on_tick() {
    MONOTONIC_TICKS.fetch_add(1, Ordering::Relaxed);
}

#[inline]
pub fn monotonic_ticks() -> u64 {
    MONOTONIC_TICKS.load(Ordering::Relaxed)
}

pub fn monotonic_ns() -> u64 {
    monotonic_ticks().saturating_mul(1_000_000_000 / TICKS_PER_SECOND)
}

#[derive(Clone, Copy)]
pub struct Deadline(u64);

impl Deadline {
    pub fn after_ms(ms: u64) -> Self {
        let ticks = ms.saturating_mul(TICKS_PER_SECOND).saturating_add(999) / 1000;
        Self(monotonic_ticks().saturating_add(ticks.max(1)))
    }

    #[inline]
    pub fn expired(self) -> bool {
        monotonic_ticks() >= self.0
    }
}
