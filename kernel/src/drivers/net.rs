use crate::drivers::pci::{self, PciBar, PciDevice};
use crate::drivers::registry::{self, DeviceClass};
use crate::memory::vmm;
use crate::serial_write;

const PCI_CLASS_NETWORK: u8 = 0x02;
const VENDOR_INTEL: u16 = 0x8086;
const VENDOR_REALTEK: u16 = 0x10ec;
const VENDOR_REDHAT: u16 = 0x1af4;
const E1000_MMIO_MAP_SIZE: usize = 0x20000;
const E1000_STATUS: usize = 0x0008;
const E1000_RAL0: usize = 0x5400;
const E1000_RAH0: usize = 0x5404;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NicKind {
    E1000,
    Rtl8139,
    VirtioNet,
    Unknown,
}

#[derive(Clone, Copy)]
pub struct NicInfo {
    pub pci: PciDevice,
    pub kind: NicKind,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NetSnapshot {
    pub total_nics: usize,
    pub supported_nics: usize,
    pub mmio_ready_nics: usize,
    pub mac_ready_nics: usize,
}

static mut NET_SNAPSHOT: NetSnapshot = NetSnapshot {
    total_nics: 0,
    supported_nics: 0,
    mmio_ready_nics: 0,
    mac_ready_nics: 0,
};

pub fn init() {
    let mut index = 0usize;
    let mut total = 0usize;
    let mut supported = 0usize;
    let mut mmio_ready = 0usize;
    let mut mac_ready = 0usize;

    crate::drivers::pci::scan(|dev| {
        if dev.class_code != PCI_CLASS_NETWORK {
            return;
        }

        total += 1;
        let kind = classify_nic(dev);
        if kind != NicKind::Unknown {
            supported += 1;
        }

        if index == 0 {
            registry::register("net0", DeviceClass::Network, driver_name(kind));
        }

        serial_write("[NET] pci nic ");
        write_pci_addr(dev);
        serial_write(" vendor=");
        write_hex16(dev.vendor_id);
        serial_write(" device=");
        write_hex16(dev.device_id);
        serial_write(" kind=");
        serial_write(kind_name(kind));
        serial_write("\r\n");

        match probe_nic(dev, kind) {
            Ok(probe) => {
                if probe.mmio_ready {
                    mmio_ready += 1;
                }
                if probe.mac_valid {
                    mac_ready += 1;
                }
            }
            Err(error) => {
                serial_write("[NET] probe failed kind=");
                serial_write(kind_name(kind));
                serial_write(" error=");
                serial_write(error.as_str());
                serial_write("\r\n");
            }
        }

        index += 1;
    });

    unsafe {
        NET_SNAPSHOT = NetSnapshot {
            total_nics: total,
            supported_nics: supported,
            mmio_ready_nics: mmio_ready,
            mac_ready_nics: mac_ready,
        };
    }

    if total == 0 {
        serial_write("[NET] no PCI network controller detected\r\n");
    } else {
        serial_write("[NET] discovery ready nics=");
        write_dec(total);
        serial_write(" supported=");
        write_dec(supported);
        serial_write(" mmio-ready=");
        write_dec(mmio_ready);
        serial_write(" mac-ready=");
        write_dec(mac_ready);
        serial_write(" stack=not-implemented\r\n");
    }
}

pub fn snapshot() -> NetSnapshot {
    unsafe { NET_SNAPSHOT }
}

fn classify_nic(dev: PciDevice) -> NicKind {
    match (dev.vendor_id, dev.device_id) {
        (VENDOR_INTEL, 0x100e | 0x100f | 0x10d3) => NicKind::E1000,
        (VENDOR_REALTEK, 0x8139) => NicKind::Rtl8139,
        (VENDOR_REDHAT, 0x1000) => NicKind::VirtioNet,
        _ => NicKind::Unknown,
    }
}

struct ProbeResult {
    mmio_ready: bool,
    mac_valid: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProbeError {
    Unsupported,
    NoMmioBar,
    MmioMap,
    InvalidRegisters,
}

impl ProbeError {
    fn as_str(self) -> &'static str {
        match self {
            Self::Unsupported => "unsupported",
            Self::NoMmioBar => "no-mmio-bar",
            Self::MmioMap => "mmio-map",
            Self::InvalidRegisters => "invalid-registers",
        }
    }
}

fn probe_nic(dev: PciDevice, kind: NicKind) -> Result<ProbeResult, ProbeError> {
    match kind {
        NicKind::E1000 => probe_e1000(dev),
        NicKind::Rtl8139 | NicKind::VirtioNet | NicKind::Unknown => Err(ProbeError::Unsupported),
    }
}

fn probe_e1000(dev: PciDevice) -> Result<ProbeResult, ProbeError> {
    pci::enable_mmio_bus_master(dev);
    let mmio_phys = find_mmio_bar(dev)?;
    let mmio_virt =
        vmm::map_mmio_region(mmio_phys as usize, E1000_MMIO_MAP_SIZE).ok_or(ProbeError::MmioMap)?;

    let status = read32(mmio_virt, E1000_STATUS);
    if status == 0 || status == 0xFFFF_FFFF {
        return Err(ProbeError::InvalidRegisters);
    }

    let ral = read32(mmio_virt, E1000_RAL0);
    let rah = read32(mmio_virt, E1000_RAH0);
    let mac_valid = (rah & (1 << 31)) != 0 && (ral != 0 || (rah & 0xFFFF) != 0);

    serial_write("[NET:e1000] mmio=");
    write_hex(mmio_phys, 8);
    serial_write(" status=");
    write_hex(status as u64, 8);
    serial_write(" mac=");
    write_mac(ral, rah);
    serial_write(" packet-io=not-implemented\r\n");

    Ok(ProbeResult {
        mmio_ready: true,
        mac_valid,
    })
}

fn find_mmio_bar(dev: PciDevice) -> Result<u64, ProbeError> {
    let mut index = 0u8;
    while index < 6 {
        match pci::read_bar_decoded(dev, index) {
            PciBar::Memory32(addr) if addr != 0 => return Ok(addr as u64),
            PciBar::Memory64(addr) if addr != 0 => return Ok(addr),
            PciBar::Memory64(_) => {
                index += 2;
                continue;
            }
            PciBar::None | PciBar::Io(_) | PciBar::Memory32(_) => {}
        }
        index += 1;
    }
    Err(ProbeError::NoMmioBar)
}

fn driver_name(kind: NicKind) -> &'static str {
    match kind {
        NicKind::E1000 => "e1000-discovery",
        NicKind::Rtl8139 => "rtl8139-discovery",
        NicKind::VirtioNet => "virtio-net-discovery",
        NicKind::Unknown => "net-discovery",
    }
}

