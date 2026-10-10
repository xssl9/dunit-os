use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::UnsafeCell;
use core::fmt;

use super::memfs::MemFs;

// Applications (/app/*), desktop assets (/assets/**) and the DWM system config
// tree (/system/share/dwm/**) are NO LONGER embedded in the kernel via
// `include_bytes!`. They ship in a Limine-loaded initrd archive (tools/
// pack_initrd.py -> kernel/src/initrd.rs); `init()` below populates the root
// MemFS from `crate::initrd::get_initrd().entries()`, so the kernel knows no app
// or asset name at compile time — every path comes from the archive at runtime.

pub type FileDescriptor = u32;
pub type FileHandle = usize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenFlags {
    bits: u32,
}

impl OpenFlags {
    pub const READ: Self = Self { bits: 1 << 0 };
    pub const WRITE: Self = Self { bits: 1 << 1 };
    pub const CREATE: Self = Self { bits: 1 << 2 };
    pub const TRUNC: Self = Self { bits: 1 << 3 };
    pub const APPEND: Self = Self { bits: 1 << 4 };
    pub const READ_WRITE: Self = Self {
        bits: Self::READ.bits | Self::WRITE.bits,
    };

    const VALID_BITS: u32 = Self::READ.bits
        | Self::WRITE.bits
        | Self::CREATE.bits
        | Self::TRUNC.bits
        | Self::APPEND.bits;

    pub const fn from_bits(bits: u32) -> Self {
        Self { bits }
    }

    pub const fn bits(self) -> u32 {
        self.bits
    }

    pub const fn contains(self, other: Self) -> bool {
        (self.bits & other.bits) == other.bits
    }

    pub const fn can_read(self) -> bool {
        self.contains(Self::READ)
    }

    pub const fn can_write(self) -> bool {
        self.contains(Self::WRITE)
    }

    pub const fn create(self) -> bool {
        self.contains(Self::CREATE)
    }

    pub const fn trunc(self) -> bool {
        self.contains(Self::TRUNC)
    }

    pub const fn append(self) -> bool {
        self.contains(Self::APPEND)
    }

    pub const fn is_valid(self) -> bool {
        let has_unknown = (self.bits & !Self::VALID_BITS) != 0;
        let has_access_mode = self.can_read() || self.can_write();
        let write_only_modifiers_ok = (!self.trunc() && !self.append()) || self.can_write();

        !has_unknown && has_access_mode && write_only_modifiers_ok
    }
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
    DirectoryNotEmpty,
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
            VfsError::DirectoryNotEmpty => write!(f, "Directory not empty"),
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
    /// Remove an empty directory. Filesystems that cannot support it inherit the
    /// default (Unsupported).
    fn remove_dir(&mut self, _path: &str) -> Result<()> {
        Err(VfsError::Unsupported)
    }
    fn truncate(&mut self, path: &str) -> Result<()>;
    fn stat(&mut self, path: &str) -> Result<FileStat>;

    /// Rename/move an entry within this filesystem. Filesystems that cannot
    /// support it (device/proc/read-only backends) inherit this default.
    fn rename(&mut self, _old: &str, _new: &str) -> Result<()> {
        Err(VfsError::Unsupported)
    }

    /// Flush any buffered metadata/data for an open handle to stable storage.
    /// In-memory and mechanism filesystems have nothing to flush and inherit
    /// this no-op; persistent backends (DunitFS) override it to commit.
    fn fsync(&mut self, _handle: FileHandle) -> Result<()> {
        Ok(())
    }
}

pub struct OpenFile {
    fs: *mut dyn FileSystem,
    handle: FileHandle,
}

impl OpenFile {
    pub fn new(fs: *mut dyn FileSystem, handle: FileHandle) -> Self {
        Self { fs, handle }
    }

    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        unsafe { (&mut *self.fs).read(self.handle, buf) }
    }

    pub fn write(&mut self, buf: &[u8]) -> Result<usize> {
        unsafe { (&mut *self.fs).write(self.handle, buf) }
    }

    pub fn fsync(&mut self) -> Result<()> {
        unsafe { (&mut *self.fs).fsync(self.handle) }
    }
}

