use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicUsize, Ordering};

const PAGE_SIZE: usize = 4096;

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

static mut PMM_INSTANCE: Option<PhysicalMemoryManager> = None;

pub fn init_pmm(memory_start: usize, memory_size: usize, bitmap: &'static mut [u8]) {
    unsafe {
        PMM_INSTANCE = Some(PhysicalMemoryManager::new(memory_start, memory_size, bitmap));
    }
}

pub fn get_pmm() -> Option<&'static PhysicalMemoryManager> {
    unsafe { PMM_INSTANCE.as_ref() }
}
