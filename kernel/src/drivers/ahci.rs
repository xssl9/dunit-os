use core::sync::atomic::{fence, AtomicBool, AtomicUsize, Ordering};

use crate::drivers::block::{self, BlockDeviceInfo, BlockError};
use crate::drivers::pci::{self, PciBar, PciDevice};
use crate::memory::pmm::{get_pmm, PhysicalAddress};
use crate::memory::vmm;
use crate::serial_write;

const AHCI_CLASS: u8 = 0x01;
const AHCI_SUBCLASS: u8 = 0x06;
const AHCI_PROG_IF: u8 = 0x01;
const AHCI_ABAR_INDEX: u8 = 5;
const AHCI_MMIO_SIZE: usize = 0x2000;
const MAX_CONTROLLERS: usize = 4;
const MAX_DISKS: usize = 4;
const PAGE_SIZE: usize = 4096;
const SECTOR_SIZE: usize = 512;
const TIMEOUT_SPINS: usize = 10_000_000;

const HBA_CAP: usize = 0x00;
const HBA_GHC: usize = 0x04;
const HBA_PI: usize = 0x0c;
const HBA_VS: usize = 0x10;
const HBA_CAP2: usize = 0x24;
const HBA_BOHC: usize = 0x28;
const HBA_PORTS: usize = 0x100;
const HBA_PORT_STRIDE: usize = 0x80;

const GHC_HR: u32 = 1 << 0;
const GHC_AE: u32 = 1 << 31;
const CAP_S64A: u32 = 1 << 31;
const CAP2_BOH: u32 = 1 << 0;
const BOHC_BOS: u32 = 1 << 0;
const BOHC_OOS: u32 = 1 << 1;
const BOHC_BB: u32 = 1 << 4;

const PX_CLB: usize = 0x00;
const PX_CLBU: usize = 0x04;
const PX_FB: usize = 0x08;
const PX_FBU: usize = 0x0c;
const PX_IS: usize = 0x10;
const PX_IE: usize = 0x14;
const PX_CMD: usize = 0x18;
const PX_TFD: usize = 0x20;
const PX_SIG: usize = 0x24;
const PX_SSTS: usize = 0x28;
const PX_SERR: usize = 0x30;
const PX_SACT: usize = 0x34;
const PX_CI: usize = 0x38;

const PXCMD_ST: u32 = 1 << 0;
const PXCMD_SUD: u32 = 1 << 1;
const PXCMD_POD: u32 = 1 << 2;
const PXCMD_FRE: u32 = 1 << 4;
const PXCMD_FR: u32 = 1 << 14;
const PXCMD_CR: u32 = 1 << 15;
const PXIS_TFES: u32 = 1 << 30;
const TFD_ERR: u32 = 1 << 0;
const TFD_DRQ: u32 = 1 << 3;
const TFD_BSY: u32 = 1 << 7;
const SATA_SIG_ATA: u32 = 0x0000_0101;

const FIS_TYPE_REG_H2D: u8 = 0x27;
const ATA_CMD_IDENTIFY: u8 = 0xec;
const ATA_CMD_READ_DMA_EXT: u8 = 0x25;
const ATA_CMD_WRITE_DMA_EXT: u8 = 0x35;
const ATA_CMD_FLUSH_CACHE_EXT: u8 = 0xea;

const COMMAND_HEADER_SIZE: usize = 32;
const COMMAND_TABLE_CFIS: usize = 0;
const COMMAND_TABLE_PRDT: usize = 128;

