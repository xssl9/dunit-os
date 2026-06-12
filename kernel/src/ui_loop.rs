use crate::gui::renderer::{BackBuffer, DamageTracker, Framebuffer, Rect};
use crate::drivers::{keyboard, mouse};
use crate::serial_write;
use crate::window_manager::{self, AppType};

const BG: u32 = 0x030504;
const PANEL: u32 = 0x11161b;
const TEXT: u32 = 0xe8f0ea;
const MUTED: u32 = 0x92a29a;
const ACCENT: u32 = 0x22c55e;
const BLUE: u32 = 0x10b981;
const GREEN: u32 = 0x22c55e;
const YELLOW: u32 = 0xd6b85f;
const RED: u32 = 0xef6666;
const PURPLE: u32 = 0x8b9cf6;
const ORANGE: u32 = 0xd79d4b;
const WINDOW_BG: u32 = 0x151b20;
const WINDOW_TITLE: u32 = 0x1c242b;
const TERMINAL_BG: u32 = 0x070b0d;
const GLASS: u32 = 0x1b232a;
const GLASS_SOFT: u32 = 0x222c34;
const GLASS_EDGE: u32 = 0x3b474f;
const SHADOW: u32 = 0x020304;
const CURSOR_W: usize = 16;
const CURSOR_H: usize = 22;
const CURSOR_AREA: usize = CURSOR_W * CURSOR_H;
const WALLPAPER_BMP: &[u8] = include_bytes!("../../wallpaper.bmp");
const WALLPAPER_WIDTH: usize = 1600;
const WALLPAPER_HEIGHT: usize = 900;
const WALLPAPER_OFFSET: usize = 54;
const WALLPAPER_STRIDE: usize = WALLPAPER_WIDTH * 3;
const ICON_SIZE: usize = 44;
const TERMINAL_ICON: &[u8] = include_bytes!("../assets/terminal.rgba");
const TEXT_ICON: &[u8] = include_bytes!("../assets/text.rgba");
const MONITOR_ICON: &[u8] = include_bytes!("../assets/monitor.rgba");
const DOCK_APPS: [(AppType, u32, &'static str); 3] = [
    (AppType::Terminal, GREEN, "Term"),
    (AppType::Monitor, ORANGE, "Stats"),
    (AppType::Editor, PURPLE, "Edit"),
];

#[derive(Clone, Copy)]
struct UiState {
    launcher_open: bool,
    quick_open: bool,
    notifications_open: bool,
    brightness: u8,
    keyboard_extended: bool,
}

impl UiState {
    const fn new() -> Self {
        Self {
            launcher_open: false,
            quick_open: true,
            notifications_open: true,
            brightness: 80,
            keyboard_extended: false,
        }
    }
}

#[derive(Clone, Copy)]
enum UiAction {
    ToggleLauncher,
    ToggleQuick,
    ToggleNotifications,
    SetBrightness(u8),
    ToggleApp(AppType),
}

fn put_pixel(fb: Framebuffer, _width: usize, _height: usize, x: usize, y: usize, color: u32) {
    fb.put_pixel(x, y, color);
}

fn draw_rect(fb: Framebuffer, _width: usize, _height: usize, x: usize, y: usize, w: usize, h: usize, color: u32) {
    fb.fill_rect(Rect::new(x, y, w, h), color);
}

fn draw_rect_border(fb: Framebuffer, _width: usize, _height: usize, x: usize, y: usize, w: usize, h: usize, color: u32) {
    fb.stroke_rect(Rect::new(x, y, w, h), color);
}

fn rgb_blend(dst: u32, src: u32, alpha: u32) -> u32 {
    let inv = 255u32.saturating_sub(alpha);
    let dr = (dst >> 16) & 0xff;
    let dg = (dst >> 8) & 0xff;
    let db = dst & 0xff;
    let sr = (src >> 16) & 0xff;
    let sg = (src >> 8) & 0xff;
    let sb = src & 0xff;
    (((sr * alpha + dr * inv) / 255) << 16)
        | (((sg * alpha + dg * inv) / 255) << 8)
        | ((sb * alpha + db * inv) / 255)
}

fn rounded_contains(px: usize, py: usize, x: usize, y: usize, w: usize, h: usize, radius: usize) -> bool {
    if w == 0 || h == 0 {
        return false;
    }

    let r = radius.min(w / 2).min(h / 2);
    if r == 0 {
        return px >= x && px < x + w && py >= y && py < y + h;
    }

    let right = x + w - 1;
    let bottom = y + h - 1;
    let cx = if px < x + r {
        x + r
    } else if px > right.saturating_sub(r) {
        right.saturating_sub(r)
    } else {
        px
    };
    let cy = if py < y + r {
        y + r
    } else if py > bottom.saturating_sub(r) {
        bottom.saturating_sub(r)
    } else {
        py
    };
    let dx = px.max(cx) - px.min(cx);
    let dy = py.max(cy) - py.min(cy);
    dx * dx + dy * dy <= r * r
}

fn blurred_framebuffer_pixel(fb: Framebuffer, x: usize, y: usize, width: usize, height: usize) -> u32 {
    let mut r = 0u32;
    let mut g = 0u32;
    let mut b = 0u32;
    let mut count = 0u32;
    let offsets = [0usize, 3, 7, 11];

    for oy in offsets {
        for ox in offsets {
            let sx = x.saturating_add(ox).saturating_sub(6).min(width.saturating_sub(1));
            let sy = y.saturating_add(oy).saturating_sub(6).min(height.saturating_sub(1));
            let color = fb.read_pixel(sx, sy);
            r += (color >> 16) & 0xff;
            g += (color >> 8) & 0xff;
            b += color & 0xff;
            count += 1;
        }
    }

    if count == 0 {
        return BG;
    }

    ((r / count) << 16) | ((g / count) << 8) | (b / count)
}

fn draw_blur_round_rect(
    fb: Framebuffer,
    width: usize,
    height: usize,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    radius: usize,
    tint: u32,
    tint_alpha: u32,
) {
    let Some(rect) = Rect::new(x, y, w, h).clipped(width, height) else {
        return;
    };

    for py in rect.y..rect.bottom() {
        for px in rect.x..rect.right() {
            if rounded_contains(px, py, x, y, w, h, radius) {
                let blurred = blurred_framebuffer_pixel(fb, px, py, width, height);
                put_pixel(fb, width, height, px, py, rgb_blend(blurred, tint, tint_alpha));
            }
        }
    }
}

fn draw_rgba_icon(fb: Framebuffer, width: usize, height: usize, x: usize, y: usize, data: &[u8]) {
    for iy in 0..ICON_SIZE {
        for ix in 0..ICON_SIZE {
            let src = (iy * ICON_SIZE + ix) * 4;
            if src + 3 >= data.len() {
                return;
            }

            let alpha = data[src + 3] as u32;
            if alpha < 8 {
                continue;
            }

            let px = x + ix;
            let py = y + iy;
            if px >= width || py >= height {
                continue;
            }

            let src_color = ((data[src] as u32) << 16) | ((data[src + 1] as u32) << 8) | data[src + 2] as u32;
            let dst_color = fb.read_pixel(px, py);
            put_pixel(fb, width, height, px, py, rgb_blend(dst_color, src_color, alpha));
        }
    }
}

fn apply_brightness(fb: Framebuffer, width: usize, height: usize, state: &UiState, rect: Rect) {
    let Some(rect) = rect.clipped(width, height) else {
        return;
    };
    let brightness = state.brightness.max(25) as u32;

    for y in rect.y..rect.bottom() {
        for x in rect.x..rect.right() {
            let color = fb.read_pixel(x, y);
            let r = ((color >> 16) & 0xff) * brightness / 100;
            let g = ((color >> 8) & 0xff) * brightness / 100;
            let b = (color & 0xff) * brightness / 100;
            put_pixel(fb, width, height, x, y, (r << 16) | (g << 8) | b);
        }
    }
}

fn draw_round_rect(fb: Framebuffer, width: usize, height: usize, x: usize, y: usize, w: usize, h: usize, radius: usize, color: u32) {
    let Some(rect) = Rect::new(x, y, w, h).clipped(width, height) else {
        return;
    };

    for py in rect.y..rect.bottom() {
        for px in rect.x..rect.right() {
            if rounded_contains(px, py, x, y, w, h, radius) {
                put_pixel(fb, width, height, px, py, color);
            }
        }
    }
}

fn draw_round_rect_border(fb: Framebuffer, width: usize, height: usize, x: usize, y: usize, w: usize, h: usize, radius: usize, color: u32) {
    if w < 2 || h < 2 {
        return;
    }

    let Some(rect) = Rect::new(x, y, w, h).clipped(width, height) else {
        return;
    };

    for py in rect.y..rect.bottom() {
        for px in rect.x..rect.right() {
            let outer = rounded_contains(px, py, x, y, w, h, radius);
            let inner = rounded_contains(px, py, x + 1, y + 1, w - 2, h - 2, radius.saturating_sub(1));
            if outer && !inner {
                put_pixel(fb, width, height, px, py, color);
            }
        }
    }
}

fn glyph(ch: u8) -> [u8; 5] {
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
        _ => [0x00, 0x00, 0x00, 0x00, 0x00],
    }
}

