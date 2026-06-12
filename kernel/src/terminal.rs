use alloc::boxed::Box;
use core::mem::MaybeUninit;

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
    stride: usize,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct CursorInfo {
    pub x: u32,
    pub y: u32,
    pub char_width: u32,
    pub char_height: u32,
}

impl FbConsole {
    pub fn new(fb_addr: *mut u32, width: usize, height: usize, pitch: usize) -> Self {
        unsafe {
            serial_write(b"[FBCON-001] FbConsole::new() entered\r\n\0".as_ptr());
            screen_log_c(b"[FBCON-001] FbConsole::new() entered\0".as_ptr(), false);
            
            serial_write(b"[FBCON-002] Calculating stride\r\n\0".as_ptr());
            screen_log_c(b"[FBCON-002] Calculating stride\0".as_ptr(), false);
        }
        let stride = pitch / 4;
        unsafe {
            serial_write(b"[FBCON-003] Creating Framebuffer struct\r\n\0".as_ptr());
            screen_log_c(b"[FBCON-003] Creating Framebuffer struct\0".as_ptr(), false);
        }
        
        let fb = Framebuffer {
            address: fb_addr,
            width,
            height,
            pitch,
        };
        unsafe {
            serial_write(b"[FBCON-004] Creating FbConsole struct\r\n\0".as_ptr());
            screen_log_c(b"[FBCON-004] Creating FbConsole struct\0".as_ptr(), false);
        }
        
        let console = Self {
            fb,
            cursor_x: 0,
            cursor_y: 0,
            char_width: 8,
            char_height: 16,
            fg_color: 0xFFFFFF,
            bg_color: 0x000000,
            stride,
        };
        unsafe {
            serial_write(b"[FBCON-005] FbConsole::new() returning\r\n\0".as_ptr());
            screen_log_c(b"[FBCON-005] FbConsole::new() returning\0".as_ptr(), false);
        }
        
        console
    }

    pub fn clear(&mut self) {
        self.cursor_x = 0;
        self.cursor_y = 0;
    }
    
    fn clear_line(&mut self, line_y: usize) {
        let px_y = line_y * self.char_height;
        unsafe {
            for row in 0..self.char_height {
                if px_y + row >= self.fb.height {
                    break;
                }
                for x in 0..self.fb.width {
                    let offset = (px_y + row) * self.stride + x;
                    self.fb.address.add(offset).write_volatile(self.bg_color);
                }
            }
        }
    }
    
    pub fn clear_screen(&mut self) {
        unsafe {
            let ptr = self.fb.address;
            let color = self.bg_color;
            let lines_to_clear = 50;
            let count = lines_to_clear * self.stride;
            
            for i in 0..count {
                ptr.add(i).write_volatile(color);
            }
        }
        self.cursor_x = 0;
        self.cursor_y = 0;
    }
    
    pub fn clear_top_area(&mut self, lines: usize) {
        unsafe {
            let ptr = self.fb.address;
            let color = self.bg_color;
            let count = lines * self.char_height * self.stride;
            
            for i in 0..count {
                ptr.add(i).write_volatile(color);
            }
        }
    }

    pub fn cursor_info(&self) -> CursorInfo {
        CursorInfo {
            x: (self.cursor_x * self.char_width) as u32,
            y: (self.cursor_y * self.char_height) as u32,
            char_width: self.char_width as u32,
            char_height: self.char_height as u32,
        }
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
                let offset = (px_y + row) * self.stride + px_x;
                
                for col in 0..self.char_width {
                    if px_x + col >= self.fb.width {
                        break;
                    }
                    let bit = (glyph_row >> (7 - col)) & 1;
                    let color = if bit == 1 { self.fg_color } else { self.bg_color };
                    *self.fb.address.add(offset + col) = color;
                }
            }
        }
    }

    fn scroll(&mut self) {
        let line_height = self.char_height;
        unsafe {
            for y in line_height..self.fb.height {
                for x in 0..self.fb.width {
                    let src_offset = y * self.stride + x;
                    let dst_offset = (y - line_height) * self.stride + x;
                    let pixel = self.fb.address.add(src_offset).read_volatile();
                    self.fb.address.add(dst_offset).write_volatile(pixel);
                }
            }
            for y in (self.fb.height - line_height)..self.fb.height {
                for x in 0..self.fb.width {
                    let offset = y * self.stride + x;
                    self.fb.address.add(offset).write_volatile(self.bg_color);
                }
            }
        }
        self.cursor_y -= 1;
    }

    pub fn write_str(&mut self, s: &str) {
        serial_write_text(s);
        for c in s.chars() {
            self.draw_char(c);
        }
    }
    
    pub fn draw_cursor(&mut self, visible: bool) {
        let px_x = self.cursor_x * self.char_width;
        let px_y = self.cursor_y * self.char_height;
        let color = if visible { self.fg_color } else { self.bg_color };
        
        unsafe {
            for row in (self.char_height - 2)..self.char_height {
                if px_y + row >= self.fb.height {
                    break;
                }
                for col in 0..self.char_width {
                    if px_x + col >= self.fb.width {
                        break;
                    }
                    let offset = (px_y + row) * self.stride + (px_x + col);
                    *self.fb.address.add(offset) = color;
                }
            }
        }
    }
}