fn kind_name(kind: NicKind) -> &'static str {
    match kind {
        NicKind::E1000 => "e1000",
        NicKind::Rtl8139 => "rtl8139",
        NicKind::VirtioNet => "virtio-net",
        NicKind::Unknown => "unknown",
    }
}

fn write_pci_addr(dev: PciDevice) {
    write_hex8(dev.bus);
    serial_write(":");
    write_hex8(dev.device);
    serial_write(".");
    write_hex(dev.function as u64, 1);
}

fn write_hex8(value: u8) {
    write_hex(value as u64, 2);
}

fn write_hex16(value: u16) {
    write_hex(value as u64, 4);
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

fn read32(base: usize, offset: usize) -> u32 {
    unsafe { core::ptr::read_volatile((base + offset) as *const u32) }
}

fn write_mac(ral: u32, rah: u32) {
    let bytes = [
        (ral & 0xFF) as u8,
        ((ral >> 8) & 0xFF) as u8,
        ((ral >> 16) & 0xFF) as u8,
        ((ral >> 24) & 0xFF) as u8,
        (rah & 0xFF) as u8,
        ((rah >> 8) & 0xFF) as u8,
    ];

    let mut index = 0usize;
    while index < bytes.len() {
        if index != 0 {
            serial_write(":");
        }
        write_hex(bytes[index] as u64, 2);
        index += 1;
    }
}
