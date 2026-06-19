#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
}

impl Rect {
    pub const fn new(x: usize, y: usize, width: usize, height: usize) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn right(self) -> usize {
        self.x.saturating_add(self.width)
    }

    pub fn bottom(self) -> usize {
        self.y.saturating_add(self.height)
    }

    pub fn clipped(self, width: usize, height: usize) -> Option<Self> {
        let x0 = self.x.min(width);
        let y0 = self.y.min(height);
        let x1 = self.right().min(width);
        let y1 = self.bottom().min(height);
        if x1 <= x0 || y1 <= y0 {
            return None;
        }
        Some(Self::new(x0, y0, x1 - x0, y1 - y0))
    }

    pub fn union(self, other: Self) -> Self {
        let x0 = self.x.min(other.x);
        let y0 = self.y.min(other.y);
        let x1 = self.right().max(other.right());
        let y1 = self.bottom().max(other.bottom());
        Self::new(x0, y0, x1.saturating_sub(x0), y1.saturating_sub(y0))
    }
}

#[derive(Clone, Copy)]
pub struct Framebuffer {
    addr: *mut u32,
    width: usize,
    height: usize,
    pitch_pixels: usize,
}

impl Framebuffer {
    pub const fn new(addr: *mut u32, width: usize, height: usize, pitch_bytes: usize) -> Self {
        Self {
            addr,
            width,
            height,
            pitch_pixels: pitch_bytes / 4,
        }
    }

    pub const fn width(self) -> usize {
        self.width
    }

    pub const fn height(self) -> usize {
        self.height
    }

    pub fn put_pixel(self, x: usize, y: usize, color: u32) {
        if x < self.width && y < self.height {
            unsafe {
                core::ptr::write_volatile(self.addr.add(y * self.pitch_pixels + x), color);
            }
        }
    }

    #[inline(always)]
    pub unsafe fn put_pixel_unchecked(self, x: usize, y: usize, color: u32) {
        core::ptr::write_volatile(self.addr.add(y * self.pitch_pixels + x), color);
    }

    pub fn read_pixel(self, x: usize, y: usize) -> u32 {
        if x < self.width && y < self.height {
            unsafe { core::ptr::read_volatile(self.addr.add(y * self.pitch_pixels + x)) }
        } else {
            0
        }
    }

    pub fn fill_rect(self, rect: Rect, color: u32) {
        let Some(rect) = rect.clipped(self.width, self.height) else {
            return;
        };
        for y in rect.y..rect.bottom() {
            let row = unsafe { self.addr.add(y * self.pitch_pixels + rect.x) };
            for x in rect.x..rect.right() {
                unsafe {
                    core::ptr::write_volatile(row.add(x - rect.x), color);
                }
            }
        }
    }

    pub fn stroke_rect(self, rect: Rect, color: u32) {
        if rect.width == 0 || rect.height == 0 {
            return;
        }
        self.fill_rect(Rect::new(rect.x, rect.y, rect.width, 1), color);
        self.fill_rect(
            Rect::new(rect.x, rect.bottom().saturating_sub(1), rect.width, 1),
            color,
        );
        self.fill_rect(Rect::new(rect.x, rect.y, 1, rect.height), color);
        self.fill_rect(
            Rect::new(rect.right().saturating_sub(1), rect.y, 1, rect.height),
            color,
        );
    }

    pub fn draw_text(self, x: usize, y: usize, text: &str, color: u32) {
        for (index, byte) in text.bytes().enumerate() {
            self.draw_glyph(x + index * 6, y, byte, color);
        }
    }

    fn draw_glyph(self, x: usize, y: usize, ch: u8, color: u32) {
        let glyph = glyph_5x8(ch);
        for dx in 0..5 {
            let col = glyph[dx];
            for dy in 0..8 {
                if ((col >> dy) & 1) != 0 {
                    self.put_pixel(x + dx, y + dy, color);
                }
            }
        }
    }
}

const MAX_BACKBUFFER_WIDTH: usize = 1920;
const MAX_BACKBUFFER_HEIGHT: usize = 1080;
const MAX_BACKBUFFER_PIXELS: usize = MAX_BACKBUFFER_WIDTH * MAX_BACKBUFFER_HEIGHT;

