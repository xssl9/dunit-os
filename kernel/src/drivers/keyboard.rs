use crate::hal;

static mut LAST_SCANCODE: u8 = 0;
static mut SCANCODE_BUFFER: [u8; 16] = [0; 16];
static mut BUFFER_READ: usize = 0;
static mut BUFFER_WRITE: usize = 0;

pub fn init() {
}

pub fn read_scancode() -> Option<u8> {
    unsafe {
        if BUFFER_READ != BUFFER_WRITE {
            let scancode = SCANCODE_BUFFER[BUFFER_READ];
            BUFFER_READ = (BUFFER_READ + 1) % 16;
            Some(scancode)
        } else {
            None
        }
    }
}

pub fn push_scancode(scancode: u8) {
    unsafe {
        let next_write = (BUFFER_WRITE + 1) % 16;
        if next_write != BUFFER_READ {
            SCANCODE_BUFFER[BUFFER_WRITE] = scancode;
            BUFFER_WRITE = next_write;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpecialKey {
    UpArrow,
    DownArrow,
    LeftArrow,
    RightArrow,
}

pub fn scancode_to_special_key(scancode: u8) -> Option<SpecialKey> {
    match scancode {
        0x48 => Some(SpecialKey::UpArrow),
        0x50 => Some(SpecialKey::DownArrow),
        0x4B => Some(SpecialKey::LeftArrow),
        0x4D => Some(SpecialKey::RightArrow),
        _ => None,
    }
}

pub fn scancode_to_char(scancode: u8) -> Option<char> {
    match scancode {
        0x1E => Some('a'),
        0x30 => Some('b'),
        0x2E => Some('c'),
        0x20 => Some('d'),
        0x12 => Some('e'),
        0x21 => Some('f'),
        0x22 => Some('g'),
        0x23 => Some('h'),
        0x17 => Some('i'),
        0x24 => Some('j'),
        0x25 => Some('k'),
        0x26 => Some('l'),
        0x32 => Some('m'),
        0x31 => Some('n'),
        0x18 => Some('o'),
        0x19 => Some('p'),
        0x10 => Some('q'),
        0x13 => Some('r'),
        0x1F => Some('s'),
        0x14 => Some('t'),
        0x16 => Some('u'),
        0x2F => Some('v'),
        0x11 => Some('w'),
        0x2D => Some('x'),
        0x15 => Some('y'),
        0x2C => Some('z'),
        0x39 => Some(' '),
        0x1C => Some('\n'),
        _ => None,
    }
}
