use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

use super::memfs::MemFs;

pub type FileDescriptor = u32;
pub type FileHandle = usize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenFlags {
    ReadOnly,
    WriteOnly,
    ReadWrite,
    Create,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileType {
    File,
    Directory,
    Device,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    pub name: String,
    pub file_type: FileType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileStat {
    pub file_type: FileType,
    pub size: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VfsError {
    NotFound,
    PermissionDenied,
    InvalidDescriptor,
    AlreadyExists,
    NotADirectory,
    IsADirectory,
    InvalidPath,
    Unsupported,
    IoError,
}

impl fmt::Display for VfsError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            VfsError::NotFound => write!(f, "File not found"),
            VfsError::PermissionDenied => write!(f, "Permission denied"),
            VfsError::InvalidDescriptor => write!(f, "Invalid file descriptor"),
            VfsError::AlreadyExists => write!(f, "File already exists"),
            VfsError::NotADirectory => write!(f, "Not a directory"),
            VfsError::IsADirectory => write!(f, "Is a directory"),
            VfsError::InvalidPath => write!(f, "Invalid path"),
            VfsError::Unsupported => write!(f, "Operation unsupported"),
            VfsError::IoError => write!(f, "I/O error"),
        }
    }
}

pub type Result<T> = core::result::Result<T, VfsError>;

pub trait FileSystem: Send {
    fn open(&mut self, path: &str, flags: OpenFlags) -> Result<FileHandle>;
    fn read(&mut self, handle: FileHandle, buf: &mut [u8]) -> Result<usize>;
    fn write(&mut self, handle: FileHandle, buf: &[u8]) -> Result<usize>;
    fn close(&mut self, handle: FileHandle) -> Result<()>;
    fn readdir(&mut self, path: &str) -> Result<Vec<DirEntry>>;
    fn create(&mut self, path: &str) -> Result<()>;
    fn mkdir(&mut self, path: &str) -> Result<()>;
    fn stat(&mut self, path: &str) -> Result<FileStat>;
}

pub struct OpenFile {
    fs: *mut dyn FileSystem,
    handle: FileHandle,
    position: usize,
}

impl OpenFile {
    pub fn new(fs: *mut dyn FileSystem, handle: FileHandle) -> Self {
        Self {
            fs,
            handle,
            position: 0,
        }
    }

    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        let bytes_read = unsafe { (&mut *self.fs).read(self.handle, buf)? };
        self.position += bytes_read;
        Ok(bytes_read)
    }

    pub fn write(&mut self, buf: &[u8]) -> Result<usize> {
        let bytes_written = unsafe { (&mut *self.fs).write(self.handle, buf)? };
        self.position += bytes_written;
        Ok(bytes_written)
    }

    pub fn position(&self) -> usize {
        self.position
    }
}

impl Drop for OpenFile {
    fn drop(&mut self) {
        let _ = unsafe { (&mut *self.fs).close(self.handle) };
    }
}

pub struct VirtualFileSystem {
    root_fs: Option<*mut dyn FileSystem>,
    open_files: BTreeMap<FileDescriptor, OpenFile>,
    next_fd: FileDescriptor,
}

impl VirtualFileSystem {
    pub fn new() -> Self {
        Self {
            root_fs: None,
            open_files: BTreeMap::new(),
            next_fd: 3,
        }
    }

    pub fn mount(&mut self, path: &str, fs: &'static mut dyn FileSystem) -> Result<()> {
        if path == "/" {
            self.root_fs = Some(fs as *mut dyn FileSystem);
            Ok(())
        } else {
            Err(VfsError::Unsupported)
        }
    }

    fn resolve_path(&self, path: &str, cwd: &str) -> Result<(*mut dyn FileSystem, String)> {
        let normalized = normalize_path(path, cwd)?;
        let fs = self.root_fs.ok_or(VfsError::NotFound)?;
        let relative = normalized.trim_start_matches('/').into();

        Ok((fs, relative))
    }

