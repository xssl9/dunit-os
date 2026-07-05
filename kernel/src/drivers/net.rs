use crate::drivers::pci::PciDevice;
use crate::drivers::registry::{self, DeviceClass};
use crate::serial_write;

const PCI_CLASS_NETWORK: u8 = 0x02;
const VENDOR_INTEL: u16 = 0x8086;
const VENDOR_REALTEK: u16 = 0x10ec;
const VENDOR_REDHAT: u16 = 0x1af4;

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
}

static mut NET_SNAPSHOT: NetSnapshot = NetSnapshot {
    total_nics: 0,
    supported_nics: 0,
};

pub fn init() {
    let mut index = 0usize;
    let mut total = 0usize;
    let mut supported = 0usize;

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
        serial_write(" packet-io=not-implemented\r\n");

        index += 1;
    });

    unsafe {
        NET_SNAPSHOT = NetSnapshot {
            total_nics: total,
            supported_nics: supported,
        };
    }

    if total == 0 {
        serial_write("[NET] no PCI network controller detected\r\n");
    } else {
        serial_write("[NET] discovery ready nics=");
        write_dec(total);
        serial_write(" supported=");
        write_dec(supported);
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
