use crate::serial::{write_dec, write_hex, write_hex16, write_hex8};
use crate::{hal, serial_write};
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, Ordering};

const PCI_CONFIG_ADDRESS: u16 = 0xCF8;
const PCI_CONFIG_DATA: u16 = 0xCFC;
const INVALID_VENDOR: u16 = 0xFFFF;
const MAX_SNAPSHOT_DEVICES: usize = 64;
const PCI_STATUS_CAPABILITIES: u16 = 1 << 4;
const CAP_ID_MSI: u8 = 0x05;
const CAP_ID_MSIX: u8 = 0x11;

const COMMAND_IO_SPACE: u16 = 1 << 0;
const COMMAND_MEMORY_SPACE: u16 = 1 << 1;
const COMMAND_BUS_MASTER: u16 = 1 << 2;

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
    pub header_type: u8,
    pub interrupt_line: u8,
    pub interrupt_pin: u8,
    pub capabilities: PciCapabilities,
}

#[derive(Clone, Copy)]
pub struct PciCapabilities {
    pub first_pointer: u8,
    pub count: u8,
    pub has_msi: bool,
    pub has_msix: bool,
}

#[derive(Clone, Copy)]
pub struct PciSnapshot {
    pub devices: [Option<PciDevice>; MAX_SNAPSHOT_DEVICES],
    pub total_devices: usize,
    pub stored_devices: usize,
    pub usb_controllers: usize,
    pub network_controllers: usize,
    pub msi_devices: usize,
    pub msix_devices: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PciBar {
    None,
    Io(u32),
    Memory32(u32),
    Memory64(u64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PciBarInfo {
    pub bar: PciBar,
    pub size: u64,
}

static PCI_LOCK: AtomicBool = AtomicBool::new(false);
static PCI_SCAN_READY: AtomicBool = AtomicBool::new(false);
/// Кэш результатов сканирования PCI. Был массивом + шестью счётчиками в
/// `static mut`; теперь одна структура за `UnsafeCell`, доступ к которой всегда
/// под `PCI_LOCK`.
struct PciCache {
    devices: [Option<PciDevice>; MAX_SNAPSHOT_DEVICES],
    total_devices: usize,
    stored_devices: usize,
    usb_controllers: usize,
    network_controllers: usize,
    msi_devices: usize,
    msix_devices: usize,
}

struct PciCacheCell(UnsafeCell<PciCache>);
unsafe impl Sync for PciCacheCell {}

static PCI_CACHE: PciCacheCell = PciCacheCell(UnsafeCell::new(PciCache {
    devices: [None; MAX_SNAPSHOT_DEVICES],
    total_devices: 0,
    stored_devices: 0,
    usb_controllers: 0,
    network_controllers: 0,
    msi_devices: 0,
    msix_devices: 0,
}));

pub fn init() {
    crate::drivers::registry::register("pci", crate::drivers::registry::DeviceClass::Bus, "pci");
    refresh_cache();

    for_each_device(|dev| {
        if dev.class_code == 0x0C && dev.subclass == 0x03 {
            serial_write("[PCI] USB controller ");
            write_pci_addr(dev);
            serial_write(" prog_if=");
            write_hex8(dev.prog_if);
            serial_write(" vendor=");
            write_hex16(dev.vendor_id);
            serial_write(" device=");
            write_hex16(dev.device_id);
            serial_write("\r\n");
        }
    });

    let snapshot = snapshot();
    serial_write("[PCI] devices detected=");
    write_dec(snapshot.total_devices as u64);
    serial_write(" usb=");
    write_dec(snapshot.usb_controllers as u64);
    serial_write(" net=");
    write_dec(snapshot.network_controllers as u64);
    serial_write(" msi=");
    write_dec(snapshot.msi_devices as u64);
    serial_write(" msix=");
    write_dec(snapshot.msix_devices as u64);
    serial_write("\r\n");
}

pub fn snapshot() -> PciSnapshot {
    let mut snapshot = empty_snapshot();

    if !PCI_SCAN_READY.load(Ordering::Acquire) {
        return scan_raw_into_snapshot();
    }

    lock_cache();
    unsafe {
        let cache = &*PCI_CACHE.0.get();
        snapshot.total_devices = cache.total_devices;
        snapshot.stored_devices = cache.stored_devices;
        snapshot.usb_controllers = cache.usb_controllers;
        snapshot.network_controllers = cache.network_controllers;
        snapshot.msi_devices = cache.msi_devices;
        snapshot.msix_devices = cache.msix_devices;

        let mut index = 0usize;
        while index < cache.stored_devices && index < snapshot.devices.len() {
            snapshot.devices[index] = cache.devices[index];
            index += 1;
        }
    }
    PCI_LOCK.store(false, Ordering::Release);

    snapshot
}

pub fn refresh_cache() {
    let snapshot = scan_raw_into_snapshot();

    lock_cache();
    unsafe {
        let cache = &mut *PCI_CACHE.0.get();
        cache.devices = snapshot.devices;
        cache.total_devices = snapshot.total_devices;
        cache.stored_devices = snapshot.stored_devices;
        cache.usb_controllers = snapshot.usb_controllers;
        cache.network_controllers = snapshot.network_controllers;
        cache.msi_devices = snapshot.msi_devices;
        cache.msix_devices = snapshot.msix_devices;
    }
    PCI_SCAN_READY.store(true, Ordering::Release);
    PCI_LOCK.store(false, Ordering::Release);
}

pub fn for_each_device<F: FnMut(PciDevice)>(mut visitor: F) {
    if !PCI_SCAN_READY.load(Ordering::Acquire) {
        scan_raw(visitor);
        return;
    }

    let snapshot = snapshot();
    for entry in snapshot.devices.iter().take(snapshot.stored_devices) {
        if let Some(dev) = entry {
            visitor(*dev);
        }
    }
}

pub fn scan<F: FnMut(PciDevice)>(mut visitor: F) {
    for_each_device(|dev| visitor(dev));
}

pub fn read_bar(bus: u8, device: u8, function: u8, index: u8) -> u32 {
    if index >= 6 {
        return 0;
    }
    read_config(bus, device, function, 0x10 + index * 4)
}

pub fn read_bar_decoded(dev: PciDevice, index: u8) -> PciBar {
    let raw = read_bar(dev.bus, dev.device, dev.function, index);
    if raw == 0 {
        return PciBar::None;
    }

    if (raw & 0x1) != 0 {
        return PciBar::Io(raw & !0x3);
    }

    match raw & 0x6 {
        0x4 if index < 5 => {
            let high = read_bar(dev.bus, dev.device, dev.function, index + 1) as u64;
            PciBar::Memory64((high << 32) | ((raw & !0xF) as u64))
        }
        _ => PciBar::Memory32(raw & !0xF),
    }
}

pub fn read_bar_info(dev: PciDevice, index: u8) -> PciBarInfo {
    let bar = read_bar_decoded(dev, index);
    let size = match bar {
        PciBar::None => 0,
        PciBar::Io(_) => size_bar32(dev, index, 0xFFFF_FFFC) as u64,
        PciBar::Memory32(_) => size_bar32(dev, index, 0xFFFF_FFF0) as u64,
        PciBar::Memory64(_) if index < 5 => size_bar64(dev, index),
        PciBar::Memory64(_) => 0,
    };

    PciBarInfo { bar, size }
}

pub fn command(dev: PciDevice) -> u16 {
    (read_config(dev.bus, dev.device, dev.function, 0x04) & 0xFFFF) as u16
}

pub fn set_command(dev: PciDevice, value: u16) {
    let current = read_config(dev.bus, dev.device, dev.function, 0x04);
    let next = (current & 0xFFFF_0000) | value as u32;
    write_config(dev.bus, dev.device, dev.function, 0x04, next);
}

pub fn enable_mmio_bus_master(dev: PciDevice) {
    set_command(
        dev,
        command(dev) | COMMAND_MEMORY_SPACE | COMMAND_BUS_MASTER,
    );
}

pub fn enable_io_bus_master(dev: PciDevice) {
    set_command(dev, command(dev) | COMMAND_IO_SPACE | COMMAND_BUS_MASTER);
}

fn scan_raw_into_snapshot() -> PciSnapshot {
    let mut snapshot = empty_snapshot();
    scan_raw(|dev| {
        if snapshot.stored_devices < snapshot.devices.len() {
            snapshot.devices[snapshot.stored_devices] = Some(dev);
            snapshot.stored_devices += 1;
        }
        snapshot.total_devices += 1;
        if dev.class_code == 0x0C && dev.subclass == 0x03 {
            snapshot.usb_controllers += 1;
        }
        if dev.class_code == 0x02 {
            snapshot.network_controllers += 1;
        }
        if dev.capabilities.has_msi {
            snapshot.msi_devices += 1;
        }
        if dev.capabilities.has_msix {
            snapshot.msix_devices += 1;
        }
    });
    snapshot
}

fn scan_raw<F: FnMut(PciDevice)>(mut visitor: F) {
    let mut bus = 0u16;
    while bus <= 255 {
        let mut device = 0u8;
        while device < 32 {
            let ids = read_config(bus as u8, device, 0, 0x00);
            if (ids & 0xFFFF) as u16 == INVALID_VENDOR {
                device += 1;
                continue;
            }

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

fn read_device(bus: u8, device: u8, function: u8) -> Option<PciDevice> {
    let ids = read_config(bus, device, function, 0x00);
    let vendor_id = (ids & 0xFFFF) as u16;
    if vendor_id == INVALID_VENDOR {
        return None;
    }

    let class = read_config(bus, device, function, 0x08);
    let header = read_config(bus, device, function, 0x0C);
    let irq = read_config(bus, device, function, 0x3C);
    Some(PciDevice {
        bus,
        device,
        function,
        vendor_id,
        device_id: ((ids >> 16) & 0xFFFF) as u16,
        class_code: ((class >> 24) & 0xFF) as u8,
        subclass: ((class >> 16) & 0xFF) as u8,
        prog_if: ((class >> 8) & 0xFF) as u8,
        header_type: ((header >> 16) & 0xFF) as u8,
        interrupt_line: (irq & 0xFF) as u8,
        interrupt_pin: ((irq >> 8) & 0xFF) as u8,
        capabilities: read_capabilities(bus, device, function),
    })
}

fn read_capabilities(bus: u8, device: u8, function: u8) -> PciCapabilities {
    let status = ((read_config(bus, device, function, 0x04) >> 16) & 0xFFFF) as u16;
    if (status & PCI_STATUS_CAPABILITIES) == 0 {
        return empty_capabilities();
    }

    let first_pointer = (read_config(bus, device, function, 0x34) & 0xFC) as u8;
    let mut pointer = first_pointer;
    let mut count = 0u8;
    let mut has_msi = false;
    let mut has_msix = false;

    while pointer >= 0x40 && count < 48 {
        let cap = read_config(bus, device, function, pointer);
        let id = (cap & 0xFF) as u8;
        let next = ((cap >> 8) & 0xFC) as u8;

        if id == CAP_ID_MSI {
            has_msi = true;
        } else if id == CAP_ID_MSIX {
            has_msix = true;
        }

        count += 1;
        if next == 0 || next == pointer {
            break;
        }
        pointer = next;
    }

    PciCapabilities {
        first_pointer,
        count,
        has_msi,
        has_msix,
    }
}

fn size_bar32(dev: PciDevice, index: u8, mask: u32) -> u32 {
    let offset = 0x10 + index * 4;
    let original = read_config(dev.bus, dev.device, dev.function, offset);
    write_config(dev.bus, dev.device, dev.function, offset, 0xFFFF_FFFF);
    let probe = read_config(dev.bus, dev.device, dev.function, offset);
    write_config(dev.bus, dev.device, dev.function, offset, original);

    let masked = probe & mask;
    if masked == 0 {
        0
    } else {
        (!masked).wrapping_add(1)
    }
}

fn size_bar64(dev: PciDevice, index: u8) -> u64 {
    let low_offset = 0x10 + index * 4;
    let high_offset = low_offset + 4;
    let original_low = read_config(dev.bus, dev.device, dev.function, low_offset);
    let original_high = read_config(dev.bus, dev.device, dev.function, high_offset);

    write_config(dev.bus, dev.device, dev.function, low_offset, 0xFFFF_FFFF);
    write_config(dev.bus, dev.device, dev.function, high_offset, 0xFFFF_FFFF);
    let probe_low = read_config(dev.bus, dev.device, dev.function, low_offset);
    let probe_high = read_config(dev.bus, dev.device, dev.function, high_offset);
    write_config(
        dev.bus,
        dev.device,
        dev.function,
        high_offset,
        original_high,
    );
    write_config(dev.bus, dev.device, dev.function, low_offset, original_low);

    let masked = ((probe_high as u64) << 32) | ((probe_low & 0xFFFF_FFF0) as u64);
    if masked == 0 {
        0
    } else {
        (!masked).wrapping_add(1)
    }
}

pub fn read_config(bus: u8, device: u8, function: u8, offset: u8) -> u32 {
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

pub fn write_config(bus: u8, device: u8, function: u8, offset: u8, value: u32) {
    let address = 0x8000_0000u32
        | ((bus as u32) << 16)
        | ((device as u32) << 11)
        | ((function as u32) << 8)
        | ((offset as u32) & 0xFC);

    unsafe {
        hal::hal_outl(PCI_CONFIG_ADDRESS, address);
        hal::hal_outl(PCI_CONFIG_DATA, value);
    }
}

fn empty_snapshot() -> PciSnapshot {
    PciSnapshot {
        devices: [None; MAX_SNAPSHOT_DEVICES],
        total_devices: 0,
        stored_devices: 0,
        usb_controllers: 0,
        network_controllers: 0,
        msi_devices: 0,
        msix_devices: 0,
    }
}

fn empty_capabilities() -> PciCapabilities {
    PciCapabilities {
        first_pointer: 0,
        count: 0,
        has_msi: false,
        has_msix: false,
    }
}

fn lock_cache() {
    while PCI_LOCK
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        core::hint::spin_loop();
    }
}

fn write_pci_addr(dev: PciDevice) {
    write_hex8(dev.bus);
    serial_write(":");
    write_hex8(dev.device);
    serial_write(".");
    write_hex(dev.function as u64, 1);
}
