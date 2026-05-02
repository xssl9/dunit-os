use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

pub struct Terminal {
}

impl Terminal {
    pub fn new() -> Self {
        Self {}
    }
}

static mut TERMINAL_INSTANCE: Option<Terminal> = None;

pub fn init() {
}

pub fn get_terminal() -> Option<&'static mut Terminal> {
    unsafe {
        if TERMINAL_INSTANCE.is_none() {
            TERMINAL_INSTANCE = Some(Terminal::new());
        }
        TERMINAL_INSTANCE.as_mut()
    }
}
