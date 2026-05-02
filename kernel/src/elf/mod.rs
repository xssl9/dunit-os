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

use crate::memory::pmm::{PhysicalAddress, PhysicalMemoryManager};
use crate::memory::vmm::{VirtualAddress, VirtualMemoryManager, PageFlags};
use crate::process::{Process, ProcessId};

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
