//! Pure window-management and software-composition model.
//!
//! Ported from the `userspace/display_server` prototype, dropping its `egui`
//! binary lifecycle (the `Context`/`RawInput` plumbing and scancode→egui-key
//! table). What survives is the part worth keeping for the DGUI reference: a
//! deterministic, headless model of a window stack with a focus policy,
//! top-most hit-testing, click-drag movement and a CPU compositor that paints
//! into an ARGB `Vec<u32>` — no framebuffer hardware, no wall clock.
//!
//! Coordinates are screen-space pixels. A window's *body* rectangle is
//! `[x, x+width) × [y, y+height)`; its title bar is drawn in the
//! [`TITLE_BAR_HEIGHT`] rows immediately above `y` and is not part of the
//! hit-test rectangle (matching the prototype).

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

pub type Pid = u32;
pub type WindowId = u32;
/// Opaque handle to a client-provided buffer (a DGUI buffer object id in the
/// full server; an arbitrary token here).
pub type BufferRef = u64;

/// Left mouse button mask in the `buttons` bitfield of [`WindowManager::handle_mouse`].
pub const MOUSE_LEFT: u8 = 0x01;

/// Compositor geometry and palette (ARGB8888, alpha in the high byte).
pub const BORDER_WIDTH: u32 = 2;
pub const TITLE_BAR_HEIGHT: u32 = 24;
pub const CURSOR_SIZE: u32 = 12;
pub const COLOR_FOCUSED_BORDER: u32 = 0xFF40_80FF;
pub const COLOR_UNFOCUSED_BORDER: u32 = 0xFF80_8080;
pub const COLOR_CONTENT: u32 = 0xFFFF_FFFF;
pub const COLOR_CURSOR: u32 = 0xFF00_0000;

/// A managed top-level window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    pub id: WindowId,
    pub owner_pid: Pid,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub buffer: BufferRef,
    pub visible: bool,
    pub focused: bool,
}

impl Window {
    fn new(id: WindowId, owner_pid: Pid, x: i32, y: i32, width: u32, height: u32, buffer: BufferRef) -> Self {
        Self { id, owner_pid, x, y, width, height, buffer, visible: true, focused: false }
    }

    /// Whether the window *body* contains the screen point (title bar excluded).
    fn body_contains(&self, x: i32, y: i32) -> bool {
        x >= self.x
            && x < self.x + self.width as i32
            && y >= self.y
            && y < self.y + self.height as i32
    }
}

/// Pointer/drag interaction state, kept separate from the window map.
#[derive(Debug, Clone, Copy, Default)]
pub struct Pointer {
    pub x: i32,
    pub y: i32,
    pub buttons: u8,
    pub dragging: Option<WindowId>,
    pub drag_offset_x: i32,
    pub drag_offset_y: i32,
}

/// A deterministic window stack with focus, hit-testing and a CPU compositor.
///
/// Stacking order is the ascending [`WindowId`] order of the map: a
/// higher-id window is drawn later (on top) and is hit-tested first.
pub struct WindowManager {
    windows: BTreeMap<WindowId, Window>,
    focused: Option<WindowId>,
    next_id: WindowId,
    pointer: Pointer,
}

impl Default for WindowManager {
    fn default() -> Self {
        Self::new()
    }
}

impl WindowManager {
    pub fn new() -> Self {
        Self { windows: BTreeMap::new(), focused: None, next_id: 1, pointer: Pointer::default() }
    }

    /// Create a visible window owned by `owner_pid`. The first window created
    /// (while nothing is focused) receives focus automatically.
    pub fn create_window(&mut self, owner_pid: Pid, x: i32, y: i32, width: u32, height: u32, buffer: BufferRef) -> WindowId {
        let id = self.next_id;
        self.next_id += 1;
        self.windows.insert(id, Window::new(id, owner_pid, x, y, width, height, buffer));

        if self.focused.is_none() {
            self.focused = Some(id);
            if let Some(w) = self.windows.get_mut(&id) {
                w.focused = true;
            }
        }
        id
    }

    /// Destroy a window. If it held focus, focus falls to the lowest-id
    /// surviving window (deterministic), else clears.
    pub fn destroy_window(&mut self, id: WindowId) -> bool {
        if self.windows.remove(&id).is_none() {
            return false;
        }
        if self.focused == Some(id) {
            self.focused = self.windows.keys().next().copied();
            if let Some(next) = self.focused {
                if let Some(w) = self.windows.get_mut(&next) {
                    w.focused = true;
                }
            }
        }
        true
    }

    pub fn get_window(&self, id: WindowId) -> Option<&Window> {
        self.windows.get(&id)
    }

    pub fn get_window_mut(&mut self, id: WindowId) -> Option<&mut Window> {
        self.windows.get_mut(&id)
    }

    /// Windows in ascending id (== bottom-to-top) order.
    pub fn windows(&self) -> impl Iterator<Item = &Window> {
        self.windows.values()
    }

    pub fn window_count(&self) -> usize {
        self.windows.len()
    }

    pub fn focused_window(&self) -> Option<WindowId> {
        self.focused
    }

    pub fn pointer(&self) -> &Pointer {
        &self.pointer
    }

