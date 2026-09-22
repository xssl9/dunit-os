use super::pmm::{get_pmm, PhysicalAddress};
use alloc::vec::Vec;
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

const PAGE_SIZE: usize = 4096;
const USER_SPACE_END: usize = 0x0000_8000_0000_0000;
const PML4_KERNEL_START: usize = 256;
const KERNEL_MMIO_BASE: usize = 0xFFFF_C000_0000_0000;
const KERNEL_MMIO_SIZE: usize = 0x0000_0080_0000_0000;
const UNINITIALIZED_ROOT: usize = usize::MAX;
const MAX_MMIO_RANGES: usize = 128;

#[derive(Clone, Copy)]
struct MmioRange {
    start: usize,
    length: usize,
}

impl MmioRange {
    const EMPTY: Self = Self {
        start: 0,
        length: 0,
    };
}

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
    KernelRootNotInitialized,
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

        if let Err(error) = unsafe { sync_kernel_half_into(root_frame.as_usize()) } {
            pmm.free_frame(root_frame);
            return Err(error);
        }

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

    pub fn user_page_mapping(
        &self,
        virt: VirtualAddress,
    ) -> Result<Option<(PhysicalAddress, PageFlags)>, AddressSpaceError> {
        user_page_mapping_from_root(self.root_frame, virt)
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
        if let Err(error) = self.map_user_frame(virt, frame, user_flags) {
            pmm.free_frame(frame);
            return Err(error);
        }
        self.user_frames.push(frame);
        Ok(frame)
    }

    /// Removes an owned userspace page and returns its physical frame to the
    /// PMM. Page-table pages are intentionally retained until the address
    /// space is destroyed so subsequent mappings can reuse the hierarchy.
    pub fn unmap_user_page(&mut self, virt: VirtualAddress) -> Result<bool, AddressSpaceError> {
        let virt_addr = virt.as_usize();
        if virt_addr == 0 || virt_addr >= USER_SPACE_END || (virt_addr & (PAGE_SIZE - 1)) != 0 {
            return Err(AddressSpaceError::InvalidUserAddress);
        }

        let mapping = self.user_page_mapping(virt)?;
        let Some((frame, _)) = mapping else {
            return Ok(false);
        };

        unsafe {
            let root = page_table_from_phys_mut(self.root_frame);
            let p4 = root.get_entry(virt.p4_index());
            let p3_table = page_table_from_phys_mut(p4.addr());
            let p3 = p3_table.get_entry(virt.p3_index());
            let p2_table = page_table_from_phys_mut(p3.addr());
            let p2 = p2_table.get_entry(virt.p2_index());
            let p1_table = page_table_from_phys_mut(p2.addr());
            p1_table.get_entry_mut(virt.p1_index()).set_unused();
            if read_cr3() == self.root_frame.as_usize() {
                flush_page(virt_addr);
            }
        }

        let frame_start = PhysicalAddress(frame.as_usize() & !(PAGE_SIZE - 1));
        if let Some(index) = self
            .user_frames
            .iter()
            .position(|owned| *owned == frame_start)
        {
            self.user_frames.swap_remove(index);
            if let Some(pmm) = get_pmm() {
                pmm.free_frame(frame_start);
            }
        }
        Ok(true)
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
            let entry = p1.get_entry_mut(virt.p1_index());
            if !entry.is_unused() {
                return Err(AddressSpaceError::InvalidUserAddress);
            }
            entry.set(phys, flags | PageFlags::PRESENT | PageFlags::USER);
        }

        Ok(())
    }

    /// Change access to an existing user page, including PROT_NONE (not present).
    pub fn protect_user_page(
        &mut self,
        virt: VirtualAddress,
        flags: PageFlags,
        present: bool,
    ) -> Result<bool, AddressSpaceError> {
        let addr = virt.as_usize();
        if addr == 0 || addr >= USER_SPACE_END || addr & (PAGE_SIZE - 1) != 0 {
            return Err(AddressSpaceError::InvalidUserAddress);
        }
        let Some((phys, _)) = self.user_page_mapping(virt)? else {
            return Ok(false);
        };
        unsafe {
            let root = page_table_from_phys_mut(self.root_frame);
            let p3 = page_table_from_phys_mut(root.get_entry(virt.p4_index()).addr());
            let p2 = page_table_from_phys_mut(p3.get_entry(virt.p3_index()).addr());
            let p1 = page_table_from_phys_mut(p2.get_entry(virt.p2_index()).addr());
            let mut new_flags = flags | PageFlags::USER;
            if present {
                new_flags |= PageFlags::PRESENT;
            }
            p1.get_entry_mut(virt.p1_index()).set(phys, new_flags);
            if read_cr3() == self.root_frame.as_usize() {
                flush_page(addr);
            }
        }
        Ok(true)
    }

    pub unsafe fn activate(&self) -> ActiveAddressSpace {
        let previous_cr3 = read_cr3();
        switch_to_root_frame(self.root_frame.as_usize());
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

fn user_page_mapping_from_root(
    root_frame: PhysicalAddress,
    virt: VirtualAddress,
) -> Result<Option<(PhysicalAddress, PageFlags)>, AddressSpaceError> {
    if virt.as_usize() >= USER_SPACE_END {
        return Err(AddressSpaceError::InvalidUserAddress);
    }

    unsafe {
        let root = page_table_from_phys_mut(root_frame);
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

/// Returns the flags for a user page in the currently active address space.
/// This is used by syscall copy helpers before touching an untrusted pointer.
pub fn active_user_page_flags(
    virt: VirtualAddress,
) -> Result<Option<PageFlags>, AddressSpaceError> {
    let root = unsafe { PhysicalAddress(read_cr3()) };
    user_page_mapping_from_root(root, virt).map(|mapping| mapping.map(|(_, flags)| flags))
}

/// Maps a frame into the user half of the currently active address space.
///
/// Process ELF loading should use [`AddressSpace`] so ownership remains
/// explicit. This entry point exists for the early ring-3 syscall smoke test,
/// which runs before a userspace process is scheduled.
#[cfg(feature = "boot-smoke-tests")]
pub unsafe fn map_active_user_frame(
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

    let root = page_table_from_phys_mut(PhysicalAddress(read_cr3()));
    let p3 = ensure_active_user_table(root.get_entry_mut(virt.p4_index()))?;
    let p2 = ensure_active_user_table(p3.get_entry_mut(virt.p3_index()))?;
    let p1 = ensure_active_user_table(p2.get_entry_mut(virt.p2_index()))?;
    p1.get_entry_mut(virt.p1_index())
        .set(phys, flags | PageFlags::PRESENT | PageFlags::USER);
    flush_page(virt_addr);
    Ok(())
}

/// Makes an existing active mapping reachable from ring 3, including every
/// parent table entry. Huge pages are valid leaves at the P3 or P2 level.
#[cfg(feature = "boot-smoke-tests")]
pub unsafe fn mark_active_mapping_user(virt: VirtualAddress) -> Result<(), AddressSpaceError> {
    let root = page_table_from_phys_mut(PhysicalAddress(read_cr3()));
    let p4 = root.get_entry_mut(virt.p4_index());
    mark_entry_user(p4)?;

    let p3_table = page_table_from_phys_mut(p4.addr());
    let p3 = p3_table.get_entry_mut(virt.p3_index());
    mark_entry_user(p3)?;
    if p3.flags().contains(PageFlags::HUGE) {
        flush_page(virt.as_usize());
        return Ok(());
    }

    let p2_table = page_table_from_phys_mut(p3.addr());
    let p2 = p2_table.get_entry_mut(virt.p2_index());
    mark_entry_user(p2)?;
    if p2.flags().contains(PageFlags::HUGE) {
        flush_page(virt.as_usize());
        return Ok(());
    }

    let p1_table = page_table_from_phys_mut(p2.addr());
    mark_entry_user(p1_table.get_entry_mut(virt.p1_index()))?;
    flush_page(virt.as_usize());
    Ok(())
}

#[cfg(feature = "boot-smoke-tests")]
unsafe fn ensure_active_user_table(
    entry: &mut PageTableEntry,
) -> Result<&'static mut PageTable, AddressSpaceError> {
    if !entry.is_unused() {
        if entry.flags().contains(PageFlags::HUGE) {
            return Err(AddressSpaceError::HugePageInPath);
        }
        entry.entry |= (PageFlags::USER | PageFlags::WRITABLE).bits();
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
    Ok(table)
}

#[cfg(feature = "boot-smoke-tests")]
fn mark_entry_user(entry: &mut PageTableEntry) -> Result<(), AddressSpaceError> {
    if entry.is_unused() {
        return Err(AddressSpaceError::InvalidUserAddress);
    }
    entry.entry |= PageFlags::USER.bits();
    Ok(())
}

unsafe fn flush_page(virt: usize) {
    core::arch::asm!("invlpg [{}]", in(reg) virt, options(nostack, preserves_flags));
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
            switch_to_root_frame(self.previous_cr3);
        }
    }
}

#[cfg(feature = "boot-smoke-tests")]
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

    // Emulate an address space created before a new kernel PML4 slot appears.
    // Activation must refresh the complete shared kernel half before CR3 changes.
    let kernel_root_frame = KERNEL_ROOT_FRAME.load(Ordering::Acquire);
    let kernel_root = unsafe { page_table_from_phys_mut(PhysicalAddress(kernel_root_frame)) };
    let kernel_slot =
        match (PML4_KERNEL_START..512).find(|idx| !kernel_root.get_entry(*idx).is_unused()) {
            Some(index) => index,
            None => {
                super::serial_write("[ADDRSPACE-TEST] no kernel mapping found\r\n");
                return false;
            }
        };
    let expected_kernel_entry = kernel_root.get_entry(kernel_slot).entry;
    unsafe {
        page_table_from_phys_mut(address_space.root_frame)
            .get_entry_mut(kernel_slot)
            .set_unused();
    }

    unsafe {
        let _active = address_space.activate();
        super::serial_write("[ADDRSPACE-TEST] switched\r\n");

        let active_root = page_table_from_phys_mut(address_space.root_frame);
        if active_root.get_entry(kernel_slot).entry != expected_kernel_entry {
            super::serial_write("[ADDRSPACE-TEST] kernel half refresh failed\r\n");
            return false;
        }
        super::serial_write("[ADDRSPACE-TEST] kernel half refreshed\r\n");

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

    let Some(first_mmio) = map_mmio_region(0xFEE0_0000, PAGE_SIZE) else {
        super::serial_write("[MMIO-ALLOC-TEST] first map failed\r\n");
        return false;
    };
    if !unmap_mmio_region(first_mmio, PAGE_SIZE) {
        super::serial_write("[MMIO-ALLOC-TEST] unmap failed\r\n");
        return false;
    }
    let Some(second_mmio) = map_mmio_region(0xFEE0_0000, PAGE_SIZE) else {
        super::serial_write("[MMIO-ALLOC-TEST] remap failed\r\n");
        return false;
    };
    if second_mmio != first_mmio || !unmap_mmio_region(second_mmio, PAGE_SIZE) {
        super::serial_write("[MMIO-ALLOC-TEST] range was not reused\r\n");
        return false;
    }
    super::serial_write("[MMIO-ALLOC-TEST] OK\r\n");

    super::serial_write("[ADDRSPACE-TEST] OK\r\n");
    true
}

unsafe fn sync_kernel_half_into(root_frame: usize) -> Result<(), AddressSpaceError> {
    let kernel_root_frame = KERNEL_ROOT_FRAME.load(Ordering::Acquire);
    if kernel_root_frame == UNINITIALIZED_ROOT {
        return Err(AddressSpaceError::KernelRootNotInitialized);
    }
    if root_frame == kernel_root_frame {
        return Ok(());
    }

    let kernel_root = &*(phys_to_virt(kernel_root_frame) as *const PageTable);
    let root = page_table_from_phys_mut(PhysicalAddress(root_frame));
    for idx in PML4_KERNEL_START..512 {
        root.entries[idx] = kernel_root.entries[idx];
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
    if sync_kernel_half_into(root_frame).is_err() {
        panic!("VMM canonical kernel root is unavailable during address-space switch");
    }
    write_cr3(root_frame);
}
/// Смещение HHDM (higher-half direct map) задаётся один раз при загрузке и
/// затем только читается на горячих путях (`phys_to_virt`). Атомик убирает
/// `static mut` без накладных расходов относительно прежнего сырого доступа.
static HHDM_OFFSET: AtomicU64 = AtomicU64::new(0);
static KERNEL_ROOT_FRAME: AtomicUsize = AtomicUsize::new(UNINITIALIZED_ROOT);
static MMIO_LOCK: AtomicBool = AtomicBool::new(false);

/// MMIO virtual-range bookkeeping. Previously four `static mut` arrays/counters;
/// now bundled into one struct behind an `UnsafeCell`. Every access happens only
/// while `MMIO_LOCK` is held (see `lock_mmio`/`unlock_mmio`), so the manual
/// spinlock already serialises all readers and writers — the cell just removes
/// the `static mut` aliasing UB. `init` touches it once at boot before any lock
/// contention exists.
struct MmioState {
    free_ranges: [MmioRange; MAX_MMIO_RANGES],
    free_count: usize,
    allocations: [MmioRange; MAX_MMIO_RANGES],
    allocation_count: usize,
}

struct MmioStateCell(UnsafeCell<MmioState>);
unsafe impl Sync for MmioStateCell {}

static MMIO_STATE: MmioStateCell = MmioStateCell(UnsafeCell::new(MmioState {
    free_ranges: [MmioRange::EMPTY; MAX_MMIO_RANGES],
    free_count: 0,
    allocations: [MmioRange::EMPTY; MAX_MMIO_RANGES],
    allocation_count: 0,
}));

pub fn init() {
    super::serial_write("[VMM] START\r\n");
    let active_root = unsafe { read_cr3() };
    if KERNEL_ROOT_FRAME
        .compare_exchange(
            UNINITIALIZED_ROOT,
            active_root,
            Ordering::AcqRel,
            Ordering::Acquire,
        )
        .is_err()
    {
        super::serial_write("[VMM] canonical kernel root already initialized\r\n");
    }
    unsafe {
        let state = &mut *MMIO_STATE.0.get();
        state.free_ranges[0] = MmioRange {
            start: KERNEL_MMIO_BASE,
            length: KERNEL_MMIO_SIZE,
        };
        state.free_count = 1;
        state.allocation_count = 0;
    }
    super::serial_write("[VMM] OK\r\n");
}

pub fn set_hhdm_offset(offset: u64) {
    HHDM_OFFSET.store(offset, Ordering::Relaxed);
}

pub fn get_hhdm_offset() -> u64 {
    HHDM_OFFSET.load(Ordering::Relaxed)
}

pub fn phys_to_virt(phys: usize) -> usize {
    phys + (HHDM_OFFSET.load(Ordering::Relaxed) as usize)
}

pub fn virt_to_phys(virt: usize) -> usize {
    virt - (HHDM_OFFSET.load(Ordering::Relaxed) as usize)
}

pub fn map_mmio_region(phys: usize, length: usize) -> Option<usize> {
    if length == 0 {
        return None;
    }

    let phys_start = phys & !(PAGE_SIZE - 1);
    let phys_offset = phys.saturating_sub(phys_start);
    let map_length = align_up(phys_offset.checked_add(length)?, PAGE_SIZE)?;

    let root_frame = KERNEL_ROOT_FRAME.load(Ordering::Acquire);
    if root_frame == UNINITIALIZED_ROOT {
        return None;
    }

    lock_mmio();
    let virt_start = unsafe { allocate_mmio_range(map_length) };
    let Some(virt_start) = virt_start else {
        unlock_mmio();
        return None;
    };
    let mut offset = 0usize;
    while offset < map_length {
        let mapped = unsafe {
            map_kernel_page(
                root_frame,
                virt_start + offset,
                phys_start + offset,
                PageFlags::WRITABLE
                    | PageFlags::NO_CACHE
                    | PageFlags::WRITE_THROUGH
                    | PageFlags::NO_EXECUTE,
            )
        };
        if mapped.is_none() {
            let mut rollback = 0usize;
            while rollback < offset {
                unsafe { unmap_kernel_page(root_frame, virt_start + rollback) };
                rollback += PAGE_SIZE;
            }
            unsafe { release_mmio_range(virt_start, map_length) };
            unlock_mmio();
            return None;
        }
        offset += PAGE_SIZE;
    }

    unsafe {
        let active_root = active_root_frame();
        if sync_kernel_half_into(active_root).is_err() {
            let mut rollback = 0usize;
            while rollback < map_length {
                unmap_kernel_page(root_frame, virt_start + rollback);
                rollback += PAGE_SIZE;
            }
            release_mmio_range(virt_start, map_length);
            unlock_mmio();
            return None;
        }
        // Reloading CR3 invalidates stale translations after installing a new
        // shared kernel hierarchy in the currently active address space.
        write_cr3(active_root);
    }

    unlock_mmio();
    Some(virt_start + phys_offset)
}

/// Unmaps an MMIO allocation previously returned by [`map_mmio_region`] and
/// returns its virtual extent to the coalescing allocator.
pub fn unmap_mmio_region(virt: usize, length: usize) -> bool {
    if length == 0 {
        return false;
    }
    let virt_start = virt & !(PAGE_SIZE - 1);
    let virt_offset = virt - virt_start;
    let Some(map_length) = virt_offset
        .checked_add(length)
        .and_then(|value| align_up(value, PAGE_SIZE))
    else {
        return false;
    };

    lock_mmio();
    let allocation_exists = unsafe {
        let state = &*MMIO_STATE.0.get();
        (0..state.allocation_count).any(|index| {
            state.allocations[index].start == virt_start
                && state.allocations[index].length == map_length
        })
    };
    if !allocation_exists {
        unlock_mmio();
        return false;
    }

    let root_frame = KERNEL_ROOT_FRAME.load(Ordering::Acquire);
    let mut offset = 0usize;
    while offset < map_length {
        unsafe { unmap_kernel_page(root_frame, virt_start + offset) };
        offset += PAGE_SIZE;
    }
    unsafe { release_mmio_range(virt_start, map_length) };

    unsafe {
        let active_root = active_root_frame();
        let _ = sync_kernel_half_into(active_root);
        write_cr3(active_root);
    }
    unlock_mmio();
    true
}

fn lock_mmio() {
    while MMIO_LOCK
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        core::hint::spin_loop();
    }
}

fn unlock_mmio() {
    MMIO_LOCK.store(false, Ordering::Release);
}

unsafe fn allocate_mmio_range(length: usize) -> Option<usize> {
    let state = &mut *MMIO_STATE.0.get();
    if state.allocation_count >= MAX_MMIO_RANGES {
        return None;
    }
    let index = (0..state.free_count).find(|index| state.free_ranges[*index].length >= length)?;
    let start = state.free_ranges[index].start;
    state.free_ranges[index].start += length;
    state.free_ranges[index].length -= length;
    if state.free_ranges[index].length == 0 {
        let mut cursor = index;
        while cursor + 1 < state.free_count {
            state.free_ranges[cursor] = state.free_ranges[cursor + 1];
            cursor += 1;
        }
        state.free_count -= 1;
        state.free_ranges[state.free_count] = MmioRange::EMPTY;
    }
    state.allocations[state.allocation_count] = MmioRange { start, length };
    state.allocation_count += 1;
    Some(start)
}

unsafe fn release_mmio_range(start: usize, length: usize) {
    let state = &mut *MMIO_STATE.0.get();
    let Some(allocation_index) = (0..state.allocation_count).find(|index| {
        state.allocations[*index].start == start && state.allocations[*index].length == length
    }) else {
        return;
    };
    let mut cursor = allocation_index;
    while cursor + 1 < state.allocation_count {
        state.allocations[cursor] = state.allocations[cursor + 1];
        cursor += 1;
    }
    state.allocation_count -= 1;
    state.allocations[state.allocation_count] = MmioRange::EMPTY;

    if state.free_count >= MAX_MMIO_RANGES {
        return;
    }
    let insert_at = (0..state.free_count)
        .find(|index| state.free_ranges[*index].start > start)
        .unwrap_or(state.free_count);
    let mut move_index = state.free_count;
    while move_index > insert_at {
        state.free_ranges[move_index] = state.free_ranges[move_index - 1];
        move_index -= 1;
    }
    state.free_ranges[insert_at] = MmioRange { start, length };
    state.free_count += 1;

    let mut index = 0usize;
    while index + 1 < state.free_count {
        let current_end = state.free_ranges[index].start + state.free_ranges[index].length;
        if current_end == state.free_ranges[index + 1].start {
            state.free_ranges[index].length += state.free_ranges[index + 1].length;
            let mut shift = index + 1;
            while shift + 1 < state.free_count {
                state.free_ranges[shift] = state.free_ranges[shift + 1];
                shift += 1;
            }
            state.free_count -= 1;
            state.free_ranges[state.free_count] = MmioRange::EMPTY;
        } else {
            index += 1;
        }
    }
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

    Some(())
}

unsafe fn unmap_kernel_page(root_frame: usize, virt: usize) {
    let root = page_table_from_phys_mut(PhysicalAddress(root_frame));
    let p4 = root.get_entry((virt >> 39) & 0x1FF);
    if p4.is_unused() || p4.flags().contains(PageFlags::HUGE) {
        return;
    }
    let p3_table = page_table_from_phys_mut(p4.addr());
    let p3 = p3_table.get_entry((virt >> 30) & 0x1FF);
    if p3.is_unused() || p3.flags().contains(PageFlags::HUGE) {
        return;
    }
    let p2_table = page_table_from_phys_mut(p3.addr());
    let p2 = p2_table.get_entry((virt >> 21) & 0x1FF);
    if p2.is_unused() || p2.flags().contains(PageFlags::HUGE) {
        return;
    }
    let p1_table = page_table_from_phys_mut(p2.addr());
    p1_table.get_entry_mut((virt >> 12) & 0x1FF).set_unused();
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