fn draw_char(fb: Framebuffer, width: usize, height: usize, x: usize, y: usize, ch: u8, color: u32) {
    let font = glyph(ch);
    for dx in 0..5 {
        let col = font[dx];
        for dy in 0..8 {
            if (col >> dy) & 1 == 1 {
                put_pixel(fb, width, height, x + dx, y + dy, color);
            }
        }
    }
}

fn draw_text(fb: Framebuffer, width: usize, height: usize, x: usize, y: usize, text: &str, color: u32) {
    for (i, ch) in text.bytes().enumerate() {
        draw_char(fb, width, height, x + i * 6, y, ch, color);
    }
}

fn dock_layout(width: usize, height: usize) -> (usize, usize, usize, usize, usize) {
    let icon_size = 48;
    let icon_spacing = 12;
    let dock_width = DOCK_APPS.len() * icon_size + DOCK_APPS.len().saturating_sub(1) * icon_spacing + 48;
    let dock_x = width.saturating_sub(dock_width) / 2;
    let dock_y = height.saturating_sub(82);
    (dock_x, dock_y, dock_width, icon_size, icon_spacing)
}

fn wallpaper_pixel(x: usize, y: usize, width: usize, height: usize) -> u32 {
    let src_x = x.saturating_mul(WALLPAPER_WIDTH) / width.max(1);
    let src_y = y.saturating_mul(WALLPAPER_HEIGHT) / height.max(1);
    let bmp_y = WALLPAPER_HEIGHT.saturating_sub(1).saturating_sub(src_y.min(WALLPAPER_HEIGHT - 1));
    let offset = WALLPAPER_OFFSET + bmp_y * WALLPAPER_STRIDE + src_x.min(WALLPAPER_WIDTH - 1) * 3;

    if offset + 2 >= WALLPAPER_BMP.len() {
        return BG;
    }

    let b = WALLPAPER_BMP[offset] as u32;
    let g = WALLPAPER_BMP[offset + 1] as u32;
    let r = WALLPAPER_BMP[offset + 2] as u32;
    let shade = 46;
    ((r * shade / 100) << 16) | ((g * shade / 100) << 8) | (b * shade / 100)
}