static mut GUI_BACK_BUFFER: [u32; MAX_BACKBUFFER_PIXELS] = [0; MAX_BACKBUFFER_PIXELS];

pub struct BackBuffer {
    canvas: Framebuffer,
}

impl BackBuffer {
    pub fn init(width: usize, height: usize) -> Option<Self> {
        if width == 0
            || height == 0
            || width > MAX_BACKBUFFER_WIDTH
            || height > MAX_BACKBUFFER_HEIGHT
        {
            return None;
        }

        Some(Self {
            canvas: Framebuffer::new(
                core::ptr::addr_of_mut!(GUI_BACK_BUFFER).cast::<u32>(),
                width,
                height,
                width * 4,
            ),
        })
    }

    pub fn canvas(&self) -> Framebuffer {
        self.canvas
    }

    pub fn present_full(&self, front: Framebuffer) {
        self.present_rect(
            front,
            Rect::new(0, 0, self.canvas.width(), self.canvas.height()),
        );
    }

    pub fn present_rect(&self, front: Framebuffer, rect: Rect) {
        let Some(rect) = rect.clipped(self.canvas.width(), self.canvas.height()) else {
            return;
        };

        for y in rect.y..rect.bottom() {
            let src = unsafe { self.canvas.addr.add(y * self.canvas.pitch_pixels + rect.x) };
            let dst = unsafe { front.addr.add(y * front.pitch_pixels + rect.x) };
            for offset in 0..rect.width {
                unsafe {
                    core::ptr::write_volatile(
                        dst.add(offset),
                        core::ptr::read_volatile(src.add(offset)),
                    );
                }
            }
        }
    }
}

const MAX_DAMAGE_RECTS: usize = 16;

pub struct DamageTracker {
    rects: [Rect; MAX_DAMAGE_RECTS],
    len: usize,
    overflow: bool,
}

impl DamageTracker {
    pub const fn new() -> Self {
        Self {
            rects: [Rect::new(0, 0, 0, 0); MAX_DAMAGE_RECTS],
            len: 0,
            overflow: false,
        }
    }

    pub fn mark(&mut self, rect: Rect) {
        if rect.width == 0 || rect.height == 0 {
            return;
        }

        if self.len < MAX_DAMAGE_RECTS {
            self.rects[self.len] = rect;
            self.len += 1;
        } else {
            self.overflow = true;
            self.rects[0] = self.rects[0].union(rect);
            self.len = 1;
        }
    }

