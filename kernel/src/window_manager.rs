const MAX_WINDOWS: usize = 10;

#[derive(Clone, Copy)]
pub struct Window {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
    pub title: &'static str,
    pub visible: bool,
    pub app_type: AppType,
}

#[derive(Clone, Copy, PartialEq)]
pub enum AppType {
    Terminal,
    Files,
    Settings,
    Monitor,
    Editor,
}

impl AppType {
    pub fn has_terminal(&self) -> bool {
        matches!(self, AppType::Terminal)
    }
}

pub struct WindowManager {
    windows: [Option<Window>; MAX_WINDOWS],
    window_count: usize,
    menu_open: bool,
}

impl WindowManager {
    pub fn new() -> Self {
        Self {
            windows: [None; MAX_WINDOWS],
            window_count: 0,
            menu_open: false,
        }
    }

    pub fn add_window(&mut self, window: Window) {
        if self.window_count < MAX_WINDOWS {
            self.windows[self.window_count] = Some(window);
            self.window_count += 1;
        }
    }

    pub fn toggle_window(&mut self, app_type: AppType) {
        for i in 0..self.window_count {
            if let Some(ref mut window) = self.windows[i] {
                if window.app_type == app_type {
                    window.visible = !window.visible;
                    return;
                }
            }
        }
        
        let (x, y, width, height, title) = match app_type {
            AppType::Terminal => (50, 80, 400, 300, "Terminal"),
            AppType::Files => (470, 80, 400, 300, "Files"),
            AppType::Settings => (260, 150, 400, 300, "Settings"),
            AppType::Monitor => (50, 400, 820, 200, "System Monitor"),
            AppType::Editor => (150, 120, 600, 400, "Text Editor"),
        };
        
        self.add_window(Window {
            x, y, width, height, title, visible: true, app_type
        });
    }

    pub fn get_windows(&self) -> impl Iterator<Item = &Window> {
        self.windows[..self.window_count].iter().filter_map(|w| w.as_ref())
    }

    pub fn close_at(&mut self, x: usize, y: usize) -> bool {
        for i in (0..self.window_count).rev() {
            if let Some(ref mut window) = self.windows[i] {
                if !window.visible {
                    continue;
                }

                let close_x = window.x + window.width.saturating_sub(25);
                let close_y = window.y + 11;
                if x >= close_x && x < close_x + 10 && y >= close_y && y < close_y + 10 {
                    window.visible = false;
                    return true;
                }
            }
        }

        false
    }

    pub fn begin_drag_at(&self, x: usize, y: usize) -> Option<(usize, usize, usize)> {
        for i in (0..self.window_count).rev() {
            if let Some(window) = self.windows[i] {
                if !window.visible {
                    continue;
                }

                let inside_x = x >= window.x && x < window.x + window.width;
                let inside_title = y >= window.y && y < window.y + 32;
                let close_x = window.x + window.width.saturating_sub(25);
                let over_close = x >= close_x && x < close_x + 10 && y >= window.y + 11 && y < window.y + 21;

                if inside_x && inside_title && !over_close {
                    return Some((i, x - window.x, y - window.y));
                }
            }
        }

        None
    }

    pub fn drag_window(&mut self, idx: usize, x: usize, y: usize, screen_width: usize, screen_height: usize) {
        if idx >= self.window_count {
            return;
        }

        if let Some(ref mut window) = self.windows[idx] {
            let max_x = screen_width.saturating_sub(window.width);
            let max_y = screen_height.saturating_sub(window.height);
            window.x = x.min(max_x);
            window.y = y.min(max_y).max(42);
        }
    }

    pub fn move_window(&mut self, idx: usize, x: usize, y: usize) {
        if idx < self.window_count {
            if let Some(ref mut window) = self.windows[idx] {
                window.x = x;
                window.y = y;
            }
        }
    }

    pub fn toggle_menu(&mut self) {
        self.menu_open = !self.menu_open;
    }

    pub fn is_menu_open(&self) -> bool {
        self.menu_open
    }

    pub fn close_menu(&mut self) {
        self.menu_open = false;
    }
}

static mut WM_INSTANCE: Option<WindowManager> = None;

pub fn init() {
    unsafe {
        WM_INSTANCE = Some(WindowManager::new());
    }
}

pub fn get_wm() -> Option<&'static mut WindowManager> {
    unsafe { WM_INSTANCE.as_mut() }
}