fn get_font_glyph(ch: u8) -> [u8; 8] {
    match ch {
        b'A' => [0x18, 0x3C, 0x66, 0x66, 0x7E, 0x66, 0x66, 0x00],
        b'B' => [0x7C, 0x66, 0x66, 0x7C, 0x66, 0x66, 0x7C, 0x00],
        b'C' => [0x3C, 0x66, 0x60, 0x60, 0x60, 0x66, 0x3C, 0x00],
        b'D' => [0x78, 0x6C, 0x66, 0x66, 0x66, 0x6C, 0x78, 0x00],
        b'E' => [0x7E, 0x60, 0x60, 0x7C, 0x60, 0x60, 0x7E, 0x00],
        b'F' => [0x7E, 0x60, 0x60, 0x7C, 0x60, 0x60, 0x60, 0x00],
        b'G' => [0x3C, 0x66, 0x60, 0x6E, 0x66, 0x66, 0x3C, 0x00],
        b'H' => [0x66, 0x66, 0x66, 0x7E, 0x66, 0x66, 0x66, 0x00],
        b'I' => [0x3C, 0x18, 0x18, 0x18, 0x18, 0x18, 0x3C, 0x00],
        b'J' => [0x1E, 0x0C, 0x0C, 0x0C, 0x0C, 0x6C, 0x38, 0x00],
        b'K' => [0x66, 0x6C, 0x78, 0x70, 0x78, 0x6C, 0x66, 0x00],
        b'L' => [0x60, 0x60, 0x60, 0x60, 0x60, 0x60, 0x7E, 0x00],
        b'M' => [0x63, 0x77, 0x7F, 0x6B, 0x63, 0x63, 0x63, 0x00],
        b'N' => [0x66, 0x76, 0x7E, 0x7E, 0x6E, 0x66, 0x66, 0x00],
        b'O' => [0x3C, 0x66, 0x66, 0x66, 0x66, 0x66, 0x3C, 0x00],
        b'P' => [0x7C, 0x66, 0x66, 0x7C, 0x60, 0x60, 0x60, 0x00],
        b'Q' => [0x3C, 0x66, 0x66, 0x66, 0x66, 0x3C, 0x0E, 0x00],
        b'R' => [0x7C, 0x66, 0x66, 0x7C, 0x78, 0x6C, 0x66, 0x00],
        b'S' => [0x3C, 0x66, 0x60, 0x3C, 0x06, 0x66, 0x3C, 0x00],
        b'T' => [0x7E, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x00],
        b'U' => [0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x3C, 0x00],
        b'V' => [0x66, 0x66, 0x66, 0x66, 0x66, 0x3C, 0x18, 0x00],
        b'W' => [0x63, 0x63, 0x63, 0x6B, 0x7F, 0x77, 0x63, 0x00],
        b'X' => [0x66, 0x66, 0x3C, 0x18, 0x3C, 0x66, 0x66, 0x00],
        b'Y' => [0x66, 0x66, 0x66, 0x3C, 0x18, 0x18, 0x18, 0x00],
        b'Z' => [0x7E, 0x06, 0x0C, 0x18, 0x30, 0x60, 0x7E, 0x00],
        b'a' => [0x00, 0x00, 0x3C, 0x06, 0x3E, 0x66, 0x3E, 0x00],
        b'b' => [0x60, 0x60, 0x7C, 0x66, 0x66, 0x66, 0x7C, 0x00],
        b'c' => [0x00, 0x00, 0x3C, 0x60, 0x60, 0x60, 0x3C, 0x00],
        b'd' => [0x06, 0x06, 0x3E, 0x66, 0x66, 0x66, 0x3E, 0x00],
        b'e' => [0x00, 0x00, 0x3C, 0x66, 0x7E, 0x60, 0x3C, 0x00],
        b'f' => [0x0E, 0x18, 0x18, 0x3E, 0x18, 0x18, 0x18, 0x00],
        b'g' => [0x00, 0x00, 0x3E, 0x66, 0x66, 0x3E, 0x06, 0x7C],
        b'h' => [0x60, 0x60, 0x7C, 0x66, 0x66, 0x66, 0x66, 0x00],
        b'i' => [0x18, 0x00, 0x38, 0x18, 0x18, 0x18, 0x3C, 0x00],
        b'j' => [0x06, 0x00, 0x06, 0x06, 0x06, 0x06, 0x66, 0x3C],
        b'k' => [0x60, 0x60, 0x6C, 0x78, 0x70, 0x78, 0x6C, 0x00],
        b'l' => [0x38, 0x18, 0x18, 0x18, 0x18, 0x18, 0x3C, 0x00],
        b'm' => [0x00, 0x00, 0x66, 0x7F, 0x7F, 0x6B, 0x63, 0x00],
        b'n' => [0x00, 0x00, 0x7C, 0x66, 0x66, 0x66, 0x66, 0x00],
        b'o' => [0x00, 0x00, 0x3C, 0x66, 0x66, 0x66, 0x3C, 0x00],
        b'p' => [0x00, 0x00, 0x7C, 0x66, 0x66, 0x7C, 0x60, 0x60],
        b'q' => [0x00, 0x00, 0x3E, 0x66, 0x66, 0x3E, 0x06, 0x06],
        b'r' => [0x00, 0x00, 0x7C, 0x66, 0x60, 0x60, 0x60, 0x00],
        b's' => [0x00, 0x00, 0x3E, 0x60, 0x3C, 0x06, 0x7C, 0x00],
        b't' => [0x18, 0x18, 0x7E, 0x18, 0x18, 0x18, 0x0E, 0x00],
        b'u' => [0x00, 0x00, 0x66, 0x66, 0x66, 0x66, 0x3E, 0x00],
        b'v' => [0x00, 0x00, 0x66, 0x66, 0x66, 0x3C, 0x18, 0x00],
        b'w' => [0x00, 0x00, 0x63, 0x6B, 0x7F, 0x3E, 0x36, 0x00],
        b'x' => [0x00, 0x00, 0x66, 0x3C, 0x18, 0x3C, 0x66, 0x00],
        b'y' => [0x00, 0x00, 0x66, 0x66, 0x66, 0x3E, 0x0C, 0x78],
        b'z' => [0x00, 0x00, 0x7E, 0x0C, 0x18, 0x30, 0x7E, 0x00],
        b'0' => [0x3C, 0x66, 0x6E, 0x76, 0x66, 0x66, 0x3C, 0x00],
        b'1' => [0x18, 0x38, 0x18, 0x18, 0x18, 0x18, 0x7E, 0x00],
        b'2' => [0x3C, 0x66, 0x06, 0x0C, 0x30, 0x60, 0x7E, 0x00],
        b'3' => [0x3C, 0x66, 0x06, 0x1C, 0x06, 0x66, 0x3C, 0x00],
        b'4' => [0x0C, 0x1C, 0x3C, 0x6C, 0x7E, 0x0C, 0x0C, 0x00],
        b'5' => [0x7E, 0x60, 0x7C, 0x06, 0x06, 0x66, 0x3C, 0x00],
        b'6' => [0x3C, 0x60, 0x60, 0x7C, 0x66, 0x66, 0x3C, 0x00],
        b'7' => [0x7E, 0x06, 0x0C, 0x18, 0x30, 0x30, 0x30, 0x00],
        b'8' => [0x3C, 0x66, 0x66, 0x3C, 0x66, 0x66, 0x3C, 0x00],
        b'9' => [0x3C, 0x66, 0x66, 0x3E, 0x06, 0x06, 0x3C, 0x00],
        b' ' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
        b'!' => [0x18, 0x18, 0x18, 0x18, 0x00, 0x00, 0x18, 0x00],
        b'"' => [0x66, 0x66, 0x66, 0x00, 0x00, 0x00, 0x00, 0x00],
        b'#' => [0x36, 0x36, 0x7F, 0x36, 0x7F, 0x36, 0x36, 0x00],
        b'$' => [0x18, 0x3E, 0x60, 0x3C, 0x06, 0x7C, 0x18, 0x00],
        b'%' => [0x62, 0x66, 0x0C, 0x18, 0x30, 0x66, 0x46, 0x00],
        b'&' => [0x3C, 0x66, 0x3C, 0x38, 0x67, 0x66, 0x3F, 0x00],
        b'\'' => [0x18, 0x18, 0x18, 0x00, 0x00, 0x00, 0x00, 0x00],
        b'(' => [0x0C, 0x18, 0x30, 0x30, 0x30, 0x18, 0x0C, 0x00],
        b')' => [0x30, 0x18, 0x0C, 0x0C, 0x0C, 0x18, 0x30, 0x00],
        b'*' => [0x00, 0x66, 0x3C, 0xFF, 0x3C, 0x66, 0x00, 0x00],
        b'+' => [0x00, 0x18, 0x18, 0x7E, 0x18, 0x18, 0x00, 0x00],
        b',' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x18, 0x18, 0x30],
        b'-' => [0x00, 0x00, 0x00, 0x7E, 0x00, 0x00, 0x00, 0x00],
        b'.' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x18, 0x18, 0x00],
        b'/' => [0x00, 0x03, 0x06, 0x0C, 0x18, 0x30, 0x60, 0x00],
        b':' => [0x00, 0x00, 0x18, 0x00, 0x00, 0x18, 0x00, 0x00],
        b';' => [0x00, 0x00, 0x18, 0x00, 0x00, 0x18, 0x18, 0x30],
        b'<' => [0x0C, 0x18, 0x30, 0x60, 0x30, 0x18, 0x0C, 0x00],
        b'=' => [0x00, 0x00, 0x7E, 0x00, 0x7E, 0x00, 0x00, 0x00],
        b'>' => [0x30, 0x18, 0x0C, 0x06, 0x0C, 0x18, 0x30, 0x00],
        b'?' => [0x3C, 0x66, 0x06, 0x0C, 0x18, 0x00, 0x18, 0x00],
        b'@' => [0x3C, 0x66, 0x6E, 0x6E, 0x60, 0x62, 0x3C, 0x00],
        b'[' => [0x3C, 0x30, 0x30, 0x30, 0x30, 0x30, 0x3C, 0x00],
        b'\\' => [0x00, 0x60, 0x30, 0x18, 0x0C, 0x06, 0x03, 0x00],
        b']' => [0x3C, 0x0C, 0x0C, 0x0C, 0x0C, 0x0C, 0x3C, 0x00],
        b'^' => [0x18, 0x3C, 0x66, 0x00, 0x00, 0x00, 0x00, 0x00],
        b'_' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFF],
        b'`' => [0x18, 0x18, 0x0C, 0x00, 0x00, 0x00, 0x00, 0x00],
        b'{' => [0x0E, 0x18, 0x18, 0x70, 0x18, 0x18, 0x0E, 0x00],
        b'|' => [0x18, 0x18, 0x18, 0x00, 0x18, 0x18, 0x18, 0x00],
        b'}' => [0x70, 0x18, 0x18, 0x0E, 0x18, 0x18, 0x70, 0x00],
        b'~' => [0x00, 0x00, 0x76, 0xDC, 0x00, 0x00, 0x00, 0x00],
        _ => [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
    }
}