fn desktop_pixel(x: usize, y: usize, width: usize, height: usize) -> u32 {
    if y < 42 {
        return PANEL;
    }

    wallpaper_pixel(x, y, width, height)
}

fn draw_icon_symbol(fb: Framebuffer, width: usize, height: usize, x: usize, y: usize, app_type: AppType) {
    let cx = x + 24;
    let cy = y + 24;
    match app_type {
        AppType::Terminal => {
            draw_text(fb, width, height, cx - 13, cy - 4, ">_", 0xffffff);
        }
        AppType::Files => {
            draw_rect(fb, width, height, cx - 13, cy - 8, 26, 17, 0xffffff);
            draw_rect(fb, width, height, cx - 13, cy - 12, 13, 5, 0xffffff);
        }
        AppType::Settings => {
            draw_rect_border(fb, width, height, cx - 10, cy - 10, 20, 20, 0xffffff);
            draw_rect(fb, width, height, cx - 2, cy - 2, 5, 5, 0xffffff);
        }
        AppType::Monitor => {
            draw_rect(fb, width, height, cx - 12, cy + 5, 5, 8, 0xffffff);
            draw_rect(fb, width, height, cx - 3, cy - 6, 5, 19, 0xffffff);
            draw_rect(fb, width, height, cx + 6, cy - 12, 5, 25, 0xffffff);
        }
        AppType::Editor => {
            draw_rect_border(fb, width, height, cx - 11, cy - 12, 22, 25, 0xffffff);
            draw_rect(fb, width, height, cx - 6, cy - 5, 12, 2, 0xffffff);
            draw_rect(fb, width, height, cx - 6, cy + 1, 12, 2, 0xffffff);
        }
    }
}

fn draw_traffic_button(fb: Framebuffer, width: usize, height: usize, x: usize, y: usize, color: u32) {
    draw_round_rect(fb, width, height, x, y, 12, 12, 6, color);
    draw_round_rect_border(fb, width, height, x, y, 12, 12, 6, 0x9aa3ad);
}

fn draw_dock(fb: Framebuffer, width: usize, height: usize) {
    let (dock_x, dock_y, dock_width, icon_size, icon_spacing) = dock_layout(width, height);
    draw_round_rect(fb, width, height, dock_x + 8, dock_y + 8, dock_width, 68, 20, SHADOW);
    draw_blur_round_rect(fb, width, height, dock_x, dock_y, dock_width, 68, 20, GLASS, 182);
    draw_blur_round_rect(fb, width, height, dock_x + 2, dock_y + 2, dock_width.saturating_sub(4), 64, 18, 0x202a31, 156);
    draw_round_rect_border(fb, width, height, dock_x, dock_y, dock_width, 68, 20, GLASS_EDGE);

    let first_icon_x = dock_x + 24;
    for i in 0..DOCK_APPS.len() {
        let icon_x = first_icon_x + i * (icon_size + icon_spacing);
        let icon_y = dock_y + 10;
        draw_round_rect(fb, width, height, icon_x + 3, icon_y + 5, icon_size, icon_size, 12, SHADOW);
        draw_round_rect(fb, width, height, icon_x, icon_y, icon_size, icon_size, 12, 0x000000);
        draw_round_rect_border(fb, width, height, icon_x, icon_y, icon_size, icon_size, 12, 0x2d353b);
        match DOCK_APPS[i].0 {
            AppType::Terminal => draw_rgba_icon(fb, width, height, icon_x + 2, icon_y + 2, TERMINAL_ICON),
            AppType::Monitor => draw_rgba_icon(fb, width, height, icon_x + 2, icon_y + 2, MONITOR_ICON),
            AppType::Editor => draw_rgba_icon(fb, width, height, icon_x + 2, icon_y + 2, TEXT_ICON),
            _ => draw_icon_symbol(fb, width, height, icon_x, icon_y, DOCK_APPS[i].0),
        }
        let active = window_manager::get_wm()
            .map(|wm| wm.app_visible(DOCK_APPS[i].0))
            .unwrap_or(false);
        draw_round_rect(fb, width, height, icon_x + 18, icon_y + icon_size + 7, 12, 3, 2, if active { GREEN } else { 0x56616a });
        draw_text(fb, width, height, icon_x + 8, icon_y + icon_size + 12, DOCK_APPS[i].2, MUTED);
    }
}

fn draw_finder_button(fb: Framebuffer, width: usize, height: usize, x: usize, y: usize, label: &str, active: bool) {
    let fill = if active { 0x173622 } else { 0x141b20 };
    let border = if active { GREEN } else { 0x2f3a42 };
    draw_round_rect(fb, width, height, x, y, 132, 34, 10, fill);
    draw_round_rect_border(fb, width, height, x, y, 132, 34, 10, border);
    draw_round_rect(fb, width, height, x + 10, y + 10, 14, 14, 7, if active { GREEN } else { MUTED });
    draw_text(fb, width, height, x + 34, y + 13, label, TEXT);
}