impl Drop for OpenFile {
    fn drop(&mut self) {
        let _ = unsafe { (&mut *self.fs).close(self.handle) };
    }
}

pub struct VirtualFileSystem {
    root_fs: Option<*mut dyn FileSystem>,
    mounts: Vec<MountPoint>,
    open_files: BTreeMap<FileDescriptor, OpenFile>,
    next_fd: FileDescriptor,
}

struct MountPoint {
    path: String,
    fs: *mut dyn FileSystem,
}

impl VirtualFileSystem {
    pub fn new() -> Self {
        Self {
            root_fs: None,
            mounts: Vec::new(),
            open_files: BTreeMap::new(),
            next_fd: 3,
        }
    }

    pub fn mount(&mut self, path: &str, fs: &'static mut dyn FileSystem) -> Result<()> {
        if path == "/" {
            self.root_fs = Some(fs as *mut dyn FileSystem);
            return Ok(());
        }
        if !path.starts_with('/') || path.ends_with('/') || path.len() > 255 {
            return Err(VfsError::InvalidPath);
        }
        if self.mounts.iter().any(|mount| mount.path == path) {
            return Err(VfsError::AlreadyExists);
        }
        self.mounts.push(MountPoint {
            path: String::from(path),
            fs: fs as *mut dyn FileSystem,
        });
        Ok(())
    }

