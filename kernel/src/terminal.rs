pub struct Framebuffer {
    pub address: *mut u32,
    pub width: usize,
    pub height: usize,
    pub pitch: usize,
}

pub struct FbConsole {
    fb: Framebuffer,
    cursor_x: usize,
    cursor_y: usize,
    char_width: usize,
    char_height: usize,
    fg_color: u32,
    bg_color: u32,
}

impl FbConsole {
    pub fn new(fb_addr: *mut u32, width: usize, height: usize, pitch: usize) -> Self {
        Self {
            fb: Framebuffer {
                address: fb_addr,
                width,
                height,
                pitch,
            },
            cursor_x: 0,
            cursor_y: 0,
            char_width: 8,
            char_height: 16,
            fg_color: 0xFFFFFF,
            bg_color: 0x002b36,
        }
    }

    pub fn clear(&mut self) {
        unsafe {
            for y in 0..self.fb.height {
                for x in 0..self.fb.width {
                    let offset = y * self.fb.width + x;
                    self.fb.address.add(offset).write_volatile(self.bg_color);
                }
            }
        }
        self.cursor_x = 0;
        self.cursor_y = 0;
    }

    pub fn draw_char(&mut self, c: char) {
        if c == '\n' {
            self.cursor_x = 0;
            self.cursor_y += 1;
            if self.cursor_y * self.char_height >= self.fb.height {
                self.scroll();
            }
            return;
        }

        if c == '\r' {
            self.cursor_x = 0;
            return;
        }

        if c == '\x08' {
            if self.cursor_x > 0 {
                self.cursor_x -= 1;
                self.draw_glyph(self.cursor_x, self.cursor_y, b' ');
            }
            return;
        }

        let max_chars = self.fb.width / self.char_width;
        if self.cursor_x >= max_chars {
            self.cursor_x = 0;
            self.cursor_y += 1;
            if self.cursor_y * self.char_height >= self.fb.height {
                self.scroll();
            }
        }

        self.draw_glyph(self.cursor_x, self.cursor_y, c as u8);
        self.cursor_x += 1;
    }

    fn draw_glyph(&mut self, char_x: usize, char_y: usize, ch: u8) {
        let glyph = get_font_glyph(ch);
        let px_x = char_x * self.char_width;
        let px_y = char_y * self.char_height;

        unsafe {
            for row in 0..self.char_height {
                if px_y + row >= self.fb.height {
                    break;
                }
                let glyph_row = if row < 8 { glyph[row] } else { 0 };
                for col in 0..self.char_width {
                    if px_x + col >= self.fb.width {
                        break;
                    }
                    let bit = (glyph_row >> (7 - col)) & 1;
                    let color = if bit == 1 { self.fg_color } else { self.bg_color };
                    let offset = (px_y + row) * self.fb.width + (px_x + col);
                    self.fb.address.add(offset).write_volatile(color);
                }
            }
        }
    }

    fn scroll(&mut self) {
        let line_height = self.char_height;
        unsafe {
            for y in line_height..self.fb.height {
                for x in 0..self.fb.width {
                    let src_offset = y * self.fb.width + x;
                    let dst_offset = (y - line_height) * self.fb.width + x;
                    let pixel = self.fb.address.add(src_offset).read_volatile();
                    self.fb.address.add(dst_offset).write_volatile(pixel);
                }
            }
            for y in (self.fb.height - line_height)..self.fb.height {
                for x in 0..self.fb.width {
                    let offset = y * self.fb.width + x;
                    self.fb.address.add(offset).write_volatile(self.bg_color);
                }
            }
        }
        self.cursor_y -= 1;
    }

    pub fn write_str(&mut self, s: &str) {
        for c in s.chars() {
            self.draw_char(c);
        }
    }
}

