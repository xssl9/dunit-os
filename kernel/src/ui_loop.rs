use crate::drivers::mouse;
use crate::window_manager::{self, AppType};

fn get_pixel_color(x: usize, y: usize, width: usize, height: usize) -> u32 {
    let bg_color = 0x002b36u32;
    let panel_color = 0x073642u32;
    let plank_color = 0x1c1c1cu32;
    let icon_color = 0xffffffu32;
    
    let plank_height = 64;
    let plank_y = height - plank_height;
    let icon_size = 48;
    let icon_spacing = 8;
    let plank_start_x = (width - (5 * (icon_size + icon_spacing))) / 2;
    
    if y < 40 {
        return panel_color;
    }
    
    if let Some(wm) = window_manager::get_wm() {
        for window in wm.get_windows() {
            if window.visible && x >= window.x && x < window.x + window.width && y >= window.y && y < window.y + window.height {
                let dx = x - window.x;
                let dy = y - window.y;
                if dy < 30 {
                    return 0x268bd2;
                } else if dx == 0 || dx == window.width - 1 || dy == 0 || dy == window.height - 1 {
                    return 0x586e75;
                } else if window.app_type == AppType::Terminal {
                    return 0x002b36;
                } else {
                    return 0xfdf6e3;
                }
            }
        }
    }
    
    if y >= plank_y && y < height {
        let plank_start = plank_start_x.saturating_sub(20);
        let plank_end = plank_start_x + 5 * (icon_size + icon_spacing) + 20;
        
        if x >= plank_start && x < plank_end {
            let icon_colors = [0x268bd2u32, 0x859900u32, 0xb58900u32, 0xdc322fu32, 0x6c71c4u32];
            
            for i in 0..5 {
                let icon_x = plank_start_x + i * (icon_size + icon_spacing);
                let icon_y = plank_y + 8;
                
                if x >= icon_x && x < icon_x + icon_size && y >= icon_y && y < icon_y + icon_size {
                    let dx = x - icon_x;
                    let dy = y - icon_y;
                    let is_border = dx < 2 || dx >= icon_size - 2 || dy < 2 || dy >= icon_size - 2;
                    if is_border {
                        return 0x2c2c2cu32;
                    }
                    
                    let cx = icon_x + icon_size / 2;
                    let cy = icon_y + icon_size / 2;
                    let in_symbol = match i {
                        0 => {
                            (y >= cy - 8 && y < cy - 8 + 3 && x >= cx - 10 && x < cx + 10) ||
                            (y >= cy && y < cy + 3 && x >= cx - 10 && x < cx + 10) ||
                            (y >= cy + 8 && y < cy + 11 && x >= cx - 10 && x < cx + 10) ||
                            (x >= cx - 10 && x < cx - 7 && y >= cy - 8 && y < cy + 8) ||
                            (x >= cx + 7 && x < cx + 10 && y >= cy - 8 && y < cy + 8)
                        },
                        1 => {
                            (x >= cx - 8 && x < cx - 5 && y >= cy - 8 && y < cy + 8) ||
                            (x >= cx + 2 && x < cx + 5 && y >= cy - 4 && y < cy + 8) ||
                            (y >= cy - 8 && y < cy - 5 && x >= cx - 8 && x < cx + 2)
                        },
                        2 => {
                            let rdx = (x as i32 - cx as i32).abs();
                            let rdy = (y as i32 - cy as i32).abs();
                            let dist = rdx * rdx + rdy * rdy;
                            dist >= 49 && dist <= 81
                        },
                        3 => {
                            (y >= cy - 10 && y < cy + 10 && x >= cx - 1 && x < cx + 2) ||
                            (x >= cx - 8 && x < cx + 8 && y >= cy - 1 && y < cy + 2)
                        },
                        4 => {
                            (y >= cy - 8 && y < cy - 5 && x >= cx - 6 && x < cx + 6) ||
                            (x >= cx - 6 && x < cx - 3 && y >= cy - 8 && y < cy + 8) ||
                            (x >= cx + 3 && x < cx + 6 && y >= cy - 8 && y < cy + 8) ||
                            (y >= cy - 2 && y < cy + 4 && x >= cx - 2 && x < cx + 1)
                        },
                        _ => false
                    };
                    
                    return if in_symbol { icon_color } else { icon_colors[i] };
                }
            }
            
            return plank_color;
        }
    }
    
    bg_color
}