    fn resolve_path<'a>(
        &self,
        path: &str,
        cwd: &str,
        buffer: &'a mut [u8; 256],
    ) -> Result<(*mut dyn FileSystem, &'a str)> {
        let normalized = normalize_path_into(path, cwd, buffer)?;
        let mut selected: Option<&MountPoint> = None;
        for mount in &self.mounts {
            let exact = normalized == mount.path;
            let child = normalized.starts_with(&mount.path)
                && normalized.as_bytes().get(mount.path.len()) == Some(&b'/');
            if (exact || child)
                && selected
                    .map(|current| mount.path.len() > current.path.len())
                    .unwrap_or(true)
            {
                selected = Some(mount);
            }
        }
        let (fs, relative) = if let Some(mount) = selected {
            let relative = normalized[mount.path.len()..].trim_start_matches('/');
            (mount.fs, relative)
        } else {
            (
                self.root_fs.ok_or(VfsError::NotFound)?,
                normalized.trim_start_matches('/'),
            )
        };

        Ok((fs, relative))
    }

    pub fn open_at(&mut self, cwd: &str, path: &str, flags: OpenFlags) -> Result<FileDescriptor> {
        let (fs, relative_path) = unsafe { self.resolve_path(path, cwd, &mut *VFS_PATH_BUFFER.0.get())? };
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
        let open_file = self
            .open_files
            .get_mut(&fd)
            .ok_or(VfsError::InvalidDescriptor)?;
        open_file.read(buf)
    }

    pub fn write(&mut self, fd: FileDescriptor, buf: &[u8]) -> Result<usize> {
        let open_file = self
            .open_files
            .get_mut(&fd)
            .ok_or(VfsError::InvalidDescriptor)?;
        open_file.write(buf)
    }

    pub fn close(&mut self, fd: FileDescriptor) -> Result<()> {
        self.open_files
            .remove(&fd)
            .ok_or(VfsError::InvalidDescriptor)?;
        Ok(())
    }

    pub fn fsync(&mut self, fd: FileDescriptor) -> Result<()> {
        let open_file = self
            .open_files
            .get_mut(&fd)
            .ok_or(VfsError::InvalidDescriptor)?;
        open_file.fsync()
    }

    pub fn open_file_count(&self) -> usize {
        self.open_files.len()
    }

    pub fn readdir_at(&mut self, cwd: &str, path: &str) -> Result<Vec<DirEntry>> {
        let (fs, relative_path) = unsafe { self.resolve_path(path, cwd, &mut *VFS_PATH_BUFFER.0.get())? };
        unsafe { (&mut *fs).readdir(relative_path) }
    }

    pub fn readdir_into_at(
        &mut self,
        cwd: &str,
        path: &str,
        entries: &mut [DirEntry],
    ) -> Result<usize> {
        let (fs, relative_path) = unsafe { self.resolve_path(path, cwd, &mut *VFS_PATH_BUFFER.0.get())? };
        unsafe { (&mut *fs).readdir_into(relative_path, entries) }
    }

    pub fn create_at(&mut self, cwd: &str, path: &str) -> Result<()> {
        let (fs, relative_path) = unsafe { self.resolve_path(path, cwd, &mut *VFS_PATH_BUFFER.0.get())? };
        unsafe { (&mut *fs).create(relative_path) }
    }

    pub fn mkdir_at(&mut self, cwd: &str, path: &str) -> Result<()> {
        let (fs, relative_path) = unsafe { self.resolve_path(path, cwd, &mut *VFS_PATH_BUFFER.0.get())? };
        unsafe { (&mut *fs).mkdir(relative_path) }
    }

    pub fn remove_at(&mut self, cwd: &str, path: &str) -> Result<()> {
        let (fs, relative_path) = unsafe { self.resolve_path(path, cwd, &mut *VFS_PATH_BUFFER.0.get())? };
        unsafe { (&mut *fs).remove(relative_path) }
    }

    pub fn remove_dir_at(&mut self, cwd: &str, path: &str) -> Result<()> {
        let (fs, relative_path) = unsafe { self.resolve_path(path, cwd, &mut *VFS_PATH_BUFFER.0.get())? };
        unsafe { (&mut *fs).remove_dir(relative_path) }
    }

    pub fn rename_at(&mut self, cwd: &str, old: &str, new: &str) -> Result<()> {
        // Resolve both endpoints to (filesystem, relative path). The path
        // buffer is shared, so copy each relative path out before the next
        // resolve overwrites it. Cross-filesystem renames are unsupported.
        let (old_fs, old_rel) = {
            let (fs, rel) =
                unsafe { self.resolve_path(old, cwd, &mut *VFS_PATH_BUFFER.0.get())? };
            (fs, String::from(rel))
        };
        let (new_fs, new_rel) = {
            let (fs, rel) =
                unsafe { self.resolve_path(new, cwd, &mut *VFS_PATH_BUFFER.0.get())? };
            (fs, String::from(rel))
        };
        if old_fs as *const () != new_fs as *const () {
            return Err(VfsError::Unsupported);
        }
        unsafe { (&mut *old_fs).rename(&old_rel, &new_rel) }
    }

    pub fn truncate_at(&mut self, cwd: &str, path: &str) -> Result<()> {
        let (fs, relative_path) = unsafe { self.resolve_path(path, cwd, &mut *VFS_PATH_BUFFER.0.get())? };
        unsafe { (&mut *fs).truncate(relative_path) }
    }

    pub fn stat_at(&mut self, cwd: &str, path: &str) -> Result<FileStat> {
        let (fs, relative_path) = unsafe { self.resolve_path(path, cwd, &mut *VFS_PATH_BUFFER.0.get())? };
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

pub fn normalize_path_into<'a>(path: &str, cwd: &str, out: &'a mut [u8; 256]) -> Result<&'a str> {
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

/// Синглтон VFS, корневая MemFs и переиспользуемый буфер для нормализации путей.
/// Раньше три `static mut`; теперь `UnsafeCell`-newtype без `static mut`. Всё это
/// инициализируется на этапе загрузки и используется кооперативно на одном CPU;
/// раздача `&'static mut` через `as_mut()`/`.get()` сохраняет прежнюю семантику
/// (в т.ч. монтирование `&'static mut ROOT_MEMFS` в VFS).
struct VfsInstanceCell(UnsafeCell<Option<VirtualFileSystem>>);
unsafe impl Sync for VfsInstanceCell {}
static VFS_INSTANCE: VfsInstanceCell = VfsInstanceCell(UnsafeCell::new(None));

