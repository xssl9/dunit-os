use super::vfs::{FileSystem, FileHandle, OpenFlags, Result, VfsError};
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
            self.open_handles.insert(handle, String::from(path));
            Ok(handle)
        } else {
            Err(VfsError::NotFound)
        }
    }

    fn read(&mut self, handle: FileHandle, buf: &mut [u8]) -> Result<usize> {
        let _ = (handle, buf);
        Ok(0)
    }

    fn write(&mut self, handle: FileHandle, buf: &[u8]) -> Result<usize> {
        let _ = handle;
        Ok(buf.len())
    }

    fn close(&mut self, handle: FileHandle) -> Result<()> {
        self.open_handles.remove(&handle);
        Ok(())
    }
}

impl Default for DevFs {
    fn default() -> Self {
        Self::new()
    }
}
