use alloc::vec;

use core::sync::atomic::{AtomicUsize, Ordering};

use crate::drivers::block::{self, BlockDeviceInfo};
use crate::fs::dunitfs;
use crate::fs::vfs::VirtualFileSystem;
use crate::storage::gpt::{self, PartitionSpec};
use crate::storage::policy;

const BLOCK_SIZE: usize = 512;
const ESP_START: u64 = 2048;
const GPT_TRAILING_BLOCKS: u64 = 34;
const GPT_ENTRY_SIZE: usize = 128;
const GPT_PRIMARY_ENTRIES_LBA: u64 = 2;
const GPT_FULL_ENTRY_BLOCKS: u64 = 32;

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
    VerifyFailed,
    SyncFailed,
    SnapshotFailed,
    MountFailed,
    BiosInstallFailed,
    Injected,
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
            Self::VerifyFailed => "post-write verification failed",
            Self::SyncFailed => "final sync/read-back failed",
            Self::SnapshotFailed => "failed to snapshot partition table",
            Self::MountFailed => "installed DunitFS could not be mounted",
            Self::BiosInstallFailed => "failed to install Limine BIOS stages",
            Self::Injected => "injected test fault",
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
    let layout = run_transaction(device, Fault::None)?;
    dunitfs::mount_global(vfs, "/persist", device, layout.root_start, layout.root_blocks)
        .map_err(|_| InstallError::MountFailed)
}

/// Result of a committed install transaction: where the DunitFS root lives, so
/// the caller can mount it. The transaction itself never mounts.
#[derive(Clone, Copy)]
struct InstallLayout {
    root_start: u64,
    root_blocks: u64,
}

