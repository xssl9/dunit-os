use super::vfs::{
    DirEntry, FileHandle, FileStat, FileSystem, FileType, OpenFlags, Result, VfsError,
};
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};

const BASE_DIRS: [&str; 6] = ["kernel", "proc", "app", "cfg", "usr", "tmp"];

struct MemNode {
    path: String,
    file_type: FileType,
    data: Vec<u8>,
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
            self.nodes[idx].data = data;
        }
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
        if self.node_type(path).is_none() && flags == OpenFlags::Create {
            self.create(path)?;
        }

        match self.node_type(path).ok_or(VfsError::NotFound)? {
            FileType::Directory => Err(VfsError::IsADirectory),
            FileType::File => {
                let handle = self.next_handle.fetch_add(1, Ordering::SeqCst);
                self.open_handles.push((handle, OpenMemHandle {
                    path: String::from(Self::clean(path)),
                    offset: 0,
                    flags,
                }));
                Ok(handle)
            }
            FileType::Device => Err(VfsError::Unsupported),
        }
    }

    fn read(&mut self, handle: FileHandle, buf: &mut [u8]) -> Result<usize> {
        let hidx = self.handle_index(handle).ok_or(VfsError::InvalidDescriptor)?;
        let path = self.open_handles[hidx].1.path.clone();
        let offset = self.open_handles[hidx].1.offset;
        let nidx = self.node_index(&path).ok_or(VfsError::NotFound)?;

        if self.nodes[nidx].file_type != FileType::File {
            return Err(VfsError::IsADirectory);
        }

        let data = &self.nodes[nidx].data;
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
        let hidx = self.handle_index(handle).ok_or(VfsError::InvalidDescriptor)?;
        if self.open_handles[hidx].1.flags == OpenFlags::ReadOnly {
            return Err(VfsError::PermissionDenied);
        }

        let path = self.open_handles[hidx].1.path.clone();
        let offset = self.open_handles[hidx].1.offset;
        let nidx = self.node_index(&path).ok_or(VfsError::NotFound)?;

        if self.nodes[nidx].file_type != FileType::File {
            return Err(VfsError::IsADirectory);
        }

        let end = offset + buf.len();
        if end > self.nodes[nidx].data.len() {
            self.nodes[nidx].data.resize(end, 0);
        }
        self.nodes[nidx].data[offset..end].copy_from_slice(buf);
        self.open_handles[hidx].1.offset += buf.len();
        Ok(buf.len())
    }

    fn close(&mut self, handle: FileHandle) -> Result<()> {
        let idx = self.handle_index(handle).ok_or(VfsError::InvalidDescriptor)?;
        self.open_handles.remove(idx);
        Ok(())
    }

    fn readdir(&mut self, path: &str) -> Result<Vec<DirEntry>> {
        if self.node_type(path) != Some(FileType::Directory) {
            return Err(VfsError::NotADirectory);
        }

        let mut entries = Vec::new();
        let clean = Self::clean(path);

        if clean.is_empty() {
            for dir in BASE_DIRS.iter() {
                entries.push(DirEntry {
                    name: String::from(*dir),
                    file_type: FileType::Directory,
                });
            }
        }

        for node in self.nodes.iter() {
            if Self::is_direct_child(clean, &node.path) {
                entries.push(DirEntry {
                    name: String::from(Self::basename(&node.path)),
                    file_type: node.file_type,
                });
            }
        }

        Ok(entries)
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
            data: Vec::new(),
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
            data: Vec::new(),
        });
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
