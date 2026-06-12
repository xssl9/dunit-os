use crate::gui::renderer::{BackBuffer, DamageTracker, Framebuffer, Rect};
use crate::drivers::mouse;
use crate::window_manager::{self, AppType};

const BG: u32 = 0x030504;
const BG_ALT: u32 = 0x0b1510;
const PANEL: u32 = 0x17251d;
const PANEL_DARK: u32 = 0x070a08;
const PANEL_LIGHT: u32 = 0x24362a;
const TEXT: u32 = 0xf4f1e8;
const MUTED: u32 = 0x93aa91;
const ACCENT: u32 = 0xa7bca4;
const GREEN: u32 = 0x2f7b51;
const GREEN_DARK: u32 = 0x185238;
const YELLOW: u32 = 0xe5d4a6;
const RED: u32 = 0xd86a62;
const PURPLE: u32 = 0x8ea88a;
const ORANGE: u32 = 0xd7b772;
const CURSOR_W: usize = 16;
const CURSOR_H: usize = 22;
const CURSOR_AREA: usize = CURSOR_W * CURSOR_H;

fn put_pixel(fb: Framebuffer, _width: usize, _height: usize, x: usize, y: usize, color: u32) {
    fb.put_pixel(x, y, color);
}

fn draw_rect(fb: Framebuffer, _width: usize, _height: usize, x: usize, y: usize, w: usize, h: usize, color: u32) {
    fb.fill_rect(Rect::new(x, y, w, h), color);
}

fn draw_rect_border(fb: Framebuffer, _width: usize, _height: usize, x: usize, y: usize, w: usize, h: usize, color: u32) {
    fb.stroke_rect(Rect::new(x, y, w, h), color);
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
    let dock_width = 5 * icon_size + 4 * icon_spacing + 48;
    let dock_x = width.saturating_sub(dock_width) / 2;
    let dock_y = height.saturating_sub(82);
    (dock_x, dock_y, dock_width, icon_size, icon_spacing)
}