fn draw_char(fb: *mut u32, width: usize, x: usize, y: usize, ch: u8, color: u32) {
    let font: &[u8] = match ch {
        b'A' => &[0x7C, 0x12, 0x11, 0x12, 0x7C],
        b'B' => &[0x7F, 0x49, 0x49, 0x49, 0x36],
        b'C' => &[0x3E, 0x41, 0x41, 0x41, 0x22],
        b'D' => &[0x7F, 0x41, 0x41, 0x22, 0x1C],
        b'E' => &[0x7F, 0x49, 0x49, 0x49, 0x41],
        b'F' => &[0x7F, 0x09, 0x09, 0x09, 0x01],
        b'G' => &[0x3E, 0x41, 0x49, 0x49, 0x7A],
        b'H' => &[0x7F, 0x08, 0x08, 0x08, 0x7F],
        b'I' => &[0x00, 0x41, 0x7F, 0x41, 0x00],
        b'J' => &[0x20, 0x40, 0x41, 0x3F, 0x01],
        b'K' => &[0x7F, 0x08, 0x14, 0x22, 0x41],
        b'L' => &[0x7F, 0x40, 0x40, 0x40, 0x40],
        b'M' => &[0x7F, 0x02, 0x0C, 0x02, 0x7F],
        b'N' => &[0x7F, 0x04, 0x08, 0x10, 0x7F],
        b'O' => &[0x3E, 0x41, 0x41, 0x41, 0x3E],
        b'P' => &[0x7F, 0x09, 0x09, 0x09, 0x06],
        b'Q' => &[0x3E, 0x41, 0x51, 0x21, 0x5E],
        b'R' => &[0x7F, 0x09, 0x19, 0x29, 0x46],
        b'S' => &[0x46, 0x49, 0x49, 0x49, 0x31],
        b'T' => &[0x01, 0x01, 0x7F, 0x01, 0x01],
        b'U' => &[0x3F, 0x40, 0x40, 0x40, 0x3F],
        b'V' => &[0x1F, 0x20, 0x40, 0x20, 0x1F],
        b'W' => &[0x3F, 0x40, 0x38, 0x40, 0x3F],
        b'X' => &[0x63, 0x14, 0x08, 0x14, 0x63],
        b'Y' => &[0x07, 0x08, 0x70, 0x08, 0x07],
        b'Z' => &[0x61, 0x51, 0x49, 0x45, 0x43],
        b'a' => &[0x20, 0x54, 0x54, 0x54, 0x78],
        b'b' => &[0x7F, 0x48, 0x44, 0x44, 0x38],
        b'c' => &[0x38, 0x44, 0x44, 0x44, 0x20],
        b'd' => &[0x38, 0x44, 0x44, 0x48, 0x7F],
        b'e' => &[0x38, 0x54, 0x54, 0x54, 0x18],
        b'f' => &[0x08, 0x7E, 0x09, 0x01, 0x02],
        b'g' => &[0x0C, 0x52, 0x52, 0x52, 0x3E],
        b'h' => &[0x7F, 0x08, 0x04, 0x04, 0x78],
        b'i' => &[0x00, 0x44, 0x7D, 0x40, 0x00],
        b'j' => &[0x20, 0x40, 0x44, 0x3D, 0x00],
        b'k' => &[0x7F, 0x10, 0x28, 0x44, 0x00],
        b'l' => &[0x00, 0x41, 0x7F, 0x40, 0x00],
        b'm' => &[0x7C, 0x04, 0x18, 0x04, 0x78],
        b'n' => &[0x7C, 0x08, 0x04, 0x04, 0x78],
        b'o' => &[0x38, 0x44, 0x44, 0x44, 0x38],
        b'p' => &[0x7C, 0x14, 0x14, 0x14, 0x08],
        b'q' => &[0x08, 0x14, 0x14, 0x18, 0x7C],
        b'r' => &[0x7C, 0x08, 0x04, 0x04, 0x08],
        b's' => &[0x48, 0x54, 0x54, 0x54, 0x20],
        b't' => &[0x04, 0x3F, 0x44, 0x40, 0x20],
        b'u' => &[0x3C, 0x40, 0x40, 0x20, 0x7C],
        b'v' => &[0x1C, 0x20, 0x40, 0x20, 0x1C],
        b'w' => &[0x3C, 0x40, 0x30, 0x40, 0x3C],
        b'x' => &[0x44, 0x28, 0x10, 0x28, 0x44],
        b'y' => &[0x0C, 0x50, 0x50, 0x50, 0x3C],
        b'z' => &[0x44, 0x64, 0x54, 0x4C, 0x44],
        b'0' => &[0x3E, 0x51, 0x49, 0x45, 0x3E],
        b'1' => &[0x00, 0x42, 0x7F, 0x40, 0x00],
        b'2' => &[0x42, 0x61, 0x51, 0x49, 0x46],
        b'3' => &[0x21, 0x41, 0x45, 0x4B, 0x31],
        b'4' => &[0x18, 0x14, 0x12, 0x7F, 0x10],
        b'5' => &[0x27, 0x45, 0x45, 0x45, 0x39],
        b'6' => &[0x3C, 0x4A, 0x49, 0x49, 0x30],
        b'7' => &[0x01, 0x71, 0x09, 0x05, 0x03],
        b'8' => &[0x36, 0x49, 0x49, 0x49, 0x36],
        b'9' => &[0x06, 0x49, 0x49, 0x29, 0x1E],
        b' ' => &[0x00, 0x00, 0x00, 0x00, 0x00],
        b'.' => &[0x00, 0x60, 0x60, 0x00, 0x00],
        b':' => &[0x00, 0x36, 0x36, 0x00, 0x00],
        b'$' => &[0x24, 0x2A, 0x7F, 0x2A, 0x12],
        b'-' => &[0x08, 0x08, 0x08, 0x08, 0x08],
        b'/' => &[0x20, 0x10, 0x08, 0x04, 0x02],
        b'(' => &[0x00, 0x1C, 0x22, 0x41, 0x00],
        b')' => &[0x00, 0x41, 0x22, 0x1C, 0x00],
        _ => &[0x00, 0x00, 0x00, 0x00, 0x00],
    };
    
    unsafe {
        for dx in 0..5 {
            let col = font[dx];
            for dy in 0..8 {
                if (col >> dy) & 1 == 1 {
                    let px = x + dx;
                    let py = y + dy;
                    if px < width {
                        *fb.add(py * width + px) = color;
                    }
                }
            }
        }
    }
}

