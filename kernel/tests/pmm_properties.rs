use proptest::prelude::*;

const PAGE_SIZE: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PhysicalAddress(usize);

impl PhysicalAddress {
    fn as_usize(&self) -> usize {
        self.0
    }

    fn from_usize(addr: usize) -> Self {
        Self(addr)
    }
}

struct PhysicalMemoryManager {
    bitmap: Vec<u8>,
    total_frames: usize,
    free_frames: usize,
    base_addr: usize,
}

impl PhysicalMemoryManager {
    fn new(memory_start: usize, memory_size: usize) -> Self {
        let total_frames = memory_size / PAGE_SIZE;
        let bitmap_size = (total_frames + 7) / 8;

        Self {
            bitmap: vec![0u8; bitmap_size],
            total_frames,
            free_frames: total_frames,
            base_addr: memory_start,
        }
    }

    fn alloc_frame(&mut self) -> Option<PhysicalAddress> {
        for byte_idx in 0..self.bitmap.len() {
            let byte = self.bitmap[byte_idx];
            if byte != 0xFF {
                for bit_idx in 0..8 {
                    let mask = 1u8 << bit_idx;
                    if (byte & mask) == 0 {
                        self.bitmap[byte_idx] |= mask;

                        let frame_idx = byte_idx * 8 + bit_idx;
                        if frame_idx < self.total_frames {
                            self.free_frames -= 1;
                            let addr = self.base_addr + frame_idx * PAGE_SIZE;
                            return Some(PhysicalAddress(addr));
                        }
                    }
                }
            }
        }
        None
    }

    fn free_frame(&mut self, addr: PhysicalAddress) {
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

        if byte_idx < self.bitmap.len() {
            if (self.bitmap[byte_idx] & mask) != 0 {
                self.bitmap[byte_idx] &= !mask;
                self.free_frames += 1;
            }
        }
    }

    fn available_memory(&self) -> usize {
        self.free_frames * PAGE_SIZE
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn prop_allocator_provides_memory(memory_size in 1usize..1024) {
        let memory_size = memory_size * PAGE_SIZE;
        let mut pmm = PhysicalMemoryManager::new(0x100000, memory_size);

        if let Some(addr) = pmm.alloc_frame() {
            assert!(addr.as_usize() >= 0x100000);
            assert!(addr.as_usize() < 0x100000 + memory_size);
            assert_eq!(addr.as_usize() % PAGE_SIZE, 0);
        }
    }

    #[test]
    fn prop_alloc_free_roundtrip(num_allocs in 1usize..100) {
        let memory_size = 1024 * PAGE_SIZE;
        let mut pmm = PhysicalMemoryManager::new(0x100000, memory_size);

        let initial_free = pmm.available_memory();
        let mut allocated = Vec::new();

        for _ in 0..num_allocs.min(1024) {
            if let Some(addr) = pmm.alloc_frame() {
                allocated.push(addr);
            } else {
                break;
            }
        }

        for addr in allocated {
            pmm.free_frame(addr);
        }

        assert_eq!(pmm.available_memory(), initial_free);
    }
}
