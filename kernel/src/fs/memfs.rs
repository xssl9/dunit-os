use super::vfs::{
    DirEntry, FileHandle, FileStat, FileSystem, FileType, OpenFlags, Result, VfsError,
};
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};

const BASE_DIRS: [&str; 8] = ["kernel", "proc", "app", "assets", "dev", "cfg", "usr", "tmp"];

struct MemNode {
    path: String,
    file_type: FileType,
    data: MemData,
}

enum MemData {
    Owned(Vec<u8>),
    Static(&'static [u8]),
}

impl MemData {
    fn as_slice(&self) -> &[u8] {
        match self {
            Self::Owned(data) => data.as_slice(),
            Self::Static(data) => data,
        }
    }

    fn len(&self) -> usize {
        self.as_slice().len()
    }

    fn clear(&mut self) -> Result<()> {
        match self {
            Self::Owned(data) => {
                data.clear();
                Ok(())
            }
            Self::Static(_) => Err(VfsError::PermissionDenied),
        }
    }

    fn resize(&mut self, len: usize) -> Result<()> {
        match self {
            Self::Owned(data) => {
                data.resize(len, 0);
                Ok(())
            }
            Self::Static(_) => Err(VfsError::PermissionDenied),
        }
    }

    fn write_at(&mut self, offset: usize, buf: &[u8]) -> Result<()> {
        match self {
            Self::Owned(data) => {
                data[offset..offset + buf.len()].copy_from_slice(buf);
                Ok(())
            }
            Self::Static(_) => Err(VfsError::PermissionDenied),
        }
    }
}

struct OpenMemHandle {
    path: String,
    offset: usize,
    flags: OpenFlags,
}

pub struct MemFs {
    nodes: Vec<MemNode>,
    next_handle: AtomicUsize,
    open_handles: Vec<(FileHandle, OpenMemHandle)>,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct MemFsStats {
    pub files: u64,
    pub directories: u64,
    pub bytes: u64,
    pub open_handles: u64,
}

impl MemFs {
    pub const fn empty() -> Self {
        Self {
            nodes: Vec::new(),
            next_handle: AtomicUsize::new(1),
            open_handles: Vec::new(),
        }
    }

    pub fn new() -> Self {
        Self::empty()
    }

    pub fn with_base_tree() -> Self {
        Self::new()
    }

    pub fn add_file(&mut self, name: &str, data: Vec<u8>) {
        let _ = self.create(name);
        if let Some(idx) = self.node_index(name) {
            self.nodes[idx].data = MemData::Owned(data);
        }
    }

    pub fn add_static_file(&mut self, name: &str, data: &'static [u8]) {
        let _ = self.create(name);
        if let Some(idx) = self.node_index(name) {
            self.nodes[idx].data = MemData::Static(data);
        }
    }

    pub fn add_device(&mut self, name: &str) {
        let clean = Self::clean(name);
        if clean.is_empty() || !self.parent_exists(clean) {
            return;
        }

        if let Some(idx) = self.node_index(clean) {
            self.nodes[idx].file_type = FileType::Device;
            self.nodes[idx].data = MemData::Owned(Vec::new());
            return;
        }

        self.nodes.push(MemNode {
            path: String::from(clean),
            file_type: FileType::Device,
            data: MemData::Owned(Vec::new()),
        });
    }

    pub fn static_file(&self, name: &str) -> Option<&'static [u8]> {
        let idx = self.node_index(name)?;
        match self.nodes[idx].data {
            MemData::Static(data) => Some(data),
            MemData::Owned(_) => None,
        }
    }

    pub fn stats(&self) -> MemFsStats {
        let mut stats = MemFsStats {
            directories: BASE_DIRS.len() as u64,
            ..MemFsStats::default()
        };
        for node in self.nodes.iter() {
            match node.file_type {
                FileType::File => {
                    stats.files += 1;
                    stats.bytes += node.data.len() as u64;
                }
                FileType::Directory => stats.directories += 1,
                FileType::Device => {}
            }
        }
        stats.open_handles = self.open_handles.len() as u64;
        stats
    }

    fn clean(path: &str) -> &str {
        path.trim_matches('/')
    }

    fn is_base_dir(path: &str) -> bool {
        let clean = Self::clean(path);
        BASE_DIRS.iter().any(|dir| clean == *dir)
    }

    fn node_index(&self, path: &str) -> Option<usize> {
        let clean = Self::clean(path);
        self.nodes.iter().position(|node| node.path == clean)
    }