struct RootMemFsCell(UnsafeCell<MemFs>);
unsafe impl Sync for RootMemFsCell {}
static ROOT_MEMFS: RootMemFsCell = RootMemFsCell(UnsafeCell::new(MemFs::empty()));

struct VfsPathBufferCell(UnsafeCell<[u8; 256]>);
unsafe impl Sync for VfsPathBufferCell {}
static VFS_PATH_BUFFER: VfsPathBufferCell = VfsPathBufferCell(UnsafeCell::new([0; 256]));
const GUI_SHORTCUTS_CONFIG: &[u8] = b"super+q=close_window\nsuper+enter=open_terminal\n";

fn serial_log(msg: &str) {
    crate::serial::serial_write(msg);
}

/// Create every intermediate directory for `path` (all components except the
/// last) in the root MemFS, idempotently. MemFS `mkdir` is not recursive and
/// rejects an existing or base directory with `AlreadyExists`, so each level is
/// created top-down and errors are ignored. The archive ships files in
/// path-sorted order, but this routine does not rely on that.
fn ensure_parent_dirs(memfs: &mut MemFs, path: &str) {
    let trimmed = path.trim_matches('/');
    let mut parts = trimmed.split('/').peekable();
    let mut prefix = String::new();
    while let Some(part) = parts.next() {
        if parts.peek().is_none() {
            break; // the last component is the file itself, not a directory
        }
        prefix.push('/');
        prefix.push_str(part);
        let _ = memfs.mkdir(&prefix);
    }
}

pub fn init() -> Result<()> {
    serial_log("[VFS] init START\r\n");
    unsafe {
        let mut vfs = VirtualFileSystem::new();
        let memfs: &'static mut MemFs = &mut *ROOT_MEMFS.0.get();

        // Populate /app and /assets entirely from the Limine-loaded initrd
        // archive (kernel/src/initrd.rs). The kernel embeds no application
        // binaries or desktop assets; every path comes from the archive. Asset
        // files are mounted zero-copy read-only (they reference module memory,
        // which Limine never reclaims); everything else is copied into a
        // writable MemFS node.
        match crate::initrd::get_initrd() {
            Some(initrd) => {
                for entry in initrd.entries() {
                    ensure_parent_dirs(memfs, &entry.name);
                    if entry.name.starts_with("/assets/") {
                        memfs.add_static_file(&entry.name, entry.data);
                    } else {
                        memfs.add_file(&entry.name, entry.data.to_vec());
                    }
                }
            }
            None => serial_log("[VFS] WARN: no initrd archive; /app and /assets are empty\r\n"),
        }

        // Kernel-seeded writable nodes that are not shipped in the archive.
        let _ = memfs.mkdir("/persist");
        ensure_parent_dirs(memfs, "/cfg/gui/shortcuts.conf");
        memfs.add_file("/cfg/gui/shortcuts.conf", GUI_SHORTCUTS_CONFIG.to_vec());

        // Writable window/session store (M4): pre-created empty so gui_server
        // can rewrite it at runtime. RAM-only until DunitFS v2 provides a
        // persistent user-config partition, so geometry survives a
        // reload/reconnect within a boot, not across boots.
        ensure_parent_dirs(memfs, "/system/share/dwm/session.toml");
        memfs.add_file(
            "/system/share/dwm/session.toml",
            b"# DWM window session store (auto-written by gui_server).\n".to_vec(),
        );

        vfs.mount("/", memfs)?;
        serial_log("[MEMFS] mounted as /\r\n");

        *VFS_INSTANCE.0.get() = Some(vfs);
    }

    serial_log("[VFS] init OK\r\n");
    Ok(())
}

