use super::vfs::{
    DirEntry, FileHandle, FileStat, FileSystem, FileType, OpenFlags, Result, VfsError,
};
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};

pub struct DeviceFile {
    name: String,
    data: Vec<u8>,
}

impl DeviceFile {
    pub fn new(name: &str) -> Self {
        Self {
            name: String::from(name),
            data: Vec::new(),
        }
    }

    pub fn with_data(name: &str, data: Vec<u8>) -> Self {
        Self {
            name: String::from(name),
            data,
        }
    }
}

pub struct DevFs {
    devices: BTreeMap<String, DeviceFile>,
    next_handle: AtomicUsize,
    open_handles: BTreeMap<FileHandle, String>,
}

impl DevFs {
    pub fn new() -> Self {
        let mut devices = BTreeMap::new();

        devices.insert(String::from("fb0"), DeviceFile::new("fb0"));
        devices.insert(String::from("kbd"), DeviceFile::new("kbd"));
        devices.insert(String::from("mouse"), DeviceFile::new("mouse"));

        Self {
            devices,
            next_handle: AtomicUsize::new(1),
            open_handles: BTreeMap::new(),
        }
    }

    pub fn add_device(&mut self, name: &str, device: DeviceFile) {
        self.devices.insert(String::from(name), device);
    }
}

impl FileSystem for DevFs {
    fn open(&mut self, path: &str, _flags: OpenFlags) -> Result<FileHandle> {
        if self.devices.contains_key(path) {
            let handle = self.next_handle.fetch_add(1, Ordering::SeqCst);
            Ok(handle)
        } else {
            Err(VfsError::NotFound)
        }
    }

    fn read(&mut self, handle: FileHandle, buf: &mut [u8]) -> Result<usize> {
        let _ = handle;
        let len = buf.len().min(0);
        Ok(len)
    }

    fn write(&mut self, handle: FileHandle, buf: &[u8]) -> Result<usize> {
        let _ = handle;
        Ok(buf.len())
    }

    fn close(&mut self, _handle: FileHandle) -> Result<()> {
        Ok(())
    }

    fn readdir(&mut self, path: &str) -> Result<Vec<DirEntry>> {
        if !path.is_empty() {
            return Err(VfsError::NotADirectory);
        }

        let mut entries = Vec::new();
        for name in self.devices.keys() {
            entries.push(DirEntry::new(name, FileType::Device));
        }
        Ok(entries)
    }

    fn readdir_into(&mut self, path: &str, entries: &mut [DirEntry]) -> Result<usize> {
        if !path.is_empty() {
            return Err(VfsError::NotADirectory);
        }

        let mut count = 0;
        for name in self.devices.keys() {
            if count < entries.len() {
                entries[count] = DirEntry::new(name, FileType::Device);
                count += 1;
            }
        }
        Ok(count)
    }

    fn create(&mut self, _path: &str) -> Result<()> {
        Err(VfsError::Unsupported)
    }

    fn mkdir(&mut self, _path: &str) -> Result<()> {
        Err(VfsError::Unsupported)
    }

    fn remove(&mut self, _path: &str) -> Result<()> {
        Err(VfsError::Unsupported)
    }

    fn truncate(&mut self, _path: &str) -> Result<()> {
        Err(VfsError::Unsupported)
    }

    fn stat(&mut self, path: &str) -> Result<FileStat> {
        if path.is_empty() {
            return Ok(FileStat {
                file_type: FileType::Directory,
                size: self.devices.len(),
            });
        }

        if self.devices.contains_key(path) {
            Ok(FileStat {
                file_type: FileType::Device,
                size: 0,
            })
        } else {
            Err(VfsError::NotFound)
        }
    }
}

impl Default for DevFs {
    fn default() -> Self {
        Self::new()
    }
}