/// Everything the destructive phases need, computed by the read-only validate
/// phase. Borrows into the installer payloads, which live for `'static`.
struct InstallPlan {
    payload: &'static [u8],
    bios_payload: &'static [u8],
    disk_guid: [u8; 16],
    partitions: [PartitionSpec<'static>; 2],
    root_start: u64,
    root_blocks: u64,
}

/// Fault-injection point for the transaction. `None` is the only value used in
/// production; the boot-smoke installer self-test injects the others to force a
/// mid-transaction failure and prove rollback.
#[derive(Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
enum Fault {
    None,
    AfterPartition,
    AfterFormat,
    AfterCopy,
    AfterBootConfig,
}

fn fault_check(active: Fault, point: Fault) -> Result<(), InstallError> {
    if active == point {
        Err(InstallError::Injected)
    } else {
        Ok(())
    }
}
/// Validate phase (read-only): check the device and both payloads, compute the
/// GPT geometry, and build the two partition specs. No disk writes happen here,
/// so a validation failure needs no rollback.
fn plan(device: BlockDeviceInfo) -> Result<InstallPlan, InstallError> {
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
            type_guid: policy::ESP_TYPE_GUID,
            unique_guid: make_guid(seed, 1),
            first_lba: ESP_START,
            last_lba: root_start - 1,
            attributes: 0,
            name: "DUNIT-ESP",
        },
        PartitionSpec {
            type_guid: policy::DUNIT_SYSTEM_TYPE_GUID,
            unique_guid: make_guid(seed, 2),
            first_lba: root_start,
            last_lba: root_end,
            attributes: 0,
            name: "DUNIT-SYSTEM",
        },
    ];
    Ok(InstallPlan {
        payload,
        bios_payload,
        disk_guid,
        partitions,
        root_start,
        root_blocks: root_end - root_start + 1,
    })
}
/// Transactional install: validate -> snapshot -> (partition -> format -> copy
/// -> verify -> boot config -> sync). Any failure after the snapshot restores
/// the partition-table regions, reverting the disk to its prior layout; the
/// ESP/FS payloads written into the data area become unreferenced and harmless.
/// On success the DunitFS root is formatted and the disk is bootable, but NOT
/// mounted (the caller mounts).
fn run_transaction(device: BlockDeviceInfo, fault: Fault) -> Result<InstallLayout, InstallError> {
    let plan = plan(device)?;

    // Snapshot the GPT regions BEFORE any destructive write. FRONT = protective
    // MBR + primary header + primary entries (LBA 0..=33). BACK = backup entries
    // + backup header (LBA blocks-33..=blocks-1). install_bios writes its Limine
    // stages inside these same regions, so both are covered.
    let front_blocks = GPT_TRAILING_BLOCKS;
    let back_lba = device.blocks - (GPT_TRAILING_BLOCKS - 1);
    let back_blocks = GPT_TRAILING_BLOCKS - 1;
    let front = snapshot_region(device, 0, front_blocks)?;
    let back = snapshot_region(device, back_lba, back_blocks)?;

    match commit(device, &plan, fault) {
        Ok(()) => {
            crate::serial_write("[INSTALL] transaction committed root_start=");
            serial_write_u64(plan.root_start);
            crate::serial_write(" root_blocks=");
            serial_write_u64(plan.root_blocks);
            crate::serial_write("\r\n");
            Ok(InstallLayout {
                root_start: plan.root_start,
                root_blocks: plan.root_blocks,
            })
        }
        Err(error) => {
            rollback(device, &front, back_lba, &back);
            Err(error)
        }
    }
}
/// The destructive half of the transaction, kept separate so `run_transaction`
/// can roll back on any `Err`. `fault` forces a failure after a chosen phase.
fn commit(device: BlockDeviceInfo, plan: &InstallPlan, fault: Fault) -> Result<(), InstallError> {
    // partition
    gpt::write(device, plan.disk_guid, &plan.partitions).map_err(|_| InstallError::Partitioning)?;
    fault_check(fault, Fault::AfterPartition)?;

    // format
    dunitfs::format(device, plan.root_start, plan.root_blocks)
        .map_err(|_| InstallError::FormatFailed)?;
    fault_check(fault, Fault::AfterFormat)?;

    // copy (write the ESP contents into the EFI system partition)
    let written = block::write_block(device.name, ESP_START, plan.payload)
        .map_err(|_| InstallError::WriteFailed)?;
    if written != plan.payload.len() {
        return Err(InstallError::WriteFailed);
    }
    fault_check(fault, Fault::AfterCopy)?;

    // verify (read the layout back and prove it is consistent before we commit
    // to making the disk bootable)
    verify_install(device, plan)?;

    // boot config (install the Limine BIOS stages + stage pointers)
    install_bios(device, plan.bios_payload)?;
    fault_check(fault, Fault::AfterBootConfig)?;

    // sync (confirm the critical structures are durably readable)
    sync_phase(device)
}
/// Verify phase: the partition table must read back with both the ESP and the
/// DunitFS system partition, the freshly formatted root must fsck Clean, and the
/// ESP's first block must match the payload we wrote.
fn verify_install(device: BlockDeviceInfo, plan: &InstallPlan) -> Result<(), InstallError> {
    let table = gpt::read(device).map_err(|_| InstallError::VerifyFailed)?;
    let mut esp_ok = false;
    let mut system_ok = false;
    for partition in table.partitions.iter().flatten() {
        if partition.type_guid == policy::ESP_TYPE_GUID && partition.first_lba == ESP_START {
            esp_ok = true;
        }
        if partition.type_guid == policy::DUNIT_SYSTEM_TYPE_GUID
            && partition.first_lba == plan.root_start
        {
            system_ok = true;
        }
    }
    if !esp_ok || !system_ok {
        return Err(InstallError::VerifyFailed);
    }
    if dunitfs::fsck(device, plan.root_start, plan.root_blocks).verdict != dunitfs::FsckVerdict::Clean
    {
        return Err(InstallError::VerifyFailed);
    }
    let mut block = [0u8; BLOCK_SIZE];
    let read = block::read_block(device.name, ESP_START, &mut block)
        .map_err(|_| InstallError::VerifyFailed)?;
    if read != BLOCK_SIZE || block[..] != plan.payload[..BLOCK_SIZE] {
        return Err(InstallError::VerifyFailed);
    }
    Ok(())
}

