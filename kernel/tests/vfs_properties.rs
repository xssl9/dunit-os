use proptest::prelude::*;
use std::collections::BTreeMap;
use std::string::String;
use std::vec::Vec;
use std::sync::atomic::{AtomicUsize, Ordering};

type FileDescriptor = u32;
type FileHandle = usize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OpenFlags {
    ReadOnly,
    WriteOnly,
    ReadWrite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VfsError {
    NotFound,
    PermissionDenied,
    InvalidDescriptor,
    AlreadyExists,
    NotADirectory,
    IsADirectory,
    IoError,
}

type Result<T> = std::result::Result<T, VfsError>;

trait FileSystem: Send + Sync {
    fn open(&self, path: &str, flags: OpenFlags) -> Result<FileHandle>;
    fn read(&self, handle: FileHandle, buf: &mut [u8]) -> Result<usize>;
    fn write(&self, handle: FileHandle, buf: &[u8]) -> Result<usize>;
    fn close(&self, handle: FileHandle) -> Result<()>;
}

struct OpenFile {
    fs: &'static dyn FileSystem,
    handle: FileHandle,
    position: usize,
}

impl OpenFile {
    fn new(fs: &'static dyn FileSystem, handle: FileHandle) -> Self {
        Self {
            fs,
            handle,
            position: 0,
        }
    }

    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        let bytes_read = self.fs.read(self.handle, buf)?;
        self.position += bytes_read;
        Ok(bytes_read)
    }

    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        let bytes_written = self.fs.write(self.handle, buf)?;
        self.position += bytes_written;
        Ok(bytes_written)
    }
}

impl Drop for OpenFile {
    fn drop(&mut self) {
        let _ = self.fs.close(self.handle);
    }
}

struct VirtualFileSystem {
    mount_points: BTreeMap<String, &'static dyn FileSystem>,
    open_files: BTreeMap<FileDescriptor, OpenFile>,
    next_fd: FileDescriptor,
}

impl VirtualFileSystem {
    fn new() -> Self {
        Self {
            mount_points: BTreeMap::new(),
            open_files: BTreeMap::new(),
            next_fd: 3,
        }
    }

    fn mount(&mut self, path: &str, fs: &'static dyn FileSystem) {
        self.mount_points.insert(String::from(path), fs);
    }

    fn resolve_path<'a>(&self, path: &'a str) -> Result<(&'static dyn FileSystem, &'a str)> {
        for (mount_point, fs) in self.mount_points.iter().rev() {
            if path.starts_with(mount_point.as_str()) {
                let relative_path = &path[mount_point.len()..];
                let relative_path = if relative_path.starts_with('/') {
                    &relative_path[1..]
                } else {
                    relative_path
                };
                return Ok((*fs, relative_path));
            }
        }
        Err(VfsError::NotFound)
    }

    fn open(&mut self, path: &str, flags: OpenFlags) -> Result<FileDescriptor> {
        let (fs, relative_path) = self.resolve_path(path)?;
        let handle = fs.open(relative_path, flags)?;
        
        let fd = self.next_fd;
        self.next_fd += 1;
        
        let open_file = OpenFile::new(fs, handle);
        self.open_files.insert(fd, open_file);
        
        Ok(fd)
    }

    fn read(&mut self, fd: FileDescriptor, buf: &mut [u8]) -> Result<usize> {
        let open_file = self.open_files.get_mut(&fd)
            .ok_or(VfsError::InvalidDescriptor)?;
        open_file.read(buf)
    }

    fn write(&mut self, fd: FileDescriptor, buf: &[u8]) -> Result<usize> {
        let open_file = self.open_files.get_mut(&fd)
            .ok_or(VfsError::InvalidDescriptor)?;
        open_file.write(buf)
    }

    fn close(&mut self, fd: FileDescriptor) -> Result<()> {
        self.open_files.remove(&fd)
            .ok_or(VfsError::InvalidDescriptor)?;
        Ok(())
    }
}

struct TestFs {
    files: BTreeMap<String, Vec<u8>>,
    next_handle: AtomicUsize,
    open_handles: BTreeMap<FileHandle, String>,
}

