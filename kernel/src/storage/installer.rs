use alloc::vec;

use core::sync::atomic::{AtomicUsize, Ordering};

use crate::drivers::block::{self, BlockDeviceInfo};
use crate::fs::dunitfs;
use crate::fs::vfs::VirtualFileSystem;
use crate::storage::gpt::{self, PartitionSpec};

const BLOCK_SIZE: usize = 512;
const ESP_START: u64 = 2048;
const GPT_TRAILING_BLOCKS: u64 = 34;
const GPT_ENTRY_SIZE: usize = 128;
const GPT_PRIMARY_ENTRIES_LBA: u64 = 2;
const GPT_FULL_ENTRY_BLOCKS: u64 = 32;

const ESP_TYPE_GUID: [u8; 16] = [
    0x28, 0x73, 0x2a, 0xc1, 0x1f, 0xf8, 0xd2, 0x11, 0xba, 0x4b, 0x00, 0xa0, 0xc9, 0x3e, 0xc9, 0x3b,
];
const DUNITFS_TYPE_GUID: [u8; 16] = [
    0xaf, 0x3d, 0xc6, 0x0f, 0x83, 0x84, 0x72, 0x47, 0x8e, 0x79, 0x3d, 0x69, 0xd8, 0x47, 0x7d, 0xe4,
];

static PAYLOAD_ADDRESS: AtomicUsize = AtomicUsize::new(0);
static PAYLOAD_SIZE: AtomicUsize = AtomicUsize::new(0);
static BIOS_PAYLOAD_ADDRESS: AtomicUsize = AtomicUsize::new(0);
static BIOS_PAYLOAD_SIZE: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstallError {
    PayloadUnavailable,
    InvalidPayload,
    BiosPayloadUnavailable,
    InvalidBiosPayload,
    UnsupportedDevice,
    DiskTooSmall,
    AlreadyMounted,
    Partitioning,
    WriteFailed,
    FormatFailed,
    MountFailed,
    BiosInstallFailed,
}

impl InstallError {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PayloadUnavailable => "installer payload is unavailable",
            Self::InvalidPayload => "installer payload is invalid",
            Self::BiosPayloadUnavailable => "BIOS installer payload is unavailable",
            Self::InvalidBiosPayload => "BIOS installer payload is invalid",
            Self::UnsupportedDevice => "installer currently requires a writable AHCI disk",
            Self::DiskTooSmall => "target disk is too small",
            Self::AlreadyMounted => "a DunitFS volume is already mounted",
            Self::Partitioning => "failed to write GPT",
            Self::WriteFailed => "failed to write EFI system partition",
            Self::FormatFailed => "failed to format DunitFS",
            Self::MountFailed => "installed DunitFS could not be mounted",
            Self::BiosInstallFailed => "failed to install Limine BIOS stages",
        }
    }
}

pub unsafe fn set_payload(address: *const u8, size: usize) {
    PAYLOAD_ADDRESS.store(address as usize, Ordering::Release);
    PAYLOAD_SIZE.store(size, Ordering::Release);
}