fn get_font_glyph(ch: u8) -> [u8; 8] {
    match ch {
        b'A' => [0x7C, 0x12, 0x11, 0x12, 0x7C, 0x00, 0x00, 0x00],
        b'B' => [0x7F, 0x49, 0x49, 0x49, 0x36, 0x00, 0x00, 0x00],
        b'C' => [0x3E, 0x41, 0x41, 0x41, 0x22, 0x00, 0x00, 0x00],
        b'D' => [0x7F, 0x41, 0x41, 0x22, 0x1C, 0x00, 0x00, 0x00],
        b'E' => [0x7F, 0x49, 0x49, 0x49, 0x41, 0x00, 0x00, 0x00],
        b'G' => [0x3E, 0x41, 0x49, 0x49, 0x7A, 0x00, 0x00, 0x00],
        b'H' => [0x7F, 0x08, 0x08, 0x08, 0x7F, 0x00, 0x00, 0x00],
        b'I' => [0x00, 0x41, 0x7F, 0x41, 0x00, 0x00, 0x00, 0x00],
        b'M' => [0x7F, 0x02, 0x0C, 0x02, 0x7F, 0x00, 0x00, 0x00],
        b'O' => [0x3E, 0x41, 0x41, 0x41, 0x3E, 0x00, 0x00, 0x00],
        b'S' => [0x46, 0x49, 0x49, 0x49, 0x31, 0x00, 0x00, 0x00],
        b'T' => [0x01, 0x01, 0x7F, 0x01, 0x01, 0x00, 0x00, 0x00],
        b'a' => [0x20, 0x54, 0x54, 0x54, 0x78, 0x00, 0x00, 0x00],
        b'c' => [0x38, 0x44, 0x44, 0x44, 0x20, 0x00, 0x00, 0x00],
        b'd' => [0x38, 0x44, 0x44, 0x48, 0x7F, 0x00, 0x00, 0x00],
        b'e' => [0x38, 0x54, 0x54, 0x54, 0x18, 0x00, 0x00, 0x00],
        b'h' => [0x7F, 0x08, 0x04, 0x04, 0x78, 0x00, 0x00, 0x00],
        b'i' => [0x00, 0x44, 0x7D, 0x40, 0x00, 0x00, 0x00, 0x00],
        b'l' => [0x00, 0x41, 0x7F, 0x40, 0x00, 0x00, 0x00, 0x00],
        b'm' => [0x7C, 0x04, 0x18, 0x04, 0x78, 0x00, 0x00, 0x00],
        b'n' => [0x7C, 0x08, 0x04, 0x04, 0x78, 0x00, 0x00, 0x00],
        b'o' => [0x38, 0x44, 0x44, 0x44, 0x38, 0x00, 0x00, 0x00],
        b'p' => [0x7C, 0x14, 0x14, 0x14, 0x08, 0x00, 0x00, 0x00],
        b'r' => [0x7C, 0x08, 0x04, 0x04, 0x08, 0x00, 0x00, 0x00],
        b's' => [0x48, 0x54, 0x54, 0x54, 0x20, 0x00, 0x00, 0x00],
        b't' => [0x04, 0x3F, 0x44, 0x40, 0x20, 0x00, 0x00, 0x00],
        b'u' => [0x3C, 0x40, 0x40, 0x20, 0x7C, 0x00, 0x00, 0x00],
        b'v' => [0x1C, 0x20, 0x40, 0x20, 0x1C, 0x00, 0x00, 0x00],
        b'w' => [0x3C, 0x40, 0x30, 0x40, 0x3C, 0x00, 0x00, 0x00],
        b'y' => [0x0C, 0x50, 0x50, 0x50, 0x3C, 0x00, 0x00, 0x00],
        b'0' => [0x3E, 0x51, 0x49, 0x45, 0x3E, 0x00, 0x00, 0x00],
        b'1' => [0x00, 0x42, 0x7F, 0x40, 0x00, 0x00, 0x00, 0x00],
        b'2' => [0x42, 0x61, 0x51, 0x49, 0x46, 0x00, 0x00, 0x00],
        b'3' => [0x21, 0x41, 0x45, 0x4B, 0x31, 0x00, 0x00, 0x00],
        b' ' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
        b'-' => [0x08, 0x08, 0x08, 0x08, 0x08, 0x00, 0x00, 0x00],
        b'=' => [0x14, 0x14, 0x14, 0x14, 0x14, 0x00, 0x00, 0x00],
        b':' => [0x00, 0x36, 0x36, 0x00, 0x00, 0x00, 0x00, 0x00],
        b'~' => [0x02, 0x01, 0x02, 0x04, 0x02, 0x00, 0x00, 0x00],
        b'#' => [0x14, 0x7F, 0x14, 0x7F, 0x14, 0x00, 0x00, 0x00],
        b'.' => [0x00, 0x60, 0x60, 0x00, 0x00, 0x00, 0x00, 0x00],
        b'/' => [0x20, 0x10, 0x08, 0x04, 0x02, 0x00, 0x00, 0x00],
        b'@' => [0x3E, 0x41, 0x5D, 0x55, 0x1E, 0x00, 0x00, 0x00],
        _ => [0x7F, 0x41, 0x41, 0x41, 0x7F, 0x00, 0x00, 0x00],
    }
}

static mut CONSOLE_INSTANCE: Option<FbConsole> = None;

pub fn init(fb_addr: *mut u32, width: usize, height: usize, pitch: usize) {
    unsafe {
        CONSOLE_INSTANCE = Some(FbConsole::new(fb_addr, width, height, pitch));
    }
}

pub fn get_console() -> Option<&'static mut FbConsole> {
    unsafe { CONSOLE_INSTANCE.as_mut() }
}