fn draw_text(fb: *mut u32, width: usize, x: usize, y: usize, text: &str, color: u32) {
    for (i, ch) in text.bytes().enumerate() {
        draw_char(fb, width, x + i * 6, y, ch, color);
    }
}

fn draw_window(fb_addr: *mut u32, width: usize, height: usize, x: usize, y: usize, w: usize, h: usize, title: &str, app_type: AppType) {
    unsafe {
        for dy in 0..h {
            for dx in 0..w {
                let px = x + dx;
                let py = y + dy;
                if px < width && py < height {
                    let offset = py * width + px;
                    let color = if dy < 30 {
                        0x268bd2
                    } else if dx == 0 || dx == w-1 || dy == 0 || dy == h-1 {
                        0x586e75
                    } else if app_type == AppType::Terminal {
                        0x002b36
                    } else {
                        0xfdf6e3
                    };
                    *fb_addr.add(offset) = color;
                }
            }
        }
        
        draw_text(fb_addr, width, x + 10, y + 10, title, 0xfdf6e3);
    }
}

fn redraw_full_screen(fb_addr: *mut u32, width: usize, height: usize) {
    let bg_color = 0x002b36u32;
    let panel_color = 0x073642u32;
    let plank_color = 0x1c1c1cu32;
    
    unsafe {
        for y in 0..height {
            for x in 0..width {
                let color = if y < 40 {
                    panel_color
                } else if y >= height - 64 {
                    bg_color
                } else {
                    bg_color
                };
                *fb_addr.add(y * width + x) = color;
            }
        }
        
        draw_text(fb_addr, width, 10, 15, "Workspace 1", 0x93a1a1);
        draw_text(fb_addr, width, width - 60, 15, "13:37", 0x93a1a1);
        
        let plank_height = 64;
        let plank_y = height - plank_height;
        let icon_size = 48;
        let icon_spacing = 8;
        let plank_start_x = (width - (5 * (icon_size + icon_spacing))) / 2;
        
        for y in plank_y..height {
            for x in (plank_start_x - 20)..(plank_start_x + 5 * (icon_size + icon_spacing) + 20) {
                if x < width {
                    *fb_addr.add(y * width + x) = plank_color;
                }
            }
        }
        
        let icon_colors = [0x268bd2u32, 0x859900u32, 0xb58900u32, 0xdc322fu32, 0x6c71c4u32];
        for i in 0..5 {
            let icon_x = plank_start_x + i * (icon_size + icon_spacing);
            let icon_y = plank_y + 8;
            
            for dy in 0..icon_size {
                for dx in 0..icon_size {
                    let px = icon_x + dx;
                    let py = icon_y + dy;
                    if px < width && py < height {
                        let is_border = dx < 2 || dx >= icon_size - 2 || dy < 2 || dy >= icon_size - 2;
                        let color = if is_border { 0x2c2c2cu32 } else { icon_colors[i] };
                        *fb_addr.add(py * width + px) = color;
                    }
                }
            }
            
            let cx = icon_x + icon_size / 2;
            let cy = icon_y + icon_size / 2;
            let icon_color = 0xffffffu32;
            
            match i {
                0 => {
                    for j in 0..3 {
                        for k in 0..20 {
                            *fb_addr.add((cy - 8 + j * 8) * width + cx - 10 + k) = icon_color;
                        }
                    }
                    for j in 0..16 {
                        *fb_addr.add((cy - 8 + j) * width + cx - 10) = icon_color;
                        *fb_addr.add((cy - 8 + j) * width + cx + 9) = icon_color;
                    }
                },
                1 => {
                    for j in 0..16 {
                        for k in 0..3 {
                            *fb_addr.add((cy - 8 + j) * width + cx - 8 + k) = icon_color;
                        }
                    }
                    for j in 0..12 {
                        for k in 0..3 {
                            *fb_addr.add((cy - 4 + j) * width + cx + 2 + k) = icon_color;
                        }
                    }
                    for k in 0..10 {
                        for j in 0..3 {
                            *fb_addr.add((cy - 8 + j) * width + cx - 8 + k) = icon_color;
                        }
                    }
                },
                2 => {
                    for j in 0..8 {
                        for k in 0..8 {
                            let dx = j as i32 - 4;
                            let dy = k as i32 - 4;
                            if dx * dx + dy * dy >= 9 && dx * dx + dy * dy <= 25 {
                                *fb_addr.add((cy - 4 + k) * width + cx - 4 + j) = icon_color;
                            }
                        }
                    }
                },
                3 => {
                    for j in 0..20 {
                        for k in 0..3 {
                            *fb_addr.add((cy - 10 + j) * width + cx - 1 + k) = icon_color;
                        }
                    }
                    for j in 0..3 {
                        for k in 0..16 {
                            *fb_addr.add((cy - 1 + j) * width + cx - 8 + k) = icon_color;
                        }
                    }
                },
                4 => {
                    for j in 0..16 {
                        for k in 0..12 {
                            if j < 3 || k < 3 || k >= 9 {
                                *fb_addr.add((cy - 8 + j) * width + cx - 6 + k) = icon_color;
                            }
                        }
                    }
                    for j in 0..6 {
                        for k in 0..3 {
                            *fb_addr.add((cy - 2 + j) * width + cx - 2 + k) = icon_color;
                        }
                    }
                },
                _ => {}
            }
        }
    }
}