fn draw_window(fb: Framebuffer, width: usize, height: usize, window: &window_manager::Window, state: &UiState) {
    draw_round_rect(fb, width, height, window.x + 10, window.y + 12, window.width, window.height, 14, SHADOW);
    draw_round_rect(fb, width, height, window.x, window.y, window.width, window.height, 14, WINDOW_BG);
    draw_rect(fb, width, height, window.x, window.y, window.width, 34, WINDOW_TITLE);
    draw_rect(fb, width, height, window.x, window.y + 33, window.width, 1, 0x2f3a42);
    draw_round_rect_border(fb, width, height, window.x, window.y, window.width, window.height, 14, GLASS_EDGE);
    draw_traffic_button(fb, width, height, window.x + 12, window.y + 11, RED);
    draw_traffic_button(fb, width, height, window.x + 32, window.y + 11, YELLOW);
    draw_traffic_button(fb, width, height, window.x + 52, window.y + 11, GREEN);
    draw_text(fb, width, height, window.x + 82, window.y + 13, window.title, TEXT);

    let x = window.x + 18;
    let y = window.y + 50;
    match window.app_type {
        AppType::Terminal => {
            draw_round_rect(fb, width, height, window.x + 8, window.y + 42, window.width.saturating_sub(16), window.height.saturating_sub(50), 8, TERMINAL_BG);
            draw_text(fb, width, height, x, y, "dunit@kernel ~ % dufetch", GREEN);
            draw_text(fb, width, height, x, y + 20, "Dunit OS 2026 Green Tea", TEXT);
            draw_text(fb, width, height, x, y + 40, "VFS MemFS userspace runtime", MUTED);
        }
        AppType::Files => {
            draw_round_rect(fb, width, height, window.x + 8, window.y + 42, 92, window.height.saturating_sub(50), 8, 0x111820);
            draw_text(fb, width, height, x, y, "Favorites", MUTED);
            draw_text(fb, width, height, x, y + 24, "Widgets", GREEN);
            draw_text(fb, width, height, x + 110, y, "Finder controls", TEXT);
            draw_finder_button(fb, width, height, x + 110, y + 28, "Launcher", state.launcher_open);
            draw_finder_button(fb, width, height, x + 110, y + 72, "Quick Panel", state.quick_open);
            draw_finder_button(fb, width, height, x + 110, y + 116, "Notifications", state.notifications_open);
        }
        AppType::Settings => {
            draw_text(fb, width, height, x, y, "Mode       GUI", TEXT);
            draw_text(fb, width, height, x, y + 24, "Theme      Green Tea Dark", MUTED);
            draw_text(fb, width, height, x, y + 48, "Runtime    Single task", MUTED);
        }
        AppType::Monitor => {
            draw_text(fb, width, height, x, y, "CPU", TEXT);
            draw_round_rect(fb, width, height, x + 42, y, 180, 10, 5, 0x263039);
            draw_round_rect(fb, width, height, x + 42, y, 34, 10, 5, GREEN);
            draw_text(fb, width, height, x + 235, y, "18%", BLUE);
            draw_text(fb, width, height, x, y + 28, "RAM", TEXT);
            draw_round_rect(fb, width, height, x + 42, y + 28, 180, 10, 5, 0x263039);
            draw_round_rect(fb, width, height, x + 42, y + 28, 52, 10, 5, BLUE);
            draw_text(fb, width, height, x + 235, y + 28, "512MB", YELLOW);
        }
        AppType::Editor => {
            draw_text(fb, width, height, x, y, "notes.txt", ACCENT);
            draw_text(fb, width, height, x, y + 24, "Dunit GUI mode is alive.", TEXT);
            draw_text(fb, width, height, x, y + 44, "Cursor and dock are kernel builtins.", MUTED);
        }
    }
}

fn draw_windows(fb: Framebuffer, width: usize, height: usize, state: &UiState) {
    if let Some(wm) = window_manager::get_wm() {
        for window in wm.get_windows() {
            if window.visible {
                draw_window(fb, width, height, window, state);
            }
        }
    }
}

