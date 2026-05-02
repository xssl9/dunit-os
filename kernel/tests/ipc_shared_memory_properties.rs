use proptest::prelude::*;
use std::collections::{BTreeMap, HashMap};

const PAGE_SIZE: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct ProcessId(u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct SharedMemoryId(u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PhysicalAddress(usize);

impl PhysicalAddress {
    fn as_usize(&self) -> usize {
        self.0
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
}

struct SharedMemoryRegion {
    id: SharedMemoryId,
    physical_addr: u64,
    size: usize,
    owners: Vec<ProcessId>,
}

impl SharedMemoryRegion {
    fn new(id: SharedMemoryId, physical_addr: u64, size: usize) -> Self {
        Self {
            id,
            physical_addr,
            size,
            owners: Vec::new(),
        }
    }

    fn add_owner(&mut self, pid: ProcessId) {
        if !self.owners.contains(&pid) {
            self.owners.push(pid);
        }
    }
}

struct IpcManager {
    shared_regions: BTreeMap<SharedMemoryId, SharedMemoryRegion>,
    next_shared_id: u64,
    pmm: PhysicalMemoryManager,
}

impl IpcManager {
    fn new(pmm: PhysicalMemoryManager) -> Self {
        Self {
            shared_regions: BTreeMap::new(),
            next_shared_id: 1,
            pmm,
        }
    }

    fn create_shared_memory(&mut self, size: usize) -> Option<SharedMemoryId> {
        let num_pages = (size + 4095) / 4096;
        
        let mut physical_frames = Vec::new();
        for _ in 0..num_pages {
            match self.pmm.alloc_frame() {
                Some(frame) => physical_frames.push(frame),
                None => {
                    return None;
                }
            }
        }

        let id = SharedMemoryId(self.next_shared_id);
        self.next_shared_id += 1;

        let physical_addr = physical_frames[0].as_usize() as u64;
        let region = SharedMemoryRegion::new(id, physical_addr, size);
        self.shared_regions.insert(id, region);

        Some(id)
    }

    fn attach_shared_memory(&mut self, id: SharedMemoryId, pid: ProcessId) -> Option<u64> {
        let region = self.shared_regions.get_mut(&id)?;
        region.add_owner(pid);
        Some(region.physical_addr)
    }

    fn get_shared_region(&self, id: SharedMemoryId) -> Option<&SharedMemoryRegion> {
        self.shared_regions.get(&id)
    }
}

struct SimulatedMemory {
    memory: HashMap<u64, Vec<u8>>,
}

impl SimulatedMemory {
    fn new() -> Self {
        Self {
            memory: HashMap::new(),
        }
    }

    fn write(&mut self, addr: u64, data: &[u8]) {
        self.memory.insert(addr, data.to_vec());
    }

    fn read(&self, addr: u64) -> Option<&[u8]> {
        self.memory.get(&addr).map(|v| v.as_slice())
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]
    
    #[test]
    fn prop_shared_memory_visibility(
        size in 1usize..1024,
        data_byte in 0u8..255,
        pid1 in 1u64..1000,
        pid2 in 1u64..1000
    ) {
        let pmm = PhysicalMemoryManager::new(0x100000, 1024 * PAGE_SIZE);
        let mut ipc = IpcManager::new(pmm);
        let mut sim_mem = SimulatedMemory::new();
        
        let size = size * 64;
        
        if let Some(shm_id) = ipc.create_shared_memory(size) {
            let proc1 = ProcessId(pid1);
            let proc2 = ProcessId(pid2);
            
            if let Some(phys_addr1) = ipc.attach_shared_memory(shm_id, proc1) {
                if let Some(phys_addr2) = ipc.attach_shared_memory(shm_id, proc2) {
                    assert_eq!(phys_addr1, phys_addr2);
                    
                    let data = vec![data_byte; 64];
                    sim_mem.write(phys_addr1, &data);
                    
                    if let Some(read_data) = sim_mem.read(phys_addr2) {
                        assert_eq!(read_data, &data[..]);
                    }
                    
                    if let Some(region) = ipc.get_shared_region(shm_id) {
                        assert!(region.owners.contains(&proc1));
                        assert!(region.owners.contains(&proc2));
                    }
                }
            }
        }
    }
}