static AHCI_LOCK: AtomicBool = AtomicBool::new(false);
static AHCI_FOUND: AtomicUsize = AtomicUsize::new(0);
static AHCI_INITIALIZED: AtomicUsize = AtomicUsize::new(0);
static AHCI_DISK_COUNT: AtomicUsize = AtomicUsize::new(0);
static AHCI_LAST_ERROR: AtomicUsize = AtomicUsize::new(0);
static mut DISKS: [Option<AhciDisk>; MAX_DISKS] = [None; MAX_DISKS];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AhciError {
    NoAbar,
    MmioMap,
    BiosHandoffTimeout,
    ResetTimeout,
    PortStopTimeout,
    DmaAlloc,
    DmaAddress,
    DeviceBusy,
    CommandTimeout,
    TaskFile,
    IdentifyFailed,
    UnsupportedSectorSize,
    NoCapacity,
    UnsupportedDevice,
    UnsupportedLba48,
}

impl AhciError {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NoAbar => "ABAR missing",
            Self::MmioMap => "MMIO map failed",
            Self::BiosHandoffTimeout => "BIOS handoff timeout",
            Self::ResetTimeout => "controller reset timeout",
            Self::PortStopTimeout => "port stop timeout",
            Self::DmaAlloc => "DMA allocation failed",
            Self::DmaAddress => "controller requires 32-bit DMA",
            Self::DeviceBusy => "device stayed busy",
            Self::CommandTimeout => "command timeout",
            Self::TaskFile => "ATA task-file error",
            Self::IdentifyFailed => "IDENTIFY failed",
            Self::UnsupportedSectorSize => "logical sector is not 512 bytes",
            Self::NoCapacity => "disk capacity is zero",
            Self::UnsupportedDevice => "non-ATA device",
            Self::UnsupportedLba48 => "LBA48 is not supported",
        }
    }

    const fn code(self) -> usize {
        self as usize + 1
    }

    const fn from_code(code: usize) -> Option<Self> {
        match code {
            1 => Some(Self::NoAbar),
            2 => Some(Self::MmioMap),
            3 => Some(Self::BiosHandoffTimeout),
            4 => Some(Self::ResetTimeout),
            5 => Some(Self::PortStopTimeout),
            6 => Some(Self::DmaAlloc),
            7 => Some(Self::DmaAddress),
            8 => Some(Self::DeviceBusy),
            9 => Some(Self::CommandTimeout),
            10 => Some(Self::TaskFile),
            11 => Some(Self::IdentifyFailed),
            12 => Some(Self::UnsupportedSectorSize),
            13 => Some(Self::NoCapacity),
            14 => Some(Self::UnsupportedDevice),
            15 => Some(Self::UnsupportedLba48),
            _ => None,
        }
    }
}

#[derive(Clone, Copy)]
pub struct AhciStatus {
    pub controllers_found: usize,
    pub controllers_initialized: usize,
    pub disks: usize,
    pub last_error: Option<AhciError>,
}

#[derive(Clone, Copy)]
struct DmaPage {
    phys: u64,
    virt: usize,
}

#[derive(Clone, Copy)]
struct AhciDisk {
    port: usize,
    capacity_sectors: u64,
    command_list: DmaPage,
    _received_fis: DmaPage,
    command_table: DmaPage,
    data: DmaPage,
}

pub fn init() {
    AHCI_FOUND.store(0, Ordering::Relaxed);
    AHCI_INITIALIZED.store(0, Ordering::Relaxed);
    AHCI_DISK_COUNT.store(0, Ordering::Relaxed);
    AHCI_LAST_ERROR.store(0, Ordering::Relaxed);
    unsafe { DISKS = [None; MAX_DISKS] };

    pci::for_each_device(|dev| {
        if dev.class_code != AHCI_CLASS
            || dev.subclass != AHCI_SUBCLASS
            || dev.prog_if != AHCI_PROG_IF
            || AHCI_FOUND.load(Ordering::Relaxed) >= MAX_CONTROLLERS
        {
            return;
        }

        AHCI_FOUND.fetch_add(1, Ordering::Relaxed);
        serial_write("[AHCI] controller ");
        write_pci_addr(dev);
        serial_write(" vendor=");
        write_hex(dev.vendor_id as u64, 4);
        serial_write(" device=");
        write_hex(dev.device_id as u64, 4);
        serial_write("\r\n");

        match bring_up_controller(dev) {
            Ok(disks) => {
                AHCI_INITIALIZED.fetch_add(1, Ordering::Relaxed);
                AHCI_DISK_COUNT.fetch_add(disks, Ordering::Relaxed);
            }
            Err(error) => {
                AHCI_LAST_ERROR.store(error.code(), Ordering::Relaxed);
                serial_write("[AHCI] init failed: ");
                serial_write(error.as_str());
                serial_write("\r\n");
            }
        }
    });

    register_disks();
    serial_write("[AHCI] controllers found=");
    write_dec(AHCI_FOUND.load(Ordering::Relaxed) as u64);
    serial_write(" initialized=");
    write_dec(AHCI_INITIALIZED.load(Ordering::Relaxed) as u64);
    serial_write(" disks=");
    write_dec(AHCI_DISK_COUNT.load(Ordering::Relaxed) as u64);
    serial_write("\r\n");
}

