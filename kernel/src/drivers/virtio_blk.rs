use crate::drivers::block::{self, BlockDeviceInfo, BlockError};
use crate::drivers::pci::{self, PciBar};
use crate::hal;
use crate::memory::pmm::{get_pmm, PhysicalAddress};
use crate::memory::vmm;

const VIRTIO_VENDOR_ID: u16 = 0x1AF4;
const VIRTIO_BLK_LEGACY_DEVICE_ID: u16 = 0x1001;
const VD0_NAME: &str = "vd0";
const SECTOR_SIZE: usize = 512;
const MAX_QUEUE_PAGES: usize = 16;
const VRING_DESC_SIZE: usize = 16;
const VRING_DESC_F_NEXT: u16 = 1;
const VRING_DESC_F_WRITE: u16 = 2;
const VIRTIO_BLK_T_IN: u32 = 0;
const VIRTIO_BLK_T_OUT: u32 = 1;

const REG_DEVICE_FEATURES: u16 = 0;
const REG_GUEST_FEATURES: u16 = 4;
const REG_QUEUE_PFN: u16 = 8;
const REG_QUEUE_SIZE: u16 = 12;
const REG_QUEUE_SELECT: u16 = 14;
const REG_QUEUE_NOTIFY: u16 = 16;
const REG_DEVICE_STATUS: u16 = 18;
const REG_CONFIG: u16 = 20;

const STATUS_ACKNOWLEDGE: u8 = 1;
const STATUS_DRIVER: u8 = 2;
const STATUS_DRIVER_OK: u8 = 4;
const STATUS_FAILED: u8 = 128;

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct VirtioBlkReq {
    request_type: u32,
    reserved: u32,
    sector: u64,
}

#[derive(Clone, Copy)]
struct VirtioBlkDevice {
    io_base: u16,
    capacity_sectors: u64,
    queue_virt: usize,
    queue_size: usize,
    io_phys: usize,
    io_virt: usize,
    avail_idx: u16,
    used_idx: u16,
}

static mut VD0: Option<VirtioBlkDevice> = None;

pub fn init() {
    let Some(device) = find_legacy_virtio_blk() else {
        crate::serial_write("[VIRTIO-BLK] no legacy virtio-blk device\r\n");
        return;
    };

    match init_device(device) {
        Some(vd0) => unsafe {
            VD0 = Some(vd0);
            block::register_device(
                BlockDeviceInfo {
                    name: VD0_NAME,
                    driver: "virtio-blk",
                    block_size: SECTOR_SIZE,
                    blocks: vd0.capacity_sectors,
                    readonly: false,
                },
                vd0_read,
                vd0_write,
            );
            crate::serial_write("[VIRTIO-BLK] registered vd0 sectors=");
            write_dec(vd0.capacity_sectors);
            crate::serial_write("\r\n");
        },
        None => crate::serial_write("[VIRTIO-BLK] init failed\r\n"),
    }
}

fn find_legacy_virtio_blk() -> Option<pci::PciDevice> {
    let mut found = None;
    pci::for_each_device(|dev| {
        if found.is_none()
            && dev.vendor_id == VIRTIO_VENDOR_ID
            && dev.device_id == VIRTIO_BLK_LEGACY_DEVICE_ID
        {
            found = Some(dev);
        }
    });
    found
}

