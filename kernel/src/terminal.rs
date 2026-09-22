use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, Ordering};

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

struct ScrollbackState {
    lines: [[u8; MAX_COLS]; SCROLLBACK_LINES],
    colors: [[u32; MAX_COLS]; SCROLLBACK_LINES],
    lens: [usize; SCROLLBACK_LINES],
    len: usize,
    active_line: usize,
    viewport_top: usize,
    view_at_bottom: bool,
}
struct ScrollbackCell(UnsafeCell<ScrollbackState>);
unsafe impl Sync for ScrollbackCell {}
static SCROLLBACK: ScrollbackCell = ScrollbackCell(UnsafeCell::new(ScrollbackState {
    lines: [[b' '; MAX_COLS]; SCROLLBACK_LINES],
    colors: [[DEFAULT_FG_COLOR; MAX_COLS]; SCROLLBACK_LINES],
    lens: [0; SCROLLBACK_LINES],
    len: 1,
    active_line: 0,
    viewport_top: 0,
    view_at_bottom: true,
}));

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
        let stride = pitch / 4;
        let fb = Framebuffer {
            address: fb_addr,
            width,
            height,
            pitch,
        };

        Self {
            fb,
            cursor_x: 0,
            cursor_y: 0,
            char_width: 8,
            char_height: 16,
            fg_color: DEFAULT_FG_COLOR,
            bg_color: 0x000000,
            stride,
        }
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
            let sb = &mut *SCROLLBACK.0.get();
            for row in 0..SCROLLBACK_LINES {
                sb.lens[row] = 0;
                for col in 0..MAX_COLS {
                    sb.lines[row][col] = b' ';
                    sb.colors[row][col] = DEFAULT_FG_COLOR;
                }
            }
            sb.len = 1;
            sb.active_line = 0;
            sb.viewport_top = 0;
            sb.view_at_bottom = true;
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
            let sb = &mut *SCROLLBACK.0.get();
            let rows = self.visible_rows();
            sb.viewport_top = sb.len.saturating_sub(rows);
            sb.view_at_bottom = true;
            self.cursor_y = sb.active_line.saturating_sub(sb.viewport_top);
        }
    }

    fn append_history_line(&mut self) {
        unsafe {
            let sb = &mut *SCROLLBACK.0.get();
            if sb.len < SCROLLBACK_LINES {
                sb.active_line = sb.len;
                sb.len += 1;
            } else {
                for row in 1..SCROLLBACK_LINES {
                    sb.lines[row - 1] = sb.lines[row];
                    sb.colors[row - 1] = sb.colors[row];
                    sb.lens[row - 1] = sb.lens[row];
                }
                sb.active_line = SCROLLBACK_LINES - 1;
            }

            let al = sb.active_line;
            sb.lens[al] = 0;
            for col in 0..MAX_COLS {
                sb.lines[al][col] = b' ';
                sb.colors[al][col] = self.fg_color;
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

            let sb = &mut *SCROLLBACK.0.get();
            let line = sb.active_line;
            let col = self.cursor_x;
            if line < SCROLLBACK_LINES && col < MAX_COLS {
                sb.lines[line][col] = ch;
                sb.colors[line][col] = self.fg_color;
                if sb.lens[line] <= col {
                    sb.lens[line] = col + 1;
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
            let sb = &mut *SCROLLBACK.0.get();
            let line = sb.active_line;
            let col = self.cursor_x;
            if line < SCROLLBACK_LINES && col < MAX_COLS {
                sb.lines[line][col] = b' ';
                sb.colors[line][col] = self.fg_color;
                while sb.lens[line] > 0
                    && sb.lines[line][sb.lens[line] - 1] == b' '
                {
                    sb.lens[line] -= 1;
                }
            }
        }
    }

    fn render_viewport(&mut self) {
        self.clear_pixels();
        unsafe {
            let sb = &*SCROLLBACK.0.get();
            let rows = self.visible_rows();
            let max_chars = self.max_chars();
            for screen_row in 0..rows {
                let history_row = sb.viewport_top + screen_row;
                if history_row >= sb.len {
                    break;
                }
                let len = sb.lens[history_row].min(max_chars);
                for col in 0..len {
                    self.draw_glyph_color(
                        col,
                        screen_row,
                        sb.lines[history_row][col],
                        sb.colors[history_row][col],
                    );
                }
            }

            if sb.active_line >= sb.viewport_top && sb.active_line < sb.viewport_top + rows {
                self.cursor_y = sb.active_line - sb.viewport_top;
            } else {
                self.cursor_y = rows.saturating_sub(1);
            }
        }
    }

    pub fn scroll_view(&mut self, lines: i32) {
        unsafe {
            let sb = &mut *SCROLLBACK.0.get();
            let rows = self.visible_rows();
            let max_top = sb.len.saturating_sub(rows);
            if lines < 0 {
                sb.viewport_top = sb.viewport_top.saturating_sub((-lines) as usize);
            } else {
                sb.viewport_top = (sb.viewport_top + lines as usize).min(max_top);
            }
            sb.view_at_bottom = sb.viewport_top == max_top;
            let at_bottom = sb.view_at_bottom;
            self.render_viewport();
            self.draw_cursor(at_bottom);
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
            if !(*SCROLLBACK.0.get()).view_at_bottom {
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
            if (*SCROLLBACK.0.get()).view_at_bottom {
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

/// Хранилище единственной консоли фреймбуфера. Раньше `static mut MaybeUninit`;
/// теперь `UnsafeCell`-newtype без `static mut`. Инициализируется один раз в
/// `init` при загрузке, далее выдаётся кооперативному пути как `&'static mut`.
struct ConsoleStorageCell(UnsafeCell<core::mem::MaybeUninit<FbConsole>>);
unsafe impl Sync for ConsoleStorageCell {}
static CONSOLE_STORAGE: ConsoleStorageCell =
    ConsoleStorageCell(UnsafeCell::new(core::mem::MaybeUninit::uninit()));
static CONSOLE_INITIALIZED: AtomicBool = AtomicBool::new(false);

fn serial_write_text(text: &str) {
    use crate::serial::serial_write_byte;
    for byte in text.bytes() {
        if byte == b'\n' {
            serial_write_byte(b'\r');
        }
        serial_write_byte(byte);
    }
}

pub fn init(fb_addr: *mut u32, width: usize, height: usize, pitch: usize) {
    unsafe {
        let ptr = (*CONSOLE_STORAGE.0.get()).as_mut_ptr();
        let stride = pitch / 4;

        core::ptr::write(&mut (*ptr).fb.address, fb_addr);
        core::ptr::write(&mut (*ptr).fb.width, width);
        core::ptr::write(&mut (*ptr).fb.height, height);
        core::ptr::write(&mut (*ptr).fb.pitch, pitch);
        core::ptr::write(&mut (*ptr).cursor_x, 0);
        core::ptr::write(&mut (*ptr).cursor_y, 0);
        core::ptr::write(&mut (*ptr).char_width, 8);
        core::ptr::write(&mut (*ptr).char_height, 16);
        core::ptr::write(&mut (*ptr).fg_color, 0xFFFFFF);
        core::ptr::write(&mut (*ptr).bg_color, 0x000000);
        core::ptr::write(&mut (*ptr).stride, stride);

        CONSOLE_INITIALIZED.store(true, Ordering::Relaxed);
    }
}

pub fn get_console() -> Option<&'static mut FbConsole> {
    unsafe {
        if CONSOLE_INITIALIZED.load(Ordering::Relaxed) {
            Some((*CONSOLE_STORAGE.0.get()).assume_init_mut())
        } else {
            None
        }
    }
}

pub fn get_cursor_info() -> Option<CursorInfo> {
    get_console().map(|console| console.cursor_info())
}