pub fn status() -> AhciStatus {
    AhciStatus {
        controllers_found: AHCI_FOUND.load(Ordering::Relaxed),
        controllers_initialized: AHCI_INITIALIZED.load(Ordering::Relaxed),
        disks: AHCI_DISK_COUNT.load(Ordering::Relaxed),
        last_error: AhciError::from_code(AHCI_LAST_ERROR.load(Ordering::Relaxed)),
    }
}

fn bring_up_controller(dev: PciDevice) -> Result<usize, AhciError> {
    pci::enable_mmio_bus_master(dev);
    let abar_phys = match pci::read_bar_decoded(dev, AHCI_ABAR_INDEX) {
        PciBar::Memory32(address) if address != 0 => address as u64,
        PciBar::Memory64(address) if address != 0 => address,
        _ => return Err(AhciError::NoAbar),
    };
    let abar =
        vmm::map_mmio_region(abar_phys as usize, AHCI_MMIO_SIZE).ok_or(AhciError::MmioMap)?;

    bios_handoff(abar)?;
    write32(abar, HBA_GHC, read32(abar, HBA_GHC) | GHC_AE | GHC_HR);
    wait_until(
        || (read32(abar, HBA_GHC) & GHC_HR) == 0,
        AhciError::ResetTimeout,
    )?;
    write32(abar, HBA_GHC, read32(abar, HBA_GHC) | GHC_AE);

    let cap = read32(abar, HBA_CAP);
    let supports_64bit = (cap & CAP_S64A) != 0;
    let implemented = read32(abar, HBA_PI);
    serial_write("[AHCI] ABAR=");
    write_hex(abar_phys, 16);
    serial_write(" version=");
    write_hex(read32(abar, HBA_VS) as u64, 8);
    serial_write(" ports=");
    write_hex(implemented as u64, 8);
    serial_write(" dma64=");
    serial_write(if supports_64bit { "yes" } else { "no" });
    serial_write("\r\n");

    let mut added = 0usize;
    for port_number in 0..32u8 {
        if (implemented & (1 << port_number)) == 0
            || AHCI_DISK_COUNT.load(Ordering::Relaxed) + added >= MAX_DISKS
        {
            continue;
        }
        let port = abar + HBA_PORTS + port_number as usize * HBA_PORT_STRIDE;
        if !sata_disk_present(port, port_number) {
            continue;
        }
        match setup_disk(port, port_number, supports_64bit) {
            Ok(disk) => {
                let index = AHCI_DISK_COUNT.load(Ordering::Relaxed) + added;
                unsafe { DISKS[index] = Some(disk) };
                added += 1;
                serial_write("[AHCI] SATA disk port=");
                write_dec(port_number as u64);
                serial_write(" sectors=");
                write_dec(disk.capacity_sectors);
                serial_write("\r\n");
            }
            Err(error) => {
                if error != AhciError::UnsupportedDevice {
                    AHCI_LAST_ERROR.store(error.code(), Ordering::Relaxed);
                }
                serial_write("[AHCI] port ");
                write_dec(port_number as u64);
                serial_write(if error == AhciError::UnsupportedDevice {
                    " skipped: "
                } else {
                    " failed: "
                });
                serial_write(error.as_str());
                serial_write("\r\n");
            }
        }
    }
    Ok(added)
}