static mut CONSOLE_STORAGE: core::mem::MaybeUninit<FbConsole> = core::mem::MaybeUninit::uninit();
static mut CONSOLE_INITIALIZED: bool = false;

extern "C" {
    fn serial_write(s: *const u8);
    fn screen_log_c(text: *const u8, is_error: bool);
}

fn serial_write_byte(byte: u8) {
    unsafe {
        loop {
            let mut status: u8;
            core::arch::asm!(
                "in al, dx",
                out("al") status,
                in("dx") 0x3FDu16,
                options(nomem, nostack)
            );
            if (status & 0x20) != 0 {
                break;
            }
        }
        core::arch::asm!(
            "out dx, al",
            in("dx") 0x3F8u16,
            in("al") byte,
            options(nomem, nostack)
        );
    }
}

fn serial_write_text(text: &str) {
    for byte in text.bytes() {
        if byte == b'\n' {
            serial_write_byte(b'\r');
        }
        serial_write_byte(byte);
    }
}

pub fn init(fb_addr: *mut u32, width: usize, height: usize, pitch: usize) {
    unsafe {
        serial_write(b"[TERM-INIT-001] terminal::init() called\r\n\0".as_ptr());
        screen_log_c(b"[TERM-INIT-001] terminal::init() called\0".as_ptr(), false);
        
        serial_write(b"[TERM-INIT-002] Getting pointer to storage\r\n\0".as_ptr());
        screen_log_c(b"[TERM-INIT-002] Getting storage pointer\0".as_ptr(), false);
        
        let ptr = CONSOLE_STORAGE.as_mut_ptr();
        
        serial_write(b"[TERM-INIT-003] Writing fields directly\r\n\0".as_ptr());
        screen_log_c(b"[TERM-INIT-003] Writing fields\0".as_ptr(), false);
        
        let stride = pitch / 4;
        
        core::ptr::write(&mut (*ptr).fb.address, fb_addr);
        serial_write(b"[TERM-INIT-004] address written\r\n\0".as_ptr());
        
        core::ptr::write(&mut (*ptr).fb.width, width);
        serial_write(b"[TERM-INIT-005] width written\r\n\0".as_ptr());
        
        core::ptr::write(&mut (*ptr).fb.height, height);
        serial_write(b"[TERM-INIT-006] height written\r\n\0".as_ptr());
        
        core::ptr::write(&mut (*ptr).fb.pitch, pitch);
        serial_write(b"[TERM-INIT-007] pitch written\r\n\0".as_ptr());
        screen_log_c(b"[TERM-INIT-007] pitch written\0".as_ptr(), false);
        
        serial_write(b"[TERM-INIT-008] Writing cursor_x\r\n\0".as_ptr());
        core::ptr::write(&mut (*ptr).cursor_x, 0);
        serial_write(b"[TERM-INIT-009] cursor_x written\r\n\0".as_ptr());
        
        core::ptr::write(&mut (*ptr).cursor_y, 0);
        serial_write(b"[TERM-INIT-010] cursor_y written\r\n\0".as_ptr());
        
        core::ptr::write(&mut (*ptr).char_width, 8);
        serial_write(b"[TERM-INIT-011] char_width written\r\n\0".as_ptr());
        
        core::ptr::write(&mut (*ptr).char_height, 16);
        serial_write(b"[TERM-INIT-012] char_height written\r\n\0".as_ptr());
        
        core::ptr::write(&mut (*ptr).fg_color, 0xFFFFFF);
        serial_write(b"[TERM-INIT-013] fg_color written\r\n\0".as_ptr());
        
        core::ptr::write(&mut (*ptr).bg_color, 0x000000);
        serial_write(b"[TERM-INIT-014] bg_color written\r\n\0".as_ptr());
        
        core::ptr::write(&mut (*ptr).stride, stride);
        serial_write(b"[TERM-INIT-015] stride written\r\n\0".as_ptr());
        
        serial_write(b"[TERM-INIT-016] All fields written\r\n\0".as_ptr());
        screen_log_c(b"[TERM-INIT-016] All fields written\0".as_ptr(), false);
        
        CONSOLE_INITIALIZED = true;
        
        serial_write(b"[TERM-INIT-017] CONSOLE_INITIALIZED = true\r\n\0".as_ptr());
        screen_log_c(b"[TERM-INIT-017] Initialization complete\0".as_ptr(), false);
    }
}

pub fn get_console() -> Option<&'static mut FbConsole> {
    unsafe { 
        if CONSOLE_INITIALIZED {
            Some(CONSOLE_STORAGE.assume_init_mut())
        } else {
            None
        }
    }
}

pub fn get_cursor_info() -> Option<CursorInfo> {
    get_console().map(|console| console.cursor_info())
}
