use crate::drivers::pci::{self, PciBar, PciDevice};
use crate::memory::pmm::{get_pmm, PhysicalAddress};
use crate::memory::vmm;
use crate::serial_write;
use core::sync::atomic::{AtomicUsize, Ordering};

const XHCI_PROG_IF: u8 = 0x30;

const CAP_CAPLENGTH: usize = 0x00;
const CAP_HCIVERSION: usize = 0x02;
const CAP_HCSPARAMS1: usize = 0x04;
const CAP_HCSPARAMS2: usize = 0x08;
const CAP_HCCPARAMS1: usize = 0x10;
const CAP_DBOFF: usize = 0x14;
const CAP_RTSOFF: usize = 0x18;

const OP_USBCMD: usize = 0x00;
const OP_USBSTS: usize = 0x04;
const OP_PAGESIZE: usize = 0x08;
const OP_CRCR: usize = 0x18;
const OP_DCBAAP: usize = 0x30;
const OP_CONFIG: usize = 0x38;
const OP_PORTS_BASE: usize = 0x400;
const OP_PORT_STRIDE: usize = 0x10;

const USBCMD_RUN_STOP: u32 = 1 << 0;
const USBCMD_HOST_CONTROLLER_RESET: u32 = 1 << 1;
const USBSTS_HOST_CONTROLLER_HALTED: u32 = 1 << 0;
const USBSTS_CONTROLLER_NOT_READY: u32 = 1 << 11;

const PORTSC_CURRENT_CONNECT_STATUS: u32 = 1 << 0;
const PORTSC_PORT_ENABLED: u32 = 1 << 1;
const PORTSC_PORT_POWER: u32 = 1 << 9;
const PORTSC_CHANGE_BITS: u32 = (1 << 17) | (1 << 18) | (1 << 20) | (1 << 21) | (1 << 22);

const MAX_CONTROLLERS: usize = 4;
const TIMEOUT_SPINS: usize = 1_000_000;
const XHCI_MMIO_MAP_SIZE: usize = 0x10000;
const PAGE_SIZE: usize = 4096;
const TRB_SIZE: usize = 16;
const TRBS_PER_PAGE: usize = PAGE_SIZE / TRB_SIZE;
const COMMAND_RING_TRBS: usize = TRBS_PER_PAGE;
const EVENT_RING_TRBS: usize = TRBS_PER_PAGE;

const TRB_TYPE_LINK: u32 = 6;
const TRB_TYPE_ENABLE_SLOT_COMMAND: u32 = 9;
const TRB_TYPE_COMMAND_COMPLETION_EVENT: u32 = 33;

const TRB_CYCLE: u32 = 1 << 0;
const TRB_LINK_TOGGLE_CYCLE: u32 = 1 << 1;

static XHCI_FOUND: AtomicUsize = AtomicUsize::new(0);
static XHCI_INITIALIZED: AtomicUsize = AtomicUsize::new(0);
static XHCI_CONNECTED_PORTS: AtomicUsize = AtomicUsize::new(0);
static XHCI_LAST_ERROR: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone, Copy)]
pub struct XhciStatus {
    pub found: usize,
    pub initialized: usize,
    pub connected_ports: usize,
    pub last_error: Option<XhciError>,
}

#[derive(Clone, Copy)]
struct XhciController {
    pci: PciDevice,
    mmio_phys: u64,
    mmio_virt: usize,
    cap_length: usize,
    max_slots: u8,
    max_ports: u8,
    doorbell_offset: u32,
    runtime_offset: u32,
}

#[derive(Clone, Copy)]
struct DmaPage {
    phys: u64,
    virt: usize,
}

#[derive(Clone, Copy)]
struct XhciRings {
    dcbaa: DmaPage,
    command_ring: DmaPage,
    event_ring: DmaPage,
    event_ring_segment_table: DmaPage,
}

#[derive(Clone, Copy)]
#[repr(C)]
struct Trb {
    parameter: u64,
    status: u32,
    control: u32,
}

