use proptest::prelude::*;

const ELF_MAGIC: [u8; 4] = [0x7F, b'E', b'L', b'F'];
const ELF_CLASS_64: u8 = 2;
const ELF_DATA_LSB: u8 = 1;
const ELF_MACHINE_X86_64: u16 = 0x3E;
const ELF_TYPE_EXEC: u16 = 2;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct ElfHeader {
    magic: [u8; 4],
    class: u8,
    data: u8,
    version: u8,
    os_abi: u8,
    abi_version: u8,
    padding: [u8; 7],
    elf_type: u16,
    machine: u16,
    version2: u32,
    entry: u64,
    phoff: u64,
    shoff: u64,
    flags: u32,
    ehsize: u16,
    phentsize: u16,
    phnum: u16,
    shentsize: u16,
    shnum: u16,
    shstrndx: u16,
}

#[derive(Debug)]
enum ElfError {
    InvalidMagic,
    InvalidClass,
    InvalidArchitecture,
    InvalidType,
}

fn create_valid_elf_header(entry: u64, phoff: u64, phnum: u16) -> Vec<u8> {
    let header = ElfHeader {
        magic: ELF_MAGIC,
        class: ELF_CLASS_64,
        data: ELF_DATA_LSB,
        version: 1,
        os_abi: 0,
        abi_version: 0,
        padding: [0; 7],
        elf_type: ELF_TYPE_EXEC,
        machine: ELF_MACHINE_X86_64,
        version2: 1,
        entry,
        phoff,
        shoff: 0,
        flags: 0,
        ehsize: 64,
        phentsize: 56,
        phnum,
        shentsize: 0,
        shnum: 0,
        shstrndx: 0,
    };
    
    let mut data = vec![0u8; 4096];
    unsafe {
        core::ptr::write(data.as_mut_ptr() as *mut ElfHeader, header);
    }
    data
}

fn parse_elf_header(data: &[u8]) -> Result<ElfHeader, ElfError> {
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

    Ok(header)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]
    
    #[test]
    fn prop_elf_header_parsing(entry in 0x400000u64..0x500000u64, phnum in 1u16..10) {
        let data = create_valid_elf_header(entry, 64, phnum);
        
        let result = parse_elf_header(&data);
        assert!(result.is_ok());
        
        let header = result.unwrap();
        assert_eq!(header.magic, ELF_MAGIC);
        assert_eq!(header.class, ELF_CLASS_64);
        assert_eq!(header.machine, ELF_MACHINE_X86_64);
        assert_eq!(header.elf_type, ELF_TYPE_EXEC);
        assert_eq!(header.entry, entry);
        assert_eq!(header.phnum, phnum);
    }
    
    #[test]
    fn prop_corrupted_magic_rejection(
        b0 in 0u8..255,
        b1 in 0u8..255,
        b2 in 0u8..255,
        b3 in 0u8..255
    ) {
        let magic = [b0, b1, b2, b3];
        if magic == ELF_MAGIC {
            return Ok(());
        }
        
        let mut data = create_valid_elf_header(0x400000, 64, 1);
        data[0] = b0;
        data[1] = b1;
        data[2] = b2;
        data[3] = b3;
        
        let result = parse_elf_header(&data);
        assert!(result.is_err());
    }
    
    #[test]
    fn prop_invalid_class_rejection(class in 0u8..255) {
        if class == ELF_CLASS_64 {
            return Ok(());
        }
        
        let mut data = create_valid_elf_header(0x400000, 64, 1);
        data[4] = class;
        
        let result = parse_elf_header(&data);
        assert!(result.is_err());
    }
    
    #[test]
    fn prop_invalid_architecture_rejection(machine in 0u16..0xFFFF) {
        if machine == ELF_MACHINE_X86_64 {
            return Ok(());
        }
        
        let mut data = create_valid_elf_header(0x400000, 64, 1);
        unsafe {
            let machine_ptr = data.as_mut_ptr().add(18) as *mut u16;
            *machine_ptr = machine;
        }
        
        let result = parse_elf_header(&data);
        assert!(result.is_err());
    }
    
    #[test]
    fn prop_invalid_type_rejection(elf_type in 0u16..0xFFFF) {
        if elf_type == ELF_TYPE_EXEC {
            return Ok(());
        }
        
        let mut data = create_valid_elf_header(0x400000, 64, 1);
        unsafe {
            let type_ptr = data.as_mut_ptr().add(16) as *mut u16;
            *type_ptr = elf_type;
        }
        
        let result = parse_elf_header(&data);
        assert!(result.is_err());
    }
}
