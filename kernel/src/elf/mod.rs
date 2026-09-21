use alloc::string::String;
use alloc::vec::Vec;

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
    ProcessCreateFailed,
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

        let ph =
            unsafe { core::ptr::read(self.data.as_ptr().add(ph_offset) as *const ProgramHeader) };

        self.index += 1;
        Some(ph)
    }
}

use crate::memory::pmm::PhysicalAddress;
use crate::memory::vmm::{AddressSpace, PageFlags, VirtualAddress};
use crate::process::{Process, ProcessExit, ProcessId};

pub const USER_STACK_SIZE: usize = 0x10000;
pub const USER_STACK_TOP: usize = 0x00007FFF_FFFFF000;
const MAX_EXEC_ARGS: usize = 16;
const MAX_EXEC_ARG_LEN: usize = 128;
const EXEC_ENV: [&str; 3] = ["PATH=/app", "SHELL=dunit", "CWD=/"];

const PAGE_SIZE: usize = 4096;

fn align_down(value: usize, align: usize) -> usize {
    value & !(align - 1)
}

fn align_up(value: usize, align: usize) -> Option<usize> {
    value.checked_add(align - 1).map(|v| v & !(align - 1))
}

pub fn run_process_elf(data: &[u8], argv: &[String]) -> Result<ProcessExit, ElfError> {
    match ElfParser::new(data) {
        Ok(_) => {}
        Err(_) => {
            crate::memory::serial_write("[ELF-TEST] parse failed\r\n");
            return Err(ElfError::InvalidMagic);
        }
    }

    let path = argv.first().cloned().unwrap_or_else(|| String::from("elf"));
    let pid = match crate::process::create_user_process_record(path, true) {
        Ok(pid) => pid,
        Err(_) => {
            crate::memory::serial_write("[ELF-TEST] process create failed\r\n");
            return Err(ElfError::InvalidProgramHeader);
        }
    };

    if prepare_process_elf(pid, data, argv).is_err() {
        crate::memory::serial_write("[ELF-TEST] process prepare failed\r\n");
        return Err(ElfError::InvalidProgramHeader);
    }

    crate::memory::serial_write("[ELF-TEST] userspace app started\r\n");

    match crate::process::enter_user_process(pid) {
        Ok(exit) => Ok(exit),
        Err(_) => {
            crate::memory::serial_write("[ELF-TEST] process run failed\r\n");
            Err(ElfError::InvalidProgramHeader)
        }
    }
}

pub fn prepare_process_elf(pid: ProcessId, data: &[u8], argv: &[String]) -> Result<(), ElfError> {
    let parser = ElfParser::new(data)?;
    let prepared = crate::process::with_process_mut(pid, |process| {
        if load_into_process_address_space(&parser, process).is_err() {
            crate::memory::serial_write("[ELF-TEST] process load failed\r\n");
            return Err(crate::process::ProcessError::InvalidUserContext);
        }

        let initial_stack = match prepare_initial_stack(process, argv, &EXEC_ENV) {
            Ok(stack) => stack,
            Err(_) => {
                crate::memory::serial_write("[ELF-TEST] argv stack setup failed\r\n");
                return Err(crate::process::ProcessError::InvalidUserContext);
            }
        };

        process.context.rip = parser.entry_point();
        process.context.rsp = initial_stack.rsp as u64;
        process.context.rflags = 0x202;
        process.context.rdi = initial_stack.argc as u64;
        process.context.rsi = initial_stack.argv as u64;
        process.context.rdx = initial_stack.envp as u64;
        process.entry_argc = initial_stack.argc;
        process.entry_argv = initial_stack.argv;
        process.entry_envp = initial_stack.envp;
        process.state = crate::process::ProcessState::Ready;
        Ok(())
    });

    if prepared.is_err() || crate::process::mark_process_prepared_as_ready(pid).is_err() {
        return Err(ElfError::InvalidProgramHeader);
    }

    Ok(())
}

pub const fn initial_user_stack() -> usize {
    USER_STACK_TOP & !0xF
}

struct InitialStack {
    rsp: usize,
    argc: usize,
    argv: usize,
    envp: usize,
}