pub fn init() {
    let mut found = 0usize;
    let mut initialized = 0usize;
    let mut connected_ports = 0usize;
    XHCI_LAST_ERROR.store(0, Ordering::Relaxed);

    pci::scan(|dev| {
        if found >= MAX_CONTROLLERS {
            return;
        }

        if dev.class_code != 0x0C || dev.subclass != 0x03 || dev.prog_if != XHCI_PROG_IF {
            return;
        }

        found += 1;
        serial_write("[USB:xHCI] controller ");
        write_pci_addr(dev);
        serial_write(" vendor=");
        write_hex(dev.vendor_id as u64, 4);
        serial_write(" device=");
        write_hex(dev.device_id as u64, 4);
        serial_write("\r\n");

        match bring_up_controller(dev) {
            Ok(controller) => {
                initialized += 1;
                log_controller(controller);
                connected_ports += log_ports(controller);
                match setup_rings_and_enable_slot(controller) {
                    Ok(slot_id) => {
                        serial_write("[USB:xHCI] Enable Slot completed slot=");
                        write_dec(slot_id as usize);
                        serial_write("\r\n");
                    }
                    Err(error) => {
                        serial_write("[USB:xHCI] command path failed: ");
                        serial_write(error.as_str());
                        serial_write("\r\n");
                    }
                }
            }
            Err(error) => {
                XHCI_LAST_ERROR.store(error.code(), Ordering::Relaxed);
                serial_write("[USB:xHCI] init failed: ");
                serial_write(error.as_str());
                serial_write("\r\n");
            }
        }
    });

    serial_write("[USB:xHCI] controllers found=");
    write_dec(found);
    serial_write(" initialized=");
    write_dec(initialized);
    serial_write("\r\n");

    XHCI_FOUND.store(found, Ordering::Relaxed);
    XHCI_INITIALIZED.store(initialized, Ordering::Relaxed);
    XHCI_CONNECTED_PORTS.store(connected_ports, Ordering::Relaxed);
}

pub fn status() -> XhciStatus {
    XhciStatus {
        found: XHCI_FOUND.load(Ordering::Relaxed),
        initialized: XHCI_INITIALIZED.load(Ordering::Relaxed),
        connected_ports: XHCI_CONNECTED_PORTS.load(Ordering::Relaxed),
        last_error: XhciError::from_code(XHCI_LAST_ERROR.load(Ordering::Relaxed)),
    }
}

fn bring_up_controller(dev: PciDevice) -> Result<XhciController, XhciError> {
    pci::enable_mmio_bus_master(dev);

    let mmio_phys = find_xhci_mmio_bar(dev)?;

    let mmio_virt =
        vmm::map_mmio_region(mmio_phys as usize, XHCI_MMIO_MAP_SIZE).ok_or(XhciError::MmioMap)?;
    let cap_length = read8(mmio_virt, CAP_CAPLENGTH) as usize;
    if cap_length < 0x20 || cap_length > 0x100 {
        return Err(XhciError::BadCapabilityLength);
    }

    let raw_version = read16(mmio_virt, CAP_HCIVERSION);
    let version = normalize_hci_version(raw_version);
    let hcsparams1 = read32(mmio_virt, CAP_HCSPARAMS1);
    let hcsparams2 = read32(mmio_virt, CAP_HCSPARAMS2);
    let hccparams1 = read32(mmio_virt, CAP_HCCPARAMS1);
    let max_slots = (hcsparams1 & 0xFF) as u8;
    let max_ports = ((hcsparams1 >> 24) & 0xFF) as u8;
    if version < 0x0090 && !hcsparams_look_valid(hcsparams1) {
        serial_write("[USB:xHCI] unsupported version raw=");
        write_hex(raw_version as u64, 4);
        serial_write(" normalized=");
        write_hex(version as u64, 4);
        serial_write("\r\n");
        return Err(XhciError::UnsupportedVersion);
    }
    if version < 0x0090 {
        serial_write("[USB:xHCI] version register is non-standard raw=");
        write_hex(raw_version as u64, 4);
        serial_write("; continuing because HCSPARAMS1 is plausible\r\n");
    }

    let doorbell_offset = read32(mmio_virt, CAP_DBOFF) & !0x3;
    let runtime_offset = read32(mmio_virt, CAP_RTSOFF) & !0x1F;

    serial_write("[USB:xHCI] version=");
    write_hex(version as u64, 4);
    if raw_version != version {
        serial_write(" raw=");
        write_hex(raw_version as u64, 4);
    }
    serial_write(" hcs1=");
    write_hex(hcsparams1 as u64, 8);
    serial_write(" hcs2=");
    write_hex(hcsparams2 as u64, 8);
    serial_write(" hcc1=");
    write_hex(hccparams1 as u64, 8);
    serial_write("\r\n");

    let op = mmio_virt + cap_length;
    halt_controller(op)?;
    reset_controller(op)?;

    if max_slots != 0 {
        write32(op, OP_CONFIG, max_slots as u32);
    }

    Ok(XhciController {
        pci: dev,
        mmio_phys,
        mmio_virt,
        cap_length,
        max_slots,
        max_ports,
        doorbell_offset,
        runtime_offset,
    })
}

