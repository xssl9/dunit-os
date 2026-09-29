//! Бэкенд Bochs VBE (DISPI / BGA).
//!
//! QEMU `-device VGA` (std VGA) реализует этот legacy-интерфейс: смена режима на
//! ходу через порты ввода-вывода 0x01CE (индекс) / 0x01CF (данные). База
//! линейного фреймбуфера при смене режима НЕ меняется — меняются только размеры
//! scanout. Поэтому мы переиспользуем маппинг фреймбуфера Limine и лишь
//! перепрограммируем геометрию, ограничиваясь бюджетом boot-разрешения (без
//! ремапа BAR0). Больший режим потребовал бы ремапа — это отдельная веха.
//!
//! Bochs-бэкенд — временный scaffolding для проверки всего пользовательского и
//! конфигурационного пути на текущем харнесе; вытесняется бэкендом virtio-gpu.

use super::{DisplayError, Mode};
use crate::hal;
use crate::syscall::{KERNEL_FB_HEIGHT, KERNEL_FB_PITCH, KERNEL_FB_WIDTH};
use core::sync::atomic::Ordering;

const VBE_DISPI_IOPORT_INDEX: u16 = 0x01CE;
const VBE_DISPI_IOPORT_DATA: u16 = 0x01CF;

const VBE_DISPI_INDEX_ID: u16 = 0;
const VBE_DISPI_INDEX_XRES: u16 = 1;
const VBE_DISPI_INDEX_YRES: u16 = 2;
const VBE_DISPI_INDEX_BPP: u16 = 3;
const VBE_DISPI_INDEX_ENABLE: u16 = 4;

const VBE_DISPI_DISABLED: u16 = 0x00;
const VBE_DISPI_ENABLED: u16 = 0x01;
const VBE_DISPI_LFB_ENABLED: u16 = 0x40;

const VBE_DISPI_BPP_32: u16 = 32;

/// Кандидатные режимы, предлагаемые userspace. Каждый отсекается по бюджету
/// boot-разрешения в `set_mode`/`query_modes`.
const CANDIDATE_MODES: [(u32, u32); 4] = [(1280, 720), (1600, 900), (1920, 1080), (1024, 768)];

fn write_reg(index: u16, value: u16) {
    unsafe {
        hal::hal_outw(VBE_DISPI_IOPORT_INDEX, index);
        hal::hal_outw(VBE_DISPI_IOPORT_DATA, value);
    }
}

fn read_reg(index: u16) -> u16 {
    unsafe {
        hal::hal_outw(VBE_DISPI_IOPORT_INDEX, index);
        hal::hal_inw(VBE_DISPI_IOPORT_DATA)
    }
}

pub struct BochsVbe {
    /// Максимум пикселей, помещающихся в маппинг Limine (boot_w * boot_h).
    budget_px: u32,
}

impl BochsVbe {
    /// Детектит BGA по регистру ID (0xB0C0..=0xB0C5).
    pub fn detect(boot_w: u32, boot_h: u32) -> Option<Self> {
        let id = read_reg(VBE_DISPI_INDEX_ID);
        if !(0xB0C0..=0xB0C5).contains(&id) {
            return None;
        }
        Some(BochsVbe {
            budget_px: boot_w.saturating_mul(boot_h),
        })
    }

    pub fn query_modes(&self, out: &mut [Mode]) -> usize {
        let mut n = 0;
        for &(w, h) in CANDIDATE_MODES.iter() {
            if n >= out.len() {
                break;
            }
            if w.saturating_mul(h) <= self.budget_px {
                out[n] = Mode { width: w, height: h };
                n += 1;
            }
        }
        n
    }

    pub fn set_mode(&mut self, w: u32, h: u32) -> Result<(), DisplayError> {
        if w == 0 || h == 0 || w > u16::MAX as u32 || h > u16::MAX as u32 {
            return Err(DisplayError::InvalidMode);
        }
        // Переиспользуем маппинг boot-фреймбуфера: режим, чей бюджет превышает
        // boot-разрешение, потребовал бы ремапа BAR0 — отказываем.
        if w.saturating_mul(h) > self.budget_px {
            return Err(DisplayError::InvalidMode);
        }
        write_reg(VBE_DISPI_INDEX_ENABLE, VBE_DISPI_DISABLED);
        write_reg(VBE_DISPI_INDEX_XRES, w as u16);
        write_reg(VBE_DISPI_INDEX_YRES, h as u16);
        write_reg(VBE_DISPI_INDEX_BPP, VBE_DISPI_BPP_32);
        write_reg(
            VBE_DISPI_INDEX_ENABLE,
            VBE_DISPI_ENABLED | VBE_DISPI_LFB_ENABLED,
        );
        // Проверяем, что устройство приняло геометрию.
        if read_reg(VBE_DISPI_INDEX_XRES) != w as u16 || read_reg(VBE_DISPI_INDEX_YRES) != h as u16 {
            return Err(DisplayError::Io);
        }
        // База LFB неизменна; двигаются только размеры scanout. Обновляем глобалы,
        // чтобы все пути отрисовки (sys_fb_present) использовали новую геометрию.
        // KERNEL_FB_ADDR остаётся маппингом Limine.
        KERNEL_FB_WIDTH.store(w, Ordering::Relaxed);
        KERNEL_FB_HEIGHT.store(h, Ordering::Relaxed);
        KERNEL_FB_PITCH.store(w.saturating_mul(4), Ordering::Relaxed);
        crate::serial_write("[DISPLAY] bochs set_mode ");
        crate::serial::write_dec(w as u64);
        crate::serial_write("x");
        crate::serial::write_dec(h as u64);
        crate::serial_write("\r\n");
        Ok(())
    }

    pub fn present(&mut self, _rect: Option<(u32, u32, u32, u32)>) -> Result<(), DisplayError> {
        // std VGA сканирует KERNEL_FB_ADDR напрямую — выталкивать нечего.
        Ok(())
    }
}