pub fn get_vfs() -> Option<&'static mut VirtualFileSystem> {
    unsafe { (*VFS_INSTANCE.0.get()).as_mut() }
}

pub fn static_file(path: &str) -> Option<&'static [u8]> {
    unsafe { (*ROOT_MEMFS.0.get()).static_file(path) }
}

pub fn register_device_node(path: &str) {
    unsafe {
        (*ROOT_MEMFS.0.get()).add_device(path);
    }
}

pub fn root_memfs_stats() -> crate::fs::memfs::MemFsStats {
    unsafe { (*ROOT_MEMFS.0.get()).stats() }
}

/// Boot smoke test (feature `boot-smoke-tests`) exercising the M4 structural
/// FS syscalls at the VFS layer: mkdir, create, rename (file + directory
/// subtree re-home), unlink, the error paths (AlreadyExists / NotFound /
/// IsADirectory) and the read-only guard on `MemData::Static` asset entries.
/// Returns true only if every step behaves as expected.
#[cfg(feature = "boot-smoke-tests")]
pub fn run_fs_mutation_smoke() -> bool {
    fn expect(cond: bool, msg: &str) -> bool {
        if !cond {
            serial_log("[FS-SMOKE] FAIL: ");
            serial_log(msg);
            serial_log("\r\n");
        }
        cond
    }

    let vfs = match get_vfs() {
        Some(vfs) => vfs,
        None => return false,
    };
    let root = "/";
    let mut ok = true;

    // Happy path: mkdir under the writable /tmp tree.
    ok &= expect(vfs.mkdir_at(root, "/tmp/fs_smoke").is_ok(), "mkdir /tmp/fs_smoke");
    ok &= expect(
        vfs.mkdir_at(root, "/tmp/fs_smoke") == Err(VfsError::AlreadyExists),
        "mkdir duplicate -> AlreadyExists",
    );

    // Create a file, rename it, verify the old name is gone and new present.
    ok &= expect(vfs.create_at(root, "/tmp/fs_smoke/a.txt").is_ok(), "create a.txt");
    ok &= expect(
        vfs.rename_at(root, "/tmp/fs_smoke/a.txt", "/tmp/fs_smoke/b.txt").is_ok(),
        "rename a.txt -> b.txt",
    );
    ok &= expect(
        vfs.stat_at(root, "/tmp/fs_smoke/a.txt") == Err(VfsError::NotFound),
        "old name gone",
    );
    ok &= expect(vfs.stat_at(root, "/tmp/fs_smoke/b.txt").is_ok(), "new name present");

    // Rename the directory; its descendant file must be re-homed with it.
    ok &= expect(
        vfs.rename_at(root, "/tmp/fs_smoke", "/tmp/fs_smoke2").is_ok(),
        "rename dir -> fs_smoke2",
    );
    ok &= expect(
        vfs.stat_at(root, "/tmp/fs_smoke2/b.txt").is_ok(),
        "descendant re-homed",
    );

    // Unlink the file; a directory cannot be unlinked.
    ok &= expect(vfs.remove_at(root, "/tmp/fs_smoke2/b.txt").is_ok(), "unlink b.txt");
    ok &= expect(
        vfs.stat_at(root, "/tmp/fs_smoke2/b.txt") == Err(VfsError::NotFound),
        "b.txt gone after unlink",
    );
    ok &= expect(
        vfs.remove_at(root, "/tmp/fs_smoke2") == Err(VfsError::IsADirectory),
        "unlink dir -> IsADirectory",
    );

    // Read-only guard: a Static asset entry must reject remove and rename.
    const ASSET: &str = "/assets/fonts/DejaVuSans.ttf";
    ok &= expect(
        vfs.remove_at(root, ASSET) == Err(VfsError::PermissionDenied),
        "remove static asset -> PermissionDenied",
    );
    ok &= expect(
        vfs.rename_at(root, ASSET, "/tmp/stolen.ttf") == Err(VfsError::PermissionDenied),
        "rename static asset -> PermissionDenied",
    );

    ok
}
