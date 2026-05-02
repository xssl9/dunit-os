use crate::hal;

static mut LAST_SCANCODE: u8 = 0;

pub fn init() {
}

pub fn read_scancode() -> Option<u8> {
    unsafe {
        let status: u8;
        core::arch::asm!("in al, dx", out("al") status, in("dx") 0x64u16, options(nomem, nostack));
        
        if (status & 0x01) != 0 {
            let scancode: u8;
            core::arch::asm!("in al, dx", out("al") scancode, in("dx") 0x60u16, options(nomem, nostack));
            LAST_SCANCODE = scancode;
            Some(scancode)
        } else {
            None
        }
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
