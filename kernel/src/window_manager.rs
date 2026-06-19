const MAX_WINDOWS: usize = 10;

#[derive(Clone, Copy)]
pub struct Window {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
    pub restore_x: usize,
    pub restore_y: usize,
    pub restore_width: usize,
    pub restore_height: usize,
    pub maximized: bool,
    pub title: &'static str,
    pub visible: bool,
    pub app_type: AppType,
}

#[derive(Clone, Copy, PartialEq)]
pub enum AppType {
    Terminal,
    Calculator,
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

    fn default_window(app_type: AppType) -> Window {
        let (x, y, width, height, title) = match app_type {
            AppType::Terminal => (50, 80, 420, 310, "Terminal"),
            AppType::Calculator => (120, 115, 320, 220, "Calculator"),
            AppType::Files => (500, 88, 390, 310, "Files"),
            AppType::Settings => (285, 155, 390, 300, "System Settings"),
            AppType::Monitor => (70, 410, 820, 210, "Activity Monitor"),
            AppType::Editor => (170, 125, 610, 410, "TextEdit"),
        };

        Window {
            x,
            y,
            width,
            height,
            restore_x: x,
            restore_y: y,
            restore_width: width,
            restore_height: height,
            maximized: false,
            title,
            visible: true,
            app_type,
        }
    }

    pub fn toggle_window(&mut self, app_type: AppType) -> bool {
        for i in 0..self.window_count {
            if let Some(ref mut window) = self.windows[i] {
                if window.app_type == app_type {
                    window.visible = !window.visible;
                    return window.visible;
                }
            }
        }

        self.add_window(Self::default_window(app_type));
        true
    }

    pub fn get_windows(&self) -> impl Iterator<Item = &Window> {
        self.windows[..self.window_count]
            .iter()
            .filter_map(|w| w.as_ref())
    }

    pub fn close_at(
        &mut self,
        x: usize,
        y: usize,
    ) -> Option<(usize, usize, usize, usize, AppType)> {
        for i in (0..self.window_count).rev() {
            if let Some(ref mut window) = self.windows[i] {
                if !window.visible {
                    continue;
                }

                let close_x = window.x + 12;
                let close_y = window.y + 11;
                if x >= close_x && x < close_x + 12 && y >= close_y && y < close_y + 12 {
                    let bounds = (
                        window.x,
                        window.y,
                        window.width,
                        window.height,
                        window.app_type,
                    );
                    window.visible = false;
                    return Some(bounds);
                }
            }
        }

        None
    }

    pub fn minimize_at(
        &mut self,
        x: usize,
        y: usize,
    ) -> Option<(usize, usize, usize, usize, AppType)> {
        for i in (0..self.window_count).rev() {
            if let Some(ref mut window) = self.windows[i] {
                if !window.visible {
                    continue;
                }

                let button_x = window.x + 32;
                let button_y = window.y + 11;
                if x >= button_x && x < button_x + 12 && y >= button_y && y < button_y + 12 {
                    let bounds = (
                        window.x,
                        window.y,
                        window.width,
                        window.height,
                        window.app_type,
                    );
                    window.visible = false;
                    return Some(bounds);
                }
            }
        }

        None
    }

    pub fn zoom_at(
        &mut self,
        x: usize,
        y: usize,
        screen_width: usize,
        screen_height: usize,
    ) -> Option<(usize, usize, usize, usize, AppType)> {
        for i in (0..self.window_count).rev() {
            if let Some(ref mut window) = self.windows[i] {
                if !window.visible {
                    continue;
                }

                let button_x = window.x + 52;
                let button_y = window.y + 11;
                if x >= button_x && x < button_x + 12 && y >= button_y && y < button_y + 12 {
                    let old = (
                        window.x,
                        window.y,
                        window.width,
                        window.height,
                        window.app_type,
                    );
                    if window.maximized {
                        window.x = window.restore_x;
                        window.y = window.restore_y;
                        window.width = window.restore_width;
                        window.height = window.restore_height;
                        window.maximized = false;
                    } else {
                        window.restore_x = window.x;
                        window.restore_y = window.y;
                        window.restore_width = window.width;
                        window.restore_height = window.height;
                        window.x = 24;
                        window.y = 54;
                        window.width = screen_width.saturating_sub(48).max(260);
                        window.height = screen_height.saturating_sub(150).max(180);
                        window.maximized = true;
                    }
                    return Some(old);
                }
            }
        }

        None
    }

    pub fn begin_drag_at(&self, x: usize, y: usize) -> Option<(usize, usize, usize)> {
        for i in (0..self.window_count).rev() {
            if let Some(window) = self.windows[i] {
                if !window.visible {
                    continue;
                }

                let inside_x = x >= window.x && x < window.x + window.width;
                let inside_title = y >= window.y && y < window.y + 32;
                let over_buttons = x >= window.x + 12
                    && x < window.x + 64
                    && y >= window.y + 11
                    && y < window.y + 23;

                if inside_x && inside_title && !over_buttons {
                    return Some((i, x - window.x, y - window.y));
                }
            }
        }

        None
    }

    pub fn drag_window(
        &mut self,
        idx: usize,
        x: usize,
        y: usize,
        screen_width: usize,
        screen_height: usize,
    ) {
        if idx >= self.window_count {
            return;
        }

        if let Some(ref mut window) = self.windows[idx] {
            let max_x = screen_width.saturating_sub(window.width);
            let max_y = screen_height.saturating_sub(window.height);
            window.x = x.min(max_x);
            window.y = y.min(max_y).max(42);
            window.maximized = false;
        }
    }

    pub fn begin_resize_at(&self, x: usize, y: usize) -> Option<(usize, usize, usize)> {
        for i in (0..self.window_count).rev() {
            if let Some(window) = self.windows[i] {
                if !window.visible {
                    continue;
                }

                let grip_x = window.x + window.width.saturating_sub(18);
                let grip_y = window.y + window.height.saturating_sub(18);
                if x >= grip_x
                    && x < window.x + window.width
                    && y >= grip_y
                    && y < window.y + window.height
                {
                    return Some((i, window.x + window.width - x, window.y + window.height - y));
                }
            }
        }

        None
    }

    pub fn resize_window(
        &mut self,
        idx: usize,
        width: usize,
        height: usize,
        screen_width: usize,
        screen_height: usize,
    ) {
        if idx >= self.window_count {
            return;
        }

        if let Some(ref mut window) = self.windows[idx] {
            let max_width = screen_width.saturating_sub(window.x).max(260);
            let max_height = screen_height.saturating_sub(window.y).max(160);
            window.width = width.clamp(260, max_width);
            window.height = height.clamp(160, max_height);
            window.maximized = false;
        }
    }

    pub fn window_bounds(&self, idx: usize) -> Option<(usize, usize, usize, usize)> {
        if idx >= self.window_count {
            return None;
        }

        self.windows[idx].map(|window| (window.x, window.y, window.width, window.height))
    }

    pub fn app_bounds(&self, app_type: AppType) -> Option<(usize, usize, usize, usize)> {
        for i in 0..self.window_count {
            if let Some(window) = self.windows[i] {
                if window.app_type == app_type {
                    return Some((window.x, window.y, window.width, window.height));
                }
            }
        }

        let window = Self::default_window(app_type);
        Some((window.x, window.y, window.width, window.height))
    }

    pub fn app_visible(&self, app_type: AppType) -> bool {
        for i in 0..self.window_count {
            if let Some(window) = self.windows[i] {
                if window.app_type == app_type {
                    return window.visible;
                }
            }
        }

        false
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
