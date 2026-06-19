use proptest::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PhysicalAddress(usize);

impl PhysicalAddress {
    fn as_usize(&self) -> usize {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct VirtualAddress(usize);

impl VirtualAddress {
    fn as_usize(&self) -> usize {
        self.0
    }

    fn from_usize(addr: usize) -> Self {
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
    struct PageFlags: u64 {
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
struct PageTable {
    entries: [PageTableEntry; 512],
}

impl PageTable {
    fn new() -> Self {
        Self {
            entries: [PageTableEntry::new(); 512],
        }
    }

    fn zero(&mut self) {
        for entry in self.entries.iter_mut() {
            entry.set_unused();
        }
    }

    fn get_entry(&self, index: usize) -> &PageTableEntry {
        &self.entries[index]
    }

    fn get_entry_mut(&mut self, index: usize) -> &mut PageTableEntry {
        &mut self.entries[index]
    }
}

#[derive(Clone, Copy)]
#[repr(transparent)]
struct PageTableEntry {
    entry: u64,
}

impl PageTableEntry {
    const fn new() -> Self {
        Self { entry: 0 }
    }

    fn is_unused(&self) -> bool {
        self.entry == 0
    }

    fn set_unused(&mut self) {
        self.entry = 0;
    }

    fn flags(&self) -> PageFlags {
        PageFlags::from_bits_truncate(self.entry)
    }

    fn addr(&self) -> PhysicalAddress {
        PhysicalAddress((self.entry & 0x000F_FFFF_FFFF_F000) as usize)
    }

    fn set(&mut self, addr: PhysicalAddress, flags: PageFlags) {
        self.entry = (addr.as_usize() as u64) | flags.bits();
    }
}

struct VirtualMemoryManager {
    page_table: Box<PageTable>,
    p3_tables: Vec<Box<PageTable>>,
    p2_tables: Vec<Box<PageTable>>,
    p1_tables: Vec<Box<PageTable>>,
}

impl VirtualMemoryManager {
    fn new() -> Self {
        let mut page_table = Box::new(PageTable::new());
        page_table.zero();

        Self {
            page_table,
            p3_tables: Vec::new(),
            p2_tables: Vec::new(),
            p1_tables: Vec::new(),
        }
    }

    fn ensure_table_hierarchy(&mut self, virt: VirtualAddress) {
        let p4_index = virt.p4_index();
        let p3_index = virt.p3_index();
        let p2_index = virt.p2_index();

        let p4_entry = self.page_table.get_entry_mut(p4_index);
        if p4_entry.is_unused() {
            let mut p3_table = Box::new(PageTable::new());
            p3_table.zero();
            let p3_addr = &*p3_table as *const PageTable as usize;
            p4_entry.set(
                PhysicalAddress(p3_addr),
                PageFlags::PRESENT | PageFlags::WRITABLE,
            );
            self.p3_tables.push(p3_table);
        }

        let p3_table = unsafe { &mut *(p4_entry.addr().as_usize() as *mut PageTable) };
        let p3_entry = p3_table.get_entry_mut(p3_index);
        if p3_entry.is_unused() {
            let mut p2_table = Box::new(PageTable::new());
            p2_table.zero();
            let p2_addr = &*p2_table as *const PageTable as usize;
            p3_entry.set(
                PhysicalAddress(p2_addr),
                PageFlags::PRESENT | PageFlags::WRITABLE,
            );
            self.p2_tables.push(p2_table);
        }

        let p2_table = unsafe { &mut *(p3_entry.addr().as_usize() as *mut PageTable) };
        let p2_entry = p2_table.get_entry_mut(p2_index);
        if p2_entry.is_unused() {
            let mut p1_table = Box::new(PageTable::new());
            p1_table.zero();
            let p1_addr = &*p1_table as *const PageTable as usize;
            p2_entry.set(
                PhysicalAddress(p1_addr),
                PageFlags::PRESENT | PageFlags::WRITABLE,
            );
            self.p1_tables.push(p1_table);
        }
    }

    fn map_page(&mut self, virt: VirtualAddress, phys: PhysicalAddress, flags: PageFlags) {
        self.ensure_table_hierarchy(virt);

        let p4_index = virt.p4_index();
        let p3_index = virt.p3_index();
        let p2_index = virt.p2_index();
        let p1_index = virt.p1_index();

        let p4_entry = self.page_table.get_entry_mut(p4_index);
        let p3_table = unsafe { &mut *(p4_entry.addr().as_usize() as *mut PageTable) };
        let p3_entry = p3_table.get_entry_mut(p3_index);
        let p2_table = unsafe { &mut *(p3_entry.addr().as_usize() as *mut PageTable) };
        let p2_entry = p2_table.get_entry_mut(p2_index);
        let p1_table = unsafe { &mut *(p2_entry.addr().as_usize() as *mut PageTable) };
        let p1_entry = p1_table.get_entry_mut(p1_index);

        p1_entry.set(phys, flags | PageFlags::PRESENT);
    }

    fn translate(&self, virt: VirtualAddress) -> Option<PhysicalAddress> {
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

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn prop_virtual_memory_mapping_consistency(
        virt_addr in 0x1000usize..0x7FFF_FFFF_F000usize,
        phys_addr in 0x1000usize..0xFFFF_FFFF_F000usize
    ) {
        let virt_addr = (virt_addr / 4096) * 4096;
        let phys_addr = (phys_addr / 4096) * 4096;

        let mut vmm = VirtualMemoryManager::new();
        let virt = VirtualAddress::from_usize(virt_addr);
        let phys = PhysicalAddress(phys_addr);

        vmm.map_page(virt, phys, PageFlags::WRITABLE);

        if let Some(translated) = vmm.translate(virt) {
            assert_eq!(translated.as_usize(), phys_addr);
        }
    }

    #[test]
    fn prop_unmapped_pages_return_none(virt_addr in 0x1000usize..0x7FFF_FFFF_F000usize) {
        let virt_addr = (virt_addr / 4096) * 4096;
        let vmm = VirtualMemoryManager::new();
        let virt = VirtualAddress::from_usize(virt_addr);

        assert_eq!(vmm.translate(virt), None);
    }
}
