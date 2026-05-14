use super::vfs::{FileSystem, FileHandle, OpenFlags, Result, VfsError};
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};

pub struct MemFile {
    name: String,
    data: Vec<u8>,
}

impl MemFile {
    pub fn new(name: &str, data: Vec<u8>) -> Self {
        Self {
            name: String::from(name),
            data,
        }
    }
}

pub struct MemFs {
    files: BTreeMap<String, MemFile>,
    next_handle: AtomicUsize,
    open_handles: BTreeMap<FileHandle, (String, usize)>,
}

impl MemFs {
    pub fn new() -> Self {
        Self {
            files: BTreeMap::new(),
            next_handle: AtomicUsize::new(1),
            open_handles: BTreeMap::new(),
        }
    }

    pub fn add_file(&mut self, name: &str, data: Vec<u8>) {
        self.files.insert(String::from(name), MemFile::new(name, data));
    }
}

impl FileSystem for MemFs {
    fn open(&mut self, path: &str, _flags: OpenFlags) -> Result<FileHandle> {
        if self.files.contains_key(path) {
            let handle = self.next_handle.fetch_add(1, Ordering::SeqCst);
            self.open_handles.insert(handle, (String::from(path), 0));
            Ok(handle)
        } else {
            Err(VfsError::NotFound)
        }
    }

    fn read(&mut self, handle: FileHandle, buf: &mut [u8]) -> Result<usize> {
        let (path, offset) = self.open_handles.get(&handle)
            .ok_or(VfsError::InvalidDescriptor)?
            .clone();
        
        let file = self.files.get(&path).ok_or(VfsError::NotFound)?;
        let remaining = file.data.len().saturating_sub(offset);
        let to_read = buf.len().min(remaining);
        
        buf[..to_read].copy_from_slice(&file.data[offset..offset + to_read]);
        
        if let Some(entry) = self.open_handles.get_mut(&handle) {
            entry.1 += to_read;
        }
        
        Ok(to_read)
    }

    fn write(&mut self, _handle: FileHandle, _buf: &[u8]) -> Result<usize> {
        Err(VfsError::PermissionDenied)
    }

    fn close(&mut self, handle: FileHandle) -> Result<()> {
        self.open_handles.remove(&handle);
        Ok(())
    }
}

impl Default for MemFs {
    fn default() -> Self {
        Self::new()
    }
}
