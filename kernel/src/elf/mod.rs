#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ElfHeader {
    pub magic: [u8; 4],
    pub class: u8,
    pub data: u8,
    pub version: u8,
    pub os_abi: u8,
    pub abi_version: u8,
    pub padding: [u8; 7],
    pub elf_type: u16,
    pub machine: u16,
    pub version2: u32,
    pub entry: u64,
    pub phoff: u64,
    pub shoff: u64,
    pub flags: u32,
    pub ehsize: u16,
    pub phentsize: u16,
    pub phnum: u16,
    pub shentsize: u16,
    pub shnum: u16,
    pub shstrndx: u16,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ProgramHeader {
    pub p_type: u32,
    pub p_flags: u32,
    pub p_offset: u64,
    pub p_vaddr: u64,
    pub p_paddr: u64,
    pub p_filesz: u64,
    pub p_memsz: u64,
    pub p_align: u64,
}

pub const ELF_MAGIC: [u8; 4] = [0x7F, b'E', b'L', b'F'];
pub const ELF_CLASS_64: u8 = 2;
pub const ELF_DATA_LSB: u8 = 1;
pub const ELF_MACHINE_X86_64: u16 = 0x3E;
pub const ELF_TYPE_EXEC: u16 = 2;
pub const PT_LOAD: u32 = 1;
pub const PF_X: u32 = 0x1;
pub const PF_W: u32 = 0x2;
pub const PF_R: u32 = 0x4;

#[derive(Debug)]
pub enum ElfError {
    InvalidMagic,
    InvalidClass,
    InvalidArchitecture,
    InvalidType,
    InvalidProgramHeader,
    TooManyProgramHeaders,
}

pub struct ElfParser<'a> {
    data: &'a [u8],
    header: ElfHeader,
}

impl<'a> ElfParser<'a> {
    pub fn new(data: &'a [u8]) -> Result<Self, ElfError> {
        if data.len() < core::mem::size_of::<ElfHeader>() {
            return Err(ElfError::InvalidMagic);
        }

        let header = unsafe { core::ptr::read(data.as_ptr() as *const ElfHeader) };

        if header.magic != ELF_MAGIC {
            return Err(ElfError::InvalidMagic);
        }

        if header.class != ELF_CLASS_64 {
            return Err(ElfError::InvalidClass);
        }

        if header.machine != ELF_MACHINE_X86_64 {
            return Err(ElfError::InvalidArchitecture);
        }

        if header.elf_type != ELF_TYPE_EXEC {
            return Err(ElfError::InvalidType);
        }

        Ok(Self { data, header })
    }

    pub fn header(&self) -> &ElfHeader {
        &self.header
    }

    pub fn entry_point(&self) -> u64 {
        self.header.entry
    }

    pub fn program_headers(&self) -> Result<ProgramHeaderIterator<'a>, ElfError> {
        let phoff = self.header.phoff as usize;
        let phnum = self.header.phnum as usize;
        let phentsize = self.header.phentsize as usize;

        if phoff + (phnum * phentsize) > self.data.len() {
            return Err(ElfError::InvalidProgramHeader);
        }

        if phnum > 256 {
            return Err(ElfError::TooManyProgramHeaders);
        }

        Ok(ProgramHeaderIterator {
            data: self.data,
            offset: phoff,
            count: phnum,
            index: 0,
            entsize: phentsize,
        })
    }
}

pub struct ProgramHeaderIterator<'a> {
    data: &'a [u8],
    offset: usize,
    count: usize,
    index: usize,
    entsize: usize,
}

impl<'a> Iterator for ProgramHeaderIterator<'a> {
    type Item = ProgramHeader;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.count {
            return None;
        }

        let ph_offset = self.offset + (self.index * self.entsize);
        if ph_offset + core::mem::size_of::<ProgramHeader>() > self.data.len() {
            return None;
        }

        let ph = unsafe {
            core::ptr::read(self.data.as_ptr().add(ph_offset) as *const ProgramHeader)
        };

        self.index += 1;
        Some(ph)
    }
}

use crate::memory::pmm::PhysicalMemoryManager;
use crate::memory::pmm::PhysicalAddress;
use crate::memory::vmm::{AddressSpace, VirtualAddress, VirtualMemoryManager, PageFlags};
use crate::process::{Process, ProcessExit, ProcessId};

pub const USER_STACK_SIZE: usize = 0x10000;
pub const USER_STACK_TOP: usize = 0x00007FFF_FFFFF000;

pub struct ElfLoader<'a> {
    parser: ElfParser<'a>,
}