fn init_device(dev: pci::PciDevice) -> Option<VirtioBlkDevice> {
    let io_base = match pci::read_bar_decoded(dev, 0) {
        PciBar::Io(port) => port as u16,
        _ => return None,
    };
    pci::enable_io_bus_master(dev);

    outb(io_base + REG_DEVICE_STATUS, 0);
    outb(io_base + REG_DEVICE_STATUS, STATUS_ACKNOWLEDGE);
    outb(io_base + REG_DEVICE_STATUS, STATUS_ACKNOWLEDGE | STATUS_DRIVER);

    let _device_features = inl(io_base + REG_DEVICE_FEATURES);
    outl(io_base + REG_GUEST_FEATURES, 0);

    outw(io_base + REG_QUEUE_SELECT, 0);
    let queue_size = inw(io_base + REG_QUEUE_SIZE) as usize;
    let queue_bytes = vring_bytes(queue_size);
    if queue_size < 3 || queue_bytes > MAX_QUEUE_PAGES * 4096 {
        crate::serial_write("[VIRTIO-BLK] unsupported queue size=");
        write_dec(queue_size as u64);
        crate::serial_write(" bytes=");
        write_dec(queue_bytes as u64);
        crate::serial_write("\r\n");
        fail_device(io_base);
        return None;
    }

    let (queue_phys, queue_virt) = alloc_contiguous_queue(queue_bytes)?;
    unsafe {
        core::ptr::write_bytes(queue_virt as *mut u8, 0, queue_bytes);
    }
    outl(io_base + REG_QUEUE_PFN, (queue_phys / 4096) as u32);

    let (io_phys, io_virt) = alloc_frame_pair()?;
    unsafe {
        core::ptr::write_bytes(io_virt as *mut u8, 0, 4096);
    }

    outb(
        io_base + REG_DEVICE_STATUS,
        STATUS_ACKNOWLEDGE | STATUS_DRIVER | STATUS_DRIVER_OK,
    );

    let capacity_low = inl(io_base + REG_CONFIG) as u64;
    let capacity_high = inl(io_base + REG_CONFIG + 4) as u64;
    let capacity = (capacity_high << 32) | capacity_low;
    if capacity == 0 {
        fail_device(io_base);
        return None;
    }

    Some(VirtioBlkDevice {
        io_base,
        capacity_sectors: capacity,
        queue_virt,
        queue_size,
        io_phys,
        io_virt,
        avail_idx: 0,
        used_idx: 0,
    })
}

fn vd0_read(lba: u64, buf: &mut [u8]) -> Result<usize, BlockError> {
    transfer(lba, buf, false)
}

fn vd0_write(lba: u64, buf: &[u8]) -> Result<usize, BlockError> {
    if buf.len() < SECTOR_SIZE {
        return Err(BlockError::BufferTooSmall);
    }
    let mut tmp = [0u8; SECTOR_SIZE];
    tmp.copy_from_slice(&buf[..SECTOR_SIZE]);
    transfer(lba, &mut tmp, true)
}

fn transfer(lba: u64, buf: &mut [u8], write: bool) -> Result<usize, BlockError> {
    if buf.len() < SECTOR_SIZE {
        return Err(BlockError::BufferTooSmall);
    }

    let dev = unsafe { VD0.as_mut().ok_or(BlockError::NotFound)? };
    if lba >= dev.capacity_sectors {
        return Err(BlockError::OutOfRange);
    }

    unsafe {
        let header_ptr = dev.io_virt as *mut VirtioBlkReq;
        (*header_ptr).request_type = if write {
            VIRTIO_BLK_T_OUT
        } else {
            VIRTIO_BLK_T_IN
        };
        (*header_ptr).reserved = 0;
        (*header_ptr).sector = lba;

        let data_virt = dev.io_virt + 64;
        let status_virt = dev.io_virt + 64 + SECTOR_SIZE;
        let status_ptr = status_virt as *mut u8;
        core::ptr::write_volatile(status_ptr, 0xFF);
        if write {
            core::ptr::copy_nonoverlapping(buf.as_ptr(), data_virt as *mut u8, SECTOR_SIZE);
        } else {
            core::ptr::write_bytes(data_virt as *mut u8, 0, SECTOR_SIZE);
        }

        write_desc(
            dev.queue_virt,
            0,
            dev.io_phys as u64,
            core::mem::size_of::<VirtioBlkReq>() as u32,
            VRING_DESC_F_NEXT,
            1,
        );
        write_desc(
            dev.queue_virt,
            1,
            (dev.io_phys + 64) as u64,
            SECTOR_SIZE as u32,
            if write {
                VRING_DESC_F_NEXT
            } else {
                VRING_DESC_F_NEXT | VRING_DESC_F_WRITE
            },
            2,
        );
        write_desc(
            dev.queue_virt,
            2,
            (dev.io_phys + 64 + SECTOR_SIZE) as u64,
            1,
            VRING_DESC_F_WRITE,
            0,
        );

        let avail = avail_offset(dev.queue_virt, dev.queue_size);
        let ring_slot = dev.avail_idx as usize % dev.queue_size;
        write_u16(avail + 4 + ring_slot * 2, 0);
        dev.avail_idx = dev.avail_idx.wrapping_add(1);
        write_u16(avail + 2, dev.avail_idx);

        outw(dev.io_base + REG_QUEUE_NOTIFY, 0);

        let used = used_offset(dev.queue_virt, dev.queue_size);
        let mut spins = 0usize;
        while read_u16(used + 2) == dev.used_idx && spins < 10_000_000 {
            core::hint::spin_loop();
            spins += 1;
        }
        if read_u16(used + 2) == dev.used_idx {
            return Err(BlockError::Io);
        }
        dev.used_idx = dev.used_idx.wrapping_add(1);

        if core::ptr::read_volatile(status_ptr) != 0 {
            return Err(BlockError::Io);
        }
        if !write {
            core::ptr::copy_nonoverlapping(data_virt as *const u8, buf.as_mut_ptr(), SECTOR_SIZE);
        }
    }

    Ok(SECTOR_SIZE)
}

