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

const MAX_COLS: usize = 160;
const SCROLLBACK_LINES: usize = 512;
const DEFAULT_FG_COLOR: u32 = 0xFFFFFF;

static mut SCROLLBACK: [[u8; MAX_COLS]; SCROLLBACK_LINES] = [[b' '; MAX_COLS]; SCROLLBACK_LINES];
static mut SCROLLBACK_COLORS: [[u32; MAX_COLS]; SCROLLBACK_LINES] =
    [[DEFAULT_FG_COLOR; MAX_COLS]; SCROLLBACK_LINES];
static mut SCROLLBACK_LENS: [usize; SCROLLBACK_LINES] = [0; SCROLLBACK_LINES];
static mut SCROLLBACK_LEN: usize = 1;
static mut ACTIVE_LINE: usize = 0;
static mut VIEWPORT_TOP: usize = 0;
static mut VIEW_AT_BOTTOM: bool = true;

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
            fg_color: DEFAULT_FG_COLOR,
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
        self.reset_scrollback();
    }

    fn max_chars(&self) -> usize {
        (self.fb.width / self.char_width).min(MAX_COLS)
    }

    fn visible_rows(&self) -> usize {
        self.fb.height / self.char_height
    }

    fn reset_scrollback(&mut self) {
        unsafe {
            for row in 0..SCROLLBACK_LINES {
                SCROLLBACK_LENS[row] = 0;
                for col in 0..MAX_COLS {
                    SCROLLBACK[row][col] = b' ';
                    SCROLLBACK_COLORS[row][col] = DEFAULT_FG_COLOR;
                }
            }
            SCROLLBACK_LEN = 1;
            ACTIVE_LINE = 0;
            VIEWPORT_TOP = 0;
            VIEW_AT_BOTTOM = true;
        }
    }

    fn clear_pixels(&mut self) {
        unsafe {
            let ptr = self.fb.address;
            let color = self.bg_color;
            let count = self.fb.height * self.stride;

            for i in 0..count {
                ptr.add(i).write_volatile(color);
            }
        }
    }

    fn follow_bottom(&mut self) {
        unsafe {
            let rows = self.visible_rows();
            VIEWPORT_TOP = SCROLLBACK_LEN.saturating_sub(rows);
            VIEW_AT_BOTTOM = true;
            self.cursor_y = ACTIVE_LINE.saturating_sub(VIEWPORT_TOP);
        }
    }

    fn append_history_line(&mut self) {
        unsafe {
            if SCROLLBACK_LEN < SCROLLBACK_LINES {
                ACTIVE_LINE = SCROLLBACK_LEN;
                SCROLLBACK_LEN += 1;
            } else {
                for row in 1..SCROLLBACK_LINES {
                    SCROLLBACK[row - 1] = SCROLLBACK[row];
                    SCROLLBACK_COLORS[row - 1] = SCROLLBACK_COLORS[row];
                    SCROLLBACK_LENS[row - 1] = SCROLLBACK_LENS[row];
                }
                ACTIVE_LINE = SCROLLBACK_LINES - 1;
            }

            SCROLLBACK_LENS[ACTIVE_LINE] = 0;
            for col in 0..MAX_COLS {
                SCROLLBACK[ACTIVE_LINE][col] = b' ';
                SCROLLBACK_COLORS[ACTIVE_LINE][col] = self.fg_color;
            }
        }
    }

    fn write_history_char(&mut self, ch: u8) {
        unsafe {
            let max_chars = self.max_chars();
            if self.cursor_x >= max_chars {
                self.cursor_x = 0;
                self.append_history_line();
                self.follow_bottom();
                self.render_viewport();
            }

            let line = ACTIVE_LINE;
            let col = self.cursor_x;
            if line < SCROLLBACK_LINES && col < MAX_COLS {
                SCROLLBACK[line][col] = ch;
                SCROLLBACK_COLORS[line][col] = self.fg_color;
                if SCROLLBACK_LENS[line] <= col {
                    SCROLLBACK_LENS[line] = col + 1;
                }
            }
        }
    }

    fn erase_history_char(&mut self) {
        unsafe {
            if self.cursor_x == 0 {
                return;
            }
            self.cursor_x -= 1;
            let line = ACTIVE_LINE;
            let col = self.cursor_x;
            if line < SCROLLBACK_LINES && col < MAX_COLS {
                SCROLLBACK[line][col] = b' ';
                SCROLLBACK_COLORS[line][col] = self.fg_color;
                while SCROLLBACK_LENS[line] > 0
                    && SCROLLBACK[line][SCROLLBACK_LENS[line] - 1] == b' '
                {
                    SCROLLBACK_LENS[line] -= 1;
                }
            }
        }
    }

    fn render_viewport(&mut self) {
        self.clear_pixels();
        unsafe {
            let rows = self.visible_rows();
            let max_chars = self.max_chars();
            for screen_row in 0..rows {
                let history_row = VIEWPORT_TOP + screen_row;
                if history_row >= SCROLLBACK_LEN {
                    break;
                }
                let len = SCROLLBACK_LENS[history_row].min(max_chars);
                for col in 0..len {
                    self.draw_glyph_color(
                        col,
                        screen_row,
                        SCROLLBACK[history_row][col],
                        SCROLLBACK_COLORS[history_row][col],
                    );
                }
            }

            if ACTIVE_LINE >= VIEWPORT_TOP && ACTIVE_LINE < VIEWPORT_TOP + rows {
                self.cursor_y = ACTIVE_LINE - VIEWPORT_TOP;
            } else {
                self.cursor_y = rows.saturating_sub(1);
            }
        }
    }

    pub fn scroll_view(&mut self, lines: i32) {
        unsafe {
            let rows = self.visible_rows();
            let max_top = SCROLLBACK_LEN.saturating_sub(rows);
            if lines < 0 {
                VIEWPORT_TOP = VIEWPORT_TOP.saturating_sub((-lines) as usize);
            } else {
                VIEWPORT_TOP = (VIEWPORT_TOP + lines as usize).min(max_top);
            }
            VIEW_AT_BOTTOM = VIEWPORT_TOP == max_top;
            self.render_viewport();
            self.draw_cursor(VIEW_AT_BOTTOM);
        }
    }

    pub fn clear_screen(&mut self) {
        self.clear_pixels();
        self.reset_scrollback();
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

    pub fn set_fg_color(&mut self, color: u32) {
        self.fg_color = color;
    }

    pub fn reset_fg_color(&mut self) {
        self.fg_color = DEFAULT_FG_COLOR;
    }

    pub fn draw_char(&mut self, c: char) {
        if c == '\n' {
            self.cursor_x = 0;
            self.append_history_line();
            self.follow_bottom();
            self.render_viewport();
            return;
        }

        if c == '\r' {
            self.cursor_x = 0;
            return;
        }

        unsafe {
            if !VIEW_AT_BOTTOM {
                self.follow_bottom();
                self.render_viewport();
            }
        }

        if c == '\x08' {
            if self.cursor_x > 0 {
                self.erase_history_char();
                self.draw_glyph(self.cursor_x, self.cursor_y, b' ');
            }
            return;
        }

        let max_chars = self.max_chars();
        if self.cursor_x >= max_chars {
            self.cursor_x = 0;
            self.append_history_line();
            self.follow_bottom();
            self.render_viewport();
        }

        self.write_history_char(c as u8);
        unsafe {
            if VIEW_AT_BOTTOM {
                self.draw_glyph(self.cursor_x, self.cursor_y, c as u8);
            }
        }
        self.cursor_x += 1;
    }

    fn draw_glyph(&mut self, char_x: usize, char_y: usize, ch: u8) {
        self.draw_glyph_color(char_x, char_y, ch, self.fg_color);
    }

    fn draw_glyph_color(&mut self, char_x: usize, char_y: usize, ch: u8, fg_color: u32) {
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
                    let color = if bit == 1 { fg_color } else { self.bg_color };
                    *self.fb.address.add(offset + col) = color;
                }
            }
        }
    }

    pub fn write_str(&mut self, s: &str) {
        serial_write_text(s);
        for c in s.chars() {
            self.draw_char(c);
        }
    }

    pub fn write_display_str(&mut self, s: &str) {
        for c in s.chars() {
            self.draw_char(c);
        }
    }

    pub fn draw_cursor(&mut self, visible: bool) {
        let px_x = self.cursor_x * self.char_width;
        let px_y = self.cursor_y * self.char_height;
        let color = if visible {
            self.fg_color
        } else {
            self.bg_color
        };

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
