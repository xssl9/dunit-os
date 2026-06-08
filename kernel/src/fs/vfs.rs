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
    Append,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileType {
    File,
    Directory,
    Device,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirEntry {
    name: [u8; 64],
    name_len: usize,
    pub file_type: FileType,
}

impl DirEntry {
    pub const fn empty() -> Self {
        Self {
            name: [0; 64],
            name_len: 0,
            file_type: FileType::File,
        }
    }

    pub fn new(name: &str, file_type: FileType) -> Self {
        let mut entry = Self {
            name: [0; 64],
            name_len: 0,
            file_type,
        };
        let bytes = name.as_bytes();
        let len = bytes.len().min(entry.name.len());
        let mut idx = 0;
        while idx < len {
            entry.name[idx] = bytes[idx];
            idx += 1;
        }
        entry.name_len = len;
        entry
    }

    pub fn name(&self) -> &str {
        core::str::from_utf8(&self.name[..self.name_len]).unwrap_or("<invalid>")
    }
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
    fn readdir_into(&mut self, path: &str, entries: &mut [DirEntry]) -> Result<usize>;
    fn create(&mut self, path: &str) -> Result<()>;
    fn mkdir(&mut self, path: &str) -> Result<()>;
    fn remove(&mut self, path: &str) -> Result<()>;
    fn truncate(&mut self, path: &str) -> Result<()>;
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

    fn resolve_path<'a>(
        &self,
        path: &str,
        cwd: &str,
        buffer: &'a mut [u8; 256],
    ) -> Result<(*mut dyn FileSystem, &'a str)> {
        let normalized = normalize_path_into(path, cwd, buffer)?;
        let fs = self.root_fs.ok_or(VfsError::NotFound)?;
        let relative = normalized.trim_start_matches('/');

        Ok((fs, relative))
    }

    pub fn open_at(&mut self, cwd: &str, path: &str, flags: OpenFlags) -> Result<FileDescriptor> {
        let (fs, relative_path) = unsafe { self.resolve_path(path, cwd, &mut VFS_PATH_BUFFER)? };
        let handle = unsafe { (&mut *fs).open(relative_path, flags)? };

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
        let (fs, relative_path) = unsafe { self.resolve_path(path, cwd, &mut VFS_PATH_BUFFER)? };
        unsafe { (&mut *fs).readdir(relative_path) }
    }

    pub fn readdir_into_at(
        &mut self,
        cwd: &str,
        path: &str,
        entries: &mut [DirEntry],
    ) -> Result<usize> {
        let (fs, relative_path) = unsafe { self.resolve_path(path, cwd, &mut VFS_PATH_BUFFER)? };
        unsafe { (&mut *fs).readdir_into(relative_path, entries) }
    }

    pub fn create_at(&mut self, cwd: &str, path: &str) -> Result<()> {
        let (fs, relative_path) = unsafe { self.resolve_path(path, cwd, &mut VFS_PATH_BUFFER)? };
        unsafe { (&mut *fs).create(relative_path) }
    }

    pub fn mkdir_at(&mut self, cwd: &str, path: &str) -> Result<()> {
        let (fs, relative_path) = unsafe { self.resolve_path(path, cwd, &mut VFS_PATH_BUFFER)? };
        unsafe { (&mut *fs).mkdir(relative_path) }
    }

    pub fn remove_at(&mut self, cwd: &str, path: &str) -> Result<()> {
        let (fs, relative_path) = unsafe { self.resolve_path(path, cwd, &mut VFS_PATH_BUFFER)? };
        unsafe { (&mut *fs).remove(relative_path) }
    }

    pub fn truncate_at(&mut self, cwd: &str, path: &str) -> Result<()> {
        let (fs, relative_path) = unsafe { self.resolve_path(path, cwd, &mut VFS_PATH_BUFFER)? };
        unsafe { (&mut *fs).truncate(relative_path) }
    }

    pub fn stat_at(&mut self, cwd: &str, path: &str) -> Result<FileStat> {
        let (fs, relative_path) = unsafe { self.resolve_path(path, cwd, &mut VFS_PATH_BUFFER)? };
        unsafe { (&mut *fs).stat(relative_path) }
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
    let mut buffer = [0u8; 256];
    let normalized = normalize_path_into(path, cwd, &mut buffer)?;
    Ok(String::from(normalized))
}

pub fn normalize_path_into<'a>(
    path: &str,
    cwd: &str,
    out: &'a mut [u8; 256],
) -> Result<&'a str> {
    let mut len = 1;
    out[0] = b'/';

    if path.is_empty() {
        append_path_components(cwd, out, &mut len)?;
    } else if !path.starts_with('/') {
        append_path_components(cwd, out, &mut len)?;
        append_path_components(path, out, &mut len)?;
    } else {
        append_path_components(path, out, &mut len)?;
    }

    core::str::from_utf8(&out[..len]).map_err(|_| VfsError::InvalidPath)
}

fn append_path_components(path: &str, out: &mut [u8; 256], len: &mut usize) -> Result<()> {
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => pop_path_component(out, len),
            _ => push_path_component(part, out, len)?,
        }
    }

    Ok(())
}

fn push_path_component(part: &str, out: &mut [u8; 256], len: &mut usize) -> Result<()> {
    let part_bytes = part.as_bytes();
    let needs_slash = *len > 1;
    let required = part_bytes.len() + if needs_slash { 1 } else { 0 };

    if *len + required > out.len() {
        return Err(VfsError::InvalidPath);
    }

    if needs_slash {
        out[*len] = b'/';
        *len += 1;
    }

    out[*len..*len + part_bytes.len()].copy_from_slice(part_bytes);
    *len += part_bytes.len();
    Ok(())
}

fn pop_path_component(out: &[u8; 256], len: &mut usize) {
    if *len <= 1 {
        *len = 1;
        return;
    }

    let mut idx = *len - 1;
    while idx > 0 && out[idx] != b'/' {
        idx -= 1;
    }

    *len = if idx == 0 { 1 } else { idx };
}

static mut VFS_INSTANCE: Option<VirtualFileSystem> = None;
static mut ROOT_MEMFS: MemFs = MemFs::empty();
static mut VFS_PATH_BUFFER: [u8; 256] = [0; 256];

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