fn draw_desktop_widgets(fb: Framebuffer, width: usize, height: usize, state: &UiState) {
    draw_blur_round_rect(fb, width, height, 0, 0, width, 42, 0, 0x0b0f12, 210);
    draw_rect(fb, width, height, 0, 40, width, 2, 0x1f292f);
    draw_round_rect(fb, width, height, 12, 8, 62, 24, 12, 0x172017);
    draw_text(fb, width, height, 24, 16, "Dunit", GREEN);
    draw_round_rect(fb, width, height, 88, 8, 86, 24, 12, if state.launcher_open { 0x173622 } else { 0x111820 });
    draw_text(fb, width, height, 104, 16, "Launcher", if state.launcher_open { GREEN } else { MUTED });
    draw_round_rect(fb, width, height, 182, 8, 62, 24, 12, if state.quick_open { 0x173622 } else { 0x111820 });
    draw_text(fb, width, height, 198, 16, "Quick", if state.quick_open { GREEN } else { MUTED });
    draw_round_rect(fb, width, height, 252, 8, 72, 24, 12, if state.notifications_open { 0x173622 } else { 0x111820 });
    draw_text(fb, width, height, 266, 16, "Alerts", if state.notifications_open { GREEN } else { MUTED });

    draw_round_rect(fb, width, height, width.saturating_sub(172), 8, 148, 24, 12, GLASS);
    draw_text(fb, width, height, width.saturating_sub(158), 16, "Brightness", MUTED);
    draw_text(fb, width, height, width.saturating_sub(66), 16, "Live", GREEN);

    draw_text(fb, width, height, 56, 78, "Dunit 2026", TEXT);
    draw_text(fb, width, height, 56, 102, "Green Tea desktop with forest-green system accents", MUTED);
    draw_text(fb, width, height, 56, 126, "Green Tea shell with live brightness control", GREEN);

    let launcher_x = 56;
    let launcher_y = 172;
    if state.launcher_open {
        draw_round_rect(fb, width, height, launcher_x + 10, launcher_y + 12, 330, 250, 16, SHADOW);
        draw_blur_round_rect(fb, width, height, launcher_x, launcher_y, 330, 250, 16, GLASS, 184);
        draw_round_rect_border(fb, width, height, launcher_x, launcher_y, 330, 250, 16, GLASS_EDGE);
        draw_text(fb, width, height, launcher_x + 22, launcher_y + 20, "Application Launcher", TEXT);
        draw_round_rect(fb, width, height, launcher_x + 20, launcher_y + 46, 290, 30, 15, 0x0d1215);
        draw_text(fb, width, height, launcher_x + 36, launcher_y + 57, "Search apps, files, settings", MUTED);
        let app_cards = [
            ("Terminal", GREEN, AppType::Terminal),
            ("Files", BLUE, AppType::Files),
            ("Settings", GLASS_SOFT, AppType::Settings),
            ("Monitor", ORANGE, AppType::Monitor),
            ("Editor", PURPLE, AppType::Editor),
        ];
        for i in 0..app_cards.len() {
            let col = i % 2;
            let row = i / 2;
            let x = launcher_x + 20 + col * 148;
            let y = launcher_y + 92 + row * 44;
            let active = window_manager::get_wm()
                .map(|wm| wm.app_visible(app_cards[i].2))
                .unwrap_or(false);
            draw_round_rect(fb, width, height, x, y, 132, 34, 10, if active { 0x173622 } else { 0x141b20 });
            draw_round_rect_border(fb, width, height, x, y, 132, 34, 10, if active { GREEN } else { 0x25313a });
            draw_round_rect(fb, width, height, x + 10, y + 9, 16, 16, 5, app_cards[i].1);
            draw_text(fb, width, height, x + 34, y + 13, app_cards[i].0, TEXT);
        }
        draw_text(fb, width, height, launcher_x + 22, launcher_y + 224, "Applications", GREEN);
    }

    let qs_x = width.saturating_sub(322);
    let qs_y = 74;
    if state.quick_open {
        draw_round_rect(fb, width, height, qs_x + 8, qs_y + 10, 282, 154, 16, SHADOW);
        draw_blur_round_rect(fb, width, height, qs_x, qs_y, 282, 154, 16, GLASS, 188);
        draw_round_rect_border(fb, width, height, qs_x, qs_y, 282, 154, 16, GLASS_EDGE);
        draw_text(fb, width, height, qs_x + 20, qs_y + 20, "Quick Settings", TEXT);
        draw_text(fb, width, height, qs_x + 20, qs_y + 58, "Display brightness", TEXT);
        draw_round_rect(fb, width, height, qs_x + 20, qs_y + 84, 240, 12, 6, 0x2a343c);
        let fill = 240usize.saturating_mul(state.brightness as usize) / 100;
        draw_round_rect(fb, width, height, qs_x + 20, qs_y + 84, fill, 12, 6, GREEN);
        draw_text(fb, width, height, qs_x + 20, qs_y + 116, "40     55     70     85     100", MUTED);
    }

    let note_x = width.saturating_sub(322);
    let note_y = qs_y + 376;
    if state.notifications_open {
        draw_round_rect(fb, width, height, note_x + 8, note_y + 10, 282, 96, 16, SHADOW);
        draw_blur_round_rect(fb, width, height, note_x, note_y, 282, 96, 16, GLASS, 188);
        draw_round_rect_border(fb, width, height, note_x, note_y, 282, 96, 16, GLASS_EDGE);
        draw_text(fb, width, height, note_x + 20, note_y + 18, "Notifications", TEXT);
        draw_round_rect(fb, width, height, note_x + 20, note_y + 42, 242, 38, 12, 0x12191e);
        draw_text(fb, width, height, note_x + 34, note_y + 54, "Dunit shell is running", GREEN);
        draw_text(fb, width, height, note_x + 34, note_y + 68, "Back buffer and input active", MUTED);
    }
}

fn redraw_full_screen(fb: Framebuffer, width: usize, height: usize, state: &UiState) {
    for y in 0..height {
        for x in 0..width {
            put_pixel(fb, width, height, x, y, desktop_pixel(x, y, width, height));
        }
    }

    draw_desktop_widgets(fb, width, height, state);
    draw_windows(fb, width, height, state);
    draw_dock(fb, width, height);
    apply_brightness(fb, width, height, state, Rect::new(0, 0, width, height));
}

fn rect_from_bounds(bounds: (usize, usize, usize, usize)) -> Rect {
    Rect::new(bounds.0, bounds.1, bounds.2, bounds.3)
}

fn padded_rect(rect: Rect, padding: usize, width: usize, height: usize) -> Rect {
    let x = rect.x.saturating_sub(padding);
    let y = rect.y.saturating_sub(padding);
    let right = rect.right().saturating_add(padding).min(width);
    let bottom = rect.bottom().saturating_add(padding).min(height);
    Rect::new(x, y, right.saturating_sub(x), bottom.saturating_sub(y))
}