impl<'a> ElfLoader<'a> {
    pub fn new(data: &'a [u8]) -> Result<Self, ElfError> {
        let parser = ElfParser::new(data)?;
        Ok(Self { parser })
    }

    pub fn load(
        &self,
        vmm: &mut VirtualMemoryManager,
        pmm: &mut PhysicalMemoryManager,
    ) -> Result<u64, ElfError> {
        let program_headers = self.parser.program_headers()?;

        for ph in program_headers {
            if ph.p_type != PT_LOAD {
                continue;
            }

            self.load_segment(&ph, vmm, pmm)?;
        }

        let stack_pages = USER_STACK_SIZE / 4096;
        for i in 0..stack_pages {
            let virt_addr = VirtualAddress::from_usize(USER_STACK_TOP - (i * 4096));
            if let Some(phys_addr) = pmm.alloc_frame() {
                let flags = PageFlags::PRESENT | PageFlags::WRITABLE | PageFlags::USER;
                vmm.map_page(virt_addr, phys_addr, flags);
            } else {
                return Err(ElfError::InvalidProgramHeader);
            }
        }

        Ok(self.parser.entry_point())
    }

    fn load_segment(
        &self,
        ph: &ProgramHeader,
        vmm: &mut VirtualMemoryManager,
        pmm: &mut PhysicalMemoryManager,
    ) -> Result<(), ElfError> {
        let virt_start = ph.p_vaddr as usize;
        let virt_end = virt_start + ph.p_memsz as usize;
        let page_start = virt_start & !0xFFF;
        let page_end = (virt_end + 0xFFF) & !0xFFF;

        let mut flags = PageFlags::PRESENT | PageFlags::USER;
        if ph.p_flags & PF_W != 0 {
            flags |= PageFlags::WRITABLE;
        }
        if ph.p_flags & PF_X == 0 {
            flags |= PageFlags::NO_EXECUTE;
        }

        for page_addr in (page_start..page_end).step_by(4096) {
            let virt_addr = VirtualAddress::from_usize(page_addr);
            if let Some(phys_addr) = pmm.alloc_frame() {
                vmm.map_page(virt_addr, phys_addr, flags);

                let offset_in_segment = if page_addr >= virt_start {
                    page_addr - virt_start
                } else {
                    0
                };

                let file_offset = ph.p_offset as usize + offset_in_segment;
                let bytes_to_copy = core::cmp::min(
                    4096,
                    ph.p_filesz as usize - offset_in_segment
                );

                if bytes_to_copy > 0 && file_offset < self.parser.data.len() {
                    let src = &self.parser.data[file_offset..file_offset + bytes_to_copy];
                    let dst = unsafe {
                        core::slice::from_raw_parts_mut(
                            phys_addr.as_usize() as *mut u8,
                            bytes_to_copy
                        )
                    };
                    dst.copy_from_slice(src);
                }
            } else {
                return Err(ElfError::InvalidProgramHeader);
            }
        }

        Ok(())
    }

    pub fn create_process(
        &self,
        pid: ProcessId,
        vmm: &mut VirtualMemoryManager,
        pmm: &mut PhysicalMemoryManager,
    ) -> Result<Process, ElfError> {
        let entry_point = self.load(vmm, pmm)?;

        let mut process = Process::new(pid);
        process.context.rip = entry_point;
        process.context.rsp = USER_STACK_TOP as u64;
        process.context.rflags = 0x202;

        Ok(process)
    }
}

const PAGE_SIZE: usize = 4096;
const PAGE_PRESENT: u64 = 1 << 0;
const PAGE_WRITABLE: u64 = 1 << 1;
const PAGE_USER: u64 = 1 << 2;
const PAGE_HUGE: u64 = 1 << 7;

fn align_down(value: usize, align: usize) -> usize {
    value & !(align - 1)
}

fn align_up(value: usize, align: usize) -> Option<usize> {
    value.checked_add(align - 1).map(|v| v & !(align - 1))
}

pub fn run_demo_elf(data: &[u8]) -> bool {
    let parser = match ElfParser::new(data) {
        Ok(parser) => parser,
        Err(_) => {
            crate::memory::serial_write("[ELF-TEST] parse failed\r\n");
            return false;
        }
    };

    if unsafe { load_into_current_address_space(&parser).is_err() } {
        crate::memory::serial_write("[ELF-TEST] load failed\r\n");
        return false;
    }

    crate::memory::serial_write("[ELF-TEST] userspace app started\r\n");
    crate::syscall::begin_elf_test();

    unsafe {
        crate::hal::run_user_syscall_smoke(parser.entry_point(), USER_STACK_TOP as u64);
    }

    crate::syscall::finish_elf_test()
}

