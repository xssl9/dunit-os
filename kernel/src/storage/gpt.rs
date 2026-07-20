use crate::drivers::block::{self, BlockDeviceInfo, BlockError};

const GPT_SIGNATURE: &[u8; 8] = b"EFI PART";
const GPT_REVISION_1_0: u32 = 0x0001_0000;
const GPT_HEADER_SIZE: usize = 92;
const GPT_ENTRY_SIZE: usize = 128;
const GPT_ENTRY_COUNT: u32 = 128;
const MAX_BLOCK_SIZE: usize = 4096;
const MAX_PARTITIONS: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GptError {
    UnsupportedBlockSize,
    DiskTooSmall,
    InvalidSignature,
    InvalidHeader,
    InvalidHeaderCrc,
    InvalidTableCrc,
    TooManyEntries,
    InvalidPartition,
    Block(BlockError),
}

impl From<BlockError> for GptError {
    fn from(error: BlockError) -> Self {
        Self::Block(error)
    }
}

#[derive(Clone, Copy)]
pub struct Partition {
    pub index: u32,
    pub type_guid: [u8; 16],
    pub unique_guid: [u8; 16],
    pub first_lba: u64,
    pub last_lba: u64,
    pub attributes: u64,
    name: [u8; 36],
    name_len: usize,
}

impl Partition {
    pub const fn empty() -> Self {
        Self {
            index: 0,
            type_guid: [0; 16],
            unique_guid: [0; 16],
            first_lba: 0,
            last_lba: 0,
            attributes: 0,
            name: [0; 36],
            name_len: 0,
        }
    }

    pub fn name(&self) -> &str {
        core::str::from_utf8(&self.name[..self.name_len]).unwrap_or("?")
    }

    pub const fn blocks(&self) -> u64 {
        self.last_lba - self.first_lba + 1
    }
}

pub struct Table {
    pub disk_guid: [u8; 16],
    pub first_usable_lba: u64,
    pub last_usable_lba: u64,
    pub partitions: [Option<Partition>; MAX_PARTITIONS],
    pub partition_count: usize,
}

#[derive(Clone, Copy)]
pub struct PartitionSpec<'a> {
    pub type_guid: [u8; 16],
    pub unique_guid: [u8; 16],
    pub first_lba: u64,
    pub last_lba: u64,
    pub attributes: u64,
    pub name: &'a str,
}

pub fn read(device: BlockDeviceInfo) -> Result<Table, GptError> {
    validate_device(device)?;
    let mut block_buf = [0u8; MAX_BLOCK_SIZE];
    read_exact(device, 1, &mut block_buf)?;

    let header = parse_header(&block_buf[..device.block_size], device)?;
    let mut table = Table {
        disk_guid: header.disk_guid,
        first_usable_lba: header.first_usable_lba,
        last_usable_lba: header.last_usable_lba,
        partitions: [None; MAX_PARTITIONS],
        partition_count: 0,
    };

    let table_bytes = header.entry_count as usize * header.entry_size;
    let table_blocks = table_bytes.div_ceil(device.block_size);
    let mut crc = Crc32::new();
    let mut entry_index = 0usize;
    for block_offset in 0..table_blocks {
        read_exact(
            device,
            header.entries_lba + block_offset as u64,
            &mut block_buf,
        )?;
        let consumed = block_offset * device.block_size;
        let bytes_here = (table_bytes - consumed).min(device.block_size);
        crc.update(&block_buf[..bytes_here]);

        let entries_here = bytes_here / header.entry_size;
        for slot in 0..entries_here {
            let start = slot * header.entry_size;
            let entry = &block_buf[start..start + header.entry_size];
            if !is_zero_guid(&entry[..16]) {
                let partition = parse_partition(entry, entry_index as u32 + 1)?;
                if partition.first_lba < header.first_usable_lba
                    || partition.last_lba > header.last_usable_lba
                {
                    return Err(GptError::InvalidPartition);
                }
                if table.partition_count >= MAX_PARTITIONS {
                    return Err(GptError::TooManyEntries);
                }
                for previous in table.partitions[..table.partition_count].iter().flatten() {
                    if partition.unique_guid == previous.unique_guid
                        || (partition.first_lba <= previous.last_lba
                            && previous.first_lba <= partition.last_lba)
                    {
                        return Err(GptError::InvalidPartition);
                    }
                }
                table.partitions[table.partition_count] = Some(partition);
                table.partition_count += 1;
            }
            entry_index += 1;
        }
    }

    if crc.finish() != header.entries_crc32 {
        return Err(GptError::InvalidTableCrc);
    }
    Ok(table)
}