/// Sync phase. The AHCI write path already issues ATA_CMD_FLUSH_CACHE_EXT after
/// every write, so data is durable per write; this barrier re-reads the primary
/// GPT (it must still parse after boot config patched its headers) and confirms
/// the protective MBR carries the boot signature, draining the write pipeline
/// before the disk is handed off.
fn sync_phase(device: BlockDeviceInfo) -> Result<(), InstallError> {
    gpt::read(device).map_err(|_| InstallError::SyncFailed)?;
    let mut mbr = [0u8; BLOCK_SIZE];
    let read = block::read_block(device.name, 0, &mut mbr).map_err(|_| InstallError::SyncFailed)?;
    if read != BLOCK_SIZE || mbr[510..512] != [0x55, 0xaa] {
        return Err(InstallError::SyncFailed);
    }
    Ok(())
}
/// Read `blocks` consecutive 512-byte blocks starting at `lba` into a buffer,
/// for the pre-write partition-table snapshot.
fn snapshot_region(
    device: BlockDeviceInfo,
    lba: u64,
    blocks: u64,
) -> Result<alloc::vec::Vec<u8>, InstallError> {
    let mut data = vec![0u8; blocks as usize * BLOCK_SIZE];
    let read =
        block::read_block(device.name, lba, &mut data).map_err(|_| InstallError::SnapshotFailed)?;
    if read != data.len() {
        return Err(InstallError::SnapshotFailed);
    }
    Ok(data)
}

/// Write a snapshot buffer back to `lba`. Best-effort (used only during
/// rollback); returns whether the restore write landed in full.
fn restore_region(device: BlockDeviceInfo, lba: u64, data: &[u8]) -> bool {
    matches!(block::write_block(device.name, lba, data), Ok(written) if written == data.len())
}

/// Restore the FRONT and BACK partition-table snapshots, reverting the disk to
/// its pre-transaction layout.
fn rollback(device: BlockDeviceInfo, front: &[u8], back_lba: u64, back: &[u8]) {
    let front_ok = restore_region(device, 0, front);
    let back_ok = restore_region(device, back_lba, back);
    if front_ok && back_ok {
        crate::serial_write("[INSTALL] rollback restored partition table\r\n");
    } else {
        crate::serial_write("[INSTALL] rollback restore FAILED\r\n");
    }
}