pub fn run_process_elf(data: &[u8]) -> Result<ProcessExit, ElfError> {
    let parser = match ElfParser::new(data) {
        Ok(parser) => parser,
        Err(_) => {
            crate::memory::serial_write("[ELF-TEST] parse failed\r\n");
            return Err(ElfError::InvalidMagic);
        }
    };

    let pid = crate::process::allocate_pid();
    let mut process = match Process::new_user(pid) {
        Ok(process) => process,
        Err(_) => {
            crate::memory::serial_write("[ELF-TEST] process create failed\r\n");
            return Err(ElfError::InvalidProgramHeader);
        }
    };

    if load_into_process_address_space(&parser, &mut process).is_err() {
        crate::memory::serial_write("[ELF-TEST] process load failed\r\n");
        return Err(ElfError::InvalidProgramHeader);
    }

    process.context.rip = parser.entry_point();
    process.context.rsp = initial_user_stack() as u64;
    process.context.rflags = 0x202;

    crate::memory::serial_write("[ELF-TEST] userspace app started\r\n");

    match crate::process::enter_user_process(process) {
        Ok(exit) => Ok(exit),
        Err(_) => {
            crate::memory::serial_write("[ELF-TEST] process run failed\r\n");
            Err(ElfError::InvalidProgramHeader)
        }
    }
}

pub const fn initial_user_stack() -> usize {
    USER_STACK_TOP & !0xF
}

fn load_into_process_address_space(
    parser: &ElfParser,
    process: &mut Process,
) -> Result<(), ElfError> {
    let address_space = process
        .address_space_mut()
        .ok_or(ElfError::InvalidProgramHeader)?;

    for ph in parser.program_headers()? {
        if ph.p_type != PT_LOAD {
            continue;
        }
        load_segment_process(parser.data, address_space, &ph)?;
    }

    for page in 1..=(USER_STACK_SIZE / PAGE_SIZE) {
        let virt = VirtualAddress::from_usize(USER_STACK_TOP - (page * PAGE_SIZE));
        ensure_process_page(address_space, virt, PageFlags::WRITABLE)?;
    }

    Ok(())
}

fn load_segment_process(
    data: &[u8],
    address_space: &mut AddressSpace,
    ph: &ProgramHeader,
) -> Result<(), ElfError> {
    if ph.p_memsz < ph.p_filesz {
        return Err(ElfError::InvalidProgramHeader);
    }

    let virt_start = ph.p_vaddr as usize;
    let mem_size = ph.p_memsz as usize;
    let file_size = ph.p_filesz as usize;
    let file_start = ph.p_offset as usize;
    let virt_end = virt_start
        .checked_add(mem_size)
        .ok_or(ElfError::InvalidProgramHeader)?;
    let file_end = file_start
        .checked_add(file_size)
        .ok_or(ElfError::InvalidProgramHeader)?;

    if file_end > data.len() {
        return Err(ElfError::InvalidProgramHeader);
    }

    let page_start = align_down(virt_start, PAGE_SIZE);
    let page_end = align_up(virt_end, PAGE_SIZE).ok_or(ElfError::InvalidProgramHeader)?;

    let mut flags = PageFlags::USER;
    if ph.p_flags & PF_W != 0 {
        flags |= PageFlags::WRITABLE;
    }
    if ph.p_flags & PF_X == 0 {
        flags |= PageFlags::NO_EXECUTE;
    }

    for page_addr in (page_start..page_end).step_by(PAGE_SIZE) {
        let virt = VirtualAddress::from_usize(page_addr);
        let phys = ensure_process_page(address_space, virt, flags)?;
        let dst_page = crate::memory::vmm::phys_to_virt(phys.as_usize()) as *mut u8;

        let copy_start = page_addr.max(virt_start);
        let copy_end = (page_addr + PAGE_SIZE).min(virt_start + file_size);
        if copy_start < copy_end {
            let src_offset = file_start + (copy_start - virt_start);
            let dst_offset = copy_start - page_addr;
            let copy_len = copy_end - copy_start;
            unsafe {
                core::ptr::copy_nonoverlapping(
                    data.as_ptr().add(src_offset),
                    dst_page.add(dst_offset),
                    copy_len,
                );
            }
        }
    }

    Ok(())
}