/// Writes a complete protective MBR, primary GPT and backup GPT. Callers must
/// perform their own explicit user confirmation before invoking this function.
pub fn write(
    device: BlockDeviceInfo,
    disk_guid: [u8; 16],
    partitions: &[PartitionSpec<'_>],
) -> Result<(), GptError> {
    validate_device(device)?;
    if device.readonly {
        return Err(GptError::Block(BlockError::Io));
    }
    if partitions.len() > GPT_ENTRY_COUNT as usize {
        return Err(GptError::TooManyEntries);
    }
    if is_zero_guid(&disk_guid) {
        return Err(GptError::InvalidHeader);
    }

    let table_bytes = GPT_ENTRY_COUNT as usize * GPT_ENTRY_SIZE;
    let table_blocks = table_bytes.div_ceil(device.block_size) as u64;
    let first_usable = 2 + table_blocks;
    let backup_header_lba = device.blocks - 1;
    let backup_entries_lba = backup_header_lba - table_blocks;
    let last_usable = backup_entries_lba - 1;
    if first_usable > last_usable {
        return Err(GptError::DiskTooSmall);
    }
    validate_specs(partitions, first_usable, last_usable)?;

    let entries_crc = entries_crc32(device.block_size, partitions);
    write_protective_mbr(device)?;
    write_entries(device, 2, partitions)?;
    write_entries(device, backup_entries_lba, partitions)?;
    write_header(
        device,
        1,
        backup_header_lba,
        first_usable,
        last_usable,
        disk_guid,
        2,
        entries_crc,
    )?;
    write_header(
        device,
        backup_header_lba,
        1,
        first_usable,
        last_usable,
        disk_guid,
        backup_entries_lba,
        entries_crc,
    )
}

#[derive(Clone, Copy)]
struct Header {
    first_usable_lba: u64,
    last_usable_lba: u64,
    disk_guid: [u8; 16],
    entries_lba: u64,
    entry_count: u32,
    entry_size: usize,
    entries_crc32: u32,
}

fn parse_header(data: &[u8], device: BlockDeviceInfo) -> Result<Header, GptError> {
    if &data[..8] != GPT_SIGNATURE {
        return Err(GptError::InvalidSignature);
    }
    let header_size = le_u32(data, 12) as usize;
    if le_u32(data, 8) != GPT_REVISION_1_0
        || !(GPT_HEADER_SIZE..=device.block_size).contains(&header_size)
        || le_u64(data, 24) != 1
        || le_u64(data, 32) != device.blocks - 1
    {
        return Err(GptError::InvalidHeader);
    }

    let expected_crc = le_u32(data, 16);
    let mut crc = Crc32::new();
    crc.update(&data[..16]);
    crc.update(&[0; 4]);
    crc.update(&data[20..header_size]);
    if crc.finish() != expected_crc {
        return Err(GptError::InvalidHeaderCrc);
    }

    let first_usable_lba = le_u64(data, 40);
    let last_usable_lba = le_u64(data, 48);
    let entries_lba = le_u64(data, 72);
    let entry_count = le_u32(data, 80);
    let entry_size = le_u32(data, 84) as usize;
    if entry_count == 0
        || entry_count > GPT_ENTRY_COUNT
        || entry_size < GPT_ENTRY_SIZE
        || !entry_size.is_power_of_two()
        || entry_size > device.block_size
        || first_usable_lba > last_usable_lba
        || last_usable_lba >= device.blocks - 1
        || is_zero_guid(&data[56..72])
    {
        return Err(GptError::InvalidHeader);
    }
    let table_bytes = (entry_count as usize)
        .checked_mul(entry_size)
        .ok_or(GptError::InvalidHeader)?;
    let table_blocks = table_bytes.div_ceil(device.block_size) as u64;
    let entries_end = entries_lba
        .checked_add(table_blocks)
        .ok_or(GptError::InvalidHeader)?;
    if entries_lba < 2 || entries_end > first_usable_lba {
        return Err(GptError::InvalidHeader);
    }

    let mut disk_guid = [0; 16];
    disk_guid.copy_from_slice(&data[56..72]);
    Ok(Header {
        first_usable_lba,
        last_usable_lba,
        disk_guid,
        entries_lba,
        entry_count,
        entry_size,
        entries_crc32: le_u32(data, 88),
    })
}

fn parse_partition(data: &[u8], index: u32) -> Result<Partition, GptError> {
    let first_lba = le_u64(data, 32);
    let last_lba = le_u64(data, 40);
    if first_lba == 0 || first_lba > last_lba || is_zero_guid(&data[16..32]) {
        return Err(GptError::InvalidPartition);
    }
    let mut partition = Partition::empty();
    partition.index = index;
    partition.type_guid.copy_from_slice(&data[..16]);
    partition.unique_guid.copy_from_slice(&data[16..32]);
    partition.first_lba = first_lba;
    partition.last_lba = last_lba;
    partition.attributes = le_u64(data, 48);
    for pair in data[56..128].chunks_exact(2) {
        let code = u16::from_le_bytes([pair[0], pair[1]]);
        if code == 0 {
            break;
        }
        partition.name[partition.name_len] = if code <= 0x7f { code as u8 } else { b'?' };
        partition.name_len += 1;
    }
    Ok(partition)
}

fn validate_device(device: BlockDeviceInfo) -> Result<(), GptError> {
    if device.block_size < 512
        || device.block_size > MAX_BLOCK_SIZE
        || !device.block_size.is_power_of_two()
    {
        return Err(GptError::UnsupportedBlockSize);
    }
    if device.blocks
        < 2 + (GPT_ENTRY_COUNT as usize * GPT_ENTRY_SIZE).div_ceil(device.block_size) as u64 * 2 + 1
    {
        return Err(GptError::DiskTooSmall);
    }
    Ok(())
}

fn validate_specs(
    partitions: &[PartitionSpec<'_>],
    first_usable: u64,
    last_usable: u64,
) -> Result<(), GptError> {
    for (index, partition) in partitions.iter().enumerate() {
        if is_zero_guid(&partition.type_guid)
            || is_zero_guid(&partition.unique_guid)
            || partition.first_lba < first_usable
            || partition.first_lba > partition.last_lba
            || partition.last_lba > last_usable
        {
            return Err(GptError::InvalidPartition);
        }
        for previous in &partitions[..index] {
            if partition.unique_guid == previous.unique_guid
                || (partition.first_lba <= previous.last_lba
                    && previous.first_lba <= partition.last_lba)
            {
                return Err(GptError::InvalidPartition);
            }
        }
    }
    Ok(())
}

fn write_protective_mbr(device: BlockDeviceInfo) -> Result<(), GptError> {
    let mut data = [0u8; MAX_BLOCK_SIZE];
    data[446 + 4] = 0xee;
    data[446 + 8..446 + 12].copy_from_slice(&1u32.to_le_bytes());
    let sectors = (device.blocks - 1).min(u32::MAX as u64) as u32;
    data[446 + 12..446 + 16].copy_from_slice(&sectors.to_le_bytes());
    data[510] = 0x55;
    data[511] = 0xaa;
    write_exact(device, 0, &data)
}

fn entries_crc32(block_size: usize, partitions: &[PartitionSpec<'_>]) -> u32 {
    let mut crc = Crc32::new();
    let mut data = [0u8; MAX_BLOCK_SIZE];
    let entries_per_block = block_size / GPT_ENTRY_SIZE;
    for block_index in 0..(GPT_ENTRY_COUNT as usize / entries_per_block) {
        data[..block_size].fill(0);
        for slot in 0..entries_per_block {
            let index = block_index * entries_per_block + slot;
            if let Some(spec) = partitions.get(index) {
                encode_partition(&mut data[slot * GPT_ENTRY_SIZE..][..GPT_ENTRY_SIZE], spec);
            }
        }
        crc.update(&data[..block_size]);
    }
    crc.finish()
}

fn write_entries(
    device: BlockDeviceInfo,
    start_lba: u64,
    partitions: &[PartitionSpec<'_>],
) -> Result<(), GptError> {
    let mut data = [0u8; MAX_BLOCK_SIZE];
    let entries_per_block = device.block_size / GPT_ENTRY_SIZE;
    let block_count = GPT_ENTRY_COUNT as usize / entries_per_block;
    for block_index in 0..block_count {
        data[..device.block_size].fill(0);
        for slot in 0..entries_per_block {
            let index = block_index * entries_per_block + slot;
            if let Some(spec) = partitions.get(index) {
                encode_partition(&mut data[slot * GPT_ENTRY_SIZE..][..GPT_ENTRY_SIZE], spec);
            }
        }
        write_exact(device, start_lba + block_index as u64, &data)?;
    }
    Ok(())
}

fn encode_partition(data: &mut [u8], spec: &PartitionSpec<'_>) {
    data[..16].copy_from_slice(&spec.type_guid);
    data[16..32].copy_from_slice(&spec.unique_guid);
    data[32..40].copy_from_slice(&spec.first_lba.to_le_bytes());
    data[40..48].copy_from_slice(&spec.last_lba.to_le_bytes());
    data[48..56].copy_from_slice(&spec.attributes.to_le_bytes());
    for (index, code) in spec.name.encode_utf16().take(36).enumerate() {
        let encoded = code.to_le_bytes();
        data[56 + index * 2..58 + index * 2].copy_from_slice(&encoded);
    }
}

#[allow(clippy::too_many_arguments)]
fn write_header(
    device: BlockDeviceInfo,
    current_lba: u64,
    backup_lba: u64,
    first_usable: u64,
    last_usable: u64,
    disk_guid: [u8; 16],
    entries_lba: u64,
    entries_crc: u32,
) -> Result<(), GptError> {
    let mut data = [0u8; MAX_BLOCK_SIZE];
    data[..8].copy_from_slice(GPT_SIGNATURE);
    data[8..12].copy_from_slice(&GPT_REVISION_1_0.to_le_bytes());
    data[12..16].copy_from_slice(&(GPT_HEADER_SIZE as u32).to_le_bytes());
    data[24..32].copy_from_slice(&current_lba.to_le_bytes());
    data[32..40].copy_from_slice(&backup_lba.to_le_bytes());
    data[40..48].copy_from_slice(&first_usable.to_le_bytes());
    data[48..56].copy_from_slice(&last_usable.to_le_bytes());
    data[56..72].copy_from_slice(&disk_guid);
    data[72..80].copy_from_slice(&entries_lba.to_le_bytes());
    data[80..84].copy_from_slice(&GPT_ENTRY_COUNT.to_le_bytes());
    data[84..88].copy_from_slice(&(GPT_ENTRY_SIZE as u32).to_le_bytes());
    data[88..92].copy_from_slice(&entries_crc.to_le_bytes());
    let crc = crc32(&data[..GPT_HEADER_SIZE]);
    data[16..20].copy_from_slice(&crc.to_le_bytes());
    write_exact(device, current_lba, &data)
}

fn read_exact(
    device: BlockDeviceInfo,
    lba: u64,
    data: &mut [u8; MAX_BLOCK_SIZE],
) -> Result<(), GptError> {
    let bytes = block::read_block(device.name, lba, &mut data[..device.block_size])?;
    if bytes != device.block_size {
        return Err(GptError::Block(BlockError::Io));
    }
    Ok(())
}

fn write_exact(
    device: BlockDeviceInfo,
    lba: u64,
    data: &[u8; MAX_BLOCK_SIZE],
) -> Result<(), GptError> {
    let bytes = block::write_block(device.name, lba, &data[..device.block_size])?;
    if bytes != device.block_size {
        return Err(GptError::Block(BlockError::Io));
    }
    Ok(())
}

fn is_zero_guid(guid: &[u8]) -> bool {
    guid.iter().all(|byte| *byte == 0)
}

fn le_u32(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap())
}

fn le_u64(data: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(data[offset..offset + 8].try_into().unwrap())
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = Crc32::new();
    crc.update(data);
    crc.finish()
}

struct Crc32(u32);

impl Crc32 {
    const fn new() -> Self {
        Self(0xffff_ffff)
    }

    fn update(&mut self, data: &[u8]) {
        for byte in data {
            self.0 ^= *byte as u32;
            for _ in 0..8 {
                self.0 = (self.0 >> 1) ^ (0xedb8_8320 & (0u32.wrapping_sub(self.0 & 1)));
            }
        }
    }

    const fn finish(self) -> u32 {
        !self.0
    }
}
