use core::sync::atomic::{AtomicBool, Ordering};

const MAX_BLOCK_DEVICES: usize = 8;
pub const RAMBLK0_NAME: &str = "ramblk0";
const RAMBLK0_BLOCK_SIZE: usize = 512;
const RAMBLK0_BLOCKS: u64 = 8;
const RAMBLK0_BYTES: usize = RAMBLK0_BLOCK_SIZE * RAMBLK0_BLOCKS as usize;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockError {
    NotFound,
    OutOfRange,
    BufferTooSmall,
    Io,
}

#[derive(Clone, Copy)]
pub struct BlockDeviceInfo {
    pub name: &'static str,
    pub driver: &'static str,
    pub block_size: usize,
    pub blocks: u64,
    pub readonly: bool,
}

impl BlockDeviceInfo {
    pub const fn bytes(&self) -> u64 {
        self.block_size as u64 * self.blocks
    }
}

#[derive(Clone, Copy)]
struct BlockDeviceRegistration {
    info: BlockDeviceInfo,
    read_block: fn(u64, &mut [u8]) -> Result<usize, BlockError>,
    write_block: fn(u64, &[u8]) -> Result<usize, BlockError>,
}

static BLOCK_LOCK: AtomicBool = AtomicBool::new(false);
static mut BLOCK_DEVICES: [Option<BlockDeviceRegistration>; MAX_BLOCK_DEVICES] =
    [None; MAX_BLOCK_DEVICES];
static mut BLOCK_DEVICE_COUNT: usize = 0;
static mut RAMBLK0_DATA: [u8; RAMBLK0_BYTES] = [0; RAMBLK0_BYTES];

pub fn init() {
    seed_ramblk0();
    register(BlockDeviceRegistration {
        info: BlockDeviceInfo {
            name: RAMBLK0_NAME,
            driver: "ram-block",
            block_size: RAMBLK0_BLOCK_SIZE,
            blocks: RAMBLK0_BLOCKS,
            readonly: false,
        },
        read_block: ramblk0_read,
        write_block: ramblk0_write,
    });
}

pub fn snapshot(out: &mut [Option<BlockDeviceInfo>]) -> usize {
    lock_block();

    let count = unsafe { BLOCK_DEVICE_COUNT.min(out.len()) };
    let mut index = 0usize;
    while index < count {
        out[index] = unsafe { BLOCK_DEVICES[index].map(|device| device.info) };
        index += 1;
    }

    BLOCK_LOCK.store(false, Ordering::Release);
    count
}

pub fn read_block(name: &str, lba: u64, buf: &mut [u8]) -> Result<usize, BlockError> {
    let Some(device) = find_device(name) else {
        return Err(BlockError::NotFound);
    };
    (device.read_block)(lba, buf)
}

pub fn write_block(name: &str, lba: u64, buf: &[u8]) -> Result<usize, BlockError> {
    let Some(device) = find_device(name) else {
        return Err(BlockError::NotFound);
    };
    (device.write_block)(lba, buf)
}

fn register(device: BlockDeviceRegistration) {
    lock_block();

    unsafe {
        let mut index = 0usize;
        while index < BLOCK_DEVICE_COUNT {
            if let Some(existing) = BLOCK_DEVICES[index] {
                if existing.info.name == device.info.name {
                    BLOCK_DEVICES[index] = Some(device);
                    BLOCK_LOCK.store(false, Ordering::Release);
                    publish_device(device.info);
                    return;
                }
            }
            index += 1;
        }

        if BLOCK_DEVICE_COUNT < BLOCK_DEVICES.len() {
            BLOCK_DEVICES[BLOCK_DEVICE_COUNT] = Some(device);
            BLOCK_DEVICE_COUNT += 1;
        }
    }

    BLOCK_LOCK.store(false, Ordering::Release);
    publish_device(device.info);
}

fn find_device(name: &str) -> Option<BlockDeviceRegistration> {
    lock_block();

    let mut found = None;
    unsafe {
        let mut index = 0usize;
        while index < BLOCK_DEVICE_COUNT {
            if let Some(device) = BLOCK_DEVICES[index] {
                if device.info.name == name {
                    found = Some(device);
                    break;
                }
            }
            index += 1;
        }
    }

    BLOCK_LOCK.store(false, Ordering::Release);
    found
}

fn publish_device(info: BlockDeviceInfo) {
    crate::drivers::registry::register(
        info.name,
        crate::drivers::registry::DeviceClass::Block,
        info.driver,
    );
}

fn seed_ramblk0() {
    let message = b"Dunit OS ramblk0 block device\n";
    unsafe {
        let mut index = 0usize;
        while index < RAMBLK0_DATA.len() {
            RAMBLK0_DATA[index] = 0;
            index += 1;
        }

        let mut msg_index = 0usize;
        while msg_index < message.len() && msg_index < RAMBLK0_DATA.len() {
            RAMBLK0_DATA[msg_index] = message[msg_index];
            msg_index += 1;
        }
    }
}

fn ramblk0_read(lba: u64, buf: &mut [u8]) -> Result<usize, BlockError> {
    if lba >= RAMBLK0_BLOCKS {
        return Err(BlockError::OutOfRange);
    }
    if buf.len() < RAMBLK0_BLOCK_SIZE {
        return Err(BlockError::BufferTooSmall);
    }

    let offset = lba as usize * RAMBLK0_BLOCK_SIZE;
    unsafe {
        buf[..RAMBLK0_BLOCK_SIZE]
            .copy_from_slice(&RAMBLK0_DATA[offset..offset + RAMBLK0_BLOCK_SIZE]);
    }
    Ok(RAMBLK0_BLOCK_SIZE)
}

fn ramblk0_write(lba: u64, buf: &[u8]) -> Result<usize, BlockError> {
    if lba >= RAMBLK0_BLOCKS {
        return Err(BlockError::OutOfRange);
    }
    if buf.len() < RAMBLK0_BLOCK_SIZE {
        return Err(BlockError::BufferTooSmall);
    }

    let offset = lba as usize * RAMBLK0_BLOCK_SIZE;
    unsafe {
        RAMBLK0_DATA[offset..offset + RAMBLK0_BLOCK_SIZE]
            .copy_from_slice(&buf[..RAMBLK0_BLOCK_SIZE]);
    }
    Ok(RAMBLK0_BLOCK_SIZE)
}

fn lock_block() {
    while BLOCK_LOCK
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        core::hint::spin_loop();
    }
}
