use alloc::string::String;
use alloc::vec::Vec;

use crate::drivers::block::{self, BlockDeviceInfo};
use crate::fs::vfs::{
    DirEntry, FileHandle, FileStat, FileSystem, FileType, OpenFlags, Result, VfsError,
    VirtualFileSystem,
};

const MAGIC: &[u8; 8] = b"DUNITFS1";
const VERSION: u32 = 1;
const BLOCK_SIZE: usize = 512;
const MAX_NODES: usize = 64;
const NODE_SIZE: usize = 128;
const PATH_SIZE: usize = 96;
const METADATA_START: u64 = 1;
const METADATA_BLOCKS: u64 = (MAX_NODES * NODE_SIZE / BLOCK_SIZE) as u64;
const DATA_START: u64 = METADATA_START + METADATA_BLOCKS;

static mut MOUNTED_FS: Option<DunitFs> = None;

pub fn is_mounted() -> bool {
    unsafe { MOUNTED_FS.is_some() }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DunitFsError {
    InvalidBlockSize,
    PartitionTooSmall,
    InvalidSuperblock,
    CorruptMetadata,
    AlreadyMounted,
    Io,
    Vfs(VfsError),
}

impl DunitFsError {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidBlockSize => "block size must be 512 bytes",
            Self::PartitionTooSmall => "partition is too small",
            Self::InvalidSuperblock => "invalid DunitFS superblock",
            Self::CorruptMetadata => "corrupt DunitFS metadata",
            Self::AlreadyMounted => "DunitFS is already mounted",
            Self::Io => "block I/O error",
            Self::Vfs(_) => "VFS mount error",
        }
    }
}

#[derive(Clone)]
struct Node {
    path: String,
    file_type: FileType,
    size: u64,
    first_block: u64,
    block_count: u64,
}

struct OpenHandle {
    node: usize,
    offset: usize,
    flags: OpenFlags,
}

pub struct DunitFs {
    device: BlockDeviceInfo,
    partition_start: u64,
    partition_blocks: u64,
    nodes: [Option<Node>; MAX_NODES],
    handles: Vec<(FileHandle, OpenHandle)>,
    next_handle: FileHandle,
}

pub fn format(
    device: BlockDeviceInfo,
    partition_start: u64,
    partition_blocks: u64,
) -> core::result::Result<(), DunitFsError> {
    if unsafe { MOUNTED_FS.is_some() } {
        return Err(DunitFsError::AlreadyMounted);
    }
    validate_geometry(device, partition_start, partition_blocks)?;
    let mut block_data = [0u8; BLOCK_SIZE];
    write_partition_block(device, partition_start, partition_blocks, 0, &block_data)?;
    for relative in METADATA_START..DATA_START {
        write_partition_block(
            device,
            partition_start,
            partition_blocks,
            relative,
            &block_data,
        )?;
    }

    block_data[..8].copy_from_slice(MAGIC);
    put_u32(&mut block_data, 8, VERSION);
    put_u32(&mut block_data, 12, BLOCK_SIZE as u32);
    put_u64(&mut block_data, 16, partition_blocks);
    put_u64(&mut block_data, 24, METADATA_START);
    put_u32(&mut block_data, 32, METADATA_BLOCKS as u32);
    put_u32(&mut block_data, 36, MAX_NODES as u32);
    put_u64(&mut block_data, 40, DATA_START);
    put_u64(&mut block_data, 48, 1);
    let checksum = crc32(&block_data[..56]);
    put_u32(&mut block_data, 56, checksum);
    write_partition_block(device, partition_start, partition_blocks, 0, &block_data)?;
    Ok(())
}

pub fn mount_global(
    vfs: &mut VirtualFileSystem,
    path: &str,
    device: BlockDeviceInfo,
    partition_start: u64,
    partition_blocks: u64,
) -> core::result::Result<(), DunitFsError> {
    unsafe {
        if MOUNTED_FS.is_some() {
            return Err(DunitFsError::AlreadyMounted);
        }
        MOUNTED_FS = Some(DunitFs::load(device, partition_start, partition_blocks)?);
        let fs = MOUNTED_FS.as_mut().ok_or(DunitFsError::Io)?;
        if let Err(error) = vfs.mount(path, fs) {
            MOUNTED_FS = None;
            return Err(DunitFsError::Vfs(error));
        }
    }
    Ok(())
}