    /// Move focus to `id`, clearing the previous holder. Returns false for an
    /// unknown window (focus unchanged).
    pub fn set_focus(&mut self, id: WindowId) -> bool {
        if !self.windows.contains_key(&id) {
            return false;
        }
        if let Some(old) = self.focused {
            if let Some(w) = self.windows.get_mut(&old) {
                w.focused = false;
            }
        }
        self.focused = Some(id);
        if let Some(w) = self.windows.get_mut(&id) {
            w.focused = true;
        }
        true
    }

    /// Top-most visible window whose body contains the point, if any.
    pub fn window_at(&self, x: i32, y: i32) -> Option<WindowId> {
        self.windows
            .iter()
            .rev()
            .find(|(_, w)| w.visible && w.body_contains(x, y))
            .map(|(&id, _)| id)
    }

    /// Feed a pointer sample. A fresh left-press over a window focuses it and
    /// begins a drag; while held the dragged window tracks the pointer; release
    /// ends the drag.
    pub fn handle_mouse(&mut self, x: i32, y: i32, buttons: u8) {
        let left = buttons & MOUSE_LEFT != 0;
        let was_left = self.pointer.buttons & MOUSE_LEFT != 0;
        self.pointer.x = x;
        self.pointer.y = y;

        if left && !was_left {
            if let Some(id) = self.window_at(x, y) {
                self.set_focus(id);
                if let Some(w) = self.windows.get(&id) {
                    self.pointer.dragging = Some(id);
                    self.pointer.drag_offset_x = x - w.x;
                    self.pointer.drag_offset_y = y - w.y;
                }
            }
        } else if !left && was_left {
            self.pointer.dragging = None;
        }

        if left {
            if let Some(id) = self.pointer.dragging {
                let (ox, oy) = (self.pointer.drag_offset_x, self.pointer.drag_offset_y);
                if let Some(w) = self.windows.get_mut(&id) {
                    w.x = x - ox;
                    w.y = y - oy;
                }
            }
        }
        self.pointer.buttons = buttons;
    }

    /// Composite the current scene into a fresh `screen_width * screen_height`
    /// ARGB8888 buffer: visible windows bottom-to-top, then the cursor on top.
    pub fn render_frame(&self, screen_width: u32, screen_height: u32) -> Vec<u32> {
        let mut fb = alloc::vec![0u32; (screen_width * screen_height) as usize];
        for w in self.windows.values() {
            if w.visible {
                self.paint_window(&mut fb, w, screen_width, screen_height);
            }
        }
        self.paint_cursor(&mut fb, screen_width, screen_height);
        fb
    }

    fn paint_window(&self, fb: &mut [u32], w: &Window, sw: u32, sh: u32) {
        let border = if w.focused { COLOR_FOCUSED_BORDER } else { COLOR_UNFOCUSED_BORDER };
        let tbh = TITLE_BAR_HEIGHT as i32;

        // Title bar: `TITLE_BAR_HEIGHT` rows directly above the body.
        for dy in 0..TITLE_BAR_HEIGHT {
            for dx in 0..w.width {
                put(fb, sw, sh, w.x + dx as i32, w.y + dy as i32 - tbh, border);
            }
        }
        // Left/right body borders.
        for dy in 0..w.height {
            for dx in 0..BORDER_WIDTH {
                put(fb, sw, sh, w.x + dx as i32, w.y + dy as i32, border);
                put(fb, sw, sh, w.x + w.width as i32 - dx as i32 - 1, w.y + dy as i32, border);
            }
        }
        // Top/bottom body borders.
        for dx in 0..w.width {
            for dy in 0..BORDER_WIDTH {
                put(fb, sw, sh, w.x + dx as i32, w.y + dy as i32, border);
                put(fb, sw, sh, w.x + dx as i32, w.y + w.height as i32 - dy as i32 - 1, border);
            }
        }
        // Interior fill (guard against windows smaller than the border frame).
        if w.width > 2 * BORDER_WIDTH && w.height > 2 * BORDER_WIDTH {
            for dy in BORDER_WIDTH..(w.height - BORDER_WIDTH) {
                for dx in BORDER_WIDTH..(w.width - BORDER_WIDTH) {
                    put(fb, sw, sh, w.x + dx as i32, w.y + dy as i32, COLOR_CONTENT);
                }
            }
        }
    }

    fn paint_cursor(&self, fb: &mut [u32], sw: u32, sh: u32) {
        for dy in 0..CURSOR_SIZE {
            for dx in 0..CURSOR_SIZE {
                if dx <= dy {
                    put(fb, sw, sh, self.pointer.x + dx as i32, self.pointer.y + dy as i32, COLOR_CURSOR);
                }
            }
        }
    }
}

/// Write one pixel if it lands on-screen (silently clips otherwise).
fn put(fb: &mut [u32], sw: u32, sh: u32, x: i32, y: i32, color: u32) {
    if x < 0 || y < 0 || x >= sw as i32 || y >= sh as i32 {
        return;
    }
    let idx = (y as u32 * sw + x as u32) as usize;
    if idx < fb.len() {
        fb[idx] = color;
    }
}
