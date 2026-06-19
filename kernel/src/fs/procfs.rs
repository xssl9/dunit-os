use super::vfs::{
    DirEntry, FileHandle, FileStat, FileSystem, FileType, OpenFlags, Result, VfsError,
};
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};

pub struct ProcFs {
    next_handle: AtomicUsize,
    open_handles: BTreeMap<FileHandle, String>,
}

impl ProcFs {
    pub fn new() -> Self {
        Self {
            next_handle: AtomicUsize::new(1),
            open_handles: BTreeMap::new(),
        }
    }
}

impl FileSystem for ProcFs {
    fn open(&mut self, path: &str, _flags: OpenFlags) -> Result<FileHandle> {
        if path == "processes" || path == "meminfo" || path.parse::<u32>().is_ok() {
            let handle = self.next_handle.fetch_add(1, Ordering::SeqCst);
            self.open_handles.insert(handle, String::from(path));
            Ok(handle)
        } else {
            Err(VfsError::NotFound)
        }
    }

    fn read(&mut self, handle: FileHandle, buf: &mut [u8]) -> Result<usize> {
        let path = self
            .open_handles
            .get(&handle)
            .ok_or(VfsError::InvalidDescriptor)?;

        let data = if path == "meminfo" {
            if let Some(pmm) = crate::memory::pmm::get_pmm() {
                format!(
                    "total: {}\nfree: {}\nused: {}\n",
                    pmm.total_memory(),
                    pmm.available_memory(),
                    pmm.total_memory() - pmm.available_memory()
                )
                .into_bytes()
            } else {
                b"pmm unavailable\n".to_vec()
            }
        } else if path == "processes" {
            let mut out = String::new();
            for proc in crate::process::get_process_snapshots() {
                out.push_str(&format!(
                    "{:4} {:10} {:10}\n",
                    proc.pid.0,
                    format!("{:?}", proc.state),
                    proc.path
                ));
            }
            out.into_bytes()
        } else {
            b"pid info unavailable\n".to_vec()
        };

        let len = buf.len().min(data.len());
        buf[..len].copy_from_slice(&data[..len]);
        Ok(len)
    }

    fn write(&mut self, _handle: FileHandle, _buf: &[u8]) -> Result<usize> {
        Err(VfsError::PermissionDenied)
    }

    fn close(&mut self, handle: FileHandle) -> Result<()> {
        self.open_handles.remove(&handle);
        Ok(())
    }

    fn readdir(&mut self, path: &str) -> Result<Vec<DirEntry>> {
        if !path.is_empty() {
            return Err(VfsError::NotADirectory);
        }

        let mut entries = Vec::new();
        entries.push(DirEntry::new("processes", FileType::File));
        entries.push(DirEntry::new("meminfo", FileType::File));
        Ok(entries)
    }

    fn readdir_into(&mut self, path: &str, entries: &mut [DirEntry]) -> Result<usize> {
        if !path.is_empty() {
            return Err(VfsError::NotADirectory);
        }

        let mut count = 0;
        if count < entries.len() {
            entries[count] = DirEntry::new("processes", FileType::File);
            count += 1;
        }
        if count < entries.len() {
            entries[count] = DirEntry::new("meminfo", FileType::File);
            count += 1;
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
                size: 2,
            });
        }

        if path == "processes" || path == "meminfo" {
            Ok(FileStat {
                file_type: FileType::File,
                size: 512, // Arbitrary
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