pub fn auto_mount(vfs: &mut VirtualFileSystem) -> bool {
    if unsafe { MOUNTED_FS.is_some() } {
        return true;
    }

    let mut devices = [None; 8];
    let count = block::snapshot(&mut devices);
    for device in devices[..count].iter().flatten().copied() {
        let Ok(table) = crate::storage::gpt::read(device) else {
            continue;
        };
        for partition in table.partitions.iter().flatten() {
            if mount_global(
                vfs,
                "/persist",
                device,
                partition.first_lba,
                partition.blocks(),
            )
            .is_ok()
            {
                crate::serial_write("[DUNITFS] auto-mounted ");
                crate::serial_write(device.name);
                crate::serial_write(" at /persist\r\n");
                return true;
            }
        }
    }
    false
}

impl DunitFs {
    fn load(
        device: BlockDeviceInfo,
        partition_start: u64,
        partition_blocks: u64,
    ) -> core::result::Result<Self, DunitFsError> {
        validate_geometry(device, partition_start, partition_blocks)?;
        let mut block_data = [0u8; BLOCK_SIZE];
        read_partition_block(
            device,
            partition_start,
            partition_blocks,
            0,
            &mut block_data,
        )?;
        if &block_data[..8] != MAGIC
            || get_u32(&block_data, 8) != VERSION
            || get_u32(&block_data, 12) != BLOCK_SIZE as u32
            || get_u64(&block_data, 16) != partition_blocks
            || get_u64(&block_data, 24) != METADATA_START
            || get_u32(&block_data, 32) != METADATA_BLOCKS as u32
            || get_u32(&block_data, 36) != MAX_NODES as u32
            || get_u64(&block_data, 40) != DATA_START
            || get_u32(&block_data, 56) != crc32(&block_data[..56])
        {
            return Err(DunitFsError::InvalidSuperblock);
        }

        let mut fs = Self {
            device,
            partition_start,
            partition_blocks,
            nodes: core::array::from_fn(|_| None),
            handles: Vec::new(),
            next_handle: 1,
        };
        fs.load_nodes()?;
        Ok(fs)
    }

    fn load_nodes(&mut self) -> core::result::Result<(), DunitFsError> {
        let mut block_data = [0u8; BLOCK_SIZE];
        for metadata_block in 0..METADATA_BLOCKS {
            self.read_block(METADATA_START + metadata_block, &mut block_data)?;
            for slot in 0..(BLOCK_SIZE / NODE_SIZE) {
                let index = metadata_block as usize * (BLOCK_SIZE / NODE_SIZE) + slot;
                let entry = &block_data[slot * NODE_SIZE..(slot + 1) * NODE_SIZE];
                if entry[0] == 0 {
                    continue;
                }
                if get_u32(entry, 124) != crc32(&entry[..124]) {
                    return Err(DunitFsError::CorruptMetadata);
                }
                let file_type = match entry[1] {
                    1 => FileType::File,
                    2 => FileType::Directory,
                    _ => return Err(DunitFsError::CorruptMetadata),
                };
                let path_len = get_u16(entry, 2) as usize;
                if path_len == 0 || path_len > PATH_SIZE {
                    return Err(DunitFsError::CorruptMetadata);
                }
                let path = core::str::from_utf8(&entry[28..28 + path_len])
                    .map_err(|_| DunitFsError::CorruptMetadata)?;
                let node = Node {
                    path: String::from(path),
                    file_type,
                    size: get_u64(entry, 4),
                    first_block: get_u64(entry, 12),
                    block_count: get_u64(entry, 20),
                };
                if node.size > node.block_count.saturating_mul(BLOCK_SIZE as u64)
                    || (node.file_type == FileType::Directory
                        && (node.size != 0 || node.block_count != 0))
                    || (node.block_count != 0
                        && (node.first_block < DATA_START
                            || node.first_block.checked_add(node.block_count).is_none()
                            || node.first_block + node.block_count > self.partition_blocks))
                    || self.node_index(&node.path).is_some()
                {
                    return Err(DunitFsError::CorruptMetadata);
                }
                self.nodes[index] = Some(node);
            }
        }
        for index in 0..MAX_NODES {
            let Some(node) = &self.nodes[index] else {
                continue;
            };
            for other in self.nodes[..index].iter().flatten() {
                if ranges_overlap(node, other) {
                    return Err(DunitFsError::CorruptMetadata);
                }
            }
        }
        Ok(())
    }