fn find_xhci_mmio_bar(dev: PciDevice) -> Result<u64, XhciError> {
    let mut index = 0u8;
    while index < 6 {
        let raw = pci::read_bar(dev.bus, dev.device, dev.function, index);
        let decoded = pci::read_bar_decoded(dev, index);
        serial_write("[USB:xHCI] BAR");
        write_dec(index as usize);
        serial_write(" raw=");
        write_hex(raw as u64, 8);
        serial_write(" ");

        match decoded {
            PciBar::Memory64(addr) if addr != 0 => {
                serial_write("mem64=");
                write_hex(addr, 16);
                serial_write("\r\n");
                if mmio_bar_looks_like_xhci(addr) {
                    return Ok(addr);
                }
                index += 2;
                continue;
            }
            PciBar::Memory32(addr) if addr != 0 => {
                serial_write("mem32=");
                write_hex(addr as u64, 8);
                serial_write("\r\n");
                if mmio_bar_looks_like_xhci(addr as u64) {
                    return Ok(addr as u64);
                }
            }
            PciBar::Io(addr) => {
                serial_write("io=");
                write_hex(addr as u64, 8);
                serial_write("\r\n");
            }
            PciBar::None | PciBar::Memory32(_) | PciBar::Memory64(_) => {
                serial_write("none\r\n");
            }
        }

        index += 1;
    }

    Err(XhciError::NoMmioBar)
}

fn mmio_bar_looks_like_xhci(phys: u64) -> bool {
    let Some(virt) = vmm::map_mmio_region(phys as usize, 0x1000) else {
        return false;
    };
    let cap_length = read8(virt, CAP_CAPLENGTH);
    let raw_version = read16(virt, CAP_HCIVERSION);
    let version = normalize_hci_version(raw_version);
    serial_write("[USB:xHCI] BAR probe cap=");
    write_hex(cap_length as u64, 2);
    serial_write(" version=");
    write_hex(raw_version as u64, 4);
    if raw_version != version {
        serial_write(" normalized=");
        write_hex(version as u64, 4);
    }
    let hcsparams1 = read32(virt, CAP_HCSPARAMS1);
    serial_write(" hcs1=");
    write_hex(hcsparams1 as u64, 8);
    serial_write("\r\n");

    (cap_length as usize) >= 0x20
        && (cap_length as usize) <= 0x100
        && (version >= 0x0090 || hcsparams_look_valid(hcsparams1))
}

fn hcsparams_look_valid(hcsparams1: u32) -> bool {
    let max_slots = hcsparams1 & 0xFF;
    let max_ports = (hcsparams1 >> 24) & 0xFF;
    max_slots > 0 && max_ports > 0 && max_ports <= 0x7F
}

fn normalize_hci_version(raw: u16) -> u16 {
    if raw < 0x0090 {
        let swapped = raw.swap_bytes();
        if swapped >= 0x0090 {
            return swapped;
        }
    }
    raw
}

fn halt_controller(op: usize) -> Result<(), XhciError> {
    let command = read32(op, OP_USBCMD);
    if (command & USBCMD_RUN_STOP) != 0 {
        write32(op, OP_USBCMD, command & !USBCMD_RUN_STOP);
    }

    wait_until(
        || (read32(op, OP_USBSTS) & USBSTS_HOST_CONTROLLER_HALTED) != 0,
        XhciError::HaltTimeout,
    )
}

