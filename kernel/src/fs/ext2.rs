use super::vfs::{FileSystem, FileHandle, OpenFlags, Result, VfsError};
use crate::drivers::ata::AtaDrive;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};

const EXT2_SUPER_MAGIC: u16 = 0xEF53;
const EXT2_ROOT_INO: u32 = 2;
const SECTOR_SIZE: usize = 512;

#[repr(C, packed)]
struct Ext2Superblock {
    inodes_count: u32,
    blocks_count: u32,
    r_blocks_count: u32,
    free_blocks_count: u32,
    free_inodes_count: u32,
    first_data_block: u32,
    log_block_size: u32,
    log_frag_size: u32,
    blocks_per_group: u32,
    frags_per_group: u32,
    inodes_per_group: u32,
    mtime: u32,
    wtime: u32,
    mnt_count: u16,
    max_mnt_count: u16,
    magic: u16,
    state: u16,
    errors: u16,
    minor_rev_level: u16,
    lastcheck: u32,
    checkinterval: u32,
    creator_os: u32,
    rev_level: u32,
    def_resuid: u16,
    def_resgid: u16,
}

#[repr(C, packed)]
struct Ext2Inode {
    mode: u16,
    uid: u16,
    size: u32,
    atime: u32,
    ctime: u32,
    mtime: u32,
    dtime: u32,
    gid: u16,
    links_count: u16,
    blocks: u32,
    flags: u32,
    osd1: u32,
    block: [u32; 15],
    generation: u32,
    file_acl: u32,
    dir_acl: u32,
    faddr: u32,
    osd2: [u8; 12],
}

#[repr(C, packed)]
struct Ext2DirEntry {
    inode: u32,
    rec_len: u16,
    name_len: u8,
    file_type: u8,
}

pub struct Ext2Fs {
    drive: &'static AtaDrive,
    block_size: u32,
    inodes_per_group: u32,
    blocks_per_group: u32,
    inode_size: u32,
    first_data_block: u32,
    next_handle: AtomicUsize,
    open_files: BTreeMap<FileHandle, (u32, usize)>,
}

impl Ext2Fs {
    pub fn new(drive: &'static AtaDrive) -> Option<Self> {
        let mut sb_buf = [0u8; 1024];
        
        if !drive.read_sector(2, &mut sb_buf[0..512].try_into().unwrap()) {
            return None;
        }
        if !drive.read_sector(3, &mut sb_buf[512..1024].try_into().unwrap()) {
            return None;
        }

        let sb = unsafe { &*(sb_buf.as_ptr() as *const Ext2Superblock) };
        
        if sb.magic != EXT2_SUPER_MAGIC {
            return None;
        }

        let block_size = 1024 << sb.log_block_size;

        Some(Self {
            drive,
            block_size,
            inodes_per_group: sb.inodes_per_group,
            blocks_per_group: sb.blocks_per_group,
            inode_size: 128,
            first_data_block: sb.first_data_block,
            next_handle: AtomicUsize::new(1),
            open_files: BTreeMap::new(),
        })
    }

    fn read_block(&self, block_num: u32, buffer: &mut [u8]) -> bool {
        let sector_start = (block_num * self.block_size / SECTOR_SIZE as u32) as u32;
        let sectors_per_block = self.block_size / SECTOR_SIZE as u32;

        for i in 0..sectors_per_block {
            let mut sector = [0u8; 512];
            if !self.drive.read_sector(sector_start + i, &mut sector) {
                return false;
            }
            let offset = (i * SECTOR_SIZE as u32) as usize;
            let len = SECTOR_SIZE.min(buffer.len() - offset);
            buffer[offset..offset + len].copy_from_slice(&sector[..len]);
        }
        true
    }

    fn write_block(&self, block_num: u32, buffer: &[u8]) -> bool {
        let sector_start = (block_num * self.block_size / SECTOR_SIZE as u32) as u32;
        let sectors_per_block = self.block_size / SECTOR_SIZE as u32;

        for i in 0..sectors_per_block {
            let offset = (i * SECTOR_SIZE as u32) as usize;
            let len = SECTOR_SIZE.min(buffer.len() - offset);
            let mut sector = [0u8; 512];
            sector[..len].copy_from_slice(&buffer[offset..offset + len]);
            if !self.drive.write_sector(sector_start + i, &sector) {
                return false;
            }
        }
        true
    }

    fn read_inode(&self, inode_num: u32) -> Option<Ext2Inode> {
        let group = (inode_num - 1) / self.inodes_per_group;
        let index = (inode_num - 1) % self.inodes_per_group;
        
        let bgd_block = self.first_data_block + 1;
        let mut bgd_buf = Vec::new();
        bgd_buf.resize(self.block_size as usize, 0);
        
        if !self.read_block(bgd_block, &mut bgd_buf) {
            return None;
        }

        let inode_table = u32::from_le_bytes([
            bgd_buf[(group * 32) as usize],
            bgd_buf[(group * 32 + 1) as usize],
            bgd_buf[(group * 32 + 2) as usize],
            bgd_buf[(group * 32 + 3) as usize],
        ]);

        let inode_offset = index * self.inode_size;
        let inode_block = inode_table + inode_offset / self.block_size;
        let offset_in_block = (inode_offset % self.block_size) as usize;

        let mut block_buf = Vec::new();
        block_buf.resize(self.block_size as usize, 0);
        
        if !self.read_block(inode_block, &mut block_buf) {
            return None;
        }

        let inode_ptr = &block_buf[offset_in_block] as *const u8 as *const Ext2Inode;
        Some(unsafe { core::ptr::read_unaligned(inode_ptr) })
    }

