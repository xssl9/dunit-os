use crate::sync::IrqSafeSpinLock;

const SCANCODE_BUFFER_LEN: usize = 64;

/// Клавиатурный ринг-буфер + флаги модификаторов. Наполняется из обработчика IRQ
/// (`push_scancode`), читается кооперативным путём (`read_scancode`,
/// `scancode_to_char`). `IrqSafeSpinLock` запрещает прерывания на время доступа,
/// поэтому IRQ клавиатуры не может вклиниться в середину чтения/записи буфера.
///
/// Кроме Shift теперь отслеживаются Ctrl/Alt/Super (Super = клавиша Windows/GUI,
/// приходит как расширенный код E0 5B/5C). `ext_pending` хранит признак того, что
/// предыдущий байт был префиксом 0xE0, чтобы отличить расширенные модификаторы от
/// базовых. Содержимое ринга (`buffer`) при этом не меняется — E0 и коды
/// модификаторов кладутся туда как раньше, поэтому `sys_get_char` (cooked ASCII)
/// остаётся полностью совместимым.
struct KeyboardState {
    buffer: [u8; SCANCODE_BUFFER_LEN],
    read: usize,
    write: usize,
    shift_down: bool,
    ctrl_down: bool,
    alt_down: bool,
    super_down: bool,
    ext_pending: bool,
}

static KEYBOARD: IrqSafeSpinLock<KeyboardState> = IrqSafeSpinLock::new(KeyboardState {
    buffer: [0; SCANCODE_BUFFER_LEN],
    read: 0,
    write: 0,
    shift_down: false,
    ctrl_down: false,
    alt_down: false,
    super_down: false,
    ext_pending: false,
});

pub fn init() {}

pub fn read_scancode() -> Option<u8> {
    let mut kb = KEYBOARD.lock();
    if kb.read != kb.write {
        let index = kb.read;
        let scancode = kb.buffer[index];
        kb.read = (index + 1) % SCANCODE_BUFFER_LEN;
        Some(scancode)
    } else {
        None
    }
}

pub fn push_scancode(scancode: u8) {
    let mut kb = KEYBOARD.lock();
    // `ext` = «предыдущий байт был префиксом 0xE0». Захватываем и сбрасываем в
    // начале: признак живёт ровно один следующий скан-код.
    let ext = kb.ext_pending;
    kb.ext_pending = false;
    match scancode {
        0xE0 => {
            kb.ext_pending = true;
        }
        0x2A | 0x36 => kb.shift_down = true,
        0xAA | 0xB6 => kb.shift_down = false,
        // Ctrl: левый 0x1D, правый E0 0x1D (обе make-версии → ctrl on).
        0x1D => kb.ctrl_down = true,
        0x9D => kb.ctrl_down = false,
        // Alt: левый 0x38, правый (AltGr) E0 0x38.
        0x38 => kb.alt_down = true,
        0xB8 => kb.alt_down = false,
        // Super/GUI приходит только расширенным: E0 5B (левый), E0 5C (правый).
        0x5B | 0x5C if ext => kb.super_down = true,
        0xDB | 0xDC if ext => kb.super_down = false,
        _ => {}
    }

    let next_write = (kb.write + 1) % SCANCODE_BUFFER_LEN;
    if next_write != kb.read {
        let index = kb.write;
        kb.buffer[index] = scancode;
        kb.write = next_write;
    }
}

/// Битовая маска модификаторов на момент вызова: bit0 Shift, bit1 Ctrl, bit2 Alt,
/// bit3 Super. Совпадает с `KEYMOD_*` в libdunit. Читается вместе со скан-кодом в
/// `sys_get_key_event`, чтобы компоситор видел горячие клавиши (Super и т.д.).
pub fn modifier_mask() -> u8 {
    let kb = KEYBOARD.lock();
    (kb.shift_down as u8)
        | ((kb.ctrl_down as u8) << 1)
        | ((kb.alt_down as u8) << 2)
        | ((kb.super_down as u8) << 3)
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
    let shifted = KEYBOARD.lock().shift_down;

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
