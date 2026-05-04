use super::pmm::PhysicalAddress;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VirtualAddress(pub usize);

impl VirtualAddress {
    pub fn as_usize(&self) -> usize {
        self.0
    }
    
    pub fn from_usize(addr: usize) -> Self {
        Self(addr)
    }
    
    fn p4_index(&self) -> usize {
        (self.0 >> 39) & 0x1FF
    }
    
    fn p3_index(&self) -> usize {
        (self.0 >> 30) & 0x1FF
    }
    
    fn p2_index(&self) -> usize {
        (self.0 >> 21) & 0x1FF
    }
    
    fn p1_index(&self) -> usize {
        (self.0 >> 12) & 0x1FF
    }
    
    fn offset(&self) -> usize {
        self.0 & 0xFFF
    }
}

bitflags::bitflags! {
    #[derive(Clone, Copy)]
    pub struct PageFlags: u64 {
        const PRESENT = 1 << 0;
        const WRITABLE = 1 << 1;
        const USER = 1 << 2;
        const WRITE_THROUGH = 1 << 3;
        const NO_CACHE = 1 << 4;
        const ACCESSED = 1 << 5;
        const DIRTY = 1 << 6;
        const HUGE = 1 << 7;
        const GLOBAL = 1 << 8;
        const NO_EXECUTE = 1 << 63;
    }
}

#[repr(C, align(4096))]
pub struct PageTable {
    entries: [PageTableEntry; 512],
}

impl PageTable {
    pub const fn new() -> Self {
        Self {
            entries: [PageTableEntry::new(); 512],
        }
    }
    
    pub fn zero(&mut self) {
        for entry in self.entries.iter_mut() {
            entry.set_unused();
        }
    }
    
    pub fn get_entry(&self, index: usize) -> &PageTableEntry {
        &self.entries[index]
    }
    
    pub fn get_entry_mut(&mut self, index: usize) -> &mut PageTableEntry {
        &mut self.entries[index]
    }
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct PageTableEntry {
    entry: u64,
}

impl PageTableEntry {
    pub const fn new() -> Self {
        Self { entry: 0 }
    }
    
    pub fn is_unused(&self) -> bool {
        self.entry == 0
    }
    
    pub fn set_unused(&mut self) {
        self.entry = 0;
    }
    
    pub fn flags(&self) -> PageFlags {
        PageFlags::from_bits_truncate(self.entry)
    }
    
    pub fn addr(&self) -> PhysicalAddress {
        PhysicalAddress((self.entry & 0x000F_FFFF_FFFF_F000) as usize)
    }
    
    pub fn set(&mut self, addr: PhysicalAddress, flags: PageFlags) {
        self.entry = (addr.as_usize() as u64) | flags.bits();
    }
}

pub struct VirtualMemoryManager {
    page_table: &'static mut PageTable,
}

impl VirtualMemoryManager {
    pub fn new(page_table: &'static mut PageTable) -> Self {
        page_table.zero();
        Self { page_table }
    }
    
    pub fn map_page(&mut self, virt: VirtualAddress, phys: PhysicalAddress, flags: PageFlags) {
        let p4_index = virt.p4_index();
        let p3_index = virt.p3_index();
        let p2_index = virt.p2_index();
        let p1_index = virt.p1_index();
        
        let p4_entry = self.page_table.get_entry_mut(p4_index);
        if p4_entry.is_unused() {
            return;
        }
        
        let p3_table = unsafe { &mut *(p4_entry.addr().as_usize() as *mut PageTable) };
        let p3_entry = p3_table.get_entry_mut(p3_index);
        if p3_entry.is_unused() {
            return;
        }
        
        let p2_table = unsafe { &mut *(p3_entry.addr().as_usize() as *mut PageTable) };
        let p2_entry = p2_table.get_entry_mut(p2_index);
        if p2_entry.is_unused() {
            return;
        }
        
        let p1_table = unsafe { &mut *(p2_entry.addr().as_usize() as *mut PageTable) };
        let p1_entry = p1_table.get_entry_mut(p1_index);
        
        p1_entry.set(phys, flags | PageFlags::PRESENT);
    }
    
    pub fn unmap_page(&mut self, virt: VirtualAddress) {
        let p4_index = virt.p4_index();
        let p3_index = virt.p3_index();
        let p2_index = virt.p2_index();
        let p1_index = virt.p1_index();
        
        let p4_entry = self.page_table.get_entry_mut(p4_index);
        if p4_entry.is_unused() {
            return;
        }
        
        let p3_table = unsafe { &mut *(p4_entry.addr().as_usize() as *mut PageTable) };
        let p3_entry = p3_table.get_entry_mut(p3_index);
        if p3_entry.is_unused() {
            return;
        }
        
        let p2_table = unsafe { &mut *(p3_entry.addr().as_usize() as *mut PageTable) };
        let p2_entry = p2_table.get_entry_mut(p2_index);
        if p2_entry.is_unused() {
            return;
        }
        
        let p1_table = unsafe { &mut *(p2_entry.addr().as_usize() as *mut PageTable) };
        let p1_entry = p1_table.get_entry_mut(p1_index);
        
        p1_entry.set_unused();
    }
    
    pub fn translate(&self, virt: VirtualAddress) -> Option<PhysicalAddress> {
        let p4_index = virt.p4_index();
        let p3_index = virt.p3_index();
        let p2_index = virt.p2_index();
        let p1_index = virt.p1_index();
        let offset = virt.offset();
        
        let p4_entry = self.page_table.get_entry(p4_index);
        if p4_entry.is_unused() {
            return None;
        }
        
        let p3_table = unsafe { &*(p4_entry.addr().as_usize() as *const PageTable) };
        let p3_entry = p3_table.get_entry(p3_index);
        if p3_entry.is_unused() {
            return None;
        }
        
        let p2_table = unsafe { &*(p3_entry.addr().as_usize() as *const PageTable) };
        let p2_entry = p2_table.get_entry(p2_index);
        if p2_entry.is_unused() {
            return None;
        }
        
        let p1_table = unsafe { &*(p2_entry.addr().as_usize() as *const PageTable) };
        let p1_entry = p1_table.get_entry(p1_index);
        
        if p1_entry.is_unused() {
            return None;
        }
        
        Some(PhysicalAddress(p1_entry.addr().as_usize() + offset))
    }
}

static mut VMM_INSTANCE: Option<VirtualMemoryManager> = None;
static mut HHDM_OFFSET: u64 = 0;

pub fn init() {
}

pub fn set_hhdm_offset(offset: u64) {
    unsafe {
        HHDM_OFFSET = offset;
    }
}

pub fn get_hhdm_offset() -> u64 {
    unsafe { HHDM_OFFSET }
}

pub fn get_vmm() -> Option<&'static mut VirtualMemoryManager> {
    unsafe { VMM_INSTANCE.as_mut() }
}

pub fn phys_to_virt(phys: usize) -> usize {
    phys + (unsafe { HHDM_OFFSET } as usize)
}

pub fn virt_to_phys(virt: usize) -> usize {
    virt - (unsafe { HHDM_OFFSET } as usize)
}

pub fn map_vga_buffer() -> *mut u16 {
    let vga_phys = 0xB8000usize;
    vga_phys as *mut u16
}
