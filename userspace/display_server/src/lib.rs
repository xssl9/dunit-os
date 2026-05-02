#![no_std]

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use egui::{Context, RawInput, ViewportId};

pub type ProcessId = u32;
pub type WindowId = u32;
pub type SharedMemoryId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    pub id: WindowId,
    pub owner_pid: ProcessId,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub buffer: SharedMemoryId,
    pub visible: bool,
    pub focused: bool,
}

impl Window {
    pub fn new(
        id: WindowId,
        owner_pid: ProcessId,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        buffer: SharedMemoryId,
    ) -> Self {
        Self {
            id,
            owner_pid,
            x,
            y,
            width,
            height,
            buffer,
            visible: true,
            focused: false,
        }
    }
}

pub struct InputState {
    pub mouse_x: i32,
    pub mouse_y: i32,
    pub mouse_buttons: u8,
    pub dragging_window: Option<WindowId>,
    pub drag_offset_x: i32,
    pub drag_offset_y: i32,
}

impl InputState {
    pub fn new() -> Self {
        Self {
            mouse_x: 0,
            mouse_y: 0,
            mouse_buttons: 0,
            dragging_window: None,
            drag_offset_x: 0,
            drag_offset_y: 0,
        }
    }
}

impl Default for InputState {
    fn default() -> Self {
        Self::new()
    }
}

pub struct DisplayServer {
    windows: BTreeMap<WindowId, Window>,
    focused_window: Option<WindowId>,
    next_window_id: WindowId,
    input_state: InputState,
    egui_ctx: Context,
    egui_input: RawInput,
}

impl DisplayServer {
    pub fn new() -> Self {
        let egui_ctx = Context::default();
        let egui_input = RawInput {
            viewport_id: ViewportId::ROOT,
            ..Default::default()
        };

        Self {
            windows: BTreeMap::new(),
            focused_window: None,
            next_window_id: 1,
            input_state: InputState::new(),
            egui_ctx,
            egui_input,
        }
    }

    pub fn create_window(
        &mut self,
        owner_pid: ProcessId,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        buffer: SharedMemoryId,
    ) -> WindowId {
        let id = self.next_window_id;
        self.next_window_id += 1;

        let window = Window::new(id, owner_pid, x, y, width, height, buffer);
        self.windows.insert(id, window);

        if self.focused_window.is_none() {
            self.focused_window = Some(id);
            if let Some(win) = self.windows.get_mut(&id) {
                win.focused = true;
            }
        }

        id
    }

    pub fn destroy_window(&mut self, id: WindowId) -> bool {
        if let Some(window) = self.windows.remove(&id) {
            if self.focused_window == Some(id) {
                self.focused_window = None;
                if let Some((&next_id, _)) = self.windows.iter().next() {
                    self.focused_window = Some(next_id);
                    if let Some(win) = self.windows.get_mut(&next_id) {
                        win.focused = true;
                    }
                }
            }
            true
        } else {
            false
        }
    }

    pub fn get_window(&self, id: WindowId) -> Option<&Window> {
        self.windows.get(&id)
    }

    pub fn get_window_mut(&mut self, id: WindowId) -> Option<&mut Window> {
        self.windows.get_mut(&id)
    }

    pub fn get_all_windows(&self) -> Vec<&Window> {
        self.windows.values().collect()
    }

    pub fn get_focused_window(&self) -> Option<WindowId> {
        self.focused_window
    }

    pub fn set_focus(&mut self, id: WindowId) -> bool {
        if !self.windows.contains_key(&id) {
            return false;
        }

        if let Some(old_focused) = self.focused_window {
            if let Some(win) = self.windows.get_mut(&old_focused) {
                win.focused = false;
            }
        }

        self.focused_window = Some(id);
        if let Some(win) = self.windows.get_mut(&id) {
            win.focused = true;
        }

        true
    }

    pub fn window_count(&self) -> usize {
        self.windows.len()
    }

