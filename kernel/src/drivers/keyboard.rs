static mut SCANCODE_BUFFER: [u8; 16] = [0; 16];
static mut BUFFER_READ: usize = 0;
static mut BUFFER_WRITE: usize = 0;
static mut SHIFT_DOWN: bool = false;

pub fn init() {}

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
        match scancode {
            0x2A | 0x36 => {
                SHIFT_DOWN = true;
            }
            0xAA | 0xB6 => {
                SHIFT_DOWN = false;
            }
            _ => {}
        }

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
    let shifted = unsafe { SHIFT_DOWN };

    let ch = match scancode {
        0x02 => {
            if shifted {
                '!'
            } else {
                '1'
            }
        }
        0x03 => {
            if shifted {
                '@'
            } else {
                '2'
            }
        }
        0x04 => {
            if shifted {
                '#'
            } else {
                '3'
            }
        }
        0x05 => {
            if shifted {
                '$'
            } else {
                '4'
            }
        }
        0x06 => {
            if shifted {
                '%'
            } else {
                '5'
            }
        }
        0x07 => {
            if shifted {
                '^'
            } else {
                '6'
            }
        }
        0x08 => {
            if shifted {
                '&'
            } else {
                '7'
            }
        }
        0x09 => {
            if shifted {
                '*'
            } else {
                '8'
            }
        }
        0x0A => {
            if shifted {
                '('
            } else {
                '9'
            }
        }
        0x0B => {
            if shifted {
                ')'
            } else {
                '0'
            }
        }
        0x0C => {
            if shifted {
                '_'
            } else {
                '-'
            }
        }
        0x0D => {
            if shifted {
                '+'
            } else {
                '='
            }
        }
        0x10 => {
            if shifted {
                'Q'
            } else {
                'q'
            }
        }
        0x11 => {
            if shifted {
                'W'
            } else {
                'w'
            }
        }
        0x12 => {
            if shifted {
                'E'
            } else {
                'e'
            }
        }
        0x13 => {
            if shifted {
                'R'
            } else {
                'r'
            }
        }
        0x14 => {
            if shifted {
                'T'
            } else {
                't'
            }
        }
        0x15 => {
            if shifted {
                'Y'
            } else {
                'y'
            }
        }
        0x16 => {
            if shifted {
                'U'
            } else {
                'u'
            }
        }
        0x17 => {
            if shifted {
                'I'
            } else {
                'i'
            }
        }
        0x18 => {
            if shifted {
                'O'
            } else {
                'o'
            }
        }
        0x19 => {
            if shifted {
                'P'
            } else {
                'p'
            }
        }
        0x1A => {
            if shifted {
                '{'
            } else {
                '['
            }
        }
        0x1B => {
            if shifted {
                '}'
            } else {
                ']'
            }
        }
        0x1E => {
            if shifted {
                'A'
            } else {
                'a'
            }
        }
        0x1F => {
            if shifted {
                'S'
            } else {
                's'
            }
        }
        0x20 => {
            if shifted {
                'D'
            } else {
                'd'
            }
        }
        0x21 => {
            if shifted {
                'F'
            } else {
                'f'
            }
        }
        0x22 => {
            if shifted {
                'G'
            } else {
                'g'
            }
        }
        0x23 => {
            if shifted {
                'H'
            } else {
                'h'
            }
        }
        0x24 => {
            if shifted {
                'J'
            } else {
                'j'
            }
        }
        0x25 => {
            if shifted {
                'K'
            } else {
                'k'
            }
        }
        0x26 => {
            if shifted {
                'L'
            } else {
                'l'
            }
        }
        0x27 => {
            if shifted {
                ':'
            } else {
                ';'
            }
        }
        0x28 => {
            if shifted {
                '"'
            } else {
                '\''
            }
        }
        0x2B => {
            if shifted {
                '|'
            } else {
                '\\'
            }
        }
        0x2C => {
            if shifted {
                'Z'
            } else {
                'z'
            }
        }
        0x2D => {
            if shifted {
                'X'
            } else {
                'x'
            }
        }
        0x2E => {
            if shifted {
                'C'
            } else {
                'c'
            }
        }
        0x2F => {
            if shifted {
                'V'
            } else {
                'v'
            }
        }
        0x30 => {
            if shifted {
                'B'
            } else {
                'b'
            }
        }
        0x31 => {
            if shifted {
                'N'
            } else {
                'n'
            }
        }
        0x32 => {
            if shifted {
                'M'
            } else {
                'm'
            }
        }
        0x33 => {
            if shifted {
                '<'
            } else {
                ','
            }
        }
        0x34 => {
            if shifted {
                '>'
            } else {
                '.'
            }
        }
        0x35 => {
            if shifted {
                '?'
            } else {
                '/'
            }
        }
        0x39 => ' ',
        0x1C => '\n',
        0x0F => '\t',
        _ => return None,
    };

    Some(ch)
}