    fn persist_node(&self, index: usize) -> Result<()> {
        let entries_per_block = BLOCK_SIZE / NODE_SIZE;
        let relative = METADATA_START + (index / entries_per_block) as u64;
        let slot = index % entries_per_block;
        let mut block_data = [0u8; BLOCK_SIZE];
        self.read_block(relative, &mut block_data)
            .map_err(|_| VfsError::IoError)?;
        let entry = &mut block_data[slot * NODE_SIZE..(slot + 1) * NODE_SIZE];
        entry.fill(0);
        if let Some(node) = &self.nodes[index] {
            entry[0] = 1;
            entry[1] = match node.file_type {
                FileType::File => 1,
                FileType::Directory => 2,
                FileType::Device => return Err(VfsError::Unsupported),
            };
            put_u16(entry, 2, node.path.len() as u16);
            put_u64(entry, 4, node.size);
            put_u64(entry, 12, node.first_block);
            put_u64(entry, 20, node.block_count);
            entry[28..28 + node.path.len()].copy_from_slice(node.path.as_bytes());
            let checksum = crc32(&entry[..124]);
            put_u32(entry, 124, checksum);
        }
        self.write_block(relative, &block_data)
            .map_err(|_| VfsError::IoError)
    }

    fn read_block(
        &self,
        relative: u64,
        data: &mut [u8; BLOCK_SIZE],
    ) -> core::result::Result<(), DunitFsError> {
        read_partition_block(
            self.device,
            self.partition_start,
            self.partition_blocks,
            relative,
            data,
        )
    }

    fn write_block(
        &self,
        relative: u64,
        data: &[u8; BLOCK_SIZE],
    ) -> core::result::Result<(), DunitFsError> {
        write_partition_block(
            self.device,
            self.partition_start,
            self.partition_blocks,
            relative,
            data,
        )
    }

    fn clean(path: &str) -> &str {
        path.trim_matches('/')
    }

    fn node_index(&self, path: &str) -> Option<usize> {
        let clean = Self::clean(path);
        self.nodes
            .iter()
            .position(|entry| entry.as_ref().map(|node| node.path.as_str()) == Some(clean))
    }

    fn free_node_index(&self) -> Option<usize> {
        self.nodes.iter().position(Option::is_none)
    }

    fn parent_exists(&self, path: &str) -> bool {
        let Some(separator) = path.rfind('/') else {
            return true;
        };
        let parent = &path[..separator];
        self.node_index(parent)
            .and_then(|index| self.nodes[index].as_ref())
            .map(|node| node.file_type == FileType::Directory)
            .unwrap_or(false)
    }

    fn handle_index(&self, handle: FileHandle) -> Option<usize> {
        self.handles.iter().position(|entry| entry.0 == handle)
    }

    fn range_free(&self, start: u64, blocks: u64, except: usize) -> bool {
        let Some(end) = start.checked_add(blocks) else {
            return false;
        };
        if start < DATA_START || end > self.partition_blocks {
            return false;
        }
        !self.nodes.iter().enumerate().any(|(index, entry)| {
            if index == except {
                return false;
            }
            let Some(node) = entry else {
                return false;
            };
            let node_end = node.first_block.saturating_add(node.block_count);
            node.block_count != 0 && start < node_end && node.first_block < end
        })
    }