    fn node_type(&self, path: &str) -> Option<FileType> {
        let clean = Self::clean(path);
        if clean.is_empty() || Self::is_base_dir(clean) {
            return Some(FileType::Directory);
        }
        self.node_index(clean).map(|idx| self.nodes[idx].file_type)
    }

    fn parent_path(path: &str) -> &str {
        match Self::clean(path).rfind('/') {
            Some(idx) => &Self::clean(path)[..idx],
            None => "",
        }
    }

    fn basename(path: &str) -> &str {
        match Self::clean(path).rfind('/') {
            Some(idx) => &Self::clean(path)[idx + 1..],
            None => Self::clean(path),
        }
    }

    fn parent_exists(&self, path: &str) -> bool {
        self.node_type(Self::parent_path(path)) == Some(FileType::Directory)
    }

    fn handle_index(&self, handle: FileHandle) -> Option<usize> {
        self.open_handles.iter().position(|(id, _)| *id == handle)
    }

    fn is_direct_child(parent: &str, child: &str) -> bool {
        let parent = Self::clean(parent);
        let child = Self::clean(child);

        if parent.is_empty() {
            !child.is_empty() && !child.contains('/')
        } else {
            child.starts_with(parent)
                && child.len() > parent.len()
                && child.as_bytes()[parent.len()] == b'/'
                && !child[parent.len() + 1..].contains('/')
        }
    }
}

impl FileSystem for MemFs {
    fn open(&mut self, path: &str, flags: OpenFlags) -> Result<FileHandle> {
        if !flags.is_valid() {
            return Err(VfsError::InvalidPath);
        }

        if self.node_type(path).is_none() && flags.create() {
            self.create(path)?;
        }

        match self.node_type(path).ok_or(VfsError::NotFound)? {
            FileType::Directory => Err(VfsError::IsADirectory),
            FileType::File => {
                let nidx = self.node_index(path).ok_or(VfsError::NotFound)?;
                if flags.trunc() {
                    if !flags.can_write() {
                        return Err(VfsError::PermissionDenied);
                    }
                    self.nodes[nidx].data.clear()?;
                }

                let handle = self.next_handle.fetch_add(1, Ordering::SeqCst);
                let offset = if flags.append() {
                    self.nodes[nidx].data.len()
                } else {
                    0
                };
                self.open_handles.push((
                    handle,
                    OpenMemHandle {
                        path: String::from(Self::clean(path)),
                        offset,
                        flags,
                    },
                ));
                Ok(handle)
            }
            FileType::Device => Err(VfsError::Unsupported),
        }
    }

    fn read(&mut self, handle: FileHandle, buf: &mut [u8]) -> Result<usize> {
        let hidx = self
            .handle_index(handle)
            .ok_or(VfsError::InvalidDescriptor)?;
        if !self.open_handles[hidx].1.flags.can_read() {
            return Err(VfsError::PermissionDenied);
        }

        let path = self.open_handles[hidx].1.path.clone();
        let offset = self.open_handles[hidx].1.offset;
        let nidx = self.node_index(&path).ok_or(VfsError::NotFound)?;

        if self.nodes[nidx].file_type != FileType::File {
            return Err(VfsError::IsADirectory);
        }

        let data = self.nodes[nidx].data.as_slice();
        let bytes_read = if offset >= data.len() {
            0
        } else {
            let len = buf.len().min(data.len() - offset);
            buf[..len].copy_from_slice(&data[offset..offset + len]);
            len
        };

        self.open_handles[hidx].1.offset += bytes_read;
        Ok(bytes_read)
    }

    fn write(&mut self, handle: FileHandle, buf: &[u8]) -> Result<usize> {
        let hidx = self
            .handle_index(handle)
            .ok_or(VfsError::InvalidDescriptor)?;
        if !self.open_handles[hidx].1.flags.can_write() {
            return Err(VfsError::PermissionDenied);
        }

        let path = self.open_handles[hidx].1.path.clone();
        let nidx = self.node_index(&path).ok_or(VfsError::NotFound)?;

        if self.nodes[nidx].file_type != FileType::File {
            return Err(VfsError::IsADirectory);
        }

        let offset = if self.open_handles[hidx].1.flags.append() {
            self.nodes[nidx].data.len()
        } else {
            self.open_handles[hidx].1.offset
        };
        let end = offset + buf.len();
        if end > self.nodes[nidx].data.len() {
            self.nodes[nidx].data.resize(end)?;
        }
        self.nodes[nidx].data.write_at(offset, buf)?;
        self.open_handles[hidx].1.offset = end;
        Ok(buf.len())
    }

