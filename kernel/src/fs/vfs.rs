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
            VfsError::NotFound => write!(f, "not found"),
            VfsError::PermissionDenied => write!(f, "permission denied"),
            VfsError::InvalidDescriptor => write!(f, "invalid file descriptor"),
            VfsError::AlreadyExists => write!(f, "already exists"),
            VfsError::NotADirectory => write!(f, "not a directory"),
            VfsError::IsADirectory => write!(f, "is a directory"),
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

    fn mkdir(&self, _path: &str) -> Result<()> {
        Err(VfsError::PermissionDenied)
    }

    fn list_dir(&self, _path: &str) -> Result<Vec<String>> {
        Err(VfsError::PermissionDenied)
    }
}

pub struct OpenFile {
    fs: &'static dyn FileSystem,
    handle: FileHandle,
    pub position: usize,
}

impl OpenFile {
    pub fn new(fs: &'static dyn FileSystem, handle: FileHandle) -> Self {
        Self { fs, handle, position: 0 }
    }

    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        let n = self.fs.read(self.handle, buf)?;
        self.position += n;
        Ok(n)
    }

    pub fn write(&mut self, buf: &[u8]) -> Result<usize> {
        let n = self.fs.write(self.handle, buf)?;
        self.position += n;
        Ok(n)
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

static mut ROOT_MEMFS: Option<crate::fs::memfs::MemFs> = None;

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
                let rel = &path[mount_point.len()..];
                let rel = if rel.starts_with('/') { &rel[1..] } else { rel };
                return Ok((*fs, rel));
            }
        }
        Err(VfsError::NotFound)
    }

    pub fn open(&mut self, path: &str, flags: OpenFlags) -> Result<FileDescriptor> {
        let (fs, rel) = self.resolve_path(path)?;
        let handle = fs.open(rel, flags)?;
        let fd = self.next_fd;
        self.next_fd += 1;
        self.open_files.insert(fd, OpenFile::new(fs, handle));
        Ok(fd)
    }

    pub fn read(&mut self, fd: FileDescriptor, buf: &mut [u8]) -> Result<usize> {
        self.open_files.get_mut(&fd)
            .ok_or(VfsError::InvalidDescriptor)?
            .read(buf)
    }

    pub fn write(&mut self, fd: FileDescriptor, buf: &[u8]) -> Result<usize> {
        self.open_files.get_mut(&fd)
            .ok_or(VfsError::InvalidDescriptor)?
            .write(buf)
    }

    pub fn close(&mut self, fd: FileDescriptor) -> Result<()> {
        self.open_files.remove(&fd)
            .ok_or(VfsError::InvalidDescriptor)?;
        Ok(())
    }

    pub fn mkdir(&mut self, path: &str) -> Result<()> {
        let (fs, rel) = self.resolve_path(path)?;
        fs.mkdir(rel)
    }

    pub fn list_dir(&mut self, path: &str) -> Result<Vec<String>> {
        let (fs, rel) = self.resolve_path(path)?;
        fs.list_dir(rel)
    }
}

impl Default for VirtualFileSystem {
    fn default() -> Self {
        Self::new()
    }
}

static mut VFS_INSTANCE: Option<VirtualFileSystem> = None;

pub fn init() {
    let mut vfs = VirtualFileSystem::new();

    let memfs = crate::fs::memfs::MemFs::new();

    // Create standard directories
    let _ = memfs.create_dir("kernel");
    let _ = memfs.create_dir("proc");
    let _ = memfs.create_dir("app");
    let _ = memfs.create_dir("cfg");
    let _ = memfs.create_dir("usr");
    let _ = memfs.create_dir("tmp");

    // Seed a file so cat has something to read
    let _ = memfs.create_file(
        "cfg/os-release",
        b"NAME=\"Dunit OS\"\nVERSION=\"1.0 (Green Tea)\"\nID=dunit\n".to_vec(),
    );

    unsafe {
        ROOT_MEMFS = Some(memfs);
        if let Some(fs) = ROOT_MEMFS.as_ref() {
            vfs.mount("/", fs);
        }
        VFS_INSTANCE = Some(vfs);
    }
}

pub fn get_vfs() -> Option<&'static mut VirtualFileSystem> {
    unsafe { VFS_INSTANCE.as_mut() }
}