    fn allocate_range(&self, blocks: u64, except: usize) -> Option<u64> {
        let mut candidate = DATA_START;
        while candidate.saturating_add(blocks) <= self.partition_blocks {
            if self.range_free(candidate, blocks, except) {
                return Some(candidate);
            }
            let mut next = candidate + 1;
            for (index, entry) in self.nodes.iter().enumerate() {
                if index == except {
                    continue;
                }
                if let Some(node) = entry {
                    let node_end = node.first_block.saturating_add(node.block_count);
                    let candidate_end = candidate.saturating_add(blocks);
                    if node.block_count != 0
                        && candidate < node_end
                        && node.first_block < candidate_end
                    {
                        next = next.max(node_end);
                    }
                }
            }
            candidate = next;
        }
        None
    }

    fn ensure_capacity(&mut self, index: usize, required_blocks: u64) -> Result<()> {
        let node = self.nodes[index].as_ref().ok_or(VfsError::NotFound)?;
        if required_blocks <= node.block_count {
            return Ok(());
        }
        let old_first = node.first_block;
        let old_blocks = node.block_count;
        if old_blocks != 0 && self.range_free(old_first, required_blocks, index) {
            self.nodes[index].as_mut().unwrap().block_count = required_blocks;
            return Ok(());
        }
        let new_first = self
            .allocate_range(required_blocks, index)
            .ok_or(VfsError::IoError)?;
        let mut sector = [0u8; BLOCK_SIZE];
        for offset in 0..old_blocks {
            self.read_block(old_first + offset, &mut sector)
                .map_err(|_| VfsError::IoError)?;
            self.write_block(new_first + offset, &sector)
                .map_err(|_| VfsError::IoError)?;
        }
        let node = self.nodes[index].as_mut().unwrap();
        node.first_block = new_first;
        node.block_count = required_blocks;
        Ok(())
    }

    fn read_node(&self, index: usize, offset: usize, output: &mut [u8]) -> Result<usize> {
        let node = self.nodes[index].as_ref().ok_or(VfsError::NotFound)?;
        if offset >= node.size as usize {
            return Ok(0);
        }
        let length = output.len().min(node.size as usize - offset);
        let mut done = 0usize;
        let mut sector = [0u8; BLOCK_SIZE];
        while done < length {
            let position = offset + done;
            let sector_index = position / BLOCK_SIZE;
            let within = position % BLOCK_SIZE;
            self.read_block(node.first_block + sector_index as u64, &mut sector)
                .map_err(|_| VfsError::IoError)?;
            let count = (length - done).min(BLOCK_SIZE - within);
            output[done..done + count].copy_from_slice(&sector[within..within + count]);
            done += count;
        }
        Ok(length)
    }

    fn write_node(&mut self, index: usize, offset: usize, input: &[u8]) -> Result<usize> {
        let end = offset.checked_add(input.len()).ok_or(VfsError::IoError)?;
        let required_blocks = end.div_ceil(BLOCK_SIZE) as u64;
        self.ensure_capacity(index, required_blocks)?;
        let first_block = self.nodes[index].as_ref().unwrap().first_block;
        let mut done = 0usize;
        let mut sector = [0u8; BLOCK_SIZE];
        while done < input.len() {
            let position = offset + done;
            let sector_index = position / BLOCK_SIZE;
            let within = position % BLOCK_SIZE;
            let count = (input.len() - done).min(BLOCK_SIZE - within);
            let lba = first_block + sector_index as u64;
            if within != 0 || count != BLOCK_SIZE {
                self.read_block(lba, &mut sector)
                    .map_err(|_| VfsError::IoError)?;
            } else {
                sector.fill(0);
            }
            sector[within..within + count].copy_from_slice(&input[done..done + count]);
            self.write_block(lba, &sector)
                .map_err(|_| VfsError::IoError)?;
            done += count;
        }
        self.nodes[index].as_mut().unwrap().size =
            self.nodes[index].as_ref().unwrap().size.max(end as u64);
        self.persist_node(index)?;
        Ok(input.len())
    }
}