fn reset_controller(op: usize) -> Result<(), XhciError> {
    let command = read32(op, OP_USBCMD);
    write32(op, OP_USBCMD, command | USBCMD_HOST_CONTROLLER_RESET);

    wait_until(
        || (read32(op, OP_USBCMD) & USBCMD_HOST_CONTROLLER_RESET) == 0,
        XhciError::ResetTimeout,
    )?;
    wait_until(
        || (read32(op, OP_USBSTS) & USBSTS_CONTROLLER_NOT_READY) == 0,
        XhciError::NotReadyTimeout,
    )
}

fn log_controller(controller: XhciController) {
    let op = controller.mmio_virt + controller.cap_length;
    serial_write("[USB:xHCI] ready ");
    write_pci_addr(controller.pci);
    serial_write(" mmio=");
    write_hex(controller.mmio_phys, 16);
    serial_write(" cap=");
    write_dec(controller.cap_length);
    serial_write(" slots=");
    write_dec(controller.max_slots as usize);
    serial_write(" ports=");
    write_dec(controller.max_ports as usize);
    serial_write(" pagesize=");
    write_hex(read32(op, OP_PAGESIZE) as u64, 8);
    serial_write(" dboff=");
    write_hex(controller.doorbell_offset as u64, 8);
    serial_write(" rtsoff=");
    write_hex(controller.runtime_offset as u64, 8);
    serial_write("\r\n");
}

fn log_ports(controller: XhciController) -> usize {
    let op = controller.mmio_virt + controller.cap_length;
    let ports = (controller.max_ports as usize).min(32);

    let mut connected = 0usize;
    let mut port_index = 0usize;
    while port_index < ports {
        let portsc_offset = OP_PORTS_BASE + port_index * OP_PORT_STRIDE;
        let mut portsc = read32(op, portsc_offset);

        if (portsc & PORTSC_PORT_POWER) == 0 {
            write32(
                op,
                portsc_offset,
                (portsc & !PORTSC_CHANGE_BITS) | PORTSC_PORT_POWER,
            );
            portsc = read32(op, portsc_offset);
        }

        if (portsc & PORTSC_CURRENT_CONNECT_STATUS) != 0 {
            connected += 1;
            serial_write("[USB:xHCI] port ");
            write_dec(port_index + 1);
            serial_write(" connected enabled=");
            serial_write(if (portsc & PORTSC_PORT_ENABLED) != 0 {
                "yes"
            } else {
                "no"
            });
            serial_write(" speed=");
            write_dec(((portsc >> 10) & 0xF) as usize);
            serial_write(" portsc=");
            write_hex(portsc as u64, 8);
            serial_write("\r\n");
        }

        port_index += 1;
    }

    serial_write("[USB:xHCI] connected ports=");
    write_dec(connected);
    serial_write("\r\n");
    connected
}

fn setup_rings_and_enable_slot(controller: XhciController) -> Result<u8, XhciError> {
    let rings = allocate_rings()?;
    initialize_rings(rings);

    let op = controller.mmio_virt + controller.cap_length;
    let runtime = controller.mmio_virt + controller.runtime_offset as usize;
    let doorbells = controller.mmio_virt + controller.doorbell_offset as usize;

    write64(op, OP_DCBAAP, rings.dcbaa.phys);
    write64(op, OP_CRCR, rings.command_ring.phys | 1);

    let interrupter = runtime + 0x20;
    write32(interrupter, 0x00, read32(interrupter, 0x00) | 0x3);
    write32(interrupter, 0x08, 1);
    write64(interrupter, 0x10, rings.event_ring_segment_table.phys);
    write64(interrupter, 0x18, rings.event_ring.phys | (1 << 3));

    let command = read32(op, OP_USBCMD);
    write32(op, OP_USBCMD, command | USBCMD_RUN_STOP);
    wait_until(
        || (read32(op, OP_USBSTS) & USBSTS_HOST_CONTROLLER_HALTED) == 0,
        XhciError::RunTimeout,
    )?;

    write_command_trb(
        rings.command_ring,
        0,
        Trb {
            parameter: 0,
            status: 0,
            control: trb_type(TRB_TYPE_ENABLE_SLOT_COMMAND) | TRB_CYCLE,
        },
    );
    ring_doorbell(doorbells, 0, 0);

    poll_enable_slot_completion(rings.event_ring)
}

