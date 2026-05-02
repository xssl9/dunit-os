use alloc::vec::Vec;
use alloc::string::String;

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
        Self {
            files: Vec::new(),
        }
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
}

static mut INITRD_INSTANCE: Option<Initrd> = None;

pub fn init() {
    unsafe {
        INITRD_INSTANCE = Some(Initrd::new());
    }
}

pub fn get_initrd() -> Option<&'static mut Initrd> {
    unsafe { INITRD_INSTANCE.as_mut() }
}
