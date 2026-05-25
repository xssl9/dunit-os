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
            free_frames: AtomicUsize::new(total_frames),
            base_addr: memory_start,
        }
    }

    fn mark_frame_index_used(&self, frame_idx: usize) -> bool {
        if frame_idx >= self.total_frames {
            return false;
        }

        let byte_idx = frame_idx / 8;
        let bit_idx = frame_idx % 8;
        let mask = 1u8 << bit_idx;

        let bitmap = unsafe { &mut *self.bitmap.get() };
        if byte_idx < bitmap.len() && (bitmap[byte_idx] & mask) == 0 {
            bitmap[byte_idx] |= mask;
            true
        } else {
            false
        }
    }

    /// Mark only frames inside the PMM pool that overlap [base, base + length).
    fn mark_region_used_in_pool(&self, base: u64, length: u64) -> usize {
        if length == 0 {
            return 0;
        }

        let pool_start = self.base_addr;
        let pool_end = pool_start.saturating_add(self.total_frames * PAGE_SIZE);

        let region_start = base as usize;
        let region_end = match (base as usize).checked_add(length as usize) {
            Some(end) => end,
            None => usize::MAX,
        };

        let overlap_start = region_start.max(pool_start);
        let overlap_end = region_end.min(pool_end);

        if overlap_start >= overlap_end {
            return 0;
        }

        let first_frame = (overlap_start - pool_start) / PAGE_SIZE;
        let last_frame = overlap_end
            .saturating_sub(pool_start)
            .saturating_add(PAGE_SIZE - 1)
            / PAGE_SIZE;

        let mut newly_used = 0usize;
        for frame_idx in first_frame..last_frame.min(self.total_frames) {
            if self.mark_frame_index_used(frame_idx) {
                newly_used += 1;
            }
        }

        if newly_used != 0 {
            self.free_frames.fetch_sub(newly_used, Ordering::SeqCst);
        }

        newly_used
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
        self.total_frames * PAGE_SIZE
    }
}

static mut PMM_BITMAP: [u8; BITMAP_BYTES] = [0; BITMAP_BYTES];
static mut PMM_INSTANCE: Option<PhysicalMemoryManager> = None;
static mut REGION_CACHE: [MemRegion; MAX_REGIONS] = [MemRegion {
    base: 0,
    length: 0,
    region_type: 0,
    _pad: 0,
}; MAX_REGIONS];

extern "C" {
    static mut boot_mem_regions: [MemRegion; 32];
    static mut boot_mem_region_count: u64;
}

#[inline(never)]
fn copy_regions_from_boot() -> usize {
    unsafe {
        let n = (boot_mem_region_count as usize).min(MAX_REGIONS);
        if n == 0 {
            return 0;
        }

        for i in 0..n {
            let src = &boot_mem_regions[i];
            REGION_CACHE[i] = MemRegion {
                base: core::ptr::read_volatile(&src.base),
                length: core::ptr::read_volatile(&src.length),
                region_type: core::ptr::read_volatile(&src.region_type),
                _pad: 0,
            };
        }
        n
    }
}

fn largest_usable_region(regions: &[MemRegion]) -> Option<MemRegion> {
    let mut best: Option<MemRegion> = None;

    for &region in regions {
        if region.region_type != MEMMAP_USABLE || region.length < PAGE_SIZE as u64 {
            continue;
        }

        match best {
            None => best = Some(region),
            Some(current) if region.length > current.length => best = Some(region),
            _ => {}
        }
    }

    best
}

pub fn init() -> bool {
    serial_write("[PMM] init start\r\n");
    serial_write("[PMM] scanning regions\r\n");

    let copied = copy_regions_from_boot();
    if copied == 0 {
        serial_write("[PMM] FAIL\r\n");
        return false;
    }
    serial_write("[PMM] regions copied\r\n");

    let regions = unsafe { &REGION_CACHE[..copied] };
    let usable = match largest_usable_region(regions) {
        Some(region) => region,
        None => {
            serial_write("[PMM] FAIL\r\n");
            return false;
        }
    };

    let base = usable.base as usize;
    let size = usable.length as usize;
    let aligned_base = (base + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
    let usable_size = size.saturating_sub(aligned_base.saturating_sub(base));
    let aligned_size = usable_size & !(PAGE_SIZE - 1);

    if aligned_size < PAGE_SIZE {
        serial_write("[PMM] FAIL\r\n");
        return false;
    }

    let total_frames = aligned_size / PAGE_SIZE;
    let bitmap_size = (total_frames + 7) / 8;

    if bitmap_size > BITMAP_BYTES {
        serial_write("[PMM] FAIL\r\n");
        return false;
    }

    let bitmap = unsafe {
        let slice = core::slice::from_raw_parts_mut(PMM_BITMAP.as_mut_ptr(), bitmap_size);
        core::mem::transmute::<&mut [u8], &'static mut [u8]>(slice)
    };

    serial_write("[PMM] pool ready\r\n");

    let pmm = PhysicalMemoryManager::new(aligned_base, aligned_size, bitmap);

    serial_write("[PMM] marking reserved\r\n");

    for region in regions {
        if region.region_type == MEMMAP_USABLE {
            continue;
        }
        pmm.mark_region_used_in_pool(region.base, region.length);
    }

    for region in regions {
        if region.region_type != MEMMAP_USABLE {
            continue;
        }
        if region.base == usable.base && region.length == usable.length {
            continue;
        }
        pmm.mark_region_used_in_pool(region.base, region.length);
    }

    serial_write("[PMM] reserved done\r\n");

    unsafe {
        PMM_INSTANCE = Some(pmm);
    }

    serial_write("[PMM] OK\r\n");
    true
}

pub fn init_pmm(memory_start: usize, memory_size: usize, bitmap: &'static mut [u8]) {
    unsafe {
        PMM_INSTANCE = Some(PhysicalMemoryManager::new(memory_start, memory_size, bitmap));
    }
}

pub fn get_pmm() -> Option<&'static PhysicalMemoryManager> {
    unsafe { PMM_INSTANCE.as_ref() }
}