    pub fn handle_mouse_event(&mut self, x: i32, y: i32, buttons: u8) {
        let left_button_pressed = (buttons & 0x01) != 0;
        let was_left_button_pressed = (self.input_state.mouse_buttons & 0x01) != 0;

        self.input_state.mouse_x = x;
        self.input_state.mouse_y = y;

        self.egui_input.events.push(egui::Event::PointerMoved(egui::pos2(
            x as f32,
            y as f32,
        )));

        if left_button_pressed && !was_left_button_pressed {
            self.egui_input.events.push(egui::Event::PointerButton {
                pos: egui::pos2(x as f32, y as f32),
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::default(),
            });

            if let Some(window_id) = self.find_window_at(x, y) {
                self.set_focus(window_id);
                
                if let Some(window) = self.windows.get(&window_id) {
                    self.input_state.dragging_window = Some(window_id);
                    self.input_state.drag_offset_x = x - window.x;
                    self.input_state.drag_offset_y = y - window.y;
                }
            }
        } else if !left_button_pressed && was_left_button_pressed {
            self.egui_input.events.push(egui::Event::PointerButton {
                pos: egui::pos2(x as f32, y as f32),
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::default(),
            });

            self.input_state.dragging_window = None;
        }

        if left_button_pressed {
            if let Some(dragging_id) = self.input_state.dragging_window {
                if let Some(window) = self.windows.get_mut(&dragging_id) {
                    window.x = x - self.input_state.drag_offset_x;
                    window.y = y - self.input_state.drag_offset_y;
                }
            }
        }

        self.input_state.mouse_buttons = buttons;
    }

    fn find_window_at(&self, x: i32, y: i32) -> Option<WindowId> {
        for (id, window) in self.windows.iter().rev() {
            if !window.visible {
                continue;
            }

            if x >= window.x
                && x < window.x + window.width as i32
                && y >= window.y
                && y < window.y + window.height as i32
            {
                return Some(*id);
            }
        }
        None
    }

    pub fn get_input_state(&self) -> &InputState {
        &self.input_state
    }

