use super::pmm::{get_pmm, PhysicalAddress};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};

const PAGE_SIZE: usize = 4096;
const USER_SPACE_END: usize = 0x0000_8000_0000_0000;
const PML4_KERNEL_START: usize = 256;
const KERNEL_MMIO_BASE: usize = 0xFFFF_C000_0000_0000;
const KERNEL_MMIO_SIZE: usize = 0x0000_0080_0000_0000;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddressSpaceError {
    NoPhysicalMemoryManager,
    OutOfMemory,
    InvalidUserAddress,
    HugePageInPath,
}

pub struct AddressSpace {
    root_frame: PhysicalAddress,
    user_frames: Vec<PhysicalAddress>,
    page_table_frames: Vec<PhysicalAddress>,
}

impl AddressSpace {
    pub fn new() -> Result<Self, AddressSpaceError> {
        let pmm = get_pmm().ok_or(AddressSpaceError::NoPhysicalMemoryManager)?;
        let root_frame = pmm.alloc_frame().ok_or(AddressSpaceError::OutOfMemory)?;
        let root_table = unsafe { page_table_from_phys_mut(root_frame) };
        root_table.zero();

        copy_kernel_half_from_active(root_table)?;

        let mut page_table_frames = Vec::new();
        page_table_frames.push(root_frame);

        Ok(Self {
            root_frame,
            user_frames: Vec::new(),
            page_table_frames,
        })
    }

    pub fn root_frame(&self) -> PhysicalAddress {
        self.root_frame
    }

    pub fn user_frame_count(&self) -> usize {
        self.user_frames.len()
    }

    pub fn translate_user_page(
        &self,
        virt: VirtualAddress,
    ) -> Result<Option<PhysicalAddress>, AddressSpaceError> {
        self.user_page_mapping(virt)
            .map(|mapping| mapping.map(|(phys, _)| phys))
    }

    pub fn user_page_flags(
        &self,
        virt: VirtualAddress,
    ) -> Result<Option<PageFlags>, AddressSpaceError> {
        self.user_page_mapping(virt)
            .map(|mapping| mapping.map(|(_, flags)| flags))
    }

    fn user_page_mapping(
        &self,
        virt: VirtualAddress,
    ) -> Result<Option<(PhysicalAddress, PageFlags)>, AddressSpaceError> {
        let virt_addr = virt.as_usize();
        if virt_addr >= USER_SPACE_END {
            return Err(AddressSpaceError::InvalidUserAddress);
        }

        unsafe {
            let root = page_table_from_phys_mut(self.root_frame);
            let p4 = root.get_entry(virt.p4_index());
            if p4.is_unused() {
                return Ok(None);
            }
            if p4.flags().contains(PageFlags::HUGE) {
                return Err(AddressSpaceError::HugePageInPath);
            }

            let p3_table = page_table_from_phys_mut(p4.addr());
            let p3 = p3_table.get_entry(virt.p3_index());
            if p3.is_unused() {
                return Ok(None);
            }
            if p3.flags().contains(PageFlags::HUGE) {
                return Err(AddressSpaceError::HugePageInPath);
            }

            let p2_table = page_table_from_phys_mut(p3.addr());
            let p2 = p2_table.get_entry(virt.p2_index());
            if p2.is_unused() {
                return Ok(None);
            }
            if p2.flags().contains(PageFlags::HUGE) {
                return Err(AddressSpaceError::HugePageInPath);
            }

            let p1_table = page_table_from_phys_mut(p2.addr());
            let p1 = p1_table.get_entry(virt.p1_index());
            if p1.is_unused() {
                return Ok(None);
            }

            Ok(Some((
                PhysicalAddress(p1.addr().as_usize() + virt.offset()),
                p1.flags(),
            )))
        }
    }

    pub fn map_user_page(
        &mut self,
        virt: VirtualAddress,
        flags: PageFlags,
    ) -> Result<PhysicalAddress, AddressSpaceError> {
        let virt_addr = virt.as_usize();
        if virt_addr == 0 || virt_addr >= USER_SPACE_END || (virt_addr & (PAGE_SIZE - 1)) != 0 {
            return Err(AddressSpaceError::InvalidUserAddress);
        }

        let pmm = get_pmm().ok_or(AddressSpaceError::NoPhysicalMemoryManager)?;
        let frame = pmm.alloc_frame().ok_or(AddressSpaceError::OutOfMemory)?;
        unsafe {
            core::ptr::write_bytes(phys_to_virt(frame.as_usize()) as *mut u8, 0, PAGE_SIZE);
        }

        let user_flags = flags | PageFlags::PRESENT | PageFlags::USER;
        self.map_user_frame(virt, frame, user_flags)?;
        self.user_frames.push(frame);
        Ok(frame)
    }