pub unsafe fn set_bios_payload(address: *const u8, size: usize) {
    BIOS_PAYLOAD_ADDRESS.store(address as usize, Ordering::Release);
    BIOS_PAYLOAD_SIZE.store(size, Ordering::Release);
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
    let bios_payload = bios_payload().ok_or(InstallError::BiosPayloadUnavailable)?;
    if bios_payload.len() <= BLOCK_SIZE || bios_payload[510..512] != [0x55, 0xaa] {
        return Err(InstallError::InvalidBiosPayload);
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
    install_bios(device, bios_payload)?;
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

fn bios_payload() -> Option<&'static [u8]> {
    let address = BIOS_PAYLOAD_ADDRESS.load(Ordering::Acquire);
    let size = BIOS_PAYLOAD_SIZE.load(Ordering::Acquire);
    if address == 0 || size == 0 {
        return None;
    }
    Some(unsafe { core::slice::from_raw_parts(address as *const u8, size) })
}

fn install_bios(device: BlockDeviceInfo, image: &[u8]) -> Result<(), InstallError> {
    let stage2_size = image.len() - BLOCK_SIZE;
    let stage2_blocks = stage2_size.div_ceil(BLOCK_SIZE);
    let stage_a_blocks = stage2_blocks.div_ceil(2);
    let stage_b_blocks = stage2_blocks / 2;
    let stage_a_size = stage_a_blocks * BLOCK_SIZE;
    let stage_b_size = stage_b_blocks * BLOCK_SIZE;
    if stage_a_size > stage2_size
        || stage_a_size > u16::MAX as usize
        || stage_b_size > u16::MAX as usize
        || stage_a_blocks as u64 >= GPT_FULL_ENTRY_BLOCKS
    {
        return Err(InstallError::InvalidBiosPayload);
    }

    let stage_a_lba = GPT_PRIMARY_ENTRIES_LBA + GPT_FULL_ENTRY_BLOCKS - stage_a_blocks as u64;
    let backup_entries_lba = device.blocks - 1 - GPT_FULL_ENTRY_BLOCKS;
    let stage_b_lba = backup_entries_lba + GPT_FULL_ENTRY_BLOCKS - stage_b_blocks as u64;
    let entry_blocks = stage_a_lba - GPT_PRIMARY_ENTRIES_LBA;
    let entry_count = entry_blocks as usize * (BLOCK_SIZE / GPT_ENTRY_SIZE);
    if entry_count < 2 {
        return Err(InstallError::InvalidBiosPayload);
    }

    let mut entries = vec![0u8; entry_count * GPT_ENTRY_SIZE];
    read_exact(device, GPT_PRIMARY_ENTRIES_LBA, &mut entries)?;
    let entries_crc = crc32(&entries);

    let mut stage_b = vec![0u8; stage_b_size];
    let stage_b_source = &image[BLOCK_SIZE + stage_a_size..];
    stage_b[..stage_b_source.len()].copy_from_slice(stage_b_source);
    write_exact(
        device,
        stage_a_lba,
        &image[BLOCK_SIZE..BLOCK_SIZE + stage_a_size],
    )?;
    write_exact(device, stage_b_lba, &stage_b)?;

    patch_gpt_header(device, 1, entry_count as u32, entries_crc)?;
    patch_gpt_header(device, device.blocks - 1, entry_count as u32, entries_crc)?;

    let mut mbr = [0u8; BLOCK_SIZE];
    read_exact(device, 0, &mut mbr)?;
    let mut timestamp = [0u8; 6];
    timestamp.copy_from_slice(&mbr[218..224]);
    let mut partition_data = [0u8; 70];
    partition_data.copy_from_slice(&mbr[440..510]);
    mbr.copy_from_slice(&image[..BLOCK_SIZE]);
    put_u16(&mut mbr, 0x1a4, stage_a_size as u16);
    put_u16(&mut mbr, 0x1a6, stage_b_size as u16);
    put_u64(&mut mbr, 0x1a8, stage_a_lba * BLOCK_SIZE as u64);
    put_u64(&mut mbr, 0x1b0, stage_b_lba * BLOCK_SIZE as u64);
    mbr[218..224].copy_from_slice(&timestamp);
    mbr[440..510].copy_from_slice(&partition_data);
    write_exact(device, 0, &mbr)
}

fn patch_gpt_header(
    device: BlockDeviceInfo,
    lba: u64,
    entry_count: u32,
    entries_crc: u32,
) -> Result<(), InstallError> {
    let mut header = [0u8; BLOCK_SIZE];
    read_exact(device, lba, &mut header)?;
    if &header[..8] != b"EFI PART" {
        return Err(InstallError::BiosInstallFailed);
    }
    let header_size = get_u32(&header, 12) as usize;
    if !(92..=BLOCK_SIZE).contains(&header_size) {
        return Err(InstallError::BiosInstallFailed);
    }
    put_u32(&mut header, 80, entry_count);
    put_u32(&mut header, 88, entries_crc);
    put_u32(&mut header, 16, 0);
    let header_crc = crc32(&header[..header_size]);
    put_u32(&mut header, 16, header_crc);
    write_exact(device, lba, &header)
}

fn read_exact(device: BlockDeviceInfo, lba: u64, data: &mut [u8]) -> Result<(), InstallError> {
    let read =
        block::read_block(device.name, lba, data).map_err(|_| InstallError::BiosInstallFailed)?;
    if read != data.len() {
        return Err(InstallError::BiosInstallFailed);
    }
    Ok(())
}

fn write_exact(device: BlockDeviceInfo, lba: u64, data: &[u8]) -> Result<(), InstallError> {
    let written =
        block::write_block(device.name, lba, data).map_err(|_| InstallError::BiosInstallFailed)?;
    if written != data.len() {
        return Err(InstallError::BiosInstallFailed);
    }
    Ok(())
}

fn get_u32(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap())
}

fn put_u16(data: &mut [u8], offset: usize, value: u16) {
    data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn put_u32(data: &mut [u8], offset: usize, value: u32) {
    data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn put_u64(data: &mut [u8], offset: usize, value: u64) {
    data[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for byte in data {
        crc ^= *byte as u32;
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & 0u32.wrapping_sub(crc & 1));
        }
    }
    !crc
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