fn redraw_region(fb: Framebuffer, width: usize, height: usize, rect: Rect, state: &UiState) {
    let Some(rect) = rect.clipped(width, height) else {
        return;
    };

    for y in rect.y..rect.bottom() {
        for x in rect.x..rect.right() {
            put_pixel(fb, width, height, x, y, desktop_pixel(x, y, width, height));
        }
    }

    draw_desktop_widgets(fb, width, height, state);
    draw_windows(fb, width, height, state);
    draw_dock(fb, width, height);
    apply_brightness(fb, width, height, state, rect);
}

fn save_cursor_area(fb: Framebuffer, _width: usize, _height: usize, x: i32, y: i32, buffer: &mut [u32; CURSOR_AREA]) {
    let start_x = x.max(0) as usize;
    let start_y = y.max(0) as usize;
    for dy in 0..CURSOR_H {
        for dx in 0..CURSOR_W {
            let px = start_x + dx;
            let py = start_y + dy;
            let index = dy * CURSOR_W + dx;
            buffer[index] = fb.read_pixel(px, py);
        }
    }
}

fn restore_cursor_area(fb: Framebuffer, width: usize, height: usize, x: i32, y: i32, buffer: &[u32; CURSOR_AREA]) {
    let start_x = x.max(0) as usize;
    let start_y = y.max(0) as usize;
    for dy in 0..CURSOR_H {
        for dx in 0..CURSOR_W {
            let px = start_x + dx;
            let py = start_y + dy;
            if px < width && py < height {
                let index = dy * CURSOR_W + dx;
                put_pixel(fb, width, height, px, py, buffer[index]);
            }
        }
    }
}

fn draw_cursor(fb: Framebuffer, width: usize, height: usize, x: i32, y: i32) {
    let x = x.max(0) as usize;
    let y = y.max(0) as usize;
    for dy in 0..18 {
        for dx in 0..12 {
            let inside = dx <= dy / 2 || (dy > 10 && dx > 4 && dx < 8 && dy - dx < 10);
            let outline = dx == 0 || dx == dy / 2 || (dy > 10 && (dx == 4 || dx == 8));
            if inside {
                let color = if outline { 0x05090b } else { 0xf6ffff };
                put_pixel(fb, width, height, x + dx, y + dy, color);
            }
        }
    }
    draw_rect(fb, width, height, x + 7, y + 14, 4, 4, ACCENT);
}

fn cursor_rect(x: i32, y: i32) -> Rect {
    Rect::new(x.max(0) as usize, y.max(0) as usize, CURSOR_W, CURSOR_H)
}

fn dock_icon_rect(index: usize, width: usize, height: usize) -> Rect {
    let (dock_x, dock_y, _dock_width, icon_size, icon_spacing) = dock_layout(width, height);
    let first_icon_x = dock_x + 24;
    Rect::new(first_icon_x + index * (icon_size + icon_spacing), dock_y + 10, icon_size, icon_size)
}

fn dock_app_index(app_type: AppType) -> Option<usize> {
    for i in 0..DOCK_APPS.len() {
        if DOCK_APPS[i].0 == app_type {
            return Some(i);
        }
    }

    None
}

fn app_from_dock_index(index: usize) -> AppType {
    DOCK_APPS[index].0
}

fn inside(mx: usize, my: usize, x: usize, y: usize, w: usize, h: usize) -> bool {
    mx >= x && mx < x + w && my >= y && my < y + h
}

fn handle_finder_widget_click(mx: usize, my: usize) -> Option<UiAction> {
    if let Some(wm) = window_manager::get_wm() {
        for window in wm.get_windows() {
            if !window.visible || window.app_type != AppType::Files {
                continue;
            }

            let x = window.x + 18;
            let y = window.y + 50;
            let button_x = x + 110;
            if inside(mx, my, button_x, y + 28, 132, 34) {
                return Some(UiAction::ToggleLauncher);
            }
            if inside(mx, my, button_x, y + 72, 132, 34) {
                return Some(UiAction::ToggleQuick);
            }
            if inside(mx, my, button_x, y + 116, 132, 34) {
                return Some(UiAction::ToggleNotifications);
            }
        }
    }

    None
}

fn handle_widget_click(mx: usize, my: usize, width: usize, _height: usize, state: &UiState) -> Option<UiAction> {
    if inside(mx, my, 88, 8, 86, 24) {
        return Some(UiAction::ToggleLauncher);
    }
    if inside(mx, my, 182, 8, 62, 24) {
        return Some(UiAction::ToggleQuick);
    }
    if inside(mx, my, 252, 8, 72, 24) {
        return Some(UiAction::ToggleNotifications);
    }
    if inside(mx, my, width.saturating_sub(172), 8, 148, 24) {
        return Some(UiAction::ToggleQuick);
    }

    if state.launcher_open {
        let launcher_x = 56;
        let launcher_y = 172;
        let apps = [
            AppType::Terminal,
            AppType::Files,
            AppType::Settings,
            AppType::Monitor,
            AppType::Editor,
        ];
        for i in 0..apps.len() {
            let col = i % 2;
            let row = i / 2;
            let x = launcher_x + 20 + col * 148;
            let y = launcher_y + 92 + row * 44;
            if inside(mx, my, x, y, 132, 34) {
                return Some(UiAction::ToggleApp(apps[i]));
            }
        }
    }

    if state.quick_open {
        let qs_x = width.saturating_sub(322);
        let qs_y = 74;
        if inside(mx, my, qs_x + 20, qs_y + 78, 240, 28) {
            let relative = mx.saturating_sub(qs_x + 20);
            let level = if relative < 48 {
                40
            } else if relative < 96 {
                55
            } else if relative < 144 {
                70
            } else if relative < 192 {
                85
            } else {
                100
            };
            return Some(UiAction::SetBrightness(level));
        }
    }

    if state.notifications_open {
        let note_x = width.saturating_sub(322);
        let note_y = 74 + 376;
        if inside(mx, my, note_x + 20, note_y + 42, 242, 38) {
            return Some(UiAction::ToggleNotifications);
        }
    }

    handle_finder_widget_click(mx, my)
}

