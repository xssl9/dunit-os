use super::vfs::{
    DirEntry, FileSystem, FileHandle, FileStat, FileType, OpenFlags, Result, VfsError,
};
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
    fn open(&mut self, path: &str, _flags: OpenFlags) -> Result<FileHandle> {
        if path.is_empty() || path.parse::<u32>().is_ok() {
            let handle = self.next_handle.fetch_add(1, Ordering::SeqCst);
            Ok(handle)
        } else {
            Err(VfsError::NotFound)
        }
    }

    fn read(&mut self, _handle: FileHandle, buf: &mut [u8]) -> Result<usize> {
        let data = b"pid: 1\nstate: Running\n";
        let len = buf.len().min(data.len());
        buf[..len].copy_from_slice(&data[..len]);
        Ok(len)
    }

    fn write(&mut self, _handle: FileHandle, _buf: &[u8]) -> Result<usize> {
        Err(VfsError::PermissionDenied)
    }

    fn close(&mut self, _handle: FileHandle) -> Result<()> {
        Ok(())
    }

    fn readdir(&mut self, path: &str) -> Result<Vec<DirEntry>> {
        if !path.is_empty() {
            return Err(VfsError::NotADirectory);
        }

        let mut entries = Vec::new();
        entries.push(DirEntry::new("1", FileType::File));
        Ok(entries)
    }

    fn readdir_into(&mut self, path: &str, entries: &mut [DirEntry]) -> Result<usize> {
        if !path.is_empty() {
            return Err(VfsError::NotADirectory);
        }

        if !entries.is_empty() {
            entries[0] = DirEntry::new("1", FileType::File);
            Ok(1)
        } else {
            Ok(0)
        }
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
                size: 1,
            });
        }

        if path == "1" {
            Ok(FileStat {
                file_type: FileType::File,
                size: 22,
            })
        } else {
            Err(VfsError::NotFound)
        }
    }
}

impl Default for ProcFs {
    fn default() -> Self {
        Self::new()
    }
}