fn allocate_rings() -> Result<XhciRings, XhciError> {
    Ok(XhciRings {
        dcbaa: alloc_dma_page()?,
        command_ring: alloc_dma_page()?,
        event_ring: alloc_dma_page()?,
        event_ring_segment_table: alloc_dma_page()?,
    })
}

fn initialize_rings(rings: XhciRings) {
    zero_page(rings.dcbaa);
    zero_page(rings.command_ring);
    zero_page(rings.event_ring);
    zero_page(rings.event_ring_segment_table);

    write_command_trb(
        rings.command_ring,
        COMMAND_RING_TRBS - 1,
        Trb {
            parameter: rings.command_ring.phys,
            status: 0,
            control: trb_type(TRB_TYPE_LINK) | TRB_CYCLE | TRB_LINK_TOGGLE_CYCLE,
        },
    );

    write64(
        rings.event_ring_segment_table.virt,
        0,
        rings.event_ring.phys,
    );
    write32(
        rings.event_ring_segment_table.virt,
        8,
        EVENT_RING_TRBS as u32,
    );
    write32(rings.event_ring_segment_table.virt, 12, 0);
}

fn alloc_dma_page() -> Result<DmaPage, XhciError> {
    let pmm = get_pmm().ok_or(XhciError::DmaAlloc)?;
    let PhysicalAddress(phys) = pmm.alloc_frame().ok_or(XhciError::DmaAlloc)?;
    let virt = vmm::phys_to_virt(phys);
    let page = DmaPage {
        phys: phys as u64,
        virt,
    };
    zero_page(page);
    Ok(page)
}

fn zero_page(page: DmaPage) {
    unsafe {
        core::ptr::write_bytes(page.virt as *mut u8, 0, PAGE_SIZE);
    }
}

fn write_command_trb(page: DmaPage, index: usize, trb: Trb) {
    let ptr = (page.virt + index * TRB_SIZE) as *mut Trb;
    unsafe {
        core::ptr::write_volatile(ptr, trb);
    }
}

fn read_event_trb(page: DmaPage, index: usize) -> Trb {
    let ptr = (page.virt + index * TRB_SIZE) as *const Trb;
    unsafe { core::ptr::read_volatile(ptr) }
}

fn poll_enable_slot_completion(event_ring: DmaPage) -> Result<u8, XhciError> {
    let mut spins = 0usize;
    while spins < TIMEOUT_SPINS {
        let event = read_event_trb(event_ring, 0);
        if (event.control & TRB_CYCLE) != 0 {
            let event_type = (event.control >> 10) & 0x3F;
            let completion_code = ((event.status >> 24) & 0xFF) as u8;
            let slot_id = ((event.control >> 24) & 0xFF) as u8;
            serial_write("[USB:xHCI] event type=");
            write_dec(event_type as usize);
            serial_write(" code=");
            write_dec(completion_code as usize);
            serial_write(" slot=");
            write_dec(slot_id as usize);
            serial_write("\r\n");

            if event_type == TRB_TYPE_COMMAND_COMPLETION_EVENT && completion_code == 1 {
                return Ok(slot_id);
            }
            return Err(XhciError::CommandFailed);
        }
        unsafe {
            core::arch::asm!("pause", options(nomem, nostack, preserves_flags));
        }
        spins += 1;
    }
    Err(XhciError::CommandTimeout)
}

fn trb_type(trb_type: u32) -> u32 {
    trb_type << 10
}

fn ring_doorbell(doorbells: usize, target: usize, value: u32) {
    write32(doorbells, target * 4, value);
}

fn wait_until<F: Fn() -> bool>(condition: F, timeout_error: XhciError) -> Result<(), XhciError> {
    let mut spins = 0usize;
    while spins < TIMEOUT_SPINS {
        if condition() {
            return Ok(());
        }
        unsafe {
            core::arch::asm!("pause", options(nomem, nostack, preserves_flags));
        }
        spins += 1;
    }
    Err(timeout_error)
}