fn bios_handoff(abar: usize) -> Result<(), AhciError> {
    if (read32(abar, HBA_CAP2) & CAP2_BOH) == 0 {
        return Ok(());
    }
    write32(abar, HBA_BOHC, read32(abar, HBA_BOHC) | BOHC_OOS);
    wait_until(
        || (read32(abar, HBA_BOHC) & (BOHC_BOS | BOHC_BB)) == 0,
        AhciError::BiosHandoffTimeout,
    )
}

fn sata_disk_present(port: usize, _port_number: u8) -> bool {
    write32(port, PX_CMD, read32(port, PX_CMD) | PXCMD_POD | PXCMD_SUD);
    let ssts = read32(port, PX_SSTS);
    let det = ssts & 0x0f;
    let ipm = (ssts >> 8) & 0x0f;
    det == 3 && ipm == 1
}

fn setup_disk(port: usize, port_number: u8, supports_64bit: bool) -> Result<AhciDisk, AhciError> {
    stop_port(port)?;
    let command_list = alloc_dma_page(supports_64bit)?;
    let received_fis = alloc_dma_page(supports_64bit)?;
    let command_table = alloc_dma_page(supports_64bit)?;
    let data = alloc_dma_page(supports_64bit)?;

    write_address(port, PX_CLB, PX_CLBU, command_list.phys);
    write_address(port, PX_FB, PX_FBU, received_fis.phys);
    write32(port, PX_IE, 0);
    write32(port, PX_IS, u32::MAX);
    write32(port, PX_SERR, u32::MAX);
    write32(port, PX_SACT, 0);
    write32(port, PX_CI, 0);

    let command = read32(port, PX_CMD) | PXCMD_FRE | PXCMD_POD | PXCMD_SUD;
    write32(port, PX_CMD, command);
    write32(port, PX_CMD, command | PXCMD_ST);

    for _ in 0..100_000 {
        let signature = read32(port, PX_SIG);
        if signature != 0 && signature != u32::MAX {
            break;
        }
        core::hint::spin_loop();
    }
    let signature = read32(port, PX_SIG);
    serial_write("[AHCI] port=");
    write_dec(port_number as u64);
    serial_write(" ssts=");
    write_hex(read32(port, PX_SSTS) as u64, 8);
    serial_write(" sig=");
    write_hex(signature as u64, 8);
    serial_write("\r\n");
    if signature != SATA_SIG_ATA {
        let _ = stop_port(port);
        free_dma_page(command_list);
        free_dma_page(received_fis);
        free_dma_page(command_table);
        free_dma_page(data);
        return Err(AhciError::UnsupportedDevice);
    }

    let mut disk = AhciDisk {
        port,
        capacity_sectors: 0,
        command_list,
        _received_fis: received_fis,
        command_table,
        data,
    };
    if let Err(error) = identify(&mut disk) {
        let _ = stop_port(port);
        free_dma_page(command_list);
        free_dma_page(received_fis);
        free_dma_page(command_table);
        free_dma_page(data);
        return Err(error);
    }
    Ok(disk)
}

fn stop_port(port: usize) -> Result<(), AhciError> {
    write32(port, PX_CMD, read32(port, PX_CMD) & !PXCMD_ST);
    wait_until(
        || (read32(port, PX_CMD) & PXCMD_CR) == 0,
        AhciError::PortStopTimeout,
    )?;
    write32(port, PX_CMD, read32(port, PX_CMD) & !PXCMD_FRE);
    wait_until(
        || (read32(port, PX_CMD) & PXCMD_FR) == 0,
        AhciError::PortStopTimeout,
    )
}

