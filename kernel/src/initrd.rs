use alloc::string::String;
use alloc::vec::Vec;
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

/// Archive magic. Must match the header written by tools/pack_initrd.py.
const MAGIC: [u8; 8] = *b"DUNITRD1";

/// One file unpacked from the initrd archive. `data` is a zero-copy slice into
/// the Limine module memory (see `parse_module`); it is never heap-copied here.
#[derive(Debug)]
pub struct InitrdFile {
    pub name: String,
    pub data: &'static [u8],
}

pub struct Initrd {
    files: Vec<InitrdFile>,
}

impl Initrd {
    pub const fn new() -> Self {
        Self { files: Vec::new() }
    }

    pub fn add_file(&mut self, name: String, data: &'static [u8]) {
        self.files.push(InitrdFile { name, data });
    }

    pub fn get_file(&self, name: &str) -> Option<&'static [u8]> {
        self.files.iter().find(|f| f.name == name).map(|f| f.data)
    }

    /// Every file in the archive, in packed (path-sorted) order. The VFS walks
    /// this to populate the root MemFS without the kernel knowing any app or
    /// asset name — the paths come entirely from the archive.
    pub fn entries(&self) -> &[InitrdFile] {
        &self.files
    }

    pub fn list_files(&self) -> impl Iterator<Item = &str> {
        self.files.iter().map(|f| f.name.as_str())
    }

    pub fn file_count(&self) -> usize {
        self.files.len()
    }
}

/// Limine module pointer+length, handed over by the boot shim (boot_main.c ->
/// kernel_main -> `set_module`). The address is an HHDM virtual address.
///
/// Limine places boot modules in memmap type 6 ("kernel and modules") memory,
/// which the PMM never reclaims (it only frees type-0 usable regions, see
/// memory/pmm.rs). The module therefore lives for the kernel's whole lifetime,
/// so slices into it are sound to hand out as `&'static [u8]` — exactly the
/// pattern `storage::installer::payload()` already relies on.
static MODULE_PTR: AtomicPtr<u8> = AtomicPtr::new(core::ptr::null_mut());
static MODULE_LEN: AtomicUsize = AtomicUsize::new(0);

pub fn set_module(ptr: *const u8, len: usize) {
    MODULE_PTR.store(ptr as *mut u8, Ordering::SeqCst);
    MODULE_LEN.store(len, Ordering::SeqCst);
}

/// Синглтон initrd. Раньше `static mut Option<..>`; теперь `UnsafeCell`-newtype
/// без `static mut`. Инициализируется один раз в `init` при загрузке, дальше к
/// нему обращается лишь кооперативный путь на одном CPU.
struct InitrdCell(UnsafeCell<Option<Initrd>>);
unsafe impl Sync for InitrdCell {}
static INITRD_INSTANCE: InitrdCell = InitrdCell(UnsafeCell::new(None));

fn read_u32_le(buf: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([buf[at], buf[at + 1], buf[at + 2], buf[at + 3]])
}

/// Parse the DUNITRD1 archive from the Limine module memory. Returns `None` if
/// no module was handed over or the archive is malformed/truncated; every
/// length is bounds-checked against the module size before it is trusted.
fn parse_module() -> Option<Initrd> {
    let ptr = MODULE_PTR.load(Ordering::SeqCst);
    let len = MODULE_LEN.load(Ordering::SeqCst);
    if ptr.is_null() || len < 12 {
        return None;
    }
    // SAFETY: see MODULE_PTR docs — module memory is never freed, so a slice of
    // the reported length is valid for 'static.
    let module: &'static [u8] = unsafe { core::slice::from_raw_parts(ptr, len) };

    if module[0..8] != MAGIC {
        return None;
    }
    let count = read_u32_le(module, 8) as usize;

    let mut initrd = Initrd::new();
    let mut off = 12usize;
    for _ in 0..count {
        if off + 8 > len {
            return None;
        }
        let name_len = read_u32_le(module, off) as usize;
        let data_len = read_u32_le(module, off + 4) as usize;
        off += 8;
        if off + name_len > len || off + name_len + data_len > len {
            return None;
        }
        let name = core::str::from_utf8(&module[off..off + name_len]).ok()?;
        off += name_len;
        let data = &module[off..off + data_len];
        off += data_len;
        initrd.add_file(String::from(name), data);
    }
    Some(initrd)
}

/// Parse the Limine-loaded initrd archive and return the number of files it
/// yields. Returns 0 if no module was provided or the archive is malformed; the
/// boot log reports that measured count. Called BEFORE `fs::vfs::init`, which
/// populates the root MemFS from `get_initrd().entries()`.
pub fn init() -> usize {
    let initrd = parse_module().unwrap_or_else(Initrd::new);
    let count = initrd.file_count();
    unsafe {
        *INITRD_INSTANCE.0.get() = Some(initrd);
    }
    count
}

/// Number of files currently held by the initrd store, or 0 before init.
pub fn file_count() -> usize {
    unsafe {
        (*INITRD_INSTANCE.0.get())
            .as_ref()
            .map(Initrd::file_count)
            .unwrap_or(0)
    }
}

pub fn get_initrd() -> Option<&'static mut Initrd> {
    unsafe { (*INITRD_INSTANCE.0.get()).as_mut() }
}