fn apply_ui_action(state: &mut UiState, action: UiAction) -> bool {
    match action {
        UiAction::ToggleLauncher => state.launcher_open = !state.launcher_open,
        UiAction::ToggleQuick => state.quick_open = !state.quick_open,
        UiAction::ToggleNotifications => state.notifications_open = !state.notifications_open,
        UiAction::SetBrightness(value) => state.brightness = value.clamp(25, 100),
        UiAction::ToggleApp(app_type) => {
            if let Some(wm) = window_manager::get_wm() {
                wm.toggle_window(app_type);
            }
        }
    }
    true
}

fn handle_keyboard_shortcuts(state: &mut UiState) -> bool {
    let mut redraw = false;

    while let Some(scancode) = keyboard::read_scancode() {
        if scancode == 0xE0 {
            state.keyboard_extended = true;
            continue;
        }

        if state.keyboard_extended {
            state.keyboard_extended = false;
            match scancode {
                0x5B | 0x5C => {
                    state.launcher_open = !state.launcher_open;
                    redraw = true;
                }
                _ => {}
            }
        }
    }

    redraw
}

fn ease_step(step: usize, total: usize) -> usize {
    let t = step.saturating_mul(1000) / total.max(1);
    t.saturating_mul(t).saturating_mul(3000usize.saturating_sub(2 * t)) / 1_000_000
}

fn lerp_usize(a: usize, b: usize, t: usize) -> usize {
    (a.saturating_mul(1000usize.saturating_sub(t)) + b.saturating_mul(t)) / 1000
}

fn draw_genie_frame(fb: Framebuffer, width: usize, height: usize, rect: Rect, color: u32) {
    draw_round_rect(fb, width, height, rect.x + 8, rect.y + 10, rect.width, rect.height, 14, SHADOW);
    draw_round_rect(fb, width, height, rect.x, rect.y, rect.width, rect.height, 14, color);
    if rect.height > 16 {
        draw_round_rect(fb, width, height, rect.x, rect.y, rect.width, 12, 6, GLASS_SOFT);
    }
    draw_round_rect_border(fb, width, height, rect.x, rect.y, rect.width, rect.height, 14, GREEN);
}

fn animate_genie(
    scene: Framebuffer,
    front: Framebuffer,
    back_buffer: Option<&BackBuffer>,
    width: usize,
    height: usize,
    dock_rect: Rect,
    window_rect: Rect,
    opening: bool,
    state: &UiState,
) {
    if back_buffer.is_none() {
        return;
    }

    let frames = 4;
    for step in 0..=frames {
        let t = ease_step(step, frames);
        let t = if opening { t } else { 1000usize.saturating_sub(t) };
        let rect = Rect::new(
            lerp_usize(dock_rect.x, window_rect.x, t),
            lerp_usize(dock_rect.y, window_rect.y, t),
            lerp_usize(dock_rect.width, window_rect.width, t),
            lerp_usize(dock_rect.height, window_rect.height, t),
        );

        redraw_full_screen(scene, width, height, state);
        draw_genie_frame(scene, width, height, rect, GLASS);
        if let Some(buffer) = back_buffer {
            buffer.present_full(front);
        }

        for _ in 0..2_000 {
            unsafe { core::arch::asm!("pause"); }
        }
    }
}

fn handle_dock_click(mx: usize, my: usize, width: usize, height: usize) -> Option<AppType> {
    let (dock_x, dock_y, _dock_width, icon_size, icon_spacing) = dock_layout(width, height);
    if my < dock_y || my >= dock_y + 68 {
        return None;
    }

    let first_icon_x = dock_x + 24;
    for i in 0..DOCK_APPS.len() {
        let icon_x = first_icon_x + i * (icon_size + icon_spacing);
        let icon_y = dock_y + 10;
        if mx >= icon_x && mx < icon_x + icon_size && my >= icon_y && my < icon_y + icon_size {
            return Some(app_from_dock_index(i));
        }
    }

    None
}