fn identify(disk: &mut AhciDisk) -> Result<(), AhciError> {
    issue_data(disk, ATA_CMD_IDENTIFY, 0, false, false).map_err(|_| AhciError::IdentifyFailed)?;
    let data = unsafe { core::slice::from_raw_parts(disk.data.virt as *const u8, SECTOR_SIZE) };
    let word106 = identify_word(data, 106);
    if (word106 & (1 << 14)) != 0 && (word106 & (1 << 15)) == 0 && (word106 & (1 << 12)) != 0 {
        let logical_words =
            identify_word(data, 117) as u32 | ((identify_word(data, 118) as u32) << 16);
        if logical_words != 256 {
            return Err(AhciError::UnsupportedSectorSize);
        }
    }

    if (identify_word(data, 83) & (1 << 10)) == 0 {
        return Err(AhciError::UnsupportedLba48);
    }
    let capacity = identify_word(data, 100) as u64
        | ((identify_word(data, 101) as u64) << 16)
        | ((identify_word(data, 102) as u64) << 32)
        | ((identify_word(data, 103) as u64) << 48);
    if capacity == 0 {
        return Err(AhciError::NoCapacity);
    }
    disk.capacity_sectors = capacity;
    Ok(())
}

fn transfer(index: usize, lba: u64, buffer: &mut [u8], write: bool) -> Result<usize, BlockError> {
    if buffer.len() < SECTOR_SIZE {
        return Err(BlockError::BufferTooSmall);
    }
    lock_ahci();
    let result = unsafe {
        let Some(disk) = DISKS[index].as_mut() else {
            AHCI_LOCK.store(false, Ordering::Release);
            return Err(BlockError::NotFound);
        };
        if lba >= disk.capacity_sectors {
            Err(BlockError::OutOfRange)
        } else {
            if write {
                core::ptr::copy_nonoverlapping(
                    buffer.as_ptr(),
                    disk.data.virt as *mut u8,
                    SECTOR_SIZE,
                );
            }
            match issue_data(
                disk,
                if write {
                    ATA_CMD_WRITE_DMA_EXT
                } else {
                    ATA_CMD_READ_DMA_EXT
                },
                lba,
                write,
                true,
            ) {
                Ok(()) => {
                    if write {
                        if issue_nodata(disk, ATA_CMD_FLUSH_CACHE_EXT).is_err() {
                            Err(BlockError::Io)
                        } else {
                            Ok(SECTOR_SIZE)
                        }
                    } else {
                        core::ptr::copy_nonoverlapping(
                            disk.data.virt as *const u8,
                            buffer.as_mut_ptr(),
                            SECTOR_SIZE,
                        );
                        Ok(SECTOR_SIZE)
                    }
                }
                Err(_) => Err(BlockError::Io),
            }
        }
    };
    AHCI_LOCK.store(false, Ordering::Release);
    result
}

fn issue_data(
    disk: &mut AhciDisk,
    command: u8,
    lba: u64,
    write: bool,
    sector_count: bool,
) -> Result<(), AhciError> {
    prepare_command(disk, command, lba, write, true, sector_count);
    issue_slot(disk.port)
}

fn issue_nodata(disk: &mut AhciDisk, command: u8) -> Result<(), AhciError> {
    prepare_command(disk, command, 0, false, false, false);
    issue_slot(disk.port)
}