fn scene_pixel(x: usize, y: usize, width: usize, height: usize) -> u32 {
    if y < 42 {
        return PANEL_DARK;
    }

    if let Some(wm) = window_manager::get_wm() {
        for window in wm.get_windows() {
            if window.visible && x >= window.x && x < window.x + window.width && y >= window.y && y < window.y + window.height {
                let dx = x - window.x;
                let dy = y - window.y;
                if dy < 32 {
                    return PANEL;
                }
                if dx == 0 || dx == window.width - 1 || dy == 0 || dy == window.height - 1 {
                    return ACCENT;
                }
                return match window.app_type {
                    AppType::Terminal => 0x020302,
                    _ => 0x101912,
                };
            }
        }
    }

    let (dock_x, dock_y, dock_width, icon_size, icon_spacing) = dock_layout(width, height);
    if x >= dock_x && x < dock_x + dock_width && y >= dock_y && y < dock_y + 68 {
        let first_icon_x = dock_x + 24;
        let colors = [GREEN, ACCENT, YELLOW, RED, PURPLE];
        for i in 0..5 {
            let icon_x = first_icon_x + i * (icon_size + icon_spacing);
            let icon_y = dock_y + 10;
            if x >= icon_x && x < icon_x + icon_size && y >= icon_y && y < icon_y + icon_size {
                return colors[i];
            }
        }
        return PANEL;
    }

    if y % 96 == 0 || x % 128 == 0 {
        0x0e1d14
    } else if ((x / 32) + (y / 32)) % 2 == 0 {
        BG
    } else {
        BG_ALT
    }
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

fn draw_dock(fb: Framebuffer, width: usize, height: usize) {
    let (dock_x, dock_y, dock_width, icon_size, icon_spacing) = dock_layout(width, height);
    draw_rect(fb, width, height, dock_x, dock_y, dock_width, 68, PANEL_DARK);
    draw_rect(fb, width, height, dock_x + 2, dock_y + 2, dock_width.saturating_sub(4), 64, PANEL);
    draw_rect_border(fb, width, height, dock_x, dock_y, dock_width, 68, GREEN_DARK);

    let apps = [
        (AppType::Terminal, GREEN, "Term"),
        (AppType::Files, ACCENT, "Files"),
        (AppType::Settings, YELLOW, "Prefs"),
        (AppType::Monitor, RED, "Stats"),
        (AppType::Editor, PURPLE, "Edit"),
    ];

    let first_icon_x = dock_x + 24;
    for i in 0..apps.len() {
        let icon_x = first_icon_x + i * (icon_size + icon_spacing);
        let icon_y = dock_y + 10;
        draw_rect(fb, width, height, icon_x, icon_y, icon_size, icon_size, apps[i].1);
        draw_rect_border(fb, width, height, icon_x, icon_y, icon_size, icon_size, 0xf4f1e8);
        draw_icon_symbol(fb, width, height, icon_x, icon_y, apps[i].0);
        draw_text(fb, width, height, icon_x + 8, icon_y + icon_size + 5, apps[i].2, MUTED);
    }
}

fn draw_window(fb: Framebuffer, width: usize, height: usize, window: &window_manager::Window) {
    draw_rect(fb, width, height, window.x, window.y, window.width, window.height, 0x101912);
    draw_rect(fb, width, height, window.x, window.y, window.width, 32, PANEL);
    draw_rect(fb, width, height, window.x, window.y + 31, window.width, 1, GREEN_DARK);
    draw_rect_border(fb, width, height, window.x, window.y, window.width, window.height, ACCENT);
    draw_text(fb, width, height, window.x + 12, window.y + 12, window.title, TEXT);
    draw_rect(fb, width, height, window.x + window.width.saturating_sub(25), window.y + 11, 10, 10, RED);

    let x = window.x + 18;
    let y = window.y + 50;
    match window.app_type {
        AppType::Terminal => {
            draw_rect(fb, width, height, window.x + 8, window.y + 40, window.width.saturating_sub(16), window.height.saturating_sub(50), 0x020302);
            draw_text(fb, width, height, x, y, "root@dunit:~# dufetch", GREEN);
            draw_text(fb, width, height, x, y + 20, "Dunit OS  Green Tea", TEXT);
            draw_text(fb, width, height, x, y + 40, "VFS MemFS userspace runtime", MUTED);
        }
        AppType::Files => {
            draw_text(fb, width, height, x, y, "/app", TEXT);
            draw_text(fb, width, height, x, y + 24, "[bin] elf_demo", ACCENT);
            draw_text(fb, width, height, x, y + 44, "[bin] fs_test", GREEN);
            draw_text(fb, width, height, x, y + 64, "[dir] tmp", YELLOW);
        }
        AppType::Settings => {
            draw_text(fb, width, height, x, y, "Mode       GUI", TEXT);
            draw_text(fb, width, height, x, y + 24, "Theme      Deep Green", MUTED);
            draw_text(fb, width, height, x, y + 48, "Runtime    Single task", MUTED);
        }
        AppType::Monitor => {
            draw_text(fb, width, height, x, y, "CPU", TEXT);
            draw_rect(fb, width, height, x + 42, y, 180, 10, PANEL_DARK);
            draw_rect(fb, width, height, x + 42, y, 34, 10, GREEN);
            draw_text(fb, width, height, x + 235, y, "18%", GREEN);
            draw_text(fb, width, height, x, y + 28, "RAM", TEXT);
            draw_rect(fb, width, height, x + 42, y + 28, 180, 10, PANEL_DARK);
            draw_rect(fb, width, height, x + 42, y + 28, 52, 10, YELLOW);
            draw_text(fb, width, height, x + 235, y + 28, "512MB", YELLOW);
        }
        AppType::Editor => {
            draw_text(fb, width, height, x, y, "notes.txt", ACCENT);
            draw_text(fb, width, height, x, y + 24, "Dunit GUI mode is alive.", TEXT);
            draw_text(fb, width, height, x, y + 44, "Cursor and dock are kernel builtins.", MUTED);
        }
    }
}

fn draw_windows(fb: Framebuffer, width: usize, height: usize) {
    if let Some(wm) = window_manager::get_wm() {
        for window in wm.get_windows() {
            if window.visible {
                draw_window(fb, width, height, window);
            }
        }
    }
}

fn redraw_full_screen(fb: Framebuffer, width: usize, height: usize) {
    for y in 0..height {
        for x in 0..width {
            put_pixel(fb, width, height, x, y, scene_pixel(x, y, width, height));
        }
    }

    draw_rect(fb, width, height, 0, 0, width, 42, PANEL_DARK);
    draw_rect(fb, width, height, 0, 40, width, 2, GREEN_DARK);
    draw_text(fb, width, height, 14, 14, "Dunit OS", TEXT);
    draw_text(fb, width, height, 104, 14, "GUI Mode", GREEN);
    draw_text(fb, width, height, width.saturating_sub(190), 14, "Single-task runtime", MUTED);

    draw_text(fb, width, height, 60, 82, "Dunit Desktop", TEXT);
    draw_text(fb, width, height, 60, 106, "Double buffered kernel GUI", MUTED);
    draw_text(fb, width, height, 60, 130, "VFS / MemFS / ELF exec ready", GREEN);

    draw_rect(fb, width, height, 60, 176, 220, 86, PANEL);
    draw_rect(fb, width, height, 60, 176, 8, 86, GREEN_DARK);
    draw_rect_border(fb, width, height, 60, 176, 220, 86, PANEL_LIGHT);
    draw_text(fb, width, height, 78, 196, "Runtime", ACCENT);
    draw_text(fb, width, height, 78, 222, "single process", TEXT);

    draw_rect(fb, width, height, 304, 176, 220, 86, PANEL);
    draw_rect(fb, width, height, 304, 176, 8, 86, GREEN);
    draw_rect_border(fb, width, height, 304, 176, 220, 86, PANEL_LIGHT);
    draw_text(fb, width, height, 322, 196, "Filesystem", GREEN);
    draw_text(fb, width, height, 322, 222, "MemFS over VFS", TEXT);

    draw_rect(fb, width, height, 548, 176, 220, 86, PANEL);
    draw_rect(fb, width, height, 548, 176, 8, 86, ORANGE);
    draw_rect_border(fb, width, height, 548, 176, 220, 86, PANEL_LIGHT);
    draw_text(fb, width, height, 566, 196, "Pointer", ORANGE);
    draw_text(fb, width, height, 566, 222, "PS/2 packet sync", TEXT);

    draw_windows(fb, width, height);
    draw_dock(fb, width, height);
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

fn handle_dock_click(mx: usize, my: usize, width: usize, height: usize) -> bool {
    let (dock_x, dock_y, _dock_width, icon_size, icon_spacing) = dock_layout(width, height);
    if my < dock_y || my >= dock_y + 68 {
        return false;
    }

    let first_icon_x = dock_x + 24;
    for i in 0..5 {
        let icon_x = first_icon_x + i * (icon_size + icon_spacing);
        let icon_y = dock_y + 10;
        if mx >= icon_x && mx < icon_x + icon_size && my >= icon_y && my < icon_y + icon_size {
            if let Some(wm) = window_manager::get_wm() {
                let app_type = match i {
                    0 => AppType::Terminal,
                    1 => AppType::Files,
                    2 => AppType::Settings,
                    3 => AppType::Monitor,
                    4 => AppType::Editor,
                    _ => AppType::Terminal,
                };
                wm.toggle_window(app_type);
            }
            return true;
        }
    }

    false
}

pub fn run_ui_loop(fb_addr: *mut u32, width: usize, height: usize, pitch: usize) -> ! {
    let front = Framebuffer::new(fb_addr, width, height, pitch);
    let back_buffer = BackBuffer::init(width, height);
    let scene = back_buffer.as_ref().map(|buffer| buffer.canvas()).unwrap_or(front);
    mouse::set_bounds(width, height);
    mouse::set_position((width / 2) as i32, (height / 2) as i32);

    redraw_full_screen(scene, width, height);
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
        mouse::update();
        let (mouse_x, mouse_y) = mouse::get_position();
        let buttons = mouse::get_buttons();
        let pressed = (buttons & 0x01) != 0;
        let was_pressed = (old_buttons & 0x01) != 0;
        let cursor_moved = mouse_x != old_mouse_x || mouse_y != old_mouse_y;
        let mut full_redraw = false;

        if pressed && !was_pressed {
            let mx = mouse_x as usize;
            let my = mouse_y as usize;

            let closed = window_manager::get_wm()
                .map(|wm| wm.close_at(mx, my))
                .unwrap_or(false);

            if closed {
                dragging = None;
                full_redraw = true;
            } else if handle_dock_click(mx, my, width, height) {
                dragging = None;
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
                    wm.drag_window(
                        idx,
                        mx.saturating_sub(offset_x),
                        my.saturating_sub(offset_y),
                        width,
                        height,
                    );
                    full_redraw = true;
                }
            }
        } else {
            dragging = None;
        }

        if full_redraw {
            redraw_full_screen(scene, width, height);
            if let Some(buffer) = back_buffer.as_ref() {
                buffer.present_full(front);
            } else {
                save_cursor_area(front, width, height, mouse_x, mouse_y, &mut cursor_background);
            }
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