impl FileSystem for DunitFs {
    fn open(&mut self, path: &str, flags: OpenFlags) -> Result<FileHandle> {
        if !flags.is_valid() {
            return Err(VfsError::PermissionDenied);
        }
        let clean = Self::clean(path);
        let index = match self.node_index(clean) {
            Some(index) => index,
            None if flags.create() => {
                self.create(clean)?;
                self.node_index(clean).ok_or(VfsError::IoError)?
            }
            None => return Err(VfsError::NotFound),
        };
        if self.nodes[index].as_ref().unwrap().file_type != FileType::File {
            return Err(VfsError::IsADirectory);
        }
        if flags.trunc() {
            self.truncate(clean)?;
        }
        let offset = if flags.append() {
            self.nodes[index].as_ref().unwrap().size as usize
        } else {
            0
        };
        let handle = self.next_handle;
        self.next_handle += 1;
        self.handles.push((
            handle,
            OpenHandle {
                node: index,
                offset,
                flags,
            },
        ));
        Ok(handle)
    }

    fn read(&mut self, handle: FileHandle, buf: &mut [u8]) -> Result<usize> {
        let handle_index = self
            .handle_index(handle)
            .ok_or(VfsError::InvalidDescriptor)?;
        if !self.handles[handle_index].1.flags.can_read() {
            return Err(VfsError::PermissionDenied);
        }
        let node = self.handles[handle_index].1.node;
        let offset = self.handles[handle_index].1.offset;
        let read = self.read_node(node, offset, buf)?;
        self.handles[handle_index].1.offset += read;
        Ok(read)
    }

    fn write(&mut self, handle: FileHandle, buf: &[u8]) -> Result<usize> {
        let handle_index = self
            .handle_index(handle)
            .ok_or(VfsError::InvalidDescriptor)?;
        if !self.handles[handle_index].1.flags.can_write() {
            return Err(VfsError::PermissionDenied);
        }
        let node = self.handles[handle_index].1.node;
        let offset = if self.handles[handle_index].1.flags.append() {
            self.nodes[node].as_ref().unwrap().size as usize
        } else {
            self.handles[handle_index].1.offset
        };
        let written = self.write_node(node, offset, buf)?;
        self.handles[handle_index].1.offset = offset + written;
        Ok(written)
    }

    fn close(&mut self, handle: FileHandle) -> Result<()> {
        let index = self
            .handle_index(handle)
            .ok_or(VfsError::InvalidDescriptor)?;
        self.handles.remove(index);
        Ok(())
    }

    fn readdir(&mut self, path: &str) -> Result<Vec<DirEntry>> {
        let mut buffer = [DirEntry::empty(); 64];
        let count = self.readdir_into(path, &mut buffer)?;
        Ok(buffer[..count].to_vec())
    }

    fn readdir_into(&mut self, path: &str, entries: &mut [DirEntry]) -> Result<usize> {
        let clean = Self::clean(path);
        if !clean.is_empty() {
            let index = self.node_index(clean).ok_or(VfsError::NotFound)?;
            if self.nodes[index].as_ref().unwrap().file_type != FileType::Directory {
                return Err(VfsError::NotADirectory);
            }
        }
        let mut count = 0usize;
        for node in self.nodes.iter().flatten() {
            if is_direct_child(clean, &node.path) && count < entries.len() {
                entries[count] = DirEntry::new(basename(&node.path), node.file_type);
                count += 1;
            }
        }
        Ok(count)
    }

    fn create(&mut self, path: &str) -> Result<()> {
        self.create_node(path, FileType::File)
    }

    fn mkdir(&mut self, path: &str) -> Result<()> {
        self.create_node(path, FileType::Directory)
    }

    fn remove(&mut self, path: &str) -> Result<()> {
        let clean = Self::clean(path);
        let index = self.node_index(clean).ok_or(VfsError::NotFound)?;
        if self.nodes[index].as_ref().unwrap().file_type != FileType::File {
            return Err(VfsError::IsADirectory);
        }
        self.nodes[index] = None;
        self.handles.retain(|entry| entry.1.node != index);
        self.persist_node(index)
    }