fn prepare_command(
    disk: &AhciDisk,
    command: u8,
    lba: u64,
    write: bool,
    has_data: bool,
    sector_count: bool,
) {
    unsafe {
        core::ptr::write_bytes(disk.command_list.virt as *mut u8, 0, COMMAND_HEADER_SIZE);
        core::ptr::write_bytes(disk.command_table.virt as *mut u8, 0, 256);
    }

    let header_flags = 5u16 | if write { 1 << 6 } else { 0 };
    mem_write16(disk.command_list.virt, 0, header_flags);
    mem_write16(disk.command_list.virt, 2, if has_data { 1 } else { 0 });
    mem_write64(disk.command_list.virt, 8, disk.command_table.phys);

    let fis = disk.command_table.virt + COMMAND_TABLE_CFIS;
    mem_write8(fis, 0, FIS_TYPE_REG_H2D);
    mem_write8(fis, 1, 1 << 7);
    mem_write8(fis, 2, command);
    mem_write8(fis, 4, lba as u8);
    mem_write8(fis, 5, (lba >> 8) as u8);
    mem_write8(fis, 6, (lba >> 16) as u8);
    mem_write8(fis, 7, 1 << 6);
    mem_write8(fis, 8, (lba >> 24) as u8);
    mem_write8(fis, 9, (lba >> 32) as u8);
    mem_write8(fis, 10, (lba >> 40) as u8);
    if sector_count {
        mem_write8(fis, 12, 1);
    }

    if has_data {
        let prdt = disk.command_table.virt + COMMAND_TABLE_PRDT;
        mem_write64(prdt, 0, disk.data.phys);
        mem_write32(prdt, 8, 0);
        mem_write32(prdt, 12, (SECTOR_SIZE - 1) as u32);
    }
    fence(Ordering::SeqCst);
}

fn issue_slot(port: usize) -> Result<(), AhciError> {
    wait_until(
        || (read32(port, PX_TFD) & (TFD_BSY | TFD_DRQ)) == 0,
        AhciError::DeviceBusy,
    )?;
    write32(port, PX_IS, u32::MAX);
    fence(Ordering::SeqCst);
    write32(port, PX_CI, read32(port, PX_CI) | 1);

    let mut spins = 0usize;
    while (read32(port, PX_CI) & 1) != 0 && spins < TIMEOUT_SPINS {
        if (read32(port, PX_IS) & PXIS_TFES) != 0 {
            return Err(AhciError::TaskFile);
        }
        core::hint::spin_loop();
        spins += 1;
    }
    fence(Ordering::SeqCst);
    if (read32(port, PX_CI) & 1) != 0 {
        return Err(AhciError::CommandTimeout);
    }
    if (read32(port, PX_IS) & PXIS_TFES) != 0 || (read32(port, PX_TFD) & TFD_ERR) != 0 {
        return Err(AhciError::TaskFile);
    }
    Ok(())
}

fn register_disks() {
    let count = AHCI_DISK_COUNT.load(Ordering::Relaxed).min(MAX_DISKS);
    for index in 0..count {
        let Some(disk) = (unsafe { DISKS[index] }) else {
            continue;
        };
        let (name, read_fn, write_fn): (
            &'static str,
            fn(u64, &mut [u8]) -> Result<usize, BlockError>,
            fn(u64, &[u8]) -> Result<usize, BlockError>,
        ) = match index {
            0 => ("sda", sda_read, sda_write),
            1 => ("sdb", sdb_read, sdb_write),
            2 => ("sdc", sdc_read, sdc_write),
            _ => ("sdd", sdd_read, sdd_write),
        };
        block::register_device(
            BlockDeviceInfo {
                name,
                driver: "ahci",
                block_size: SECTOR_SIZE,
                blocks: disk.capacity_sectors,
                readonly: false,
            },
            read_fn,
            write_fn,
        );
    }
}

macro_rules! disk_io {
    ($read:ident, $write:ident, $index:expr) => {
        fn $read(lba: u64, buffer: &mut [u8]) -> Result<usize, BlockError> {
            transfer($index, lba, buffer, false)
        }
        fn $write(lba: u64, buffer: &[u8]) -> Result<usize, BlockError> {
            if buffer.len() < SECTOR_SIZE {
                return Err(BlockError::BufferTooSmall);
            }
            let mut sector = [0u8; SECTOR_SIZE];
            sector.copy_from_slice(&buffer[..SECTOR_SIZE]);
            transfer($index, lba, &mut sector, true)
        }
    };
}