fn ensure_process_page(
    address_space: &mut AddressSpace,
    virt: VirtualAddress,
    flags: PageFlags,
) -> Result<PhysicalAddress, ElfError> {
    match address_space.translate_user_page(virt) {
        Ok(Some(phys)) => {
            let phys = PhysicalAddress(align_down(phys.as_usize(), PAGE_SIZE));
            let existing_flags = address_space
                .user_page_flags(virt)
                .ok()
                .flatten()
                .unwrap_or(PageFlags::empty());
            let mut merged_flags = existing_flags | flags | PageFlags::USER;
            if !existing_flags.contains(PageFlags::NO_EXECUTE)
                || !flags.contains(PageFlags::NO_EXECUTE)
            {
                merged_flags.remove(PageFlags::NO_EXECUTE);
            }
            address_space
                .map_user_frame(virt, phys, merged_flags)
                .map_err(|_| ElfError::InvalidProgramHeader)?;
            Ok(phys)
        }
        Ok(None) => address_space
            .map_user_page(virt, flags)
            .map_err(|_| ElfError::InvalidProgramHeader),
        Err(_) => Err(ElfError::InvalidProgramHeader),
    }
}

unsafe fn load_into_current_address_space(parser: &ElfParser) -> Result<(), ElfError> {
    for ph in parser.program_headers()? {
        if ph.p_type != PT_LOAD {
            continue;
        }
        load_segment_current(parser.data, &ph)?;
    }

    let pmm = crate::memory::pmm::get_pmm().ok_or(ElfError::InvalidProgramHeader)?;
    let hhdm = crate::memory::vmm::get_hhdm_offset() as usize;
    if hhdm == 0 {
        return Err(ElfError::InvalidProgramHeader);
    }

    for page in 1..=(USER_STACK_SIZE / PAGE_SIZE) {
        let virt = USER_STACK_TOP - (page * PAGE_SIZE);
        let phys = pmm
            .alloc_frame()
            .ok_or(ElfError::InvalidProgramHeader)?
            .as_usize();
        core::ptr::write_bytes((phys + hhdm) as *mut u8, 0, PAGE_SIZE);
        map_current_user_page(virt, phys)?;
    }

    Ok(())
}

unsafe fn load_segment_current(data: &[u8], ph: &ProgramHeader) -> Result<(), ElfError> {
    if ph.p_memsz < ph.p_filesz {
        return Err(ElfError::InvalidProgramHeader);
    }

    let virt_start = ph.p_vaddr as usize;
    let mem_size = ph.p_memsz as usize;
    let file_size = ph.p_filesz as usize;
    let file_start = ph.p_offset as usize;
    let virt_end = virt_start
        .checked_add(mem_size)
        .ok_or(ElfError::InvalidProgramHeader)?;
    let file_end = file_start
        .checked_add(file_size)
        .ok_or(ElfError::InvalidProgramHeader)?;

    if file_end > data.len() {
        return Err(ElfError::InvalidProgramHeader);
    }

    let page_start = align_down(virt_start, PAGE_SIZE);
    let page_end = align_up(virt_end, PAGE_SIZE).ok_or(ElfError::InvalidProgramHeader)?;
    let pmm = crate::memory::pmm::get_pmm().ok_or(ElfError::InvalidProgramHeader)?;
    let hhdm = crate::memory::vmm::get_hhdm_offset() as usize;
    if hhdm == 0 {
        return Err(ElfError::InvalidProgramHeader);
    }

    for page_addr in (page_start..page_end).step_by(PAGE_SIZE) {
        let (phys, newly_allocated) = match current_mapping_phys(page_addr)? {
            Some(phys) => (phys, false),
            None => {
                let phys = pmm
                    .alloc_frame()
                    .ok_or(ElfError::InvalidProgramHeader)?
                    .as_usize();
                map_current_user_page(page_addr, phys)?;
                (phys, true)
            }
        };
        let dst_page = (phys + hhdm) as *mut u8;
        if newly_allocated {
            core::ptr::write_bytes(dst_page, 0, PAGE_SIZE);
        }

        let copy_start = page_addr.max(virt_start);
        let copy_end = (page_addr + PAGE_SIZE).min(virt_start + file_size);
        if copy_start < copy_end {
            let src_offset = file_start + (copy_start - virt_start);
            let dst_offset = copy_start - page_addr;
            let copy_len = copy_end - copy_start;
            core::ptr::copy_nonoverlapping(
                data.as_ptr().add(src_offset),
                dst_page.add(dst_offset),
                copy_len,
            );
        }
    }

    Ok(())
}