    fn truncate(&mut self, path: &str) -> Result<()> {
        let index = self.node_index(path).ok_or(VfsError::NotFound)?;
        let node = self.nodes[index].as_mut().unwrap();
        if node.file_type != FileType::File {
            return Err(VfsError::IsADirectory);
        }
        node.size = 0;
        node.first_block = 0;
        node.block_count = 0;
        self.persist_node(index)
    }

    fn stat(&mut self, path: &str) -> Result<FileStat> {
        let clean = Self::clean(path);
        if clean.is_empty() {
            return Ok(FileStat {
                file_type: FileType::Directory,
                size: 0,
            });
        }
        let node = self.nodes[self.node_index(clean).ok_or(VfsError::NotFound)?]
            .as_ref()
            .unwrap();
        Ok(FileStat {
            file_type: node.file_type,
            size: node.size as usize,
        })
    }
}

impl DunitFs {
    fn create_node(&mut self, path: &str, file_type: FileType) -> Result<()> {
        let clean = Self::clean(path);
        if clean.is_empty() || clean.len() > PATH_SIZE || !self.parent_exists(clean) {
            return Err(VfsError::InvalidPath);
        }
        if self.node_index(clean).is_some() {
            return Err(VfsError::AlreadyExists);
        }
        let index = self.free_node_index().ok_or(VfsError::IoError)?;
        self.nodes[index] = Some(Node {
            path: String::from(clean),
            file_type,
            size: 0,
            first_block: 0,
            block_count: 0,
        });
        self.persist_node(index)
    }
}

fn validate_geometry(
    device: BlockDeviceInfo,
    partition_start: u64,
    partition_blocks: u64,
) -> core::result::Result<(), DunitFsError> {
    if device.block_size != BLOCK_SIZE {
        return Err(DunitFsError::InvalidBlockSize);
    }
    if partition_blocks <= DATA_START
        || partition_start
            .checked_add(partition_blocks)
            .map(|end| end > device.blocks)
            .unwrap_or(true)
    {
        return Err(DunitFsError::PartitionTooSmall);
    }
    Ok(())
}

fn read_partition_block(
    device: BlockDeviceInfo,
    start: u64,
    blocks: u64,
    relative: u64,
    data: &mut [u8; BLOCK_SIZE],
) -> core::result::Result<(), DunitFsError> {
    if relative >= blocks {
        return Err(DunitFsError::Io);
    }
    let read =
        block::read_block(device.name, start + relative, data).map_err(|_| DunitFsError::Io)?;
    if read != BLOCK_SIZE {
        return Err(DunitFsError::Io);
    }
    Ok(())
}

fn write_partition_block(
    device: BlockDeviceInfo,
    start: u64,
    blocks: u64,
    relative: u64,
    data: &[u8; BLOCK_SIZE],
) -> core::result::Result<(), DunitFsError> {
    if relative >= blocks {
        return Err(DunitFsError::Io);
    }
    let written =
        block::write_block(device.name, start + relative, data).map_err(|_| DunitFsError::Io)?;
    if written != BLOCK_SIZE {
        return Err(DunitFsError::Io);
    }
    Ok(())
}

fn ranges_overlap(left: &Node, right: &Node) -> bool {
    left.block_count != 0
        && right.block_count != 0
        && left.first_block < right.first_block + right.block_count
        && right.first_block < left.first_block + left.block_count
}

fn is_direct_child(parent: &str, child: &str) -> bool {
    if parent.is_empty() {
        return !child.is_empty() && !child.contains('/');
    }
    child
        .strip_prefix(parent)
        .and_then(|suffix| suffix.strip_prefix('/'))
        .map(|suffix| !suffix.is_empty() && !suffix.contains('/'))
        .unwrap_or(false)
}

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn get_u16(data: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(data[offset..offset + 2].try_into().unwrap())
}

fn get_u32(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap())
}

fn get_u64(data: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(data[offset..offset + 8].try_into().unwrap())
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