fn write_desc(base: usize, index: usize, addr: u64, len: u32, flags: u16, next: u16) {
    let ptr = base + index * VRING_DESC_SIZE;
    unsafe {
        write_u64(ptr, addr);
        write_u32(ptr + 8, len);
        write_u16(ptr + 12, flags);
        write_u16(ptr + 14, next);
    }
}

fn avail_offset(base: usize, queue_size: usize) -> usize {
    base + queue_size * VRING_DESC_SIZE
}

fn used_offset(base: usize, queue_size: usize) -> usize {
    (base + queue_size * VRING_DESC_SIZE + 4 + queue_size * 2 + 2 + 4095) & !4095
}

fn vring_bytes(queue_size: usize) -> usize {
    let used = used_offset(0, queue_size);
    used + 4 + queue_size * 8 + 2
}

fn alloc_contiguous_queue(bytes: usize) -> Option<(usize, usize)> {
    let pmm = get_pmm()?;
    let pages = (bytes + 4095) / 4096;
    if pages == 0 || pages > MAX_QUEUE_PAGES {
        return None;
    }

    let mut attempts = 0usize;
    while attempts < 64 {
        let mut frames = [0usize; MAX_QUEUE_PAGES];
        let mut count = 0usize;
        while count < pages {
            let frame = pmm.alloc_frame()?;
            frames[count] = frame.as_usize();
            count += 1;
        }

        let first = frames[0];
        let mut contiguous = true;
        let mut index = 1usize;
        while index < pages {
            if frames[index] != first + index * 4096 {
                contiguous = false;
                break;
            }
            index += 1;
        }

        if contiguous {
            return Some((first, vmm::phys_to_virt(first)));
        }

        let mut free_index = 0usize;
        while free_index < pages {
            pmm.free_frame(PhysicalAddress::from_usize(frames[free_index]));
            free_index += 1;
        }
        attempts += 1;
    }
    None
}

fn alloc_frame_pair() -> Option<(usize, usize)> {
    let frame = get_pmm()?.alloc_frame()?;
    Some((frame.as_usize(), vmm::phys_to_virt(frame.as_usize())))
}

fn fail_device(io_base: u16) {
    outb(io_base + REG_DEVICE_STATUS, STATUS_FAILED);
}

fn inw(port: u16) -> u16 {
    unsafe { hal::hal_inw(port) }
}

fn inl(port: u16) -> u32 {
    unsafe { hal::hal_inl(port) }
}

fn outb(port: u16, value: u8) {
    unsafe { hal::hal_outb(port, value) }
}

fn outw(port: u16, value: u16) {
    unsafe { hal::hal_outw(port, value) }
}

fn outl(port: u16, value: u32) {
    unsafe { hal::hal_outl(port, value) }
}

unsafe fn read_u16(addr: usize) -> u16 {
    core::ptr::read_volatile(addr as *const u16)
}

unsafe fn write_u16(addr: usize, value: u16) {
    core::ptr::write_volatile(addr as *mut u16, value);
}

unsafe fn write_u32(addr: usize, value: u32) {
    core::ptr::write_volatile(addr as *mut u32, value);
}

unsafe fn write_u64(addr: usize, value: u64) {
    core::ptr::write_volatile(addr as *mut u64, value);
}

fn write_dec(mut value: u64) {
    let mut buf = [0u8; 20];
    let mut index = buf.len();
    if value == 0 {
        crate::serial_write("0");
        return;
    }
    while value > 0 {
        index -= 1;
        buf[index] = b'0' + (value % 10) as u8;
        value /= 10;
    }
    if let Ok(text) = core::str::from_utf8(&buf[index..]) {
        crate::serial_write(text);
    }
}