impl TestFs {
    fn new() -> Self {
        Self {
            files: BTreeMap::new(),
            next_handle: AtomicUsize::new(1),
            open_handles: BTreeMap::new(),
        }
    }

    fn add_file(&mut self, name: &str, data: Vec<u8>) {
        self.files.insert(String::from(name), data);
    }
}

impl FileSystem for TestFs {
    fn open(&self, path: &str, _flags: OpenFlags) -> Result<FileHandle> {
        if self.files.contains_key(path) {
            let handle = self.next_handle.fetch_add(1, Ordering::SeqCst);
            Ok(handle)
        } else {
            Err(VfsError::NotFound)
        }
    }

    fn read(&self, _handle: FileHandle, buf: &mut [u8]) -> Result<usize> {
        let data = b"test data";
        let len = buf.len().min(data.len());
        buf[..len].copy_from_slice(&data[..len]);
        Ok(len)
    }

    fn write(&self, _handle: FileHandle, buf: &[u8]) -> Result<usize> {
        Ok(buf.len())
    }

    fn close(&self, _handle: FileHandle) -> Result<()> {
        Ok(())
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn prop_file_open_returns_descriptor(filename in "[a-z]{1,10}") {
        static mut TEST_FS: Option<TestFs> = None;
        
        unsafe {
            TEST_FS = Some(TestFs::new());
            if let Some(ref mut fs) = TEST_FS {
                fs.add_file(&filename, vec![1, 2, 3, 4]);
            }
        }

        let mut vfs = VirtualFileSystem::new();
        
        unsafe {
            if let Some(ref fs) = TEST_FS {
                let fs_ref: &'static dyn FileSystem = std::mem::transmute(fs as &dyn FileSystem);
                vfs.mount("/test", fs_ref);
            }
        }

        let path = format!("/test/{}", filename);
        let result = vfs.open(&path, OpenFlags::ReadOnly);
        
        assert!(result.is_ok());
        let fd = result.unwrap();
        assert!(fd >= 3);
    }

    #[test]
    fn prop_file_not_found_error(filename in "[a-z]{1,10}", nonexistent in "[A-Z]{1,10}") {
        prop_assume!(filename != nonexistent.to_lowercase());
        
        static mut TEST_FS: Option<TestFs> = None;
        
        unsafe {
            TEST_FS = Some(TestFs::new());
            if let Some(ref mut fs) = TEST_FS {
                fs.add_file(&filename, vec![1, 2, 3, 4]);
            }
        }

        let mut vfs = VirtualFileSystem::new();
        
        unsafe {
            if let Some(ref fs) = TEST_FS {
                let fs_ref: &'static dyn FileSystem = std::mem::transmute(fs as &dyn FileSystem);
                vfs.mount("/test", fs_ref);
            }
        }

        let path = format!("/test/{}", nonexistent);
        let result = vfs.open(&path, OpenFlags::ReadOnly);
        
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), VfsError::NotFound);
    }

    #[test]
    fn prop_open_read_close_roundtrip(filename in "[a-z]{1,10}") {
        static mut TEST_FS: Option<TestFs> = None;
        
        unsafe {
            TEST_FS = Some(TestFs::new());
            if let Some(ref mut fs) = TEST_FS {
                fs.add_file(&filename, vec![1, 2, 3, 4]);
            }
        }

        let mut vfs = VirtualFileSystem::new();
        
        unsafe {
            if let Some(ref fs) = TEST_FS {
                let fs_ref: &'static dyn FileSystem = std::mem::transmute(fs as &dyn FileSystem);
                vfs.mount("/test", fs_ref);
            }
        }

        let path = format!("/test/{}", filename);
        
        let fd = vfs.open(&path, OpenFlags::ReadOnly).unwrap();
        
        let mut buf = [0u8; 64];
        let bytes_read = vfs.read(fd, &mut buf).unwrap();
        assert!(bytes_read > 0);
        
        let close_result = vfs.close(fd);
        assert!(close_result.is_ok());
        
        let read_after_close = vfs.read(fd, &mut buf);
        assert!(read_after_close.is_err());
        assert_eq!(read_after_close.unwrap_err(), VfsError::InvalidDescriptor);
    }
}
