use super::vfs::{FileSystem, FileHandle, OpenFlags, Result, VfsError};
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};

pub struct ProcFs {
    next_handle: AtomicUsize,
}

impl ProcFs {
    pub fn new() -> Self {
        Self {
            next_handle: AtomicUsize::new(1),
        }
    }
}

impl FileSystem for ProcFs {
    fn open(&self, path: &str, _flags: OpenFlags) -> Result<FileHandle> {
        if path.is_empty() || path.parse::<u32>().is_ok() {
            let handle = self.next_handle.fetch_add(1, Ordering::SeqCst);
            Ok(handle)
        } else {
            Err(VfsError::NotFound)
        }
    }

    fn read(&self, _handle: FileHandle, buf: &mut [u8]) -> Result<usize> {
        let data = b"pid: 1\nstate: Running\n";
        let len = buf.len().min(data.len());
        buf[..len].copy_from_slice(&data[..len]);
        Ok(len)
    }

    fn write(&self, _handle: FileHandle, _buf: &[u8]) -> Result<usize> {
        Err(VfsError::PermissionDenied)
    }

    fn close(&self, _handle: FileHandle) -> Result<()> {
        Ok(())
    }
}

impl Default for ProcFs {
    fn default() -> Self {
        Self::new()
    }
}