    pub fn map_user_frame(
        &mut self,
        virt: VirtualAddress,
        phys: PhysicalAddress,
        flags: PageFlags,
    ) -> Result<(), AddressSpaceError> {
        let virt_addr = virt.as_usize();
        if virt_addr == 0
            || virt_addr >= USER_SPACE_END
            || (virt_addr & (PAGE_SIZE - 1)) != 0
            || (phys.as_usize() & (PAGE_SIZE - 1)) != 0
        {
            return Err(AddressSpaceError::InvalidUserAddress);
        }

        unsafe {
            let root = page_table_from_phys_mut(self.root_frame);
            let p3 = self.ensure_next_table(root.get_entry_mut(virt.p4_index()))?;
            let p2 = self.ensure_next_table(p3.get_entry_mut(virt.p3_index()))?;
            let p1 = self.ensure_next_table(p2.get_entry_mut(virt.p2_index()))?;
            p1.get_entry_mut(virt.p1_index())
                .set(phys, flags | PageFlags::PRESENT | PageFlags::USER);
        }

        Ok(())
    }

    pub unsafe fn activate(&self) -> ActiveAddressSpace {
        let previous_cr3 = read_cr3();
        write_cr3(self.root_frame.as_usize());
        ActiveAddressSpace { previous_cr3 }
    }

    unsafe fn ensure_next_table(
        &mut self,
        entry: &mut PageTableEntry,
    ) -> Result<&'static mut PageTable, AddressSpaceError> {
        if !entry.is_unused() {
            if entry.flags().contains(PageFlags::HUGE) {
                return Err(AddressSpaceError::HugePageInPath);
            }
            if !entry.flags().contains(PageFlags::USER) {
                entry.entry |= PageFlags::USER.bits();
            }
            return Ok(page_table_from_phys_mut(entry.addr()));
        }

        let pmm = get_pmm().ok_or(AddressSpaceError::NoPhysicalMemoryManager)?;
        let frame = pmm.alloc_frame().ok_or(AddressSpaceError::OutOfMemory)?;
        let table = page_table_from_phys_mut(frame);
        table.zero();
        entry.set(
            frame,
            PageFlags::PRESENT | PageFlags::WRITABLE | PageFlags::USER,
        );
        self.page_table_frames.push(frame);
        Ok(table)
    }
}

impl Drop for AddressSpace {
    fn drop(&mut self) {
        if let Some(pmm) = get_pmm() {
            for frame in self.user_frames.iter().copied() {
                pmm.free_frame(frame);
            }
            for frame in self.page_table_frames.iter().copied() {
                pmm.free_frame(frame);
            }
        }
    }
}

pub struct ActiveAddressSpace {
    previous_cr3: usize,
}

impl Drop for ActiveAddressSpace {
    fn drop(&mut self) {
        unsafe {
            write_cr3(self.previous_cr3);
        }
    }
}

pub fn run_address_space_smoke() -> bool {
    super::serial_write("[ADDRSPACE-TEST] START\r\n");

    let previous_cr3 = unsafe { read_cr3() };
    let mut address_space = match AddressSpace::new() {
        Ok(address_space) => address_space,
        Err(_) => {
            super::serial_write("[ADDRSPACE-TEST] create failed\r\n");
            return false;
        }
    };

    let user_page = VirtualAddress::from_usize(0x0040_0000);
    if address_space
        .map_user_page(user_page, PageFlags::WRITABLE)
        .is_err()
    {
        super::serial_write("[ADDRSPACE-TEST] map failed\r\n");
        return false;
    }

    unsafe {
        let _active = address_space.activate();
        super::serial_write("[ADDRSPACE-TEST] switched\r\n");

        let ptr = user_page.as_usize() as *mut u64;
        core::ptr::write_volatile(ptr, 0x4455_4E49_544F_5341);
        if core::ptr::read_volatile(ptr) != 0x4455_4E49_544F_5341 {
            super::serial_write("[ADDRSPACE-TEST] user page check failed\r\n");
            return false;
        }
    }

    if unsafe { read_cr3() } != previous_cr3 {
        super::serial_write("[ADDRSPACE-TEST] restore failed\r\n");
        return false;
    }

    super::serial_write("[ADDRSPACE-TEST] OK\r\n");
    true
}