    fn close(&mut self, handle: FileHandle) -> Result<()> {
        let idx = self
            .handle_index(handle)
            .ok_or(VfsError::InvalidDescriptor)?;
        self.open_handles.remove(idx);
        Ok(())
    }

    fn readdir(&mut self, path: &str) -> Result<Vec<DirEntry>> {
        let mut buffer = [DirEntry::empty(); 32];
        let count = self.readdir_into(path, &mut buffer)?;
        let mut entries = Vec::new();
        for entry in buffer.iter().take(count) {
            entries.push(*entry);
        }
        Ok(entries)
    }

    fn readdir_into(&mut self, path: &str, entries: &mut [DirEntry]) -> Result<usize> {
        let mut count = 0;
        let clean = Self::clean(path);

        if clean.is_empty() {
            for base_dir in BASE_DIRS.iter() {
                if count < entries.len() {
                    entries[count] = DirEntry::new(base_dir, FileType::Directory);
                    count += 1;
                }
            }

            for node in self.nodes.iter() {
                if Self::is_direct_child("", &node.path) && !Self::is_base_dir(&node.path) {
                    if count < entries.len() {
                        entries[count] = DirEntry::new(Self::basename(&node.path), node.file_type);
                        count += 1;
                    }
                }
            }
            return Ok(count);
        }

        if self.node_type(clean) != Some(FileType::Directory) {
            return Err(VfsError::NotADirectory);
        }

        for node in self.nodes.iter() {
            if Self::is_direct_child(clean, &node.path) {
                if count < entries.len() {
                    entries[count] = DirEntry::new(Self::basename(&node.path), node.file_type);
                    count += 1;
                }
            }
        }

        Ok(count)
    }

    fn create(&mut self, path: &str) -> Result<()> {
        let clean = Self::clean(path);
        if clean.is_empty() {
            return Err(VfsError::InvalidPath);
        }
        if self.node_type(clean).is_some() {
            return Err(VfsError::AlreadyExists);
        }
        if !self.parent_exists(clean) {
            return Err(VfsError::NotFound);
        }

        self.nodes.push(MemNode {
            path: String::from(clean),
            file_type: FileType::File,
            data: MemData::Owned(Vec::new()),
        });
        Ok(())
    }

    fn mkdir(&mut self, path: &str) -> Result<()> {
        let clean = Self::clean(path);
        if clean.is_empty() {
            return Err(VfsError::InvalidPath);
        }
        if self.node_type(clean).is_some() {
            return Err(VfsError::AlreadyExists);
        }
        if !self.parent_exists(clean) {
            return Err(VfsError::NotFound);
        }

        self.nodes.push(MemNode {
            path: String::from(clean),
            file_type: FileType::Directory,
            data: MemData::Owned(Vec::new()),
        });
        Ok(())
    }

    fn remove(&mut self, path: &str) -> Result<()> {
        let clean = Self::clean(path);
        if clean.is_empty() {
            return Err(VfsError::InvalidPath);
        }
        let idx = self.node_index(clean).ok_or(VfsError::NotFound)?;
        if self.nodes[idx].file_type != FileType::File {
            return Err(VfsError::IsADirectory);
        }
        self.nodes.remove(idx);
        self.open_handles.retain(|(_, handle)| handle.path != clean);
        Ok(())
    }

    fn truncate(&mut self, path: &str) -> Result<()> {
        let clean = Self::clean(path);
        if clean.is_empty() {
            return Err(VfsError::InvalidPath);
        }
        let idx = self.node_index(clean).ok_or(VfsError::NotFound)?;
        if self.nodes[idx].file_type != FileType::File {
            return Err(VfsError::IsADirectory);
        }
        self.nodes[idx].data.clear()?;
        Ok(())
    }

    fn stat(&mut self, path: &str) -> Result<FileStat> {
        let clean = Self::clean(path);
        if clean.is_empty() || Self::is_base_dir(clean) {
            return Ok(FileStat {
                file_type: FileType::Directory,
                size: 0,
            });
        }

        let idx = self.node_index(clean).ok_or(VfsError::NotFound)?;
        Ok(FileStat {
            file_type: self.nodes[idx].file_type,
            size: self.nodes[idx].data.len(),
        })
    }
}

impl Default for MemFs {
    fn default() -> Self {
        Self::new()
    }
}