fn serial_write_u64(mut value: u64) {
    if value == 0 {
        crate::serial_write("0");
        return;
    }
    let mut buf = [0u8; 20];
    let mut index = buf.len();
    while value > 0 {
        index -= 1;
        buf[index] = b'0' + (value % 10) as u8;
        value /= 10;
    }
    let s = unsafe { core::str::from_utf8_unchecked(&buf[index..]) };
    crate::serial_write(s);
}
/// Boot-smoke installer self-test (feature `boot-smoke-tests`). Runs in
/// `drivers::init` BEFORE auto_mount, in the Live/ISO environment where the
/// installer ESP/BIOS payloads are present. Targets a BLANK scratch AHCI disk
/// (never one that already carries a Dunit partition), so it can never touch a
/// real installed system. Phase A: a fault-injected transaction must leave the
/// scratch's partition table byte-for-byte as it was (rollback proof). Phase B:
/// a clean transaction must fully install and self-verify (GPT + fsck Clean +
/// bootable MBR). The install is left unmounted so the normal auto_mount then
/// brings it up at /persist.
#[cfg(feature = "boot-smoke-tests")]
pub fn installer_self_test() -> bool {
    crate::serial_write("[INSTALL-TEST] START\r\n");
    if dunitfs::is_mounted() {
        crate::serial_write("[INSTALL-TEST] skipped: a volume is already mounted\r\n");
        return true;
    }
    if payload().is_none() || bios_payload().is_none() {
        crate::serial_write("[INSTALL-TEST] skipped: installer payloads unavailable\r\n");
        return true;
    }
    let Some(device) = find_scratch_target() else {
        crate::serial_write("[INSTALL-TEST] skipped: no scratch target\r\n");
        return true;
    };
    crate::serial_write("[INSTALL-TEST] target=");
    crate::serial_write(device.name);
    crate::serial_write(" blocks=");
    serial_write_u64(device.blocks);
    crate::serial_write("\r\n");

    let front_blocks = GPT_TRAILING_BLOCKS;
    let back_lba = device.blocks - (GPT_TRAILING_BLOCKS - 1);
    let back_blocks = GPT_TRAILING_BLOCKS - 1;
    let (pre_front, pre_back) = match (
        snapshot_region(device, 0, front_blocks),
        snapshot_region(device, back_lba, back_blocks),
    ) {
        (Ok(front), Ok(back)) => (front, back),
        _ => {
            crate::serial_write("[INSTALL-TEST] FAIL (pre-snapshot)\r\n");
            return false;
        }
    };

    let mut ok = true;
    // Phase A: a mid-transaction fault must roll the scratch back to blank.
    match run_transaction(device, Fault::AfterPartition) {
        Err(_) => {
            let restored = matches!(
                (
                    snapshot_region(device, 0, front_blocks),
                    snapshot_region(device, back_lba, back_blocks),
                ),
                (Ok(front), Ok(back)) if front == pre_front && back == pre_back
            );
            if restored && gpt::read(device).is_err() {
                crate::serial_write("[INSTALL-TEST] rollback restored ok\r\n");
            } else {
                crate::serial_write("[INSTALL-TEST] rollback restored FAIL\r\n");
                ok = false;
            }
        }
        Ok(_) => {
            crate::serial_write("[INSTALL-TEST] rollback restored FAIL (fault not raised)\r\n");
            ok = false;
        }
    }
    // Phase B: a clean transaction must fully install and self-verify.
    match run_transaction(device, Fault::None) {
        Ok(layout) => {
            let table_ok = gpt::read(device)
                .map(|table| {
                    let mut esp = false;
                    let mut system = false;
                    for partition in table.partitions.iter().flatten() {
                        if partition.type_guid == policy::ESP_TYPE_GUID {
                            esp = true;
                        }
                        if partition.type_guid == policy::DUNIT_SYSTEM_TYPE_GUID
                            && partition.first_lba == layout.root_start
                        {
                            system = true;
                        }
                    }
                    esp && system
                })
                .unwrap_or(false);
            let fsck_ok = dunitfs::fsck(device, layout.root_start, layout.root_blocks).verdict
                == dunitfs::FsckVerdict::Clean;
            let mut mbr = [0u8; BLOCK_SIZE];
            let mbr_ok = matches!(
                block::read_block(device.name, 0, &mut mbr),
                Ok(read) if read == BLOCK_SIZE
            ) && mbr[510..512] == [0x55, 0xaa];
            if table_ok && fsck_ok && mbr_ok {
                crate::serial_write("[INSTALL-TEST] install verified ok\r\n");
            } else {
                crate::serial_write("[INSTALL-TEST] install verified FAIL\r\n");
                ok = false;
            }
        }
        Err(error) => {
            crate::serial_write("[INSTALL-TEST] install verified FAIL (");
            crate::serial_write(error.as_str());
            crate::serial_write(")\r\n");
            ok = false;
        }
    }

    crate::serial_write(if ok {
        "[INSTALL-TEST] OK\r\n"
    } else {
        "[INSTALL-TEST] FAIL\r\n"
    });
    ok
}
/// Pick the first writable AHCI disk that does NOT already carry a valid GPT —
/// a blank scratch target. A disk with any readable partition table (an
/// installed system, or a disk this test already provisioned) is skipped, so the
/// self-test can only ever run against a pristine scratch disk.
#[cfg(feature = "boot-smoke-tests")]
fn find_scratch_target() -> Option<BlockDeviceInfo> {
    let mut devices = [None; 8];
    let count = block::snapshot(&mut devices);
    for info in devices[..count].iter().flatten() {
        if info.driver != "ahci" || info.readonly || info.block_size != BLOCK_SIZE {
            continue;
        }
        if gpt::read(*info).is_err() {
            return Some(*info);
        }
    }
    None
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