    fn find_entry(&self, dir_inode: &Ext2Inode, name: &str) -> Option<u32> {
        let mut block_buf = Vec::new();
        block_buf.resize(self.block_size as usize, 0);

        for i in 0..12 {
            let block_num = dir_inode.block[i];
            if block_num == 0 {
                break;
            }

            if !self.read_block(block_num, &mut block_buf) {
                continue;
            }

            let mut offset = 0;
            while offset < self.block_size as usize {
                let entry_ptr = &block_buf[offset] as *const u8 as *const Ext2DirEntry;
                let entry = unsafe { &*entry_ptr };

                if entry.inode == 0 || entry.rec_len == 0 {
                    break;
                }

                let name_bytes = &block_buf[offset + 8..offset + 8 + entry.name_len as usize];
                if let Ok(entry_name) = core::str::from_utf8(name_bytes) {
                    if entry_name == name {
                        return Some(entry.inode);
                    }
                }

                offset += entry.rec_len as usize;
            }
        }
        None
    }

    fn resolve_path(&self, path: &str) -> Option<u32> {
        if path.is_empty() || path == "/" {
            return Some(EXT2_ROOT_INO);
        }

        let mut current_inode = EXT2_ROOT_INO;
        let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();

        for part in parts {
            if part.is_empty() {
                continue;
            }

            let inode = self.read_inode(current_inode)?;
            current_inode = self.find_entry(&inode, part)?;
        }

        Some(current_inode)
    }

    fn read_file_data(&self, inode: &Ext2Inode, offset: usize, buf: &mut [u8]) -> usize {
        let file_size = inode.size as usize;
        if offset >= file_size {
            return 0;
        }

        let to_read = buf.len().min(file_size - offset);
        let start_block = offset / self.block_size as usize;
        let block_offset = offset % self.block_size as usize;

        let mut bytes_read = 0;
        let mut block_buf = Vec::new();
        block_buf.resize(self.block_size as usize, 0);

        for i in start_block..12 {
            if bytes_read >= to_read {
                break;
            }

            let block_num = inode.block[i];
            if block_num == 0 {
                break;
            }

            if !self.read_block(block_num, &mut block_buf) {
                break;
            }

            let copy_offset = if i == start_block { block_offset } else { 0 };
            let copy_len = (self.block_size as usize - copy_offset).min(to_read - bytes_read);

            buf[bytes_read..bytes_read + copy_len]
                .copy_from_slice(&block_buf[copy_offset..copy_offset + copy_len]);

            bytes_read += copy_len;
        }

        bytes_read
    }

    fn list_directory(&self, inode: &Ext2Inode) -> Vec<String> {
        let mut entries = Vec::new();
        let mut block_buf = Vec::new();
        block_buf.resize(self.block_size as usize, 0);

        for i in 0..12 {
            let block_num = inode.block[i];
            if block_num == 0 {
                break;
            }

            if !self.read_block(block_num, &mut block_buf) {
                continue;
            }

            let mut offset = 0;
            while offset < self.block_size as usize {
                let entry_ptr = &block_buf[offset] as *const u8 as *const Ext2DirEntry;
                let entry = unsafe { &*entry_ptr };

                if entry.inode == 0 || entry.rec_len == 0 {
                    break;
                }

                let name_bytes = &block_buf[offset + 8..offset + 8 + entry.name_len as usize];
                if let Ok(entry_name) = core::str::from_utf8(name_bytes) {
                    if entry_name != "." && entry_name != ".." {
                        entries.push(String::from(entry_name));
                    }
                }

                offset += entry.rec_len as usize;
            }
        }

        entries
    }
}

impl FileSystem for Ext2Fs {
    fn open(&mut self, path: &str, _flags: OpenFlags) -> Result<FileHandle> {
        let inode_num = self.resolve_path(path).ok_or(VfsError::NotFound)?;
        let handle = self.next_handle.fetch_add(1, Ordering::SeqCst);
        self.open_files.insert(handle, (inode_num, 0));
        Ok(handle)
    }

    fn read(&mut self, handle: FileHandle, buf: &mut [u8]) -> Result<usize> {
        let (inode_num, offset) = self.open_files.get(&handle)
            .copied()
            .ok_or(VfsError::InvalidDescriptor)?;
        
        let inode = self.read_inode(inode_num).ok_or(VfsError::IoError)?;
        let bytes_read = self.read_file_data(&inode, offset, buf);
        
        if let Some(entry) = self.open_files.get_mut(&handle) {
            entry.1 += bytes_read;
        }
        
        Ok(bytes_read)
    }

    fn write(&mut self, _handle: FileHandle, _buf: &[u8]) -> Result<usize> {
        Err(VfsError::PermissionDenied)
    }

    fn close(&mut self, handle: FileHandle) -> Result<()> {
        self.open_files.remove(&handle);
        Ok(())
    }

    fn list_dir(&self, path: &str) -> Result<Vec<String>> {
        let inode_num = self.resolve_path(path).ok_or(VfsError::NotFound)?;
        let inode = self.read_inode(inode_num).ok_or(VfsError::IoError)?;
        Ok(self.list_directory(&inode))
    }
}
