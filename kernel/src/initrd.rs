use alloc::string::String;
use alloc::vec::Vec;
use core::cell::UnsafeCell;

#[derive(Debug)]
pub struct InitrdFile {
    pub name: String,
    pub data: Vec<u8>,
}

pub struct Initrd {
    files: Vec<InitrdFile>,
}

impl Initrd {
    pub const fn new() -> Self {
        Self { files: Vec::new() }
    }

    pub fn add_file(&mut self, name: String, data: Vec<u8>) {
        self.files.push(InitrdFile { name, data });
    }

    pub fn get_file(&self, name: &str) -> Option<&[u8]> {
        self.files
            .iter()
            .find(|f| f.name == name)
            .map(|f| f.data.as_slice())
    }

    pub fn list_files(&self) -> impl Iterator<Item = &str> {
        self.files.iter().map(|f| f.name.as_str())
    }

    pub fn file_count(&self) -> usize {
        self.files.len()
    }
}

/// Синглтон initrd. Раньше `static mut Option<..>`; теперь `UnsafeCell`-newtype
/// без `static mut`. Инициализируется один раз в `init` при загрузке, дальше к
/// нему обращается лишь кооперативный путь на одном CPU.
struct InitrdCell(UnsafeCell<Option<Initrd>>);
unsafe impl Sync for InitrdCell {}
static INITRD_INSTANCE: InitrdCell = InitrdCell(UnsafeCell::new(None));

/// Initialize the initrd store and return the number of files it actually
/// holds. No initrd archive is wired into the boot path yet, so this currently
/// returns 0; the boot log reports that measured count instead of claiming an
/// archive was located and unpacked.
pub fn init() -> usize {
    unsafe {
        *INITRD_INSTANCE.0.get() = Some(Initrd::new());
        (*INITRD_INSTANCE.0.get())
            .as_ref()
            .map(Initrd::file_count)
            .unwrap_or(0)
    }
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