    pub fn clear(&mut self) {
        self.len = 0;
        self.overflow = false;
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn rects(&self) -> &[Rect] {
        &self.rects[..self.len]
    }

    pub fn overflowed(&self) -> bool {
        self.overflow
    }
}

pub fn glyph_5x8(ch: u8) -> [u8; 5] {
    match ch {
        b'A' => [0x7C, 0x12, 0x11, 0x12, 0x7C],
        b'B' => [0x7F, 0x49, 0x49, 0x49, 0x36],
        b'C' => [0x3E, 0x41, 0x41, 0x41, 0x22],
        b'D' => [0x7F, 0x41, 0x41, 0x22, 0x1C],
        b'E' => [0x7F, 0x49, 0x49, 0x49, 0x41],
        b'F' => [0x7F, 0x09, 0x09, 0x09, 0x01],
        b'G' => [0x3E, 0x41, 0x49, 0x49, 0x7A],
        b'H' => [0x7F, 0x08, 0x08, 0x08, 0x7F],
        b'I' => [0x00, 0x41, 0x7F, 0x41, 0x00],
        b'J' => [0x20, 0x40, 0x41, 0x3F, 0x01],
        b'K' => [0x7F, 0x08, 0x14, 0x22, 0x41],
        b'L' => [0x7F, 0x40, 0x40, 0x40, 0x40],
        b'M' => [0x7F, 0x02, 0x0C, 0x02, 0x7F],
        b'N' => [0x7F, 0x04, 0x08, 0x10, 0x7F],
        b'O' => [0x3E, 0x41, 0x41, 0x41, 0x3E],
        b'P' => [0x7F, 0x09, 0x09, 0x09, 0x06],
        b'R' => [0x7F, 0x09, 0x19, 0x29, 0x46],
        b'S' => [0x46, 0x49, 0x49, 0x49, 0x31],
        b'T' => [0x01, 0x01, 0x7F, 0x01, 0x01],
        b'U' => [0x3F, 0x40, 0x40, 0x40, 0x3F],
        b'V' => [0x1F, 0x20, 0x40, 0x20, 0x1F],
        b'W' => [0x3F, 0x40, 0x38, 0x40, 0x3F],
        b'X' => [0x63, 0x14, 0x08, 0x14, 0x63],
        b'Y' => [0x07, 0x08, 0x70, 0x08, 0x07],
        b'Z' => [0x61, 0x51, 0x49, 0x45, 0x43],
        b'a' => [0x20, 0x54, 0x54, 0x54, 0x78],
        b'b' => [0x7F, 0x48, 0x44, 0x44, 0x38],
        b'c' => [0x38, 0x44, 0x44, 0x44, 0x20],
        b'd' => [0x38, 0x44, 0x44, 0x48, 0x7F],
        b'e' => [0x38, 0x54, 0x54, 0x54, 0x18],
        b'f' => [0x08, 0x7E, 0x09, 0x01, 0x02],
        b'g' => [0x0C, 0x52, 0x52, 0x52, 0x3E],
        b'h' => [0x7F, 0x08, 0x04, 0x04, 0x78],
        b'i' => [0x00, 0x44, 0x7D, 0x40, 0x00],
        b'j' => [0x20, 0x40, 0x44, 0x3D, 0x00],
        b'k' => [0x7F, 0x10, 0x28, 0x44, 0x00],
        b'l' => [0x00, 0x41, 0x7F, 0x40, 0x00],
        b'm' => [0x7C, 0x04, 0x18, 0x04, 0x78],
        b'n' => [0x7C, 0x08, 0x04, 0x04, 0x78],
        b'o' => [0x38, 0x44, 0x44, 0x44, 0x38],
        b'p' => [0x7C, 0x14, 0x14, 0x14, 0x08],
        b'q' => [0x08, 0x14, 0x14, 0x18, 0x7C],
        b'r' => [0x7C, 0x08, 0x04, 0x04, 0x08],
        b's' => [0x48, 0x54, 0x54, 0x54, 0x20],
        b't' => [0x04, 0x3F, 0x44, 0x40, 0x20],
        b'u' => [0x3C, 0x40, 0x40, 0x20, 0x7C],
        b'v' => [0x1C, 0x20, 0x40, 0x20, 0x1C],
        b'w' => [0x3C, 0x40, 0x30, 0x40, 0x3C],
        b'x' => [0x44, 0x28, 0x10, 0x28, 0x44],
        b'y' => [0x0C, 0x50, 0x50, 0x50, 0x3C],
        b'z' => [0x44, 0x64, 0x54, 0x4C, 0x44],
        b'0' => [0x3E, 0x51, 0x49, 0x45, 0x3E],
        b'1' => [0x00, 0x42, 0x7F, 0x40, 0x00],
        b'2' => [0x42, 0x61, 0x51, 0x49, 0x46],
        b'3' => [0x21, 0x41, 0x45, 0x4B, 0x31],
        b'4' => [0x18, 0x14, 0x12, 0x7F, 0x10],
        b'5' => [0x27, 0x45, 0x45, 0x45, 0x39],
        b'6' => [0x3C, 0x4A, 0x49, 0x49, 0x30],
        b'7' => [0x01, 0x71, 0x09, 0x05, 0x03],
        b'8' => [0x36, 0x49, 0x49, 0x49, 0x36],
        b'9' => [0x06, 0x49, 0x49, 0x29, 0x1E],
        b' ' => [0x00, 0x00, 0x00, 0x00, 0x00],
        b'.' => [0x00, 0x60, 0x60, 0x00, 0x00],
        b':' => [0x00, 0x36, 0x36, 0x00, 0x00],
        b'-' => [0x08, 0x08, 0x08, 0x08, 0x08],
        b'/' => [0x20, 0x10, 0x08, 0x04, 0x02],
        b'%' => [0x62, 0x64, 0x08, 0x13, 0x23],
        b'_' => [0x40, 0x40, 0x40, 0x40, 0x40],
        b'>' => [0x41, 0x22, 0x14, 0x08, 0x00],
        _ => [0x00, 0x00, 0x00, 0x00, 0x00],
    }
}