unsafe fn current_mapping_phys(virt: usize) -> Result<Option<usize>, ElfError> {
    let hhdm = crate::memory::vmm::get_hhdm_offset() as usize;
    if hhdm == 0 {
        return Err(ElfError::InvalidProgramHeader);
    }

    let mut cr3: usize;
    core::arch::asm!("mov {}, cr3", out(reg) cr3, options(nomem, nostack, preserves_flags));
    let pml4 = ((cr3 & !0xfff) + hhdm) as *mut u64;

    let p4 = (virt >> 39) & 0x1ff;
    let p3 = (virt >> 30) & 0x1ff;
    let p2 = (virt >> 21) & 0x1ff;
    let p1 = (virt >> 12) & 0x1ff;

    let pml4e = core::ptr::read_volatile(pml4.add(p4));
    if pml4e & PAGE_PRESENT == 0 || pml4e & PAGE_HUGE != 0 {
        return Ok(None);
    }
    let pdpt = (((pml4e as usize) & !0xfff) + hhdm) as *mut u64;

    let pdpte = core::ptr::read_volatile(pdpt.add(p3));
    if pdpte & PAGE_PRESENT == 0 {
        return Ok(None);
    }
    if pdpte & PAGE_HUGE != 0 {
        return Ok(Some((pdpte as usize & !0x3fff_ffff) + (virt & 0x3fff_ffff)));
    }
    let pd = (((pdpte as usize) & !0xfff) + hhdm) as *mut u64;

    let pde = core::ptr::read_volatile(pd.add(p2));
    if pde & PAGE_PRESENT == 0 {
        return Ok(None);
    }
    if pde & PAGE_HUGE != 0 {
        return Ok(Some((pde as usize & !0x1f_ffff) + (virt & 0x1f_ffff)));
    }
    let pt = (((pde as usize) & !0xfff) + hhdm) as *mut u64;

    let pte = core::ptr::read_volatile(pt.add(p1));
    if pte & PAGE_PRESENT == 0 {
        return Ok(None);
    }

    Ok(Some((pte as usize) & !0xfff))
}

unsafe fn map_current_user_page(virt: usize, phys: usize) -> Result<(), ElfError> {
    if virt & 0xfff != 0 || phys & 0xfff != 0 {
        return Err(ElfError::InvalidProgramHeader);
    }

    let hhdm = crate::memory::vmm::get_hhdm_offset() as usize;
    if hhdm == 0 {
        return Err(ElfError::InvalidProgramHeader);
    }

    let mut cr3: usize;
    core::arch::asm!("mov {}, cr3", out(reg) cr3, options(nomem, nostack, preserves_flags));
    let pml4 = ((cr3 & !0xfff) + hhdm) as *mut u64;

    let p4 = (virt >> 39) & 0x1ff;
    let p3 = (virt >> 30) & 0x1ff;
    let p2 = (virt >> 21) & 0x1ff;
    let p1 = (virt >> 12) & 0x1ff;

    let pdpt = ensure_next_table(pml4.add(p4), hhdm)?;
    let pd = ensure_next_table(pdpt.add(p3), hhdm)?;
    let pt = ensure_next_table(pd.add(p2), hhdm)?;
    core::ptr::write_volatile(pt.add(p1), (phys as u64) | PAGE_PRESENT | PAGE_WRITABLE | PAGE_USER);
    flush_user_mapping(virt);
    Ok(())
}

unsafe fn ensure_next_table(entry: *mut u64, hhdm: usize) -> Result<*mut u64, ElfError> {
    let mut value = core::ptr::read_volatile(entry);
    if value & PAGE_PRESENT != 0 {
        if value & PAGE_HUGE != 0 {
            return Err(ElfError::InvalidProgramHeader);
        }
        if value & (PAGE_USER | PAGE_WRITABLE) != (PAGE_USER | PAGE_WRITABLE) {
            value |= PAGE_USER | PAGE_WRITABLE;
            core::ptr::write_volatile(entry, value);
        }
        return Ok((((value as usize) & !0xfff) + hhdm) as *mut u64);
    }

    let pmm = crate::memory::pmm::get_pmm().ok_or(ElfError::InvalidProgramHeader)?;
    let frame = pmm
        .alloc_frame()
        .ok_or(ElfError::InvalidProgramHeader)?
        .as_usize();
    let table = (frame + hhdm) as *mut u64;
    core::ptr::write_bytes(table as *mut u8, 0, PAGE_SIZE);
    core::ptr::write_volatile(entry, (frame as u64) | PAGE_PRESENT | PAGE_WRITABLE | PAGE_USER);
    Ok(table)
}

unsafe fn flush_user_mapping(virt: usize) {
    core::arch::asm!("invlpg [{}]", in(reg) virt, options(nostack, preserves_flags));
}