fn draw_windows(fb_addr: *mut u32, width: usize, height: usize) {
    if let Some(wm) = window_manager::get_wm() {
        for window in wm.get_windows() {
            if window.visible {
                draw_window(fb_addr, width, height, window.x, window.y, window.width, window.height, window.title, window.app_type);
            }
        }
    }
}

pub fn run_ui_loop(fb_addr: *mut u32, width: usize, height: usize) -> ! {
    let plank_height = 64;
    let plank_y = height - plank_height;
    let icon_size = 48;
    let icon_spacing = 8;
    let plank_start_x = (width - (5 * (icon_size + icon_spacing))) / 2;
    
    let mut old_mouse_x = 512;
    let mut old_mouse_y = 384;
    let mut old_buttons = 0u8;
    
    loop {
        mouse::update();
        let (mouse_x, mouse_y) = mouse::get_position();
        let buttons = mouse::get_buttons();
        
        if (buttons & 0x01) != 0 && (old_buttons & 0x01) == 0 {
            let mx = mouse_x as usize;
            let my = mouse_y as usize;
            
            if my >= plank_y && my < height {
                for i in 0..5 {
                    let icon_x = plank_start_x + i * (icon_size + icon_spacing);
                    let icon_y = plank_y + 8;
                    
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
                            draw_windows(fb_addr, width, height);
                        }
                        break;
                    }
                }
            }
        }
        
        if mouse_x != old_mouse_x || mouse_y != old_mouse_y {
            unsafe {
                for dy in 0..16 {
                    for dx in 0..10 {
                        if dx < 10 - dy / 2 {
                            let px = old_mouse_x as usize + dx;
                            let py = old_mouse_y as usize + dy;
                            if px < width && py < height {
                                let color = get_pixel_color(px, py, width, height);
                                *fb_addr.add(py * width + px) = color;
                            }
                        }
                    }
                }
                
                for dy in 0..16 {
                    for dx in 0..10 {
                        if dx < 10 - dy / 2 {
                            let px = mouse_x as usize + dx;
                            let py = mouse_y as usize + dy;
                            if px < width && py < height {
                                *fb_addr.add(py * width + px) = 0xFFFFFF;
                            }
                        }
                    }
                }
            }
            
            old_mouse_x = mouse_x;
            old_mouse_y = mouse_y;
        }
        
        old_buttons = buttons;
        
        for _ in 0..1000 {
            unsafe { core::arch::asm!("pause"); }
        }
    }
}
