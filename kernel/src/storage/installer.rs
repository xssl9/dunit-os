use core::sync::atomic::{AtomicUsize, Ordering};

use crate::drivers::block::{self, BlockDeviceInfo};
use crate::fs::dunitfs;
use crate::fs::vfs::VirtualFileSystem;
use crate::storage::gpt::{self, PartitionSpec};

const BLOCK_SIZE: usize = 512;
const ESP_START: u64 = 2048;
const GPT_TRAILING_BLOCKS: u64 = 34;

const ESP_TYPE_GUID: [u8; 16] = [
    0x28, 0x73, 0x2a, 0xc1, 0x1f, 0xf8, 0xd2, 0x11, 0xba, 0x4b, 0x00, 0xa0, 0xc9, 0x3e, 0xc9, 0x3b,
];
const DUNITFS_TYPE_GUID: [u8; 16] = [
    0xaf, 0x3d, 0xc6, 0x0f, 0x83, 0x84, 0x72, 0x47, 0x8e, 0x79, 0x3d, 0x69, 0xd8, 0x47, 0x7d, 0xe4,
];

static PAYLOAD_ADDRESS: AtomicUsize = AtomicUsize::new(0);
static PAYLOAD_SIZE: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstallError {
    PayloadUnavailable,
    InvalidPayload,
    UnsupportedDevice,
    DiskTooSmall,
    AlreadyMounted,
    Partitioning,
    WriteFailed,
    FormatFailed,
    MountFailed,
}

impl InstallError {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PayloadUnavailable => "installer payload is unavailable",
            Self::InvalidPayload => "installer payload is invalid",
            Self::UnsupportedDevice => "installer currently requires a writable AHCI disk",
            Self::DiskTooSmall => "target disk is too small",
            Self::AlreadyMounted => "a DunitFS volume is already mounted",
            Self::Partitioning => "failed to write GPT",
            Self::WriteFailed => "failed to write EFI system partition",
            Self::FormatFailed => "failed to format DunitFS",
            Self::MountFailed => "installed DunitFS could not be mounted",
        }
    }
}

pub unsafe fn set_payload(address: *const u8, size: usize) {
    PAYLOAD_ADDRESS.store(address as usize, Ordering::Release);
    PAYLOAD_SIZE.store(size, Ordering::Release);
}

pub fn payload_size() -> usize {
    PAYLOAD_SIZE.load(Ordering::Acquire)
}

pub fn install(device: BlockDeviceInfo, vfs: &mut VirtualFileSystem) -> Result<(), InstallError> {
    if device.readonly || device.block_size != BLOCK_SIZE || device.driver != "ahci" {
        return Err(InstallError::UnsupportedDevice);
    }
    if dunitfs::is_mounted() {
        return Err(InstallError::AlreadyMounted);
    }
    let payload = payload().ok_or(InstallError::PayloadUnavailable)?;
    if payload.is_empty() || payload.len() % BLOCK_SIZE != 0 {
        return Err(InstallError::InvalidPayload);
    }

    let esp_blocks = (payload.len() / BLOCK_SIZE) as u64;
    let root_start = ESP_START
        .checked_add(esp_blocks)
        .ok_or(InstallError::DiskTooSmall)?;
    let root_end = device
        .blocks
        .checked_sub(GPT_TRAILING_BLOCKS)
        .ok_or(InstallError::DiskTooSmall)?;
    if root_start >= root_end || root_end - root_start < 2048 {
        return Err(InstallError::DiskTooSmall);
    }

    let seed = unsafe { core::arch::x86_64::_rdtsc() } ^ device.blocks;
    let disk_guid = make_guid(seed, 0);
    let partitions = [
        PartitionSpec {
            type_guid: ESP_TYPE_GUID,
            unique_guid: make_guid(seed, 1),
            first_lba: ESP_START,
            last_lba: root_start - 1,
            attributes: 0,
            name: "DUNIT-ESP",
        },
        PartitionSpec {
            type_guid: DUNITFS_TYPE_GUID,
            unique_guid: make_guid(seed, 2),
            first_lba: root_start,
            last_lba: root_end,
            attributes: 0,
            name: "DUNIT-ROOT",
        },
    ];
    gpt::write(device, disk_guid, &partitions).map_err(|_| InstallError::Partitioning)?;

    let written = block::write_block(device.name, ESP_START, payload)
        .map_err(|_| InstallError::WriteFailed)?;
    if written != payload.len() {
        return Err(InstallError::WriteFailed);
    }

    let root_blocks = root_end - root_start + 1;
    dunitfs::format(device, root_start, root_blocks).map_err(|_| InstallError::FormatFailed)?;
    dunitfs::mount_global(vfs, "/persist", device, root_start, root_blocks)
        .map_err(|_| InstallError::MountFailed)
}

fn payload() -> Option<&'static [u8]> {
    let address = PAYLOAD_ADDRESS.load(Ordering::Acquire);
    let size = PAYLOAD_SIZE.load(Ordering::Acquire);
    if address == 0 || size == 0 {
        return None;
    }
    Some(unsafe { core::slice::from_raw_parts(address as *const u8, size) })
}

fn make_guid(seed: u64, tag: u64) -> [u8; 16] {
    let mut left = mix(seed ^ tag.wrapping_mul(0x9e37_79b9_7f4a_7c15));
    let right = mix(left ^ 0xa5a5_5a5a_d3c4_b2e1);
    if left == 0 && right == 0 {
        left = 1;
    }
    let mut guid = [0u8; 16];
    guid[..8].copy_from_slice(&left.to_le_bytes());
    guid[8..].copy_from_slice(&right.to_le_bytes());
    guid[7] = (guid[7] & 0x0f) | 0x40;
    guid[8] = (guid[8] & 0x3f) | 0x80;
    guid
}

fn mix(mut value: u64) -> u64 {
    value ^= value >> 30;
    value = value.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value ^= value >> 27;
    value = value.wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}