fn read8(base: usize, offset: usize) -> u8 {
    unsafe { core::ptr::read_volatile((base + offset) as *const u8) }
}

fn read16(base: usize, offset: usize) -> u16 {
    unsafe { core::ptr::read_volatile((base + offset) as *const u16) }
}

fn read32(base: usize, offset: usize) -> u32 {
    unsafe { core::ptr::read_volatile((base + offset) as *const u32) }
}

fn write32(base: usize, offset: usize, value: u32) {
    unsafe {
        core::ptr::write_volatile((base + offset) as *mut u32, value);
    }
}

fn write64(base: usize, offset: usize, value: u64) {
    unsafe {
        core::ptr::write_volatile((base + offset) as *mut u64, value);
    }
}

fn write_pci_addr(dev: PciDevice) {
    write_hex(dev.bus as u64, 2);
    serial_write(":");
    write_hex(dev.device as u64, 2);
    serial_write(".");
    write_hex(dev.function as u64, 1);
}

fn write_hex(mut value: u64, digits: usize) {
    let mut buf = [0u8; 16];
    let mut index = digits.min(buf.len());
    while index > 0 {
        index -= 1;
        let nibble = (value & 0xF) as u8;
        buf[index] = if nibble < 10 {
            b'0' + nibble
        } else {
            b'a' + nibble - 10
        };
        value >>= 4;
    }
    if let Ok(text) = core::str::from_utf8(&buf[..digits.min(buf.len())]) {
        serial_write(text);
    }
}

fn write_dec(mut value: usize) {
    let mut buf = [0u8; 20];
    let mut index = buf.len();
    if value == 0 {
        serial_write("0");
        return;
    }
    while value > 0 {
        index -= 1;
        buf[index] = b'0' + (value % 10) as u8;
        value /= 10;
    }
    if let Ok(text) = core::str::from_utf8(&buf[index..]) {
        serial_write(text);
    }
}

#[derive(Clone, Copy)]
pub enum XhciError {
    NoMmioBar,
    MmioMap,
    BadCapabilityLength,
    UnsupportedVersion,
    DmaAlloc,
    HaltTimeout,
    ResetTimeout,
    NotReadyTimeout,
    RunTimeout,
    CommandTimeout,
    CommandFailed,
}

impl XhciError {
    pub fn as_str(self) -> &'static str {
        match self {
            XhciError::NoMmioBar => "missing MMIO BAR",
            XhciError::MmioMap => "MMIO map failed",
            XhciError::BadCapabilityLength => "bad capability length",
            XhciError::UnsupportedVersion => "unsupported xHCI version",
            XhciError::DmaAlloc => "DMA allocation failed",
            XhciError::HaltTimeout => "halt timeout",
            XhciError::ResetTimeout => "reset timeout",
            XhciError::NotReadyTimeout => "controller not ready timeout",
            XhciError::RunTimeout => "run timeout",
            XhciError::CommandTimeout => "command timeout",
            XhciError::CommandFailed => "command failed",
        }
    }

    fn code(self) -> usize {
        match self {
            XhciError::NoMmioBar => 1,
            XhciError::MmioMap => 2,
            XhciError::BadCapabilityLength => 3,
            XhciError::UnsupportedVersion => 4,
            XhciError::HaltTimeout => 5,
            XhciError::ResetTimeout => 6,
            XhciError::NotReadyTimeout => 7,
            XhciError::DmaAlloc => 8,
            XhciError::RunTimeout => 9,
            XhciError::CommandTimeout => 10,
            XhciError::CommandFailed => 11,
        }
    }

    fn from_code(code: usize) -> Option<Self> {
        match code {
            1 => Some(XhciError::NoMmioBar),
            2 => Some(XhciError::MmioMap),
            3 => Some(XhciError::BadCapabilityLength),
            4 => Some(XhciError::UnsupportedVersion),
            5 => Some(XhciError::HaltTimeout),
            6 => Some(XhciError::ResetTimeout),
            7 => Some(XhciError::NotReadyTimeout),
            8 => Some(XhciError::DmaAlloc),
            9 => Some(XhciError::RunTimeout),
            10 => Some(XhciError::CommandTimeout),
            11 => Some(XhciError::CommandFailed),
            _ => None,
        }
    }
}