    pub fn open_at(&mut self, cwd: &str, path: &str, flags: OpenFlags) -> Result<FileDescriptor> {
        let (fs, relative_path) = self.resolve_path(path, cwd)?;
        let handle = unsafe { (&mut *fs).open(&relative_path, flags)? };

        let fd = self.next_fd;
        self.next_fd += 1;
        self.open_files.insert(fd, OpenFile::new(fs, handle));

        Ok(fd)
    }

    pub fn open(&mut self, path: &str, flags: OpenFlags) -> Result<FileDescriptor> {
        self.open_at("/", path, flags)
    }

    pub fn read(&mut self, fd: FileDescriptor, buf: &mut [u8]) -> Result<usize> {
        let open_file = self.open_files.get_mut(&fd)
            .ok_or(VfsError::InvalidDescriptor)?;
        open_file.read(buf)
    }

    pub fn write(&mut self, fd: FileDescriptor, buf: &[u8]) -> Result<usize> {
        let open_file = self.open_files.get_mut(&fd)
            .ok_or(VfsError::InvalidDescriptor)?;
        open_file.write(buf)
    }

    pub fn close(&mut self, fd: FileDescriptor) -> Result<()> {
        self.open_files.remove(&fd)
            .ok_or(VfsError::InvalidDescriptor)?;
        Ok(())
    }

    pub fn readdir_at(&mut self, cwd: &str, path: &str) -> Result<Vec<DirEntry>> {
        let (fs, relative_path) = self.resolve_path(path, cwd)?;
        unsafe { (&mut *fs).readdir(&relative_path) }
    }

    pub fn create_at(&mut self, cwd: &str, path: &str) -> Result<()> {
        let (fs, relative_path) = self.resolve_path(path, cwd)?;
        unsafe { (&mut *fs).create(&relative_path) }
    }

    pub fn mkdir_at(&mut self, cwd: &str, path: &str) -> Result<()> {
        let (fs, relative_path) = self.resolve_path(path, cwd)?;
        unsafe { (&mut *fs).mkdir(&relative_path) }
    }

    pub fn stat_at(&mut self, cwd: &str, path: &str) -> Result<FileStat> {
        let (fs, relative_path) = self.resolve_path(path, cwd)?;
        unsafe { (&mut *fs).stat(&relative_path) }
    }

    pub fn normalize_at(&self, cwd: &str, path: &str) -> Result<String> {
        normalize_path(path, cwd)
    }
}

impl Default for VirtualFileSystem {
    fn default() -> Self {
        Self::new()
    }
}

pub fn normalize_path(path: &str, cwd: &str) -> Result<String> {
    if path.is_empty() {
        return normalize_path(cwd, "/");
    }

    let mut combined = String::new();
    if path.starts_with('/') {
        combined.push_str(path);
    } else {
        combined.push_str(if cwd.is_empty() { "/" } else { cwd });
        if !combined.ends_with('/') {
            combined.push('/');
        }
        combined.push_str(path);
    }

    let mut parts: Vec<&str> = Vec::new();
    for part in combined.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            _ => parts.push(part),
        }
    }

    let mut normalized = String::from("/");
    for (idx, part) in parts.iter().enumerate() {
        if idx > 0 {
            normalized.push('/');
        }
        normalized.push_str(part);
    }
    Ok(normalized)
}

static mut VFS_INSTANCE: Option<VirtualFileSystem> = None;
static mut ROOT_MEMFS: MemFs = MemFs::empty();

extern "C" {
    fn serial_write(s: *const u8);
}

fn serial_log(msg: &'static [u8]) {
    unsafe { serial_write(msg.as_ptr()) }
}

pub fn init() -> Result<()> {
    serial_log(b"[VFS] init START\r\n\0");
    unsafe {
        let mut vfs = VirtualFileSystem::new();

        vfs.mount("/", &mut ROOT_MEMFS)?;
        serial_log(b"[MEMFS] mounted as /\r\n\0");

        VFS_INSTANCE = Some(vfs);
    }

    serial_log(b"[VFS] init OK\r\n\0");
    Ok(())
}

pub fn get_vfs() -> Option<&'static mut VirtualFileSystem> {
    unsafe { VFS_INSTANCE.as_mut() }
}
