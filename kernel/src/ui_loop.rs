use crate::drivers::mouse;
use crate::window_manager::{self, AppType};
use crate::terminal;

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
        
        let simple_font = [
            [0,1,1,1,0],
            [1,0,0,0,1],
            [1,0,0,0,1],
            [1,0,0,0,1],
            [0,1,1,1,0],
        ];
        
        for (i, _ch) in title.chars().enumerate() {
            for dy in 0..5 {
                for dx in 0..5 {
                    if simple_font[dy][dx] == 1 {
                        let px = x + 10 + i * 6 + dx;
                        let py = y + 10 + dy;
                        if px < width && py < height {
                            *fb_addr.add(py * width + px) = 0xfdf6e3;
                        }
                    }
                }
            }
        }
        
        if app_type == AppType::Terminal {
            if let Some(term) = terminal::get_terminal() {
                let text_color = 0x859900u32;
                let mut line_y = y + 40;
                
                for line in term.get_visible_lines() {
                    if line_y + 10 > y + h {
                        break;
                    }
                    
                    for (i, _ch) in line.chars().enumerate().take(50) {
                        for dy in 0..5 {
                            for dx in 0..5 {
                                if simple_font[dy][dx] == 1 {
                                    let px = x + 10 + i * 6 + dx;
                                    let py = line_y + dy;
                                    if px < x + w && py < y + h {
                                        *fb_addr.add(py * width + px) = text_color;
                                    }
                                }
                            }
                        }
                    }
                    line_y += 10;
                }
                
                let prompt = term.get_prompt();
                for (i, _ch) in prompt.chars().enumerate().take(50) {
                    for dy in 0..5 {
                        for dx in 0..5 {
                            if simple_font[dy][dx] == 1 {
                                let px = x + 10 + i * 6 + dx;
                                let py = line_y + dy;
                                if px < x + w && py < y + h {
                                    *fb_addr.add(py * width + px) = 0xfdf6e3;
                                }
                            }
                        }
                    }
                }
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
