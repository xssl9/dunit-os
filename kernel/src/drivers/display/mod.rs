//! Механизм дисплея ядра: подключаемые бэкенды смены видеорежима.
//!
//! Ядро предоставляет только механизм (задать режим, present, перечислить
//! режимы); политика (какое разрешение и когда) живёт в userspace/конфиге.
//!
//! Бэкенды:
//! - [`Backend::Limine`] — фиксированный фреймбуфер от Limine/GOP. Смена режима
//!   не поддерживается (`set_mode` → `Unsupported`), `present` — no-op (эмулятор
//!   сканирует `KERNEL_FB_ADDR` напрямую). Fallback и путь для реального железа.
//! - [`Backend::Bochs`] — Bochs VBE/DISPI (устройство QEMU `-vga std`): смена
//!   режима на ходу через порты ввода-вывода, переиспользуя маппинг Limine.
//!
//! Глобалы `KERNEL_FB_{ADDR,WIDTH,HEIGHT,PITCH}` остаются единственным источником
//! геометрии фреймбуфера; `set_mode` — единственный писатель после загрузки.

use crate::sync::SpinLock;
use crate::syscall::{KERNEL_FB_HEIGHT, KERNEL_FB_WIDTH};
use core::sync::atomic::Ordering;

pub mod bochs;

/// Видеорежим (pitch выводится как width*4, формат XRGB8888).
#[derive(Clone, Copy)]
pub struct Mode {
    pub width: u32,
    pub height: u32,
}

/// Ошибка операции с дисплеем.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DisplayError {
    /// Бэкенд не умеет менять режим (например, фиксированный фреймбуфер Limine).
    Unsupported,
    /// Запрошенный режим неприемлем (нулевой/превышает бюджет).
    InvalidMode,
    /// Аппаратная ошибка при программировании.
    Io,
}

/// Активный бэкенд. Enum-диспетчеризация вместо `dyn` — без alloc, в стиле ядра.
enum Backend {
    Limine,
    Bochs(bochs::BochsVbe),
}

static ACTIVE: SpinLock<Backend> = SpinLock::new(Backend::Limine);

/// Пробует бэкенды и активирует лучший доступный. Вызывается из
/// `drivers::init()` после `pci::init()`. При неудаче остаётся `Limine`, и
/// фреймбуфер Limine продолжает работать как прежде.
pub fn init() {
    let boot_w = KERNEL_FB_WIDTH.load(Ordering::Relaxed);
    let boot_h = KERNEL_FB_HEIGHT.load(Ordering::Relaxed);
    if boot_w == 0 || boot_h == 0 {
        crate::serial_write("[DISPLAY] no boot framebuffer — backend: none\r\n");
        return;
    }
    if let Some(dev) = bochs::BochsVbe::detect(boot_w, boot_h) {
        *ACTIVE.lock() = Backend::Bochs(dev);
        crate::serial_write("[DISPLAY] backend: bochs-vbe\r\n");
    } else {
        crate::serial_write("[DISPLAY] backend: limine (fixed)\r\n");
    }
}

/// Может ли активный бэкенд менять режим на ходу.
pub fn can_modeset() -> bool {
    matches!(&*ACTIVE.lock(), Backend::Bochs(_))
}

/// Перечисляет доступные режимы в `out`, возвращает их количество.
pub fn query_modes(out: &mut [Mode]) -> usize {
    match &*ACTIVE.lock() {
        Backend::Limine => {
            if out.is_empty() {
                0
            } else {
                out[0] = Mode {
                    width: KERNEL_FB_WIDTH.load(Ordering::Relaxed),
                    height: KERNEL_FB_HEIGHT.load(Ordering::Relaxed),
                };
                1
            }
        }
        Backend::Bochs(d) => d.query_modes(out),
    }
}

/// Переключает scanout на `w`×`h`. Обновляет глобалы фреймбуфера как побочный
/// эффект. Бэкенд Limine возвращает `Unsupported`.
pub fn set_mode(w: u32, h: u32) -> Result<(), DisplayError> {
    match &mut *ACTIVE.lock() {
        Backend::Limine => Err(DisplayError::Unsupported),
        Backend::Bochs(d) => d.set_mode(w, h),
    }
}

/// Выталкивает текущий backing на физический дисплей. `None` = весь экран.
/// Для Limine/Bochs — no-op (запись уже сканируется). Хук для virtio-gpu.
pub fn present(rect: Option<(u32, u32, u32, u32)>) -> Result<(), DisplayError> {
    match &mut *ACTIVE.lock() {
        Backend::Limine => Ok(()),
        Backend::Bochs(d) => d.present(rect),
    }
}
