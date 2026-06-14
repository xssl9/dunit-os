use crate::gui::renderer::{BackBuffer, DamageTracker, Framebuffer, Rect};
use crate::drivers::{keyboard, mouse};
use crate::fs::vfs::{self, OpenFlags};
use crate::serial_write;
use crate::window_manager::{self, AppType};
use alloc::string::String;
use alloc::vec::Vec;

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
const WALLPAPER_WIDTH: usize = 1600;
const WALLPAPER_HEIGHT: usize = 900;
const WALLPAPER_OFFSET: usize = 54;
const WALLPAPER_STRIDE: usize = WALLPAPER_WIDTH * 3;
const WALLPAPER_PATH: &str = "/assets/wallpaper.bmp";
const GUI_PING_PATH: &str = "/app/gui_ping";
const GUI_PING_MESSAGE: &[u8] = b"gui_ping: hello from userspace";
const GUI_BRIDGE_MESSAGE_CAP: usize = 96;
const GUI_TERMINAL_STUB_PATH: &str = "/app/gui_terminal_stub";
const GUI_MSG_MAGIC: u32 = 0x3149_5547;
const GUI_MSG_VERSION: u16 = 1;
const GUI_MSG_CREATE_WINDOW: u16 = 1;
const GUI_MSG_DRAW_TEXT: u16 = 2;
const GUI_MSG_SET_STATUS: u16 = 3;
const GUI_MSG_EXIT: u16 = 4;
const GUI_MSG_COMMAND: u16 = 5;
const GUI_MSG_KEY_EVENT: u16 = 101;
const GUI_MSG_CLOSE_EVENT: u16 = 102;
const GUI_MSG_DATA_CAP: usize = 160;
const GUI_APP_LINES: usize = 8;
const GUI_APP_TEXT_CAP: usize = 96;
const GUI_APP_TITLE_CAP: usize = 32;
const GUI_APP_CWD_CAP: usize = 128;
const ICON_SIZE: usize = 44;
const TERMINAL_ICON: &[u8] = include_bytes!("../assets/terminal.rgba");
const TEXT_ICON: &[u8] = include_bytes!("../assets/text.rgba");
const MONITOR_ICON: &[u8] = include_bytes!("../assets/monitor.rgba");
const DOCK_APPS: [(AppType, u32, &'static str); 3] = [
    (AppType::Terminal, GREEN, "Term"),
    (AppType::Monitor, ORANGE, "Stats"),
    (AppType::Editor, PURPLE, "Edit"),
];
const MAX_BLUR_WIDTH: usize = 1920;
const MAX_BLUR_HEIGHT: usize = 1080;
const MAX_BLUR_PIXELS: usize = MAX_BLUR_WIDTH * MAX_BLUR_HEIGHT;
const BLUR_RADIUS: usize = 4;
const BLUR_WEIGHTS: [u32; BLUR_RADIUS * 2 + 1] = [1, 4, 10, 16, 19, 16, 10, 4, 1];
const BLUR_WEIGHT_SUM: u32 = 81;

static mut BLUR_TEMP: [u32; MAX_BLUR_PIXELS] = [0; MAX_BLUR_PIXELS];
static mut BLUR_CACHE: [u32; MAX_BLUR_PIXELS] = [0; MAX_BLUR_PIXELS];
static mut BLUR_CACHE_WIDTH: usize = 0;
static mut BLUR_CACHE_HEIGHT: usize = 0;
static mut BLUR_CACHE_READY: bool = false;
static mut WALLPAPER_READY: bool = false;

#[derive(Clone, Copy)]
struct UiState {
    launcher_open: bool,
    quick_open: bool,
    notifications_open: bool,
    brightness: u8,
    keyboard_extended: bool,
    gui_app_needs_run: bool,
    terminal_bridge: GuiRuntimeBridge,
    gui_app: GuiAppRuntime,
}

#[derive(Clone, Copy)]
struct GuiRuntimeBridge {
    attempted: bool,
    launched: bool,
    ok: bool,
    pid: u64,
    exit_code: i32,
    message: [u8; GUI_BRIDGE_MESSAGE_CAP],
    message_len: usize,
    error: &'static str,
}

impl GuiRuntimeBridge {
    const fn new() -> Self {
        Self {
            attempted: false,
            launched: false,
            ok: false,
            pid: 0,
            exit_code: 0,
            message: [0; GUI_BRIDGE_MESSAGE_CAP],
            message_len: 0,
            error: "",
        }
    }

    fn set_error(&mut self, error: &'static str) {
        self.attempted = true;
        self.ok = false;
        self.error = error;
    }

    fn set_message(&mut self, data: &[u8]) {
        let len = data.len().min(self.message.len());
        self.message[..len].copy_from_slice(&data[..len]);
        self.message_len = len;
    }

    fn message_str(&self) -> &str {
        core::str::from_utf8(&self.message[..self.message_len]).unwrap_or("<invalid utf8>")
    }
}

#[derive(Clone, Copy)]
struct GuiTextLine {
    x: i32,
    y: i32,
    len: usize,
    data: [u8; GUI_APP_TEXT_CAP],
}

impl GuiTextLine {
    const fn empty() -> Self {
        Self {
            x: 0,
            y: 0,
            len: 0,
            data: [0; GUI_APP_TEXT_CAP],
        }
    }

    fn set(&mut self, x: i32, y: i32, data: &[u8]) {
        self.x = x;
        self.y = y;
        self.len = data.len().min(self.data.len());
        self.data[..self.len].copy_from_slice(&data[..self.len]);
    }

    fn text(&self) -> &str {
        core::str::from_utf8(&self.data[..self.len]).unwrap_or("<invalid utf8>")
    }
}

#[derive(Clone, Copy)]
struct GuiAppRuntime {
    launched: bool,
    running: bool,
    exited: bool,
    pid: u64,
    window_id: u32,
    owner_pid: u64,
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    title_len: usize,
    title: [u8; GUI_APP_TITLE_CAP],
    status_len: usize,
    status: [u8; GUI_APP_TEXT_CAP],
    lines: [GuiTextLine; GUI_APP_LINES],
    line_count: usize,
    cwd: [u8; GUI_APP_CWD_CAP],
    cwd_len: usize,
}

impl GuiAppRuntime {
    const fn new() -> Self {
        Self {
            launched: false,
            running: false,
            exited: false,
            pid: 0,
            window_id: 0,
            owner_pid: 0,
            x: 0,
            y: 0,
            width: 420,
            height: 260,
            title_len: 0,
            title: [0; GUI_APP_TITLE_CAP],
            status_len: 0,
            status: [0; GUI_APP_TEXT_CAP],
            lines: [GuiTextLine::empty(); GUI_APP_LINES],
            line_count: 0,
            cwd: [0; GUI_APP_CWD_CAP],
            cwd_len: 0,
        }
    }

    fn set_title(&mut self, data: &[u8]) {
        self.title_len = data.len().min(self.title.len());
        self.title[..self.title_len].copy_from_slice(&data[..self.title_len]);
    }

    fn title(&self) -> &str {
        core::str::from_utf8(&self.title[..self.title_len]).unwrap_or("GUI App")
    }

    fn set_status(&mut self, data: &[u8]) {
        self.status_len = data.len().min(self.status.len());
        self.status[..self.status_len].copy_from_slice(&data[..self.status_len]);
    }

    fn status(&self) -> &str {
        core::str::from_utf8(&self.status[..self.status_len]).unwrap_or("<invalid status>")
    }

    fn push_line(&mut self, x: i32, y: i32, data: &[u8]) {
        let mut index = 0usize;
        while index < self.line_count {
            if self.lines[index].x == x && self.lines[index].y == y {
                self.lines[index].set(x, y, data);
                return;
            }
            index += 1;
        }
        if self.line_count < self.lines.len() {
            self.lines[self.line_count].set(x, y, data);
            self.line_count += 1;
            return;
        }
        let mut index = 1usize;
        while index < self.lines.len() {
            self.lines[index - 1] = self.lines[index];
            index += 1;
        }
        self.lines[self.lines.len() - 1].set(x, y, data);
    }

    fn cwd(&self) -> &str {
        if self.cwd_len == 0 {
            "/"
        } else {
            core::str::from_utf8(&self.cwd[..self.cwd_len]).unwrap_or("/")
        }
    }

    fn set_cwd(&mut self, cwd: &str) {
        self.cwd_len = cwd.len().min(self.cwd.len());
        self.cwd[..self.cwd_len].copy_from_slice(&cwd.as_bytes()[..self.cwd_len]);
    }

    fn reset_window(&mut self) {
        self.launched = false;
        self.running = false;
        self.exited = false;
        self.pid = 0;
        self.window_id = 0;
        self.owner_pid = 0;
        self.x = 0;
        self.y = 0;
        self.width = 420;
        self.height = 260;
        self.title_len = 0;
        self.status_len = 0;
        self.line_count = 0;
        self.cwd_len = 0;
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct GuiMessage {
    magic: u32,
    version: u16,
    kind: u16,
    window_id: u32,
    a: i32,
    b: i32,
    c: u32,
    len: u32,
    data: [u8; GUI_MSG_DATA_CAP],
}

impl UiState {
    const fn new() -> Self {
        Self {
            launcher_open: false,
            quick_open: true,
            notifications_open: true,
            brightness: 80,
            keyboard_extended: false,
            gui_app_needs_run: false,
            terminal_bridge: GuiRuntimeBridge::new(),
            gui_app: GuiAppRuntime::new(),
        }
    }
}

fn rects_intersect(a: Rect, b: Rect) -> bool {
    a.x < b.right() && a.right() > b.x && a.y < b.bottom() && a.bottom() > b.y
}

#[derive(Clone, Copy)]
enum UiAction {
    ToggleLauncher,
    ToggleQuick,
    ToggleNotifications,
    SetBrightness(u8),
    ToggleApp(AppType),
}

#[derive(Clone, Copy)]
enum PointerOp {
    GuiAppDrag {
        offset_x: usize,
        offset_y: usize,
    },
    Drag {
        idx: usize,
        offset_x: usize,
        offset_y: usize,
    },
    Resize {
        idx: usize,
        offset_x: usize,
        offset_y: usize,
    },
}

fn validate_wallpaper_bmp(data: &[u8]) -> bool {
    if data.len() < WALLPAPER_OFFSET + WALLPAPER_STRIDE * WALLPAPER_HEIGHT {
        return false;
    }

    data[0] == b'B'
        && data[1] == b'M'
        && data.get(10).copied() == Some(WALLPAPER_OFFSET as u8)
        && data.get(18).copied() == Some((WALLPAPER_WIDTH & 0xff) as u8)
        && data.get(19).copied() == Some(((WALLPAPER_WIDTH >> 8) & 0xff) as u8)
        && data.get(22).copied() == Some((WALLPAPER_HEIGHT & 0xff) as u8)
        && data.get(23).copied() == Some(((WALLPAPER_HEIGHT >> 8) & 0xff) as u8)
        && data.get(28).copied() == Some(24)
}

fn load_wallpaper() {
    unsafe {
        if WALLPAPER_READY {
            return;
        }
    }

    if let Some(data) = vfs::static_file(WALLPAPER_PATH) {
        if validate_wallpaper_bmp(data) {
            unsafe {
                WALLPAPER_READY = true;
            }
            serial_write("[GUI] wallpaper loaded from VFS\r\n");
            return;
        }

        serial_write("[GUI] wallpaper VFS asset has invalid BMP format\r\n");
    } else {
        serial_write("[GUI] wallpaper VFS asset missing\r\n");
    }

    unsafe {
        WALLPAPER_READY = false;
    }
}

fn wallpaper_bytes() -> Option<&'static [u8]> {
    unsafe {
        if WALLPAPER_READY {
            vfs::static_file(WALLPAPER_PATH)
        } else {
            None
        }
    }
}

fn serial_write_u64(mut value: u64) {
    let mut buf = [0u8; 20];
    let mut index = buf.len();
    if value == 0 {
        serial_write("0");
        return;
    }
    while value > 0 {
        index -= 1;
        buf[index] = b'0' + (value % 10) as u8;
        value /= 10;
    }
    let text = core::str::from_utf8(&buf[index..]).unwrap_or("?");
    serial_write(text);
}

fn serial_write_i32(value: i32) {
    if value < 0 {
        serial_write("-");
        serial_write_u64(value.saturating_abs() as u64);
    } else {
        serial_write_u64(value as u64);
    }
}

fn append_str(out: &mut [u8], len: &mut usize, text: &str) {
    for byte in text.bytes() {
        if *len >= out.len() {
            return;
        }
        out[*len] = byte;
        *len += 1;
    }
}

fn append_u64(out: &mut [u8], len: &mut usize, mut value: u64) {
    let mut digits = [0u8; 20];
    let mut count = 0usize;
    if value == 0 {
        digits[0] = b'0';
        count = 1;
    } else {
        while value > 0 {
            digits[count] = b'0' + (value % 10) as u8;
            count += 1;
            value /= 10;
        }
    }
    while count > 0 {
        count -= 1;
        if *len >= out.len() {
            return;
        }
        out[*len] = digits[count];
        *len += 1;
    }
}

fn append_i32(out: &mut [u8], len: &mut usize, value: i32) {
    if value < 0 {
        append_str(out, len, "-");
        append_u64(out, len, value.saturating_abs() as u64);
    } else {
        append_u64(out, len, value as u64);
    }
}

fn line_str(buf: &[u8], len: usize) -> &str {
    core::str::from_utf8(&buf[..len]).unwrap_or("<invalid utf8>")
}

fn read_vfs_file(path: &str) -> Option<Vec<u8>> {
    let vfs = vfs::get_vfs()?;
    let fd = vfs.open(path, OpenFlags::READ).ok()?;
    let mut data = Vec::new();
    let mut chunk = [0u8; 4096];

    loop {
        let read = match vfs.read(fd, &mut chunk) {
            Ok(read) => read,
            Err(_) => {
                let _ = vfs.close(fd);
                return None;
            }
        };
        if read == 0 {
            break;
        }
        data.extend_from_slice(&chunk[..read]);
    }

    let _ = vfs.close(fd);
    Some(data)
}

fn launch_terminal_bridge(state: &mut UiState) {
    if state.terminal_bridge.attempted {
        return;
    }
    state.terminal_bridge.attempted = true;
    serial_write("[GUI-BRIDGE] launching /app/gui_ping\r\n");

    let Some(data) = read_vfs_file(GUI_PING_PATH) else {
        serial_write("[GUI-BRIDGE] failed: /app/gui_ping missing\r\n");
        state.terminal_bridge.set_error("missing /app/gui_ping");
        return;
    };

    let pid = match crate::process::create_user_process_record(String::from(GUI_PING_PATH), true) {
        Ok(pid) => pid,
        Err(_) => {
            serial_write("[GUI-BRIDGE] failed: process create\r\n");
            state.terminal_bridge.set_error("process create failed");
            return;
        }
    };

    let argv = [String::from("gui_ping")];
    if crate::elf::prepare_process_elf(pid, &data, &argv).is_err() {
        serial_write("[GUI-BRIDGE] failed: ELF prepare\r\n");
        let _ = crate::process::autoreap_process(pid, "gui-bridge-prepare-failed");
        state.terminal_bridge.set_error("ELF prepare failed");
        return;
    }

    state.terminal_bridge.launched = true;
    state.terminal_bridge.pid = pid.0;
    serial_write("[GUI-BRIDGE] launched /app/gui_ping pid=");
    serial_write_u64(pid.0);
    serial_write("\r\n");

    let exit = match crate::process::enter_user_process(pid) {
        Ok(exit) => exit,
        Err(_) => {
            serial_write("[GUI-BRIDGE] failed: process run\r\n");
            let _ = crate::process::autoreap_process(pid, "gui-bridge-run-failed");
            state.terminal_bridge.set_error("process run failed");
            return;
        }
    };

    match exit.status {
        crate::process::ProcessExitStatus::Exited(code) => {
            state.terminal_bridge.exit_code = code;
            serial_write("[GUI-BRIDGE] exit code=");
            serial_write_i32(code);
            serial_write("\r\n");
        }
        _ => {
            serial_write("[GUI-BRIDGE] failed: process faulted\r\n");
            let _ = crate::process::autoreap_process(pid, "gui-bridge-faulted");
            state.terminal_bridge.set_error("process did not exit cleanly");
            return;
        }
    }

    let mut msg = [0u8; GUI_BRIDGE_MESSAGE_CAP];
    let len = match crate::ipc::recv_bytes(crate::process::ProcessId(1), &mut msg) {
        Ok(len) => len,
        Err(_) => {
            serial_write("[GUI-BRIDGE] failed: no IPC message\r\n");
            let _ = crate::process::autoreap_process(pid, "gui-bridge-no-message");
            state.terminal_bridge.set_error("no IPC message from gui_ping");
            return;
        }
    };

    state.terminal_bridge.set_message(&msg[..len]);
    serial_write("[GUI-BRIDGE] message: ");
    serial_write(state.terminal_bridge.message_str());
    serial_write("\r\n");

    if &msg[..len] != GUI_PING_MESSAGE {
        let _ = crate::process::autoreap_process(pid, "gui-bridge-message-mismatch");
        state.terminal_bridge.set_error("IPC message mismatch");
        return;
    }

    let _ = crate::process::autoreap_process(pid, "gui-bridge");
    state.terminal_bridge.ok = state.terminal_bridge.exit_code == 0;
    if state.terminal_bridge.ok {
        serial_write("[GUI-BRIDGE] Terminal runtime bridge OK\r\n");
    } else {
        state.terminal_bridge.set_error("gui_ping exit code was nonzero");
    }
}

fn gui_message_from_bytes(data: &[u8]) -> Option<GuiMessage> {
    if data.len() != core::mem::size_of::<GuiMessage>() {
        return None;
    }
    let message = unsafe { core::ptr::read_unaligned(data.as_ptr() as *const GuiMessage) };
    if message.magic != GUI_MSG_MAGIC
        || message.version != GUI_MSG_VERSION
        || (message.len as usize) > GUI_MSG_DATA_CAP
    {
        return None;
    }
    Some(message)
}

fn gui_message_data(message: &GuiMessage) -> &[u8] {
    &message.data[..(message.len as usize).min(GUI_MSG_DATA_CAP)]
}

fn gui_message_text(message: &GuiMessage) -> &str {
    core::str::from_utf8(gui_message_data(message)).unwrap_or("<invalid utf8>")
}

fn send_gui_event(app: &GuiAppRuntime, kind: u16, key: u8) -> bool {
    if app.pid == 0 || app.window_id == 0 || !app.running {
        return false;
    }

    let mut message = GuiMessage {
        magic: GUI_MSG_MAGIC,
        version: GUI_MSG_VERSION,
        kind,
        window_id: app.window_id,
        a: key as i32,
        b: 0,
        c: 0,
        len: 0,
        data: [0; GUI_MSG_DATA_CAP],
    };
    let bytes = unsafe {
        core::slice::from_raw_parts(
            &mut message as *mut GuiMessage as *const u8,
            core::mem::size_of::<GuiMessage>(),
        )
    };

    match crate::ipc::send_bytes(crate::process::ProcessId(1), crate::process::ProcessId(app.pid), bytes) {
        Ok(()) => {
            if kind == GUI_MSG_KEY_EVENT {
                serial_write("[GUI-PROTO] send KEY_EVENT pid=");
            } else {
                serial_write("[GUI-PROTO] send CLOSE_EVENT pid=");
            }
            serial_write_u64(app.pid);
            serial_write(" window_id=");
            serial_write_u64(app.window_id as u64);
            if kind == GUI_MSG_KEY_EVENT {
                serial_write(" key=");
                if key == b'\n' {
                    serial_write("Enter");
                } else if key == 8 {
                    serial_write("Backspace");
                } else {
                    let text = [key];
                    serial_write(core::str::from_utf8(&text).unwrap_or("?"));
                }
            }
            serial_write("\r\n");
            true
        }
        Err(_) => {
            if kind == GUI_MSG_KEY_EVENT {
                serial_write("[GUI-PROTO] failed to send KEY_EVENT\r\n");
            } else {
                serial_write("[GUI-PROTO] failed to send CLOSE_EVENT\r\n");
            }
            false
        }
    }
}

fn send_gui_key_event(app: &GuiAppRuntime, key: u8) -> bool {
    send_gui_event(app, GUI_MSG_KEY_EVENT, key)
}

fn send_gui_close_event(app: &GuiAppRuntime) -> bool {
    send_gui_event(app, GUI_MSG_CLOSE_EVENT, 0)
}

fn gui_app_window_rect(width: usize, height: usize, app: &GuiAppRuntime) -> Option<Rect> {
    if app.window_id == 0 || app.owner_pid == 0 {
        return None;
    }
    let window_width = app.width.max(300);
    let window_height = app.height.max(180);
    let max_x = width.saturating_sub(window_width);
    let max_y = height.saturating_sub(window_height);
    Some(Rect::new(app.x.min(max_x), app.y.min(max_y), window_width, window_height))
}

fn close_gui_app_window(state: &mut UiState) -> bool {
    if state.gui_app.pid == 0 || state.gui_app.window_id == 0 {
        state.gui_app.reset_window();
        return true;
    }
    if state.gui_app.running {
        serial_write("[GUI-PROTO] close requested window_id=");
        serial_write_u64(state.gui_app.window_id as u64);
        serial_write(" owner_pid=");
        serial_write_u64(state.gui_app.owner_pid);
        serial_write("\r\n");
        send_gui_close_event(&state.gui_app)
    } else {
        state.gui_app.reset_window();
        true
    }
}

fn begin_gui_app_drag(state: &UiState, mx: usize, my: usize, width: usize, height: usize) -> Option<PointerOp> {
    let rect = gui_app_window_rect(width, height, &state.gui_app)?;
    if inside(mx, my, rect.x + 12, rect.y + 11, 12, 12) {
        return None;
    }
    if inside(mx, my, rect.x, rect.y, rect.width, 34) {
        return Some(PointerOp::GuiAppDrag {
            offset_x: mx.saturating_sub(rect.x),
            offset_y: my.saturating_sub(rect.y),
        });
    }
    None
}

fn drag_gui_app_window(state: &mut UiState, mx: usize, my: usize, width: usize, height: usize, offset_x: usize, offset_y: usize) -> Option<(Rect, Rect)> {
    let old_rect = gui_app_window_rect(width, height, &state.gui_app)?;
    let max_x = width.saturating_sub(old_rect.width);
    let max_y = height.saturating_sub(old_rect.height);
    state.gui_app.x = mx.saturating_sub(offset_x).min(max_x);
    state.gui_app.y = my.saturating_sub(offset_y).min(max_y);
    let new_rect = gui_app_window_rect(width, height, &state.gui_app)?;
    Some((old_rect, new_rect))
}

fn gui_terminal_clear(app: &mut GuiAppRuntime) {
    app.line_count = 0;
    let mut index = 0usize;
    while index < app.lines.len() {
        app.lines[index] = GuiTextLine::empty();
        index += 1;
    }
}

fn gui_terminal_append_line(app: &mut GuiAppRuntime, text: &str) {
    let y_values = [0, 14, 28, 42, 56, 70, 84, 98];
    if app.line_count < app.lines.len() {
        let index = app.line_count;
        app.lines[index].set(0, y_values[index], text.as_bytes());
        app.line_count += 1;
        return;
    }
    let mut index = 1usize;
    while index < app.lines.len() {
        app.lines[index - 1] = app.lines[index];
        app.lines[index - 1].y = y_values[index - 1];
        index += 1;
    }
    let last = app.lines.len() - 1;
    app.lines[last].set(0, y_values[last], text.as_bytes());
}

fn gui_terminal_commit_command(app: &mut GuiAppRuntime, command: &str) {
    let mut line = [0u8; GUI_APP_TEXT_CAP];
    let mut len = 0usize;
    append_str(&mut line, &mut len, "root@dunit:# ");
    append_str(&mut line, &mut len, command);
    gui_terminal_append_bytes(app, &line[..len]);
}

fn gui_terminal_new_prompt(app: &mut GuiAppRuntime) {
    gui_terminal_append_line(app, "root@dunit:# ");
}

fn gui_terminal_update_prompt(app: &mut GuiAppRuntime, text: &str) {
    if app.line_count == 0 {
        gui_terminal_append_line(app, text);
        return;
    }
    let y = app.lines[app.line_count - 1].y;
    app.lines[app.line_count - 1].set(0, y, text.as_bytes());
}

fn gui_terminal_append_bytes(app: &mut GuiAppRuntime, bytes: &[u8]) {
    let text = core::str::from_utf8(bytes).unwrap_or("<invalid utf8>");
    gui_terminal_append_line(app, text);
}

fn gui_terminal_vfs_error(app: &mut GuiAppRuntime, command: &str, error: vfs::VfsError) {
    let mut line = [0u8; GUI_APP_TEXT_CAP];
    let mut len = 0usize;
    append_str(&mut line, &mut len, command);
    append_str(&mut line, &mut len, ": ");
    append_str(&mut line, &mut len, match error {
        vfs::VfsError::NotFound => "not found",
        vfs::VfsError::PermissionDenied => "permission denied",
        vfs::VfsError::InvalidDescriptor => "invalid descriptor",
        vfs::VfsError::AlreadyExists => "already exists",
        vfs::VfsError::NotADirectory => "not a directory",
        vfs::VfsError::IsADirectory => "is a directory",
        vfs::VfsError::InvalidPath => "invalid path",
        vfs::VfsError::Unsupported => "unsupported",
        vfs::VfsError::IoError => "io error",
    });
    gui_terminal_append_bytes(app, &line[..len]);
}

fn gui_terminal_append_process_state(out: &mut [u8], len: &mut usize, state: crate::process::ProcessState) {
    append_str(out, len, match state {
        crate::process::ProcessState::Prepared => "Prepared",
        crate::process::ProcessState::Ready => "Ready",
        crate::process::ProcessState::Running => "Running",
        crate::process::ProcessState::Blocked => "Blocked",
        crate::process::ProcessState::Dead => "Dead",
        crate::process::ProcessState::Reaped => "Reaped",
    });
}

fn execute_gui_terminal_command(state: &mut UiState, command: &str) {
    let trimmed = command.trim();
    serial_write("[GUI-TERM] command: ");
    serial_write(trimmed);
    serial_write("\r\n");
    if state.gui_app.line_count > 0 {
        state.gui_app.line_count -= 1;
    }
    gui_terminal_commit_command(&mut state.gui_app, trimmed);

    if trimmed.is_empty() {
        gui_terminal_new_prompt(&mut state.gui_app);
        return;
    }

    if trimmed == "clear" {
        gui_terminal_clear(&mut state.gui_app);
        gui_terminal_new_prompt(&mut state.gui_app);
        return;
    }

    if trimmed == "help" {
        gui_terminal_append_line(&mut state.gui_app, "Available commands:");
        gui_terminal_append_line(&mut state.gui_app, "help dufetch ls pwd cd ps clear exit");
        gui_terminal_append_line(&mut state.gui_app, "exec runs later when GUI stdout bridge exists");
        gui_terminal_new_prompt(&mut state.gui_app);
        return;
    }

    if trimmed == "dufetch" {
        gui_terminal_append_line(&mut state.gui_app, "Dunit OS 1.0.0 Green Tea");
        gui_terminal_append_line(&mut state.gui_app, "Mode: GUI userspace terminal MVP");
        gui_terminal_append_line(&mut state.gui_app, "Shell: kernel-backed GUI command bridge");
        gui_terminal_new_prompt(&mut state.gui_app);
        return;
    }

    if trimmed == "pwd" {
        let cwd = String::from(state.gui_app.cwd());
        gui_terminal_append_line(&mut state.gui_app, &cwd);
        gui_terminal_new_prompt(&mut state.gui_app);
        return;
    }

    if trimmed == "ps" || trimmed == "ps aux" {
        let mut records = Vec::new();
        crate::process::snapshot_processes(&mut records);
        gui_terminal_append_line(&mut state.gui_app, "PID  PPID  STATE  PATH");
        for record in records.iter().take(6) {
            let mut line = [0u8; GUI_APP_TEXT_CAP];
            let mut len = 0usize;
            append_u64(&mut line, &mut len, record.pid.0);
            append_str(&mut line, &mut len, "  ");
            match record.parent {
                Some(parent) => append_u64(&mut line, &mut len, parent.0),
                None => append_str(&mut line, &mut len, "-"),
            }
            append_str(&mut line, &mut len, "  ");
            gui_terminal_append_process_state(&mut line, &mut len, record.state);
            append_str(&mut line, &mut len, "  ");
            append_str(&mut line, &mut len, &record.path);
            gui_terminal_append_bytes(&mut state.gui_app, &line[..len]);
        }
        gui_terminal_new_prompt(&mut state.gui_app);
        return;
    }

    if trimmed == "ls" || trimmed.starts_with("ls ") {
        let arg = trimmed.strip_prefix("ls").unwrap_or("").trim();
        let path = if arg.is_empty() { "." } else { arg };
        let cwd_string = String::from(state.gui_app.cwd());
        if let Some(vfs) = vfs::get_vfs() {
            let mut entries = [vfs::DirEntry::empty(); 32];
            match vfs.readdir_into_at(&cwd_string, path, &mut entries) {
                Ok(count) => {
                    let mut line = [0u8; GUI_APP_TEXT_CAP];
                    let mut len = 0usize;
                    for (idx, entry) in entries.iter().take(count).enumerate() {
                        if idx > 0 {
                            append_str(&mut line, &mut len, "  ");
                        }
                        append_str(&mut line, &mut len, entry.name());
                    }
                    gui_terminal_append_bytes(&mut state.gui_app, &line[..len]);
                }
                Err(error) => gui_terminal_vfs_error(&mut state.gui_app, "ls", error),
            }
        } else {
            gui_terminal_append_line(&mut state.gui_app, "ls: VFS not initialized");
        }
        gui_terminal_new_prompt(&mut state.gui_app);
        return;
    }

    if trimmed == "cd" || trimmed.starts_with("cd ") {
        let arg = trimmed.strip_prefix("cd").unwrap_or("").trim();
        let path = if arg.is_empty() { "/" } else { arg };
        let cwd_string = String::from(state.gui_app.cwd());
        if let Some(vfs) = vfs::get_vfs() {
            match vfs.stat_at(&cwd_string, path) {
                Ok(stat) if stat.file_type == vfs::FileType::Directory => {
                    match vfs.normalize_at(&cwd_string, path) {
                        Ok(new_cwd) => state.gui_app.set_cwd(&new_cwd),
                        Err(error) => gui_terminal_vfs_error(&mut state.gui_app, "cd", error),
                    }
                }
                Ok(_) => gui_terminal_append_line(&mut state.gui_app, "cd: not a directory"),
                Err(error) => gui_terminal_vfs_error(&mut state.gui_app, "cd", error),
            }
        } else {
            gui_terminal_append_line(&mut state.gui_app, "cd: VFS not initialized");
        }
        gui_terminal_new_prompt(&mut state.gui_app);
        return;
    }

    if trimmed == "exec" || trimmed.starts_with("exec ") {
        gui_terminal_append_line(&mut state.gui_app, "exec: GUI stdout bridge not implemented yet");
        gui_terminal_new_prompt(&mut state.gui_app);
        return;
    }

    if trimmed == "exit" {
        return;
    }

    gui_terminal_append_line(&mut state.gui_app, "Command not found. Type 'help'.");
    gui_terminal_new_prompt(&mut state.gui_app);
}

fn process_gui_messages(state: &mut UiState) {
    loop {
        let mut raw = [0u8; 256];
        let len = match crate::ipc::recv_bytes(crate::process::ProcessId(1), &mut raw) {
            Ok(len) => len,
            Err(_) => break,
        };
        let Some(message) = gui_message_from_bytes(&raw[..len]) else {
            serial_write("[GUI-PROTO] ignored invalid IPC payload\r\n");
            continue;
        };
        let pid = state.gui_app.pid;
        match message.kind {
            GUI_MSG_CREATE_WINDOW => {
                if pid == 0 || message.window_id == 0 {
                    serial_write("[GUI-PROTO] ignored CREATE_WINDOW without live app\r\n");
                    continue;
                }
                state.gui_app.window_id = message.window_id;
                state.gui_app.owner_pid = pid;
                state.gui_app.width = (message.a.max(220) as usize).min(720);
                state.gui_app.height = (message.b.max(140) as usize).min(420);
                if state.gui_app.x == 0 && state.gui_app.y == 0 {
                    state.gui_app.x = 96;
                    state.gui_app.y = 72;
                }
                state.gui_app.set_title(gui_message_data(&message));
                serial_write("[GUI-PROTO] recv CREATE_WINDOW pid=");
                serial_write_u64(pid);
                serial_write(" window_id=");
                serial_write_u64(message.window_id as u64);
                serial_write(" title=");
                serial_write(gui_message_text(&message));
                serial_write("\r\n");
                serial_write("[GUI-PROTO] created window owner_pid=");
                serial_write_u64(pid);
                serial_write(" window_id=");
                serial_write_u64(message.window_id as u64);
                serial_write("\r\n");
            }
            GUI_MSG_DRAW_TEXT => {
                if message.window_id != state.gui_app.window_id || pid == 0 {
                    serial_write("[GUI-PROTO] ignored DRAW_TEXT for stale window\r\n");
                    continue;
                }
                let text = gui_message_text(&message);
                if text.starts_with("root@dunit:#") {
                    gui_terminal_update_prompt(&mut state.gui_app, text);
                } else {
                    state.gui_app.push_line(message.a, message.b, gui_message_data(&message));
                }
                serial_write("[GUI-PROTO] recv DRAW_TEXT pid=");
                serial_write_u64(pid);
                serial_write(" window_id=");
                serial_write_u64(message.window_id as u64);
                serial_write(" text=");
                serial_write(text);
                serial_write("\r\n");
                serial_write("[GUI-PROTO] render text window_id=");
                serial_write_u64(message.window_id as u64);
                serial_write("\r\n");
            }
            GUI_MSG_SET_STATUS => {
                state.gui_app.set_status(gui_message_data(&message));
                serial_write("[GUI-PROTO] recv SET_STATUS pid=");
                serial_write_u64(pid);
                serial_write(" text=");
                serial_write(gui_message_text(&message));
                serial_write("\r\n");
            }
            GUI_MSG_COMMAND => {
                if pid == 0 || message.window_id != state.gui_app.window_id {
                    serial_write("[GUI-PROTO] ignored COMMAND for stale window\r\n");
                    continue;
                }
                serial_write("[GUI-PROTO] recv COMMAND pid=");
                serial_write_u64(pid);
                serial_write(" text=");
                serial_write(gui_message_text(&message));
                serial_write("\r\n");
                execute_gui_terminal_command(state, gui_message_text(&message));
            }
            GUI_MSG_EXIT => {
                state.gui_app.running = false;
                state.gui_app.exited = true;
                serial_write("[GUI-PROTO] recv EXIT pid=");
                serial_write_u64(pid);
                serial_write("\r\n");
                state.gui_app.reset_window();
                serial_write("[GUI-PROTO] window closed after app EXIT\r\n");
            }
            _ => serial_write("[GUI-PROTO] ignored unknown message\r\n"),
        }
    }
}

fn run_gui_app_once(state: &mut UiState) {
    if state.gui_app.pid == 0 || state.gui_app.exited {
        return;
    }
    let pid = crate::process::ProcessId(state.gui_app.pid);
    let mut exited = false;
    match crate::process::enter_user_process(pid) {
        Ok(exit) => {
            state.gui_app.running = false;
            state.gui_app.exited = true;
            exited = true;
            serial_write("[GUI-PROTO] app exited pid=");
            serial_write_u64(pid.0);
            serial_write(" code=");
            serial_write_i32(exit.status.exit_code());
            serial_write("\r\n");
            let _ = crate::process::autoreap_process(pid, "gui-proto-exit");
        }
        Err(crate::process::ProcessError::SchedulerUnavailable) if crate::process::is_pid_runnable(pid) => {
        }
        Err(_) => {
            state.gui_app.running = false;
            state.gui_app.exited = true;
            exited = true;
            serial_write("[GUI-PROTO] app run failed pid=");
            serial_write_u64(pid.0);
            serial_write("\r\n");
            let _ = crate::process::autoreap_process(pid, "gui-proto-run-failed");
        }
    }
    process_gui_messages(state);
    if exited && state.gui_app.pid == pid.0 {
        serial_write("[GUI-PROTO] window closed after app exit\r\n");
        state.gui_app.reset_window();
    }
}

fn launch_gui_terminal_app(state: &mut UiState) {
    if state.gui_app.launched {
        return;
    }
    state.gui_app.launched = true;
    state.gui_app.running = true;
    serial_write("[GUI-PROTO] launching /app/gui_terminal_stub\r\n");

    let Some(data) = read_vfs_file(GUI_TERMINAL_STUB_PATH) else {
        serial_write("[GUI-PROTO] failed: /app/gui_terminal_stub missing\r\n");
        state.gui_app.running = false;
        return;
    };

    let pid = match crate::process::create_user_process_record(String::from(GUI_TERMINAL_STUB_PATH), true) {
        Ok(pid) => pid,
        Err(_) => {
            serial_write("[GUI-PROTO] failed: process create\r\n");
            state.gui_app.running = false;
            return;
        }
    };
    state.gui_app.pid = pid.0;
    serial_write("[GUI-PROTO] app pid=");
    serial_write_u64(pid.0);
    serial_write("\r\n");

    let argv = [String::from("gui_terminal_stub")];
    if crate::elf::prepare_process_elf(pid, &data, &argv).is_err() {
        serial_write("[GUI-PROTO] failed: ELF prepare\r\n");
        let _ = crate::process::autoreap_process(pid, "gui-proto-prepare-failed");
        state.gui_app.running = false;
        return;
    }

    run_gui_app_once(state);
    state.gui_app_needs_run = false;
    process_gui_messages(state);
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

fn blur_sample_horizontal(x: usize, y: usize, width: usize, height: usize) -> u32 {
    let mut r = 0u32;
    let mut g = 0u32;
    let mut b = 0u32;

    for i in 0..BLUR_WEIGHTS.len() {
        let weight = BLUR_WEIGHTS[i];
        let sx = x
            .saturating_add(i)
            .saturating_sub(BLUR_RADIUS)
            .min(width.saturating_sub(1));
        let color = desktop_pixel(sx, y, width, height);
        r += ((color >> 16) & 0xff) * weight;
        g += ((color >> 8) & 0xff) * weight;
        b += (color & 0xff) * weight;
    }

    ((r / BLUR_WEIGHT_SUM) << 16) | ((g / BLUR_WEIGHT_SUM) << 8) | (b / BLUR_WEIGHT_SUM)
}

fn blur_temp_pixel(x: usize, y: usize, width: usize) -> u32 {
    unsafe { BLUR_TEMP[y * width + x] }
}

fn rebuild_blur_cache(width: usize, height: usize) {
    if width == 0 || height == 0 || width > MAX_BLUR_WIDTH || height > MAX_BLUR_HEIGHT {
        unsafe {
            BLUR_CACHE_READY = false;
        }
        return;
    }

    unsafe {
        if BLUR_CACHE_READY && BLUR_CACHE_WIDTH == width && BLUR_CACHE_HEIGHT == height {
            return;
        }

        serial_write("[GUI] rebuilding two-pass blur cache\r\n");

        for y in 0..height {
            for x in 0..width {
                BLUR_TEMP[y * width + x] = blur_sample_horizontal(x, y, width, height);
            }
        }

        for y in 0..height {
            for x in 0..width {
                let mut r = 0u32;
                let mut g = 0u32;
                let mut b = 0u32;

                for i in 0..BLUR_WEIGHTS.len() {
                    let weight = BLUR_WEIGHTS[i];
                    let sy = y
                        .saturating_add(i)
                        .saturating_sub(BLUR_RADIUS)
                        .min(height.saturating_sub(1));
                    let color = blur_temp_pixel(x, sy, width);
                    r += ((color >> 16) & 0xff) * weight;
                    g += ((color >> 8) & 0xff) * weight;
                    b += (color & 0xff) * weight;
                }

                BLUR_CACHE[y * width + x] =
                    ((r / BLUR_WEIGHT_SUM) << 16) | ((g / BLUR_WEIGHT_SUM) << 8) | (b / BLUR_WEIGHT_SUM);
            }
        }

        BLUR_CACHE_WIDTH = width;
        BLUR_CACHE_HEIGHT = height;
        BLUR_CACHE_READY = true;
        serial_write("[GUI] two-pass blur cache ready\r\n");
    }
}

fn blurred_desktop_pixel(x: usize, y: usize, width: usize, height: usize) -> u32 {
    unsafe {
        if BLUR_CACHE_READY && BLUR_CACHE_WIDTH == width && BLUR_CACHE_HEIGHT == height {
            return BLUR_CACHE[y.min(height.saturating_sub(1)) * width + x.min(width.saturating_sub(1))];
        }
    }

    desktop_pixel(x, y, width, height)
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
                let blurred = blurred_desktop_pixel(px, py, width, height);
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
        b'_' => [0x40, 0x40, 0x40, 0x40, 0x40],
        b'=' => [0x14, 0x14, 0x14, 0x14, 0x14],
        b'/' => [0x20, 0x10, 0x08, 0x04, 0x02],
        b'%' => [0x62, 0x64, 0x08, 0x13, 0x23],
        b'@' => [0x3E, 0x41, 0x5D, 0x55, 0x5E],
        b'#' => [0x14, 0x7F, 0x14, 0x7F, 0x14],
        b'$' => [0x24, 0x2A, 0x7F, 0x2A, 0x12],
        b'~' => [0x08, 0x04, 0x08, 0x10, 0x08],
        b'|' => [0x00, 0x00, 0x7F, 0x00, 0x00],
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

fn draw_terminal_bridge(
    fb: Framebuffer,
    width: usize,
    height: usize,
    x: usize,
    y: usize,
    bridge: &GuiRuntimeBridge,
) {
    if !bridge.attempted {
        draw_text(fb, width, height, x, y, "Terminal runtime bridge pending", MUTED);
        return;
    }

    if !bridge.launched {
        draw_text(fb, width, height, x, y, "Terminal runtime bridge failed", RED);
        draw_text(fb, width, height, x, y + 20, bridge.error, MUTED);
        return;
    }

    let mut line = [0u8; 80];
    let mut len = 0usize;
    append_str(&mut line, &mut len, "launched /app/gui_ping pid=");
    append_u64(&mut line, &mut len, bridge.pid);
    draw_text(fb, width, height, x, y, line_str(&line, len), GREEN);

    let mut exit_line = [0u8; 40];
    let mut exit_len = 0usize;
    append_str(&mut exit_line, &mut exit_len, "exit code=");
    append_i32(&mut exit_line, &mut exit_len, bridge.exit_code);
    draw_text(fb, width, height, x, y + 20, line_str(&exit_line, exit_len), TEXT);

    if bridge.message_len > 0 {
        draw_text(fb, width, height, x, y + 40, "message:", MUTED);
        draw_text(fb, width, height, x + 58, y + 40, bridge.message_str(), TEXT);
    } else {
        draw_text(fb, width, height, x, y + 40, "message: <none>", RED);
    }

    if bridge.ok {
        draw_text(fb, width, height, x, y + 66, "Terminal runtime bridge OK", GREEN);
        draw_text(fb, width, height, x, y + 86, "interactive input not implemented", MUTED);
    } else {
        draw_text(fb, width, height, x, y + 66, "Terminal runtime bridge failed", RED);
        draw_text(fb, width, height, x, y + 86, bridge.error, MUTED);
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
    let Some(wallpaper) = wallpaper_bytes() else {
        return BG;
    };

    let src_x = x.saturating_mul(WALLPAPER_WIDTH) / width.max(1);
    let src_y = y.saturating_mul(WALLPAPER_HEIGHT) / height.max(1);
    let bmp_y = WALLPAPER_HEIGHT.saturating_sub(1).saturating_sub(src_y.min(WALLPAPER_HEIGHT - 1));
    let offset = WALLPAPER_OFFSET + bmp_y * WALLPAPER_STRIDE + src_x.min(WALLPAPER_WIDTH - 1) * 3;

    if offset + 2 >= wallpaper.len() {
        return BG;
    }

    let b = wallpaper[offset] as u32;
    let g = wallpaper[offset + 1] as u32;
    let r = wallpaper[offset + 2] as u32;
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
    draw_rect(fb, width, height, window.x + window.width.saturating_sub(16), window.y + window.height.saturating_sub(5), 10, 1, 0x425047);
    draw_rect(fb, width, height, window.x + window.width.saturating_sub(11), window.y + window.height.saturating_sub(10), 5, 1, 0x425047);

    let x = window.x + 18;
    let y = window.y + 50;
    match window.app_type {
        AppType::Terminal => {
            draw_round_rect(fb, width, height, window.x + 8, window.y + 42, window.width.saturating_sub(16), window.height.saturating_sub(50), 8, TERMINAL_BG);
            draw_terminal_bridge(fb, width, height, x, y, &state.terminal_bridge);
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

fn draw_gui_app_window(fb: Framebuffer, width: usize, height: usize, app: &GuiAppRuntime) {
    let Some(rect) = gui_app_window_rect(width, height, app) else {
        return;
    };

    let window_width = rect.width;
    let window_height = rect.height;
    let x = rect.x;
    let y = rect.y;
    draw_round_rect(fb, width, height, x + 10, y + 12, window_width, window_height, 14, SHADOW);
    draw_round_rect(fb, width, height, x, y, window_width, window_height, 14, WINDOW_BG);
    draw_rect(fb, width, height, x, y, window_width, 34, WINDOW_TITLE);
    draw_rect(fb, width, height, x, y + 33, window_width, 1, 0x2f3a42);
    draw_round_rect_border(fb, width, height, x, y, window_width, window_height, 14, GLASS_EDGE);
    draw_traffic_button(fb, width, height, x + 12, y + 11, RED);
    draw_traffic_button(fb, width, height, x + 32, y + 11, YELLOW);
    draw_traffic_button(fb, width, height, x + 52, y + 11, GREEN);
    draw_text(fb, width, height, x + 82, y + 13, app.title(), TEXT);

    draw_round_rect(fb, width, height, x + 8, y + 42, window_width.saturating_sub(16), window_height.saturating_sub(50), 8, TERMINAL_BG);
    let content_x = x + 18;
    let content_y = y + 50;

    for index in 0..app.line_count {
        let line = &app.lines[index];
        let draw_x = content_x.saturating_add(line.x.max(0) as usize);
        let draw_y = content_y.saturating_add(line.y.max(0) as usize);
        let color = if line.text().starts_with("Dunit GUI Terminal") { GREEN } else { TEXT };
        draw_text(fb, width, height, draw_x, draw_y, line.text(), color);
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
    draw_gui_app_window(fb, width, height, &state.gui_app);
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

    let top_panel = Rect::new(0, 0, width, 42);
    let hero = Rect::new(48, 68, 520, 72);
    let launcher = Rect::new(46, 162, 350, 272);
    let quick = Rect::new(width.saturating_sub(330), 64, 302, 176);
    let notifications = Rect::new(width.saturating_sub(330), 440, 302, 118);
    if rects_intersect(rect, top_panel)
        || rects_intersect(rect, hero)
        || (state.launcher_open && rects_intersect(rect, launcher))
        || (state.quick_open && rects_intersect(rect, quick))
        || (state.notifications_open && rects_intersect(rect, notifications))
    {
        draw_desktop_widgets(fb, width, height, state);
    }

    if let Some(wm) = window_manager::get_wm() {
        for window in wm.get_windows() {
            if window.visible {
                let window_rect = Rect::new(window.x, window.y, window.width + 12, window.height + 14);
                if rects_intersect(rect, window_rect) {
                    draw_window(fb, width, height, window, state);
                }
            }
        }
    }

    if let Some(gui_rect) = gui_app_window_rect(width, height, &state.gui_app) {
        if rects_intersect(rect, Rect::new(gui_rect.x, gui_rect.y, gui_rect.width + 12, gui_rect.height + 14)) {
            draw_gui_app_window(fb, width, height, &state.gui_app);
        }
    }

    let (dock_x, dock_y, dock_width, _, _) = dock_layout(width, height);
    if rects_intersect(rect, Rect::new(dock_x, dock_y, dock_width + 10, 76)) {
        draw_dock(fb, width, height);
    }
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
            if app_type == AppType::Terminal {
                launch_gui_terminal_app(state);
            } else if let Some(wm) = window_manager::get_wm() {
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
        if (scancode & 0x80) != 0 {
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
            continue;
        }

        if state.gui_app.running && state.gui_app.window_id != 0 {
            let key = match scancode {
                0x0E => Some(8),
                0x1C => Some(b'\n'),
                _ => keyboard::scancode_to_char(scancode).map(|ch| ch as u8),
            };
            if let Some(key) = key {
                if send_gui_key_event(&state.gui_app, key) {
                    state.gui_app_needs_run = true;
                    redraw = true;
                }
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
    let mut last_rect = dock_rect;
    for step in 0..=frames {
        let t = ease_step(step, frames);
        let t = if opening { t } else { 1000usize.saturating_sub(t) };
        let rect = Rect::new(
            lerp_usize(dock_rect.x, window_rect.x, t),
            lerp_usize(dock_rect.y, window_rect.y, t),
            lerp_usize(dock_rect.width, window_rect.width, t),
            lerp_usize(dock_rect.height, window_rect.height, t),
        );

        let damage = padded_rect(rect.union(last_rect), 18, width, height);
        redraw_region(scene, width, height, damage, state);
        draw_genie_frame(scene, width, height, rect, GLASS);
        if let Some(buffer) = back_buffer {
            buffer.present_rect(front, damage);
        }

        last_rect = rect;
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

    load_wallpaper();
    rebuild_blur_cache(width, height);
    redraw_full_screen(scene, width, height, &state);
    if let Some(buffer) = back_buffer.as_ref() {
        buffer.present_full(front);
    }

    let (mut old_mouse_x, mut old_mouse_y) = mouse::get_position();
    let mut old_buttons = mouse::get_buttons();
    let mut pointer_op: Option<PointerOp> = None;
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
            let mut handled_click = false;

            if let Some(rect) = gui_app_window_rect(width, height, &state.gui_app) {
                if inside(mx, my, rect.x + 12, rect.y + 11, 12, 12) {
                    pointer_op = None;
                    if close_gui_app_window(&mut state) {
                        state.gui_app_needs_run = true;
                    }
                    full_redraw = true;
                    handled_click = true;
                }
            }

            let closed = if handled_click {
                None
            } else {
                window_manager::get_wm()
                .map(|wm| wm.close_at(mx, my))
                .unwrap_or(None)
            };

            if let Some((x, y, w, h, app_type)) = closed {
                pointer_op = None;
                let window_rect = Rect::new(x, y, w, h);
                if let Some(index) = dock_app_index(app_type) {
                    let dock_rect = dock_icon_rect(index, width, height);
                    animate_genie(scene, front, back_buffer.as_ref(), width, height, dock_rect, window_rect, false, &state);
                }
                full_redraw = true;
            } else if handled_click {
            } else if let Some((x, y, w, h, app_type)) = window_manager::get_wm()
                .map(|wm| wm.minimize_at(mx, my))
                .unwrap_or(None)
            {
                pointer_op = None;
                let window_rect = Rect::new(x, y, w, h);
                if let Some(index) = dock_app_index(app_type) {
                    let dock_rect = dock_icon_rect(index, width, height);
                    animate_genie(scene, front, back_buffer.as_ref(), width, height, dock_rect, window_rect, false, &state);
                }
                full_redraw = true;
            } else if let Some((x, y, w, h, app_type)) = window_manager::get_wm()
                .map(|wm| wm.zoom_at(mx, my, width, height))
                .unwrap_or(None)
            {
                pointer_op = None;
                let old_rect = Rect::new(x, y, w, h);
                let new_rect = window_manager::get_wm()
                    .and_then(|wm| wm.app_bounds(app_type).map(rect_from_bounds))
                    .unwrap_or(old_rect);
                if back_buffer.is_some() {
                    drag_damage = Some(padded_rect(old_rect.union(new_rect), 18, width, height));
                } else {
                    full_redraw = true;
                }
            } else if let Some(action) = handle_widget_click(mx, my, width, height, &state) {
                pointer_op = None;
                full_redraw = apply_ui_action(&mut state, action);
            } else if let Some(app_type) = handle_dock_click(mx, my, width, height) {
                pointer_op = None;
                if app_type == AppType::Terminal {
                    launch_gui_terminal_app(&mut state);
                } else {
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
                }
                full_redraw = true;
            } else {
                pointer_op = begin_gui_app_drag(&state, mx, my, width, height)
                    .or_else(|| window_manager::get_wm()
                    .and_then(|wm| wm.begin_resize_at(mx, my))
                    .map(|(idx, offset_x, offset_y)| PointerOp::Resize { idx, offset_x, offset_y })
                    .or_else(|| {
                        window_manager::get_wm()
                            .and_then(|wm| wm.begin_drag_at(mx, my))
                            .map(|(idx, offset_x, offset_y)| PointerOp::Drag { idx, offset_x, offset_y })
                    }));
            }
        }

        if pressed {
            if let Some(op) = pointer_op {
                let mx = mouse_x.max(0) as usize;
                let my = mouse_y.max(0) as usize;
                match op {
                    PointerOp::GuiAppDrag { offset_x, offset_y } => {
                        if let Some((old_rect, new_rect)) = drag_gui_app_window(&mut state, mx, my, width, height, offset_x, offset_y) {
                            let window_damage = old_rect
                                .union(new_rect)
                                .union(cursor_rect(old_mouse_x, old_mouse_y))
                                .union(cursor_rect(mouse_x, mouse_y));
                            if back_buffer.is_some() {
                                drag_damage = Some(padded_rect(window_damage, 10, width, height));
                            } else {
                                full_redraw = true;
                            }
                        }
                    }
                    PointerOp::Drag { .. } | PointerOp::Resize { .. } => {
                        if let Some(wm) = window_manager::get_wm() {
                    let (idx, offset_x, offset_y) = match op {
                        PointerOp::Drag { idx, offset_x, offset_y } => (idx, offset_x, offset_y),
                        PointerOp::Resize { idx, offset_x, offset_y } => (idx, offset_x, offset_y),
                        PointerOp::GuiAppDrag { .. } => unreachable!(),
                    };
                    let old_bounds = wm.window_bounds(idx);
                    match op {
                        PointerOp::Drag { .. } => {
                            if let Some((x, y, _, _)) = old_bounds {
                                let target_x = mx.saturating_sub(offset_x);
                                let target_y = my.saturating_sub(offset_y);
                                let smooth_x = (x.saturating_mul(2) + target_x) / 3;
                                let smooth_y = (y.saturating_mul(2) + target_y) / 3;
                                wm.drag_window(idx, smooth_x, smooth_y, width, height);
                            }
                        }
                        PointerOp::Resize { .. } => {
                            if let Some((x, y, _, _)) = old_bounds {
                                let target_w = mx.saturating_sub(x).saturating_add(offset_x);
                                let target_h = my.saturating_sub(y).saturating_add(offset_y);
                                wm.resize_window(idx, target_w, target_h, width, height);
                            }
                        }
                        PointerOp::GuiAppDrag { .. } => {}
                    }
                    let new_bounds = wm.window_bounds(idx);
                    if let (Some(old_bounds), Some(new_bounds)) = (old_bounds, new_bounds) {
                        if old_bounds == new_bounds && !cursor_moved {
                            old_mouse_x = mouse_x;
                            old_mouse_y = mouse_y;
                            old_buttons = buttons;
                            continue;
                        }
                        let window_damage = rect_from_bounds(old_bounds)
                            .union(rect_from_bounds(new_bounds))
                            .union(cursor_rect(old_mouse_x, old_mouse_y))
                            .union(cursor_rect(mouse_x, mouse_y));
                        if back_buffer.is_some() {
                            drag_damage = Some(padded_rect(window_damage, 10, width, height));
                        } else {
                            full_redraw = true;
                        }
                    } else {
                        full_redraw = true;
                    }
                        }
                    }
                }
            }
        } else {
            pointer_op = None;
        }

        if state.gui_app_needs_run && state.gui_app.running && state.gui_app.pid != 0 {
            let before_lines = state.gui_app.line_count;
            let before_window = state.gui_app.window_id;
            let before_pid = state.gui_app.pid;
            state.gui_app_needs_run = false;
            run_gui_app_once(&mut state);
            if state.gui_app.line_count != before_lines
                || state.gui_app.window_id != before_window
                || state.gui_app.pid != before_pid
            {
                full_redraw = true;
            }
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

        for _ in 0..100 {
            unsafe { core::arch::asm!("pause"); }
        }
    }
}
