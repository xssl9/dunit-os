use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

pub type FileDescriptor = u32;
pub type FileHandle = usize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenFlags {
    ReadOnly,
    WriteOnly,
    ReadWrite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VfsError {
    NotFound,
    PermissionDenied,
    InvalidDescriptor,
    AlreadyExists,
    NotADirectory,
    IsADirectory,
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
            VfsError::IoError => write!(f, "I/O error"),
        }
    }
}

pub type Result<T> = core::result::Result<T, VfsError>;

pub trait FileSystem: Send + Sync {
    fn open(&self, path: &str, flags: OpenFlags) -> Result<FileHandle>;
    fn read(&self, handle: FileHandle, buf: &mut [u8]) -> Result<usize>;
    fn write(&self, handle: FileHandle, buf: &[u8]) -> Result<usize>;
    fn close(&self, handle: FileHandle) -> Result<()>;
}

pub struct OpenFile {
    fs: &'static dyn FileSystem,
    handle: FileHandle,
    position: usize,
}

impl OpenFile {
    pub fn new(fs: &'static dyn FileSystem, handle: FileHandle) -> Self {
        Self {
            fs,
            handle,
            position: 0,
        }
    }

    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        let bytes_read = self.fs.read(self.handle, buf)?;
        self.position += bytes_read;
        Ok(bytes_read)
    }

    pub fn write(&mut self, buf: &[u8]) -> Result<usize> {
        let bytes_written = self.fs.write(self.handle, buf)?;
        self.position += bytes_written;
        Ok(bytes_written)
    }

    pub fn position(&self) -> usize {
        self.position
    }
}

impl Drop for OpenFile {
    fn drop(&mut self) {
        let _ = self.fs.close(self.handle);
    }
}

pub struct VirtualFileSystem {
    mount_points: BTreeMap<String, &'static dyn FileSystem>,
    open_files: BTreeMap<FileDescriptor, OpenFile>,
    next_fd: FileDescriptor,
}

impl VirtualFileSystem {
    pub fn new() -> Self {
        Self {
            mount_points: BTreeMap::new(),
            open_files: BTreeMap::new(),
            next_fd: 3,
        }
    }

    pub fn mount(&mut self, path: &str, fs: &'static dyn FileSystem) {
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

    pub fn open(&mut self, path: &str, flags: OpenFlags) -> Result<FileDescriptor> {
        let (fs, relative_path) = self.resolve_path(path)?;
        let handle = fs.open(relative_path, flags)?;
        
        let fd = self.next_fd;
        self.next_fd += 1;
        
        let open_file = OpenFile::new(fs, handle);
        self.open_files.insert(fd, open_file);
        
        Ok(fd)
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
}

impl Default for VirtualFileSystem {
    fn default() -> Self {
        Self::new()
    }
}

static mut VFS_INSTANCE: Option<VirtualFileSystem> = None;

pub fn init() {
    // Временно пропускаем инициализацию VFS
    // TODO: инициализировать после настройки аллокатора
}

pub fn get_vfs() -> Option<&'static mut VirtualFileSystem> {
    unsafe { VFS_INSTANCE.as_mut() }
}
