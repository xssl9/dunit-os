use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

use super::memfs::MemFs;

static ELF_DEMO_BYTES: &[u8] = include_bytes!("../../../build/userspace/elf_demo");
static FS_TEST_BYTES: &[u8] = include_bytes!("../../../build/userspace/fs_test");
static EXIT_TEST_BYTES: &[u8] = include_bytes!("../../../build/userspace/exit_test");
static ARGS_TEST_BYTES: &[u8] = include_bytes!("../../../build/userspace/args_test");
static CWD_TEST_BYTES: &[u8] = include_bytes!("../../../build/userspace/cwd_test");
static PATH_TEST_BYTES: &[u8] = include_bytes!("../../../build/userspace/path_test");
static IMAGE_DEMO_BYTES: &[u8] = include_bytes!("../../../build/userspace/image_demo");
static BMP_VIEWER_BYTES: &[u8] = include_bytes!("../../../build/userspace/bmp_viewer");
static SCHEDULER_TEST_BYTES: &[u8] = include_bytes!("../../../build/userspace/scheduler_test");
static SPAWN_READY_TEST_BYTES: &[u8] = include_bytes!("../../../build/userspace/spawn_ready_test");
static YIELD_CHILD_BYTES: &[u8] = include_bytes!("../../../build/userspace/yield_child");
static YIELD_TEST_BYTES: &[u8] = include_bytes!("../../../build/userspace/yield_test");
static RESUMABLE_CHILD_BYTES: &[u8] = include_bytes!("../../../build/userspace/resumable_child");
static RESUMABLE_TEST_BYTES: &[u8] = include_bytes!("../../../build/userspace/resumable_test");
static IPC_CHILD_BYTES: &[u8] = include_bytes!("../../../build/userspace/ipc_child");
static IPC_PARENT_BYTES: &[u8] = include_bytes!("../../../build/userspace/ipc_parent");
static RUNTIME_STRESS_BYTES: &[u8] = include_bytes!("../../../build/userspace/runtime_stress");
static INPUT_TEST_BYTES: &[u8] = include_bytes!("../../../build/userspace/input_test");
static FILE_API_TEST_BYTES: &[u8] = include_bytes!("../../../build/userspace/file_api_test");
static ENV_TEST_BYTES: &[u8] = include_bytes!("../../../build/userspace/env_test");
static CALC_BYTES: &[u8] = include_bytes!("../../../build/userspace/calc");
static GUI_PING_BYTES: &[u8] = include_bytes!("../../../build/userspace/gui_ping");
static GUI_TERMINAL_STUB_BYTES: &[u8] =
    include_bytes!("../../../build/userspace/gui_terminal_stub");
static GUI_CALCULATOR_BYTES: &[u8] = include_bytes!("../../../build/userspace/gui_calculator");
static GUI_STATS_BYTES: &[u8] = include_bytes!("../../../build/userspace/gui_stats");
static GUI_FILE_MANAGER_BYTES: &[u8] = include_bytes!("../../../build/userspace/gui_file_manager");
static STDIN_TEST_BYTES: &[u8] = include_bytes!("../../../build/userspace/stdin_test");
static DTOP_BYTES: &[u8] = include_bytes!("../../../build/userspace/dtop");
static FAULT_PF_BYTES: &[u8] = include_bytes!("../../../build/userspace/fault_pf");
static FAULT_UD_BYTES: &[u8] = include_bytes!("../../../build/userspace/fault_ud");

pub struct AssetEntry {
    pub path: &'static str,
    pub data: &'static [u8],
}