fn copy_kernel_half_from_active(new_root: &mut PageTable) -> Result<(), AddressSpaceError> {
    let active_root = unsafe { page_table_from_phys_mut(PhysicalAddress(read_cr3())) };
    for idx in PML4_KERNEL_START..512 {
        new_root.entries[idx] = active_root.entries[idx];
    }
    Ok(())
}

unsafe fn page_table_from_phys_mut(phys: PhysicalAddress) -> &'static mut PageTable {
    &mut *(phys_to_virt(phys.as_usize()) as *mut PageTable)
}

unsafe fn read_cr3() -> usize {
    let cr3: usize;
    core::arch::asm!("mov {}, cr3", out(reg) cr3, options(nomem, nostack, preserves_flags));
    cr3 & !(PAGE_SIZE - 1)
}

unsafe fn write_cr3(cr3: usize) {
    core::arch::asm!("mov cr3, {}", in(reg) cr3, options(nostack, preserves_flags));
}

pub unsafe fn active_root_frame() -> usize {
    read_cr3()
}

pub unsafe fn switch_to_root_frame(root_frame: usize) {
    write_cr3(root_frame);
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
static NEXT_MMIO_VIRT: AtomicUsize = AtomicUsize::new(KERNEL_MMIO_BASE);

pub fn init() {
    super::serial_write("[VMM] START\r\n");
    super::serial_write("[VMM] OK\r\n");
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

pub fn map_mmio_region(phys: usize, length: usize) -> Option<usize> {
    if length == 0 {
        return None;
    }

    let phys_start = phys & !(PAGE_SIZE - 1);
    let phys_offset = phys.saturating_sub(phys_start);
    let map_length = align_up(phys_offset.checked_add(length)?, PAGE_SIZE)?;

    let virt_start = NEXT_MMIO_VIRT.fetch_add(map_length, Ordering::SeqCst);
    if virt_start.checked_add(map_length)? > KERNEL_MMIO_BASE + KERNEL_MMIO_SIZE {
        return None;
    }

    let root_frame = unsafe { active_root_frame() };
    let mut offset = 0usize;
    while offset < map_length {
        unsafe {
            map_kernel_page(
                root_frame,
                virt_start + offset,
                phys_start + offset,
                PageFlags::WRITABLE
                    | PageFlags::NO_CACHE
                    | PageFlags::WRITE_THROUGH
                    | PageFlags::NO_EXECUTE,
            )?;
        }
        offset += PAGE_SIZE;
    }

    Some(virt_start + phys_offset)
}

unsafe fn map_kernel_page(
    root_frame: usize,
    virt: usize,
    phys: usize,
    flags: PageFlags,
) -> Option<()> {
    if (virt & (PAGE_SIZE - 1)) != 0 || (phys & (PAGE_SIZE - 1)) != 0 {
        return None;
    }

    let root = page_table_from_phys_mut(PhysicalAddress(root_frame));
    let p4 = (virt >> 39) & 0x1FF;
    let p3 = (virt >> 30) & 0x1FF;
    let p2 = (virt >> 21) & 0x1FF;
    let p1 = (virt >> 12) & 0x1FF;

    let p3_table = ensure_kernel_table(root.get_entry_mut(p4))?;
    let p2_table = ensure_kernel_table(p3_table.get_entry_mut(p3))?;
    let p1_table = ensure_kernel_table(p2_table.get_entry_mut(p2))?;
    p1_table
        .get_entry_mut(p1)
        .set(PhysicalAddress(phys), flags | PageFlags::PRESENT);

    core::arch::asm!("invlpg [{}]", in(reg) virt, options(nostack, preserves_flags));
    Some(())
}

unsafe fn ensure_kernel_table(entry: &mut PageTableEntry) -> Option<&'static mut PageTable> {
    if !entry.is_unused() {
        if entry.flags().contains(PageFlags::HUGE) {
            return None;
        }
        return Some(page_table_from_phys_mut(entry.addr()));
    }

    let pmm = get_pmm()?;
    let frame = pmm.alloc_frame()?;
    let table = page_table_from_phys_mut(frame);
    table.zero();
    entry.set(frame, PageFlags::PRESENT | PageFlags::WRITABLE);
    Some(table)
}

fn align_up(value: usize, align: usize) -> Option<usize> {
    if align == 0 || !align.is_power_of_two() {
        return None;
    }
    value.checked_add(align - 1).map(|v| v & !(align - 1))
}

pub fn map_vga_buffer() -> *mut u16 {
    let vga_phys = 0xB8000usize;
    vga_phys as *mut u16
}