pub fn run_ui_loop(fb_addr: *mut u32, width: usize, height: usize, pitch: usize) -> ! {
    serial_write("[GUI] renderer init start\r\n");
    let front = Framebuffer::new(fb_addr, width, height, pitch);
    let back_buffer = BackBuffer::init(width, height);
    let scene = back_buffer.as_ref().map(|buffer| buffer.canvas()).unwrap_or(front);
    if back_buffer.is_some() {
        serial_write("[GUI] back buffer enabled\r\n");
    } else {
        serial_write("[GUI] back buffer unavailable, direct framebuffer fallback\r\n");
    }
    serial_write("[GUI] dirty cursor redraw enabled\r\n");
    mouse::set_bounds(width, height);
    mouse::set_position((width / 2) as i32, (height / 2) as i32);

    let mut state = UiState::new();

    redraw_full_screen(scene, width, height, &state);
    if let Some(buffer) = back_buffer.as_ref() {
        buffer.present_full(front);
    }

    let (mut old_mouse_x, mut old_mouse_y) = mouse::get_position();
    let mut old_buttons = mouse::get_buttons();
    let mut dragging: Option<(usize, usize, usize)> = None;
    let mut cursor_background = [0u32; CURSOR_AREA];
    let mut damage = DamageTracker::new();
    if back_buffer.is_none() {
        save_cursor_area(front, width, height, old_mouse_x, old_mouse_y, &mut cursor_background);
    }
    draw_cursor(front, width, height, old_mouse_x, old_mouse_y);

    loop {
        let keyboard_redraw = handle_keyboard_shortcuts(&mut state);
        mouse::update();
        let (mouse_x, mouse_y) = mouse::get_position();
        let buttons = mouse::get_buttons();
        let pressed = (buttons & 0x01) != 0;
        let was_pressed = (old_buttons & 0x01) != 0;
        let cursor_moved = mouse_x != old_mouse_x || mouse_y != old_mouse_y;
        let mut full_redraw = keyboard_redraw;
        let mut drag_damage: Option<Rect> = None;

        if pressed && !was_pressed {
            let mx = mouse_x as usize;
            let my = mouse_y as usize;

            let closed = window_manager::get_wm()
                .map(|wm| wm.close_at(mx, my))
                .unwrap_or(None);

            if let Some((x, y, w, h, app_type)) = closed {
                dragging = None;
                let window_rect = Rect::new(x, y, w, h);
                if let Some(index) = dock_app_index(app_type) {
                    let dock_rect = dock_icon_rect(index, width, height);
                    animate_genie(scene, front, back_buffer.as_ref(), width, height, dock_rect, window_rect, false, &state);
                }
                full_redraw = true;
            } else if let Some(action) = handle_widget_click(mx, my, width, height, &state) {
                dragging = None;
                full_redraw = apply_ui_action(&mut state, action);
            } else if let Some(app_type) = handle_dock_click(mx, my, width, height) {
                dragging = None;
                let dock_rect = dock_icon_rect(dock_app_index(app_type).unwrap_or(0), width, height);
                let app_state = window_manager::get_wm()
                    .and_then(|wm| wm.app_bounds(app_type).map(|bounds| (wm.app_visible(app_type), bounds)));
                if let Some((was_visible, bounds)) = app_state {
                    let window_rect = rect_from_bounds(bounds);
                    if was_visible {
                        if let Some(wm) = window_manager::get_wm() {
                            wm.toggle_window(app_type);
                        }
                        animate_genie(scene, front, back_buffer.as_ref(), width, height, dock_rect, window_rect, false, &state);
                    } else {
                        animate_genie(scene, front, back_buffer.as_ref(), width, height, dock_rect, window_rect, true, &state);
                        if let Some(wm) = window_manager::get_wm() {
                            wm.toggle_window(app_type);
                        }
                    }
                }
                full_redraw = true;
            } else {
                dragging = window_manager::get_wm()
                    .and_then(|wm| wm.begin_drag_at(mx, my));
            }
        }

        if pressed {
            if let Some((idx, offset_x, offset_y)) = dragging {
                if let Some(wm) = window_manager::get_wm() {
                    let mx = mouse_x.max(0) as usize;
                    let my = mouse_y.max(0) as usize;
                    let old_bounds = wm.window_bounds(idx);
                    wm.drag_window(
                        idx,
                        mx.saturating_sub(offset_x),
                        my.saturating_sub(offset_y),
                        width,
                        height,
                    );
                    let new_bounds = wm.window_bounds(idx);
                    if let (Some(old_bounds), Some(new_bounds)) = (old_bounds, new_bounds) {
                        let window_damage = rect_from_bounds(old_bounds)
                            .union(rect_from_bounds(new_bounds))
                            .union(cursor_rect(old_mouse_x, old_mouse_y))
                            .union(cursor_rect(mouse_x, mouse_y));
                        if back_buffer.is_some() {
                            drag_damage = Some(padded_rect(window_damage, 3, width, height));
                        } else {
                            full_redraw = true;
                        }
                    } else {
                        full_redraw = true;
                    }
                }
            }
        } else {
            dragging = None;
        }

        if full_redraw {
            redraw_full_screen(scene, width, height, &state);
            if let Some(buffer) = back_buffer.as_ref() {
                buffer.present_full(front);
            } else {
                save_cursor_area(front, width, height, mouse_x, mouse_y, &mut cursor_background);
            }
            draw_cursor(front, width, height, mouse_x, mouse_y);
        } else if let (Some(buffer), Some(rect)) = (back_buffer.as_ref(), drag_damage) {
            redraw_region(scene, width, height, rect, &state);
            buffer.present_rect(front, rect);
            draw_cursor(front, width, height, mouse_x, mouse_y);
        } else if cursor_moved {
            if let Some(buffer) = back_buffer.as_ref() {
                damage.clear();
                damage.mark(cursor_rect(old_mouse_x, old_mouse_y));
                damage.mark(cursor_rect(mouse_x, mouse_y));
                for rect in damage.rects() {
                    buffer.present_rect(front, *rect);
                }
                draw_cursor(front, width, height, mouse_x, mouse_y);
            } else {
                restore_cursor_area(front, width, height, old_mouse_x, old_mouse_y, &cursor_background);
                save_cursor_area(front, width, height, mouse_x, mouse_y, &mut cursor_background);
                draw_cursor(front, width, height, mouse_x, mouse_y);
            }
        }

        old_mouse_x = mouse_x;
        old_mouse_y = mouse_y;
        old_buttons = buttons;

        for _ in 0..3000 {
            unsafe { core::arch::asm!("pause"); }
        }
    }
}