include!(concat!(env!("OUT_DIR"), "/assets_manifest.rs"));

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

    pub fn open_file_count(&self) -> usize {
        self.open_files.len()
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

static mut VFS_INSTANCE: Option<VirtualFileSystem> = None;
static mut ROOT_MEMFS: MemFs = MemFs::empty();
static mut VFS_PATH_BUFFER: [u8; 256] = [0; 256];

extern "C" {
    fn serial_write(s: *const u8);
}

fn serial_log(msg: &'static [u8]) {
    unsafe { serial_write(msg.as_ptr()) }
}

fn register_assets() {
    unsafe {
        for dir in ASSET_DIRS.iter() {
            let _ = ROOT_MEMFS.mkdir(dir);
        }

        for asset in ASSETS.iter() {
            ROOT_MEMFS.add_static_file(asset.path, asset.data);
        }
    }
}

pub fn init() -> Result<()> {
    serial_log(b"[VFS] init START\r\n\0");
    unsafe {
        let mut vfs = VirtualFileSystem::new();

        let mut elf_demo = Vec::new();
        elf_demo.extend_from_slice(ELF_DEMO_BYTES);
        ROOT_MEMFS.add_file("/app/elf_demo", elf_demo);

        let mut fs_test = Vec::new();
        fs_test.extend_from_slice(FS_TEST_BYTES);
        ROOT_MEMFS.add_file("/app/fs_test", fs_test);

        let mut exit_test = Vec::new();
        exit_test.extend_from_slice(EXIT_TEST_BYTES);
        ROOT_MEMFS.add_file("/app/exit_test", exit_test);

        let mut args_test = Vec::new();
        args_test.extend_from_slice(ARGS_TEST_BYTES);
        ROOT_MEMFS.add_file("/app/args_test", args_test);

        let mut cwd_test = Vec::new();
        cwd_test.extend_from_slice(CWD_TEST_BYTES);
        ROOT_MEMFS.add_file("/app/cwd_test", cwd_test);

        let mut path_test = Vec::new();
        path_test.extend_from_slice(PATH_TEST_BYTES);
        ROOT_MEMFS.add_file("/app/path_test", path_test);

        let mut image_demo = Vec::new();
        image_demo.extend_from_slice(IMAGE_DEMO_BYTES);
        ROOT_MEMFS.add_file("/app/image_demo", image_demo);

        let mut bmp_viewer = Vec::new();
        bmp_viewer.extend_from_slice(BMP_VIEWER_BYTES);
        ROOT_MEMFS.add_file("/app/bmp_viewer", bmp_viewer);

        register_assets();

        let mut scheduler_test = Vec::new();
        scheduler_test.extend_from_slice(SCHEDULER_TEST_BYTES);
        ROOT_MEMFS.add_file("/app/scheduler_test", scheduler_test);

        let mut spawn_ready_test = Vec::new();
        spawn_ready_test.extend_from_slice(SPAWN_READY_TEST_BYTES);
        ROOT_MEMFS.add_file("/app/spawn_ready_test", spawn_ready_test);

        let mut yield_child = Vec::new();
        yield_child.extend_from_slice(YIELD_CHILD_BYTES);
        ROOT_MEMFS.add_file("/app/yield_child", yield_child);

        let mut yield_test = Vec::new();
        yield_test.extend_from_slice(YIELD_TEST_BYTES);
        ROOT_MEMFS.add_file("/app/yield_test", yield_test);

        let mut resumable_child = Vec::new();
        resumable_child.extend_from_slice(RESUMABLE_CHILD_BYTES);
        ROOT_MEMFS.add_file("/app/resumable_child", resumable_child);

        let mut resumable_test = Vec::new();
        resumable_test.extend_from_slice(RESUMABLE_TEST_BYTES);
        ROOT_MEMFS.add_file("/app/resumable_test", resumable_test);

        let mut ipc_child = Vec::new();
        ipc_child.extend_from_slice(IPC_CHILD_BYTES);
        ROOT_MEMFS.add_file("/app/ipc_child", ipc_child);

        let mut ipc_parent = Vec::new();
        ipc_parent.extend_from_slice(IPC_PARENT_BYTES);
        ROOT_MEMFS.add_file("/app/ipc_parent", ipc_parent);

        let mut runtime_stress = Vec::new();
        runtime_stress.extend_from_slice(RUNTIME_STRESS_BYTES);
        ROOT_MEMFS.add_file("/app/runtime_stress", runtime_stress);

        let mut input_test = Vec::new();
        input_test.extend_from_slice(INPUT_TEST_BYTES);
        ROOT_MEMFS.add_file("/app/input_test", input_test);

        let mut file_api_test = Vec::new();
        file_api_test.extend_from_slice(FILE_API_TEST_BYTES);
        ROOT_MEMFS.add_file("/app/file_api_test", file_api_test);

        let mut env_test = Vec::new();
        env_test.extend_from_slice(ENV_TEST_BYTES);
        ROOT_MEMFS.add_file("/app/env_test", env_test);

        let mut calc = Vec::new();
        calc.extend_from_slice(CALC_BYTES);
        ROOT_MEMFS.add_file("/app/calc", calc);

        let mut gui_ping = Vec::new();
        gui_ping.extend_from_slice(GUI_PING_BYTES);
        ROOT_MEMFS.add_file("/app/gui_ping", gui_ping);

        let mut gui_terminal_stub = Vec::new();
        gui_terminal_stub.extend_from_slice(GUI_TERMINAL_STUB_BYTES);
        ROOT_MEMFS.add_file("/app/gui_terminal_stub", gui_terminal_stub);

        let mut gui_calculator = Vec::new();
        gui_calculator.extend_from_slice(GUI_CALCULATOR_BYTES);
        ROOT_MEMFS.add_file("/app/gui_calculator", gui_calculator);

        let mut gui_stats = Vec::new();
        gui_stats.extend_from_slice(GUI_STATS_BYTES);
        ROOT_MEMFS.add_file("/app/gui_stats", gui_stats);

        let mut gui_file_manager = Vec::new();
        gui_file_manager.extend_from_slice(GUI_FILE_MANAGER_BYTES);
        ROOT_MEMFS.add_file("/app/gui_file_manager", gui_file_manager);

        let mut stdin_test = Vec::new();
        stdin_test.extend_from_slice(STDIN_TEST_BYTES);
        ROOT_MEMFS.add_file("/app/stdin_test", stdin_test);

        let mut dtop = Vec::new();
        dtop.extend_from_slice(DTOP_BYTES);
        ROOT_MEMFS.add_file("/app/dtop", dtop);

        let mut fault_pf = Vec::new();
        fault_pf.extend_from_slice(FAULT_PF_BYTES);
        ROOT_MEMFS.add_file("/app/fault_pf", fault_pf);

        let mut fault_ud = Vec::new();
        fault_ud.extend_from_slice(FAULT_UD_BYTES);
        ROOT_MEMFS.add_file("/app/fault_ud", fault_ud);

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

pub fn static_file(path: &str) -> Option<&'static [u8]> {
    unsafe { ROOT_MEMFS.static_file(path) }
}

pub fn register_device_node(path: &str) {
    unsafe {
        ROOT_MEMFS.add_device(path);
    }
}

pub fn root_memfs_stats() -> crate::fs::memfs::MemFsStats {
    unsafe { ROOT_MEMFS.stats() }
}
