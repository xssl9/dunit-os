use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicUsize, Ordering};

use super::serial_write;

const PAGE_SIZE: usize = 4096;
const MEMMAP_USABLE: u32 = 0;
const MAX_REGIONS: usize = 32;
const BITMAP_BYTES: usize = 65536;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct MemRegion {
    pub base: u64,
    pub length: u64,
    pub region_type: u32,
    pub _pad: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhysicalAddress(pub usize);

impl PhysicalAddress {
    pub fn as_usize(&self) -> usize {
        self.0
    }

    pub fn from_usize(addr: usize) -> Self {
        Self(addr)
    }
}

pub struct PhysicalMemoryManager {
    bitmap: UnsafeCell<&'static mut [u8]>,
    total_frames: usize,
    usable_frames: AtomicUsize,
    free_frames: AtomicUsize,
    base_addr: usize,
}

unsafe impl Sync for PhysicalMemoryManager {}

impl PhysicalMemoryManager {
    pub fn new(memory_start: usize, memory_size: usize, bitmap: &'static mut [u8]) -> Self {
        let total_frames = memory_size / PAGE_SIZE;
        let bitmap_size = (total_frames + 7) / 8;

        for i in 0..bitmap_size.min(bitmap.len()) {
            bitmap[i] = 0;
        }

        Self {
            bitmap: UnsafeCell::new(bitmap),
            total_frames,
            usable_frames: AtomicUsize::new(total_frames),
            free_frames: AtomicUsize::new(total_frames),
            base_addr: memory_start,
        }
    }

    fn mark_frame_index_free(&self, frame_idx: usize) -> bool {
        if frame_idx >= self.total_frames {
            return false;
        }
        let byte_idx = frame_idx / 8;
        let bit_idx = frame_idx % 8;
        let mask = 1u8 << bit_idx;
        let bitmap = unsafe { &mut *self.bitmap.get() };
        if byte_idx < bitmap.len() && (bitmap[byte_idx] & mask) != 0 {
            bitmap[byte_idx] &= !mask;
            true
        } else {
            false
        }
    }

    fn reserve_all_frames(&self) {
        let bitmap = unsafe { &mut *self.bitmap.get() };
        for byte in bitmap.iter_mut() {
            *byte = 0xFF;
        }
        self.usable_frames.store(0, Ordering::SeqCst);
        self.free_frames.store(0, Ordering::SeqCst);
    }

    /// Makes only complete pages inside a usable memory-map region allocatable.
    fn mark_region_free_in_pool(&self, base: u64, length: u64) -> usize {
        let region_start = base as usize;
        let region_end = region_start.saturating_add(length as usize);
        let aligned_start = region_start.saturating_add(PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        let aligned_end = region_end & !(PAGE_SIZE - 1);
        let pool_start = self.base_addr;
        let pool_end = pool_start.saturating_add(self.total_frames * PAGE_SIZE);
        let start = aligned_start.max(pool_start);
        let end = aligned_end.min(pool_end);
        if start >= end {
            return 0;
        }

        let first = (start - pool_start) / PAGE_SIZE;
        let last = (end - pool_start) / PAGE_SIZE;
        let mut newly_free = 0usize;
        for frame_idx in first..last.min(self.total_frames) {
            if self.mark_frame_index_free(frame_idx) {
                newly_free += 1;
            }
        }
        self.usable_frames.fetch_add(newly_free, Ordering::SeqCst);
        self.free_frames.fetch_add(newly_free, Ordering::SeqCst);
        newly_free
    }

    pub fn alloc_frame(&self) -> Option<PhysicalAddress> {
        let bitmap = unsafe { &mut *self.bitmap.get() };

        for byte_idx in 0..bitmap.len() {
            let byte = bitmap[byte_idx];
            if byte != 0xFF {
                for bit_idx in 0..8 {
                    let mask = 1u8 << bit_idx;
                    if (byte & mask) == 0 {
                        bitmap[byte_idx] |= mask;

                        let frame_idx = byte_idx * 8 + bit_idx;
                        if frame_idx < self.total_frames {
                            self.free_frames.fetch_sub(1, Ordering::SeqCst);
                            let addr = self.base_addr + frame_idx * PAGE_SIZE;
                            return Some(PhysicalAddress(addr));
                        }
                    }
                }
            }
        }
        None
    }

    /// Аллоцирует `count` подряд идущих свободных фреймов и возвращает адрес
    /// первого. Растущая куча ядра опирается на это: непрерывные физические
    /// фреймы дают непрерывный виртуальный диапазон через HHDM без правки
    /// таблиц страниц. Скан O(total_frames), но вызывается лишь при росте кучи.
    pub fn alloc_contiguous(&self, count: usize) -> Option<PhysicalAddress> {
        if count == 0 {
            return None;
        }
        if count == 1 {
            return self.alloc_frame();
        }

        let bitmap = unsafe { &mut *self.bitmap.get() };
        let mut run_start: Option<usize> = None;
        let mut run_len = 0usize;

        for frame_idx in 0..self.total_frames {
            let byte_idx = frame_idx / 8;
            let bit_idx = frame_idx % 8;
            let mask = 1u8 << bit_idx;
            let free = byte_idx < bitmap.len() && (bitmap[byte_idx] & mask) == 0;

            if free {
                if run_start.is_none() {
                    run_start = Some(frame_idx);
                    run_len = 1;
                } else {
                    run_len += 1;
                }

                if run_len == count {
                    let start = run_start.unwrap();
                    for idx in start..start + count {
                        let b = idx / 8;
                        let m = 1u8 << (idx % 8);
                        bitmap[b] |= m;
                    }
                    self.free_frames.fetch_sub(count, Ordering::SeqCst);
                    let addr = self.base_addr + start * PAGE_SIZE;
                    return Some(PhysicalAddress(addr));
                }
            } else {
                run_start = None;
                run_len = 0;
            }
        }

        None
    }

    pub fn free_frame(&self, addr: PhysicalAddress) {
        if addr.0 < self.base_addr {
            return;
        }

        let frame_idx = (addr.0 - self.base_addr) / PAGE_SIZE;
        if frame_idx >= self.total_frames {
            return;
        }

        let byte_idx = frame_idx / 8;
        let bit_idx = frame_idx % 8;
        let mask = 1u8 << bit_idx;

        let bitmap = unsafe { &mut *self.bitmap.get() };

        if byte_idx < bitmap.len() {
            if (bitmap[byte_idx] & mask) != 0 {
                bitmap[byte_idx] &= !mask;
                self.free_frames.fetch_add(1, Ordering::SeqCst);
            }
        }
    }

    pub fn available_memory(&self) -> usize {
        self.free_frames.load(Ordering::SeqCst) * PAGE_SIZE
    }

    pub fn total_memory(&self) -> usize {
        self.usable_frames.load(Ordering::SeqCst) * PAGE_SIZE
    }
}

/// Static backing storage for the physical-frame bitmap. Wrapped in an
/// `UnsafeCell` newtype instead of `static mut` so there is no `&'static mut`
/// aliasing UB: `init` takes a single mutable slice out of it exactly once and
/// hands ownership to the `PhysicalMemoryManager`, which is then published
/// read-only through `PMM_INSTANCE`.
struct BitmapStorage(UnsafeCell<[u8; BITMAP_BYTES]>);
unsafe impl Sync for BitmapStorage {}
static PMM_BITMAP: BitmapStorage = BitmapStorage(UnsafeCell::new([0; BITMAP_BYTES]));

/// Write-once singleton: initialised during `init`, only read afterwards via
/// `get_pmm`. The manager itself uses atomics + an internal cell for its mutable
/// state, so a shared `&PhysicalMemoryManager` is sufficient for callers.
static PMM_INSTANCE: crate::sync::OnceCell<PhysicalMemoryManager> = crate::sync::OnceCell::new();

// Символы карты памяти, экспортируемые ассемблерным/C-кодом HAL до входа в Rust.
// Это `extern "C" static mut` по необходимости: их определяет и заполняет ранний
// загрузчик, Rust здесь лишь читатель. Их нельзя завернуть в `UnsafeCell`/лок,
// поэтому единственная безопасная мера — читать их ровно один раз через
// `read_volatile` в `copy_regions_from_boot` (ниже), после чего вся дальнейшая
// работа PMM идёт с локальной копией.
extern "C" {
    static mut boot_mem_regions: [MemRegion; 32];
    static mut boot_mem_region_count: u64;
}

#[inline(never)]
fn copy_regions_from_boot(cache: &mut [MemRegion; MAX_REGIONS]) -> usize {
    unsafe {
        let n = (boot_mem_region_count as usize).min(MAX_REGIONS);
        if n == 0 {
            return 0;
        }

        for i in 0..n {
            let src = &boot_mem_regions[i];
            cache[i] = MemRegion {
                base: core::ptr::read_volatile(&src.base),
                length: core::ptr::read_volatile(&src.length),
                region_type: core::ptr::read_volatile(&src.region_type),
                _pad: 0,
            };
        }
        n
    }
}

fn usable_pool_bounds(regions: &[MemRegion]) -> Option<(usize, usize)> {
    let mut first = usize::MAX;
    let mut last = 0usize;
    for region in regions {
        if region.region_type != MEMMAP_USABLE || region.length < PAGE_SIZE as u64 {
            continue;
        }
        let start = (region.base as usize).saturating_add(PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        let end = (region.base as usize).saturating_add(region.length as usize) & !(PAGE_SIZE - 1);
        if start < end {
            first = first.min(start);
            last = last.max(end);
        }
    }
    (first < last).then_some((first, last))
}

pub fn init() -> bool {
    serial_write("[PMM] init start\r\n");
    serial_write("[PMM] scanning regions\r\n");

    let mut region_cache = [MemRegion {
        base: 0,
        length: 0,
        region_type: 0,
        _pad: 0,
    }; MAX_REGIONS];
    let copied = copy_regions_from_boot(&mut region_cache);
    if copied == 0 {
        serial_write("[PMM] FAIL\r\n");
        return false;
    }
    serial_write("[PMM] regions copied\r\n");

    let regions = &region_cache[..copied];
    let (pool_start, pool_end) = match usable_pool_bounds(regions) {
        Some(bounds) => bounds,
        None => {
            serial_write("[PMM] FAIL\r\n");
            return false;
        }
    };
    let pool_size = pool_end.saturating_sub(pool_start);
    if pool_size < PAGE_SIZE {
        serial_write("[PMM] FAIL\r\n");
        return false;
    }

    let total_frames = pool_size / PAGE_SIZE;
    let bitmap_size = (total_frames + 7) / 8;

    if bitmap_size > BITMAP_BYTES {
        serial_write("[PMM] FAIL\r\n");
        return false;
    }

    // Take the single mutable slice out of the static bitmap storage. `init`
    // runs once at boot before the PMM is published, so this is the only live
    // mutable borrow of PMM_BITMAP for the whole program.
    let bitmap = unsafe {
        let base = PMM_BITMAP.0.get() as *mut u8;
        core::slice::from_raw_parts_mut(base, bitmap_size)
    };

    serial_write("[PMM] pool ready\r\n");

    let pmm = PhysicalMemoryManager::new(pool_start, pool_size, bitmap);
    pmm.reserve_all_frames();

    serial_write("[PMM] enabling usable regions\r\n");
    for region in regions {
        if region.region_type == MEMMAP_USABLE {
            pmm.mark_region_free_in_pool(region.base, region.length);
        }
    }

    if pmm.free_frames.load(Ordering::SeqCst) == 0 {
        serial_write("[PMM] FAIL: no aligned usable frames\r\n");
        return false;
    }
    serial_write("[PMM] usable regions ready\r\n");

    let _ = PMM_INSTANCE.set(pmm);

    serial_write("[PMM] OK\r\n");
    true
}

pub fn init_pmm(memory_start: usize, memory_size: usize, bitmap: &'static mut [u8]) {
    let _ = PMM_INSTANCE.set(PhysicalMemoryManager::new(
        memory_start,
        memory_size,
        bitmap,
    ));
}

pub fn get_pmm() -> Option<&'static PhysicalMemoryManager> {
    PMM_INSTANCE.get()
}

/// Returns (total_bytes, free_bytes) for the physical frame pool, or (0, 0)
/// if the PMM has not been initialized yet.
pub fn stats_bytes() -> (u64, u64) {
    match get_pmm() {
        Some(pmm) => (pmm.total_memory() as u64, pmm.available_memory() as u64),
        None => (0, 0),
    }
}