    pub fn handle_keyboard_event(&mut self, scancode: u8, pressed: bool) {
        if let Some(key) = scancode_to_egui_key(scancode) {
            self.egui_input.events.push(egui::Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers: egui::Modifiers::default(),
            });
        }
    }

    pub fn get_egui_context(&self) -> &Context {
        &self.egui_ctx
    }

    pub fn get_egui_input(&self) -> &RawInput {
        &self.egui_input
    }

    pub fn clear_egui_events(&mut self) {
        self.egui_input.events.clear();
    }

    pub fn render_frame(&mut self, screen_width: u32, screen_height: u32) -> Vec<u32> {
        let mut framebuffer = alloc::vec![0u32; (screen_width * screen_height) as usize];

        for window in self.windows.values() {
            if !window.visible {
                continue;
            }

            self.render_window_frame(&mut framebuffer, window, screen_width, screen_height);
        }

        self.render_cursor(&mut framebuffer, screen_width, screen_height);

        framebuffer
    }

    fn render_window_frame(
        &self,
        framebuffer: &mut [u32],
        window: &Window,
        screen_width: u32,
        screen_height: u32,
    ) {
        let border_color = if window.focused { 0xFF4080FF } else { 0xFF808080 };
        let title_bar_height = 24;

        for dy in 0..title_bar_height {
            for dx in 0..window.width {
                let screen_x = window.x + dx as i32;
                let screen_y = window.y + dy as i32 - title_bar_height as i32;

                if screen_x >= 0
                    && screen_x < screen_width as i32
                    && screen_y >= 0
                    && screen_y < screen_height as i32
                {
                    let idx = (screen_y as u32 * screen_width + screen_x as u32) as usize;
                    if idx < framebuffer.len() {
                        framebuffer[idx] = border_color;
                    }
                }
            }
        }

        let border_width = 2;
        for dy in 0..window.height {
            for dx in 0..border_width {
                let positions = [
                    (window.x + dx as i32, window.y + dy as i32),
                    (
                        window.x + window.width as i32 - dx as i32 - 1,
                        window.y + dy as i32,
                    ),
                ];

                for (screen_x, screen_y) in positions {
                    if screen_x >= 0
                        && screen_x < screen_width as i32
                        && screen_y >= 0
                        && screen_y < screen_height as i32
                    {
                        let idx = (screen_y as u32 * screen_width + screen_x as u32) as usize;
                        if idx < framebuffer.len() {
                            framebuffer[idx] = border_color;
                        }
                    }
                }
            }
        }

        for dx in 0..window.width {
            for dy in 0..border_width {
                let positions = [
                    (window.x + dx as i32, window.y + dy as i32),
                    (
                        window.x + dx as i32,
                        window.y + window.height as i32 - dy as i32 - 1,
                    ),
                ];

                for (screen_x, screen_y) in positions {
                    if screen_x >= 0
                        && screen_x < screen_width as i32
                        && screen_y >= 0
                        && screen_y < screen_height as i32
                    {
                        let idx = (screen_y as u32 * screen_width + screen_x as u32) as usize;
                        if idx < framebuffer.len() {
                            framebuffer[idx] = border_color;
                        }
                    }
                }
            }
        }

        for dy in border_width..(window.height - border_width) {
            for dx in border_width..(window.width - border_width) {
                let screen_x = window.x + dx as i32;
                let screen_y = window.y + dy as i32;

                if screen_x >= 0
                    && screen_x < screen_width as i32
                    && screen_y >= 0
                    && screen_y < screen_height as i32
                {
                    let idx = (screen_y as u32 * screen_width + screen_x as u32) as usize;
                    if idx < framebuffer.len() {
                        framebuffer[idx] = 0xFFFFFFFF;
                    }
                }
            }
        }
    }

    fn render_cursor(&self, framebuffer: &mut [u32], screen_width: u32, screen_height: u32) {
        let cursor_size = 12;
        let cursor_color = 0xFF000000;

        for dy in 0..cursor_size {
            for dx in 0..cursor_size {
                if dx > dy {
                    continue;
                }

                let screen_x = self.input_state.mouse_x + dx as i32;
                let screen_y = self.input_state.mouse_y + dy as i32;

                if screen_x >= 0
                    && screen_x < screen_width as i32
                    && screen_y >= 0
                    && screen_y < screen_height as i32
                {
                    let idx = (screen_y as u32 * screen_width + screen_x as u32) as usize;
                    if idx < framebuffer.len() {
                        framebuffer[idx] = cursor_color;
                    }
                }
            }
        }
    }
}

fn scancode_to_egui_key(scancode: u8) -> Option<egui::Key> {
    match scancode {
        0x01 => Some(egui::Key::Escape),
        0x02..=0x0B => Some(egui::Key::Num1),
        0x0E => Some(egui::Key::Backspace),
        0x0F => Some(egui::Key::Tab),
        0x1C => Some(egui::Key::Enter),
        0x1D => None,
        0x2A => None,
        0x36 => None,
        0x38 => None,
        0x39 => Some(egui::Key::Space),
        0x3B..=0x44 => Some(egui::Key::F1),
        0x47 => Some(egui::Key::Home),
        0x48 => Some(egui::Key::ArrowUp),
        0x49 => Some(egui::Key::PageUp),
        0x4B => Some(egui::Key::ArrowLeft),
        0x4D => Some(egui::Key::ArrowRight),
        0x4F => Some(egui::Key::End),
        0x50 => Some(egui::Key::ArrowDown),
        0x51 => Some(egui::Key::PageDown),
        0x52 => Some(egui::Key::Insert),
        0x53 => Some(egui::Key::Delete),
        _ => None,
    }
}

impl Default for DisplayServer {
    fn default() -> Self {
        Self::new()
    }
}
