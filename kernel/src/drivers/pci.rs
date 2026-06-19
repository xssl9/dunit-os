use crate::{hal, serial_write};

const PCI_CONFIG_ADDRESS: u16 = 0xCF8;
const PCI_CONFIG_DATA: u16 = 0xCFC;
const INVALID_VENDOR: u16 = 0xFFFF;

#[derive(Clone, Copy)]
pub struct PciDevice {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
    pub vendor_id: u16,
    pub device_id: u16,
    pub class_code: u8,
    pub subclass: u8,
    pub prog_if: u8,
}

pub fn init() {
    let mut usb_count = 0usize;
    scan(|dev| {
        if dev.class_code == 0x0C && dev.subclass == 0x03 {
            usb_count += 1;
            serial_write("[PCI] USB controller ");
            write_hex8(dev.bus);
            serial_write(":");
            write_hex8(dev.device);
            serial_write(".");
            write_hex8(dev.function);
            serial_write(" prog_if=");
            write_hex8(dev.prog_if);
            serial_write(" vendor=");
            write_hex16(dev.vendor_id);
            serial_write(" device=");
            write_hex16(dev.device_id);
            serial_write("\r\n");
        }
    });

    serial_write("[PCI] USB controllers detected=");
    write_dec(usb_count);
    serial_write("\r\n");
}

pub fn scan<F: FnMut(PciDevice)>(mut visitor: F) {
    let mut bus = 0u16;
    while bus <= 255 {
        let mut device = 0u8;
        while device < 32 {
            let header = read_config(bus as u8, device, 0, 0x0C);
            let multifunction = ((header >> 16) & 0x80) != 0;
            let max_function = if multifunction { 8 } else { 1 };
            let mut function = 0u8;
            while function < max_function {
                if let Some(dev) = read_device(bus as u8, device, function) {
                    visitor(dev);
                }
                function += 1;
            }
            device += 1;
        }
        bus += 1;
    }
}

pub fn read_bar(bus: u8, device: u8, function: u8, index: u8) -> u32 {
    if index >= 6 {
        return 0;
    }
    read_config(bus, device, function, 0x10 + index * 4)
}

fn read_device(bus: u8, device: u8, function: u8) -> Option<PciDevice> {
    let ids = read_config(bus, device, function, 0x00);
    let vendor_id = (ids & 0xFFFF) as u16;
    if vendor_id == INVALID_VENDOR {
        return None;
    }

    let class = read_config(bus, device, function, 0x08);
    Some(PciDevice {
        bus,
        device,
        function,
        vendor_id,
        device_id: ((ids >> 16) & 0xFFFF) as u16,
        class_code: ((class >> 24) & 0xFF) as u8,
        subclass: ((class >> 16) & 0xFF) as u8,
        prog_if: ((class >> 8) & 0xFF) as u8,
    })
}

fn read_config(bus: u8, device: u8, function: u8, offset: u8) -> u32 {
    let address = 0x8000_0000u32
        | ((bus as u32) << 16)
        | ((device as u32) << 11)
        | ((function as u32) << 8)
        | ((offset as u32) & 0xFC);

    unsafe {
        hal::hal_outl(PCI_CONFIG_ADDRESS, address);
        hal::hal_inl(PCI_CONFIG_DATA)
    }
}

fn write_hex8(value: u8) {
    write_hex(value as u64, 2);
}

fn write_hex16(value: u16) {
    write_hex(value as u64, 4);
}

fn write_hex(mut value: u64, digits: usize) {
    let mut buf = [0u8; 16];
    let mut index = digits;
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
    if let Ok(text) = core::str::from_utf8(&buf[..digits]) {
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