disk_io!(sda_read, sda_write, 0);
disk_io!(sdb_read, sdb_write, 1);
disk_io!(sdc_read, sdc_write, 2);
disk_io!(sdd_read, sdd_write, 3);

fn alloc_dma_page(supports_64bit: bool) -> Result<DmaPage, AhciError> {
    let pmm = get_pmm().ok_or(AhciError::DmaAlloc)?;
    let PhysicalAddress(phys) = pmm.alloc_frame().ok_or(AhciError::DmaAlloc)?;
    if !supports_64bit && phys > u32::MAX as usize {
        pmm.free_frame(PhysicalAddress(phys));
        return Err(AhciError::DmaAddress);
    }
    let page = DmaPage {
        phys: phys as u64,
        virt: vmm::phys_to_virt(phys),
    };
    unsafe { core::ptr::write_bytes(page.virt as *mut u8, 0, PAGE_SIZE) };
    Ok(page)
}

fn free_dma_page(page: DmaPage) {
    if let Some(pmm) = get_pmm() {
        pmm.free_frame(PhysicalAddress(page.phys as usize));
    }
}

fn identify_word(data: &[u8], word: usize) -> u16 {
    u16::from_le_bytes([data[word * 2], data[word * 2 + 1]])
}

fn wait_until<F: Fn() -> bool>(condition: F, error: AhciError) -> Result<(), AhciError> {
    for _ in 0..TIMEOUT_SPINS {
        if condition() {
            return Ok(());
        }
        core::hint::spin_loop();
    }
    Err(error)
}

fn lock_ahci() {
    while AHCI_LOCK
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        core::hint::spin_loop();
    }
}

fn read32(base: usize, offset: usize) -> u32 {
    unsafe { core::ptr::read_volatile((base + offset) as *const u32) }
}

fn write32(base: usize, offset: usize, value: u32) {
    unsafe { core::ptr::write_volatile((base + offset) as *mut u32, value) }
}

fn write_address(base: usize, low: usize, high: usize, address: u64) {
    write32(base, low, address as u32);
    write32(base, high, (address >> 32) as u32);
}

fn mem_write8(base: usize, offset: usize, value: u8) {
    unsafe { core::ptr::write_volatile((base + offset) as *mut u8, value) }
}

fn mem_write16(base: usize, offset: usize, value: u16) {
    unsafe { core::ptr::write_volatile((base + offset) as *mut u16, value) }
}

fn mem_write32(base: usize, offset: usize, value: u32) {
    unsafe { core::ptr::write_volatile((base + offset) as *mut u32, value) }
}

fn mem_write64(base: usize, offset: usize, value: u64) {
    unsafe { core::ptr::write_volatile((base + offset) as *mut u64, value) }
}

fn write_pci_addr(dev: PciDevice) {
    write_hex(dev.bus as u64, 2);
    serial_write(":");
    write_hex(dev.device as u64, 2);
    serial_write(".");
    write_hex(dev.function as u64, 1);
}

fn write_hex(mut value: u64, digits: usize) {
    let mut buffer = [0u8; 16];
    let count = digits.min(buffer.len());
    for index in (0..count).rev() {
        let digit = (value & 0xf) as u8;
        buffer[index] = if digit < 10 {
            b'0' + digit
        } else {
            b'a' + digit - 10
        };
        value >>= 4;
    }
    serial_write(core::str::from_utf8(&buffer[..count]).unwrap_or("?"));
}

fn write_dec(mut value: u64) {
    if value == 0 {
        serial_write("0");
        return;
    }
    let mut buffer = [0u8; 20];
    let mut index = buffer.len();
    while value != 0 {
        index -= 1;
        buffer[index] = b'0' + (value % 10) as u8;
        value /= 10;
    }
    serial_write(core::str::from_utf8(&buffer[index..]).unwrap_or("?"));
}