/// Dunit userspace exec ABI v1.
///
/// On process entry:
/// - `%rsp` points at the stack block below and is 8 mod 16, matching the
///   x86_64 SysV function-entry contract a Rust `_start` expects after `call`.
/// - `%rdi = argc`, `%rsi = argv`, `%rdx = envp` for no-libc Rust `_start`.
/// - Stack memory contains:
///
/// ```text
/// rsp -> argc: u64
///        argv[0]: *const u8
///        ...
///        argv[argc - 1]: *const u8
///        NULL
///        envp[0]: *const u8
///        ...
///        NULL
///        padding, then NUL-terminated argv/env strings
/// ```
///
/// `envp` is intentionally minimal for now. The ABI exists so a fuller shell
/// environment can grow later without changing userspace startup shape.
fn prepare_initial_stack(
    process: &mut Process,
    argv: &[String],
    env: &[&str],
) -> Result<InitialStack, ElfError> {
    if argv.is_empty() || argv.len() > MAX_EXEC_ARGS || env.len() > MAX_EXEC_ARGS {
        return Err(ElfError::InvalidProgramHeader);
    }

    let mut sp = initial_user_stack();
    let mut argv_ptrs = Vec::new();
    let mut env_ptrs = Vec::new();

    for value in argv.iter().rev() {
        let ptr = write_stack_string(process, &mut sp, value.as_str())?;
        argv_ptrs.push(ptr);
    }
    argv_ptrs.reverse();

    for value in env.iter().rev() {
        let ptr = write_stack_string(process, &mut sp, value)?;
        env_ptrs.push(ptr);
    }
    env_ptrs.reverse();

    let words = argv_ptrs.len() + env_ptrs.len() + 3;
    sp &= !0xF;
    if words % 2 == 0 {
        sp = sp.checked_sub(8).ok_or(ElfError::InvalidProgramHeader)?;
        write_user_u64(process, sp, 0)?;
    }

    push_user_u64(process, &mut sp, 0)?;
    for ptr in env_ptrs.iter().rev() {
        push_user_u64(process, &mut sp, *ptr as u64)?;
    }
    let envp = sp;

    push_user_u64(process, &mut sp, 0)?;
    for ptr in argv_ptrs.iter().rev() {
        push_user_u64(process, &mut sp, *ptr as u64)?;
    }
    let argv_addr = sp;

    push_user_u64(process, &mut sp, argv_ptrs.len() as u64)?;

    if sp & 0xF != 8 {
        return Err(ElfError::InvalidProgramHeader);
    }

    Ok(InitialStack {
        rsp: sp,
        argc: argv_ptrs.len(),
        argv: argv_addr,
        envp,
    })
}

fn write_stack_string(
    process: &mut Process,
    sp: &mut usize,
    value: &str,
) -> Result<usize, ElfError> {
    if value.len() > MAX_EXEC_ARG_LEN {
        return Err(ElfError::InvalidProgramHeader);
    }

    let total = value.len() + 1;
    *sp = sp
        .checked_sub(total)
        .ok_or(ElfError::InvalidProgramHeader)?;
    write_user_bytes(process, *sp, value.as_bytes())?;
    write_user_bytes(process, *sp + value.len(), &[0])?;
    Ok(*sp)
}

fn push_user_u64(process: &mut Process, sp: &mut usize, value: u64) -> Result<(), ElfError> {
    *sp = sp.checked_sub(8).ok_or(ElfError::InvalidProgramHeader)?;
    write_user_u64(process, *sp, value)
}

fn write_user_u64(process: &mut Process, addr: usize, value: u64) -> Result<(), ElfError> {
    write_user_bytes(process, addr, &value.to_le_bytes())
}

fn write_user_bytes(process: &mut Process, addr: usize, bytes: &[u8]) -> Result<(), ElfError> {
    let address_space = process
        .address_space()
        .ok_or(ElfError::InvalidProgramHeader)?;
    let mut written = 0;

    while written < bytes.len() {
        let user_addr = addr
            .checked_add(written)
            .ok_or(ElfError::InvalidProgramHeader)?;
        let page_left = PAGE_SIZE - (user_addr & (PAGE_SIZE - 1));
        let chunk_len = page_left.min(bytes.len() - written);
        let phys = address_space
            .translate_user_page(VirtualAddress::from_usize(user_addr))
            .map_err(|_| ElfError::InvalidProgramHeader)?
            .ok_or(ElfError::InvalidProgramHeader)?;
        let dst = crate::memory::vmm::phys_to_virt(phys.as_usize()) as *mut u8;

        unsafe {
            core::ptr::copy_nonoverlapping(bytes.as_ptr().add(written), dst, chunk_len);
        }

        written += chunk_len;
    }

    Ok(())
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
                .protect_user_page(virt, merged_flags, true)
                .map_err(|_| ElfError::InvalidProgramHeader)?;
            Ok(phys)
        }
        Ok(None) => address_space
            .map_user_page(virt, flags)
            .map_err(|_| ElfError::InvalidProgramHeader),
        Err(_) => Err(ElfError::InvalidProgramHeader),
    }
}
