#![no_std]
#![no_main]

use core::panic::PanicInfo;

const WINDOW_ID: u32 = 4;
const WIDTH: u32 = 640;
const HEIGHT: u32 = 390;
const CONTENT_W: u32 = WIDTH - 36;
const CONTENT_H: u32 = HEIGHT - 64;
const MAX_ENTRIES: usize = 32;
const MAX_SIDE_ENTRIES: usize = 10;
const MAX_PATH: usize = 128;
const MAX_HISTORY: usize = 8;
const GRID_X: i32 = 156;
const GRID_Y: i32 = 76;
const TILE_W: i32 = 74;
const TILE_H: i32 = 78;
const SIDEBAR_W: i32 = 136;

const BG: u32 = 0x121820;
const SIDEBAR: u32 = 0x10161c;
const TOOLBAR: u32 = 0x1c2530;
const ROW_HOVER: u32 = 0x203326;
const SURFACE: u32 = 0x151d25;
const ACCENT: u32 = 0x2f8f5a;
const BLUE: u32 = 0x2f78bd;
const YELLOW: u32 = 0xcaa84a;
const MUTED: u32 = 0x566271;
const BUTTON: u32 = 0x273443;
const BUTTON_DIM: u32 = 0x1a232c;
const RED: u32 = 0x9a4652;

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    loop {
        unsafe {
            core::arch::asm!("pause", options(nomem, nostack));
        }
    }
}

#[derive(Clone, Copy)]
struct Entry {
    name: [u8; 64],
    name_len: usize,
    file_type: u32,
    size: usize,
}

impl Entry {
    const fn empty() -> Self {
        Self {
            name: [0; 64],
            name_len: 0,
            file_type: libdunit::FILE_TYPE_FILE,
            size: 0,
        }
    }

    fn name(&self) -> &str {
        core::str::from_utf8(&self.name[..self.name_len.min(self.name.len())]).unwrap_or("<invalid>")
    }
}

struct FileManager {
    path: [u8; MAX_PATH],
    path_len: usize,
    history: [[u8; MAX_PATH]; MAX_HISTORY],
    history_lens: [usize; MAX_HISTORY],
    history_len: usize,
    entries: [Entry; MAX_ENTRIES],
    entry_count: usize,
    side_entries: [Entry; MAX_SIDE_ENTRIES],
    side_count: usize,
    dir_count: usize,
    file_count: usize,
    status: [u8; 80],
    status_len: usize,
}

impl FileManager {
    const fn new() -> Self {
        Self {
            path: [0; MAX_PATH],
            path_len: 0,
            history: [[0; MAX_PATH]; MAX_HISTORY],
            history_lens: [0; MAX_HISTORY],
            history_len: 0,
            entries: [Entry::empty(); MAX_ENTRIES],
            entry_count: 0,
            side_entries: [Entry::empty(); MAX_SIDE_ENTRIES],
            side_count: 0,
            dir_count: 0,
            file_count: 0,
            status: [0; 80],
            status_len: 0,
        }
    }

    fn init(&mut self) {
        self.set_path("/");
        self.load_sidebar();
        self.load();
    }

    fn path(&self) -> &str {
        core::str::from_utf8(&self.path[..self.path_len]).unwrap_or("/")
    }

    fn set_path(&mut self, path: &str) {
        self.path_len = path.len().min(self.path.len());
        self.path[..self.path_len].copy_from_slice(&path.as_bytes()[..self.path_len]);
    }

    fn set_status(&mut self, text: &[u8]) {
        self.status_len = text.len().min(self.status.len());
        self.status[..self.status_len].copy_from_slice(&text[..self.status_len]);
    }

    fn push_history(&mut self) {
        if self.history_len == MAX_HISTORY {
            let mut idx = 1usize;
            while idx < MAX_HISTORY {
                self.history[idx - 1] = self.history[idx];
                self.history_lens[idx - 1] = self.history_lens[idx];
                idx += 1;
            }
            self.history_len -= 1;
        }
        self.history[self.history_len] = [0; MAX_PATH];
        self.history[self.history_len][..self.path_len].copy_from_slice(&self.path[..self.path_len]);
        self.history_lens[self.history_len] = self.path_len;
        self.history_len += 1;
    }

    fn go_back(&mut self) {
        if self.history_len == 0 {
            self.set_status(b"No previous folder");
            return;
        }
        self.history_len -= 1;
        let len = self.history_lens[self.history_len];
        self.path_len = len;
        self.path[..len].copy_from_slice(&self.history[self.history_len][..len]);
        self.load();
    }

    fn go_parent(&mut self) {
        if self.path_len <= 1 {
            self.set_status(b"Already at root");
            return;
        }
        self.push_history();
        let mut len = self.path_len;
        while len > 1 && self.path[len - 1] != b'/' {
            len -= 1;
        }
        if len <= 1 {
            self.set_path("/");
        } else {
            self.path_len = len - 1;
        }
        self.load();
    }

    fn open_entry(&mut self, index: usize) {
        if index >= self.entry_count {
            return;
        }
        if self.entries[index].file_type != libdunit::FILE_TYPE_DIRECTORY {
            self.set_status(b"MVP: files are read-only display items");
            return;
        }
        self.push_history();
        let mut next = [0u8; MAX_PATH];
        let mut len = 0usize;
        append_bytes(&mut next, &mut len, self.path().as_bytes());
        if len > 1 && len < next.len() {
            next[len] = b'/';
            len += 1;
        }
        append_bytes(&mut next, &mut len, self.entries[index].name().as_bytes());
        self.path_len = len;
        self.path[..len].copy_from_slice(&next[..len]);
        self.load();
    }

    fn load(&mut self) {
        self.entry_count = 0;
        self.dir_count = 0;
        self.file_count = 0;
        let mut raw = [libdunit::DirEntry::empty(); MAX_ENTRIES];
        let count = libdunit::readdir(self.path(), &mut raw);
        if count < 0 {
            self.set_status(b"readdir failed");
            return;
        }
        let mut idx = 0usize;
        while idx < count as usize && idx < MAX_ENTRIES {
            let name = raw[idx].name();
            let mut entry = Entry::empty();
            entry.name_len = name.len().min(entry.name.len());
            entry.name[..entry.name_len].copy_from_slice(&name.as_bytes()[..entry.name_len]);
            entry.file_type = raw[idx].file_type;
            if entry.file_type == libdunit::FILE_TYPE_DIRECTORY {
                self.dir_count += 1;
            } else {
                self.file_count += 1;
            }

            let mut full_path = [0u8; MAX_PATH];
            let mut full_len = 0usize;
            append_bytes(&mut full_path, &mut full_len, self.path().as_bytes());
            if full_len > 1 && full_len < full_path.len() {
                full_path[full_len] = b'/';
                full_len += 1;
            }
            append_bytes(&mut full_path, &mut full_len, name.as_bytes());
            let full = core::str::from_utf8(&full_path[..full_len]).unwrap_or("/");
            let mut stat = libdunit::FileStat::default();
            if libdunit::stat(full, &mut stat) == 0 {
                entry.file_type = stat.file_type;
                entry.size = stat.size;
            }

            self.entries[idx] = entry;
            idx += 1;
        }
        self.entry_count = idx;
        self.set_status(b"Ready");
    }

    fn load_sidebar(&mut self) {
        self.side_count = 0;
        let mut raw = [libdunit::DirEntry::empty(); MAX_ENTRIES];
        let count = libdunit::readdir("/", &mut raw);
        if count < 0 {
            return;
        }
        let mut idx = 0usize;
        while idx < count as usize && self.side_count < MAX_SIDE_ENTRIES {
            if raw[idx].file_type == libdunit::FILE_TYPE_DIRECTORY {
                let name = raw[idx].name();
                let mut entry = Entry::empty();
                entry.name_len = name.len().min(entry.name.len());
                entry.name[..entry.name_len].copy_from_slice(&name.as_bytes()[..entry.name_len]);
                entry.file_type = libdunit::FILE_TYPE_DIRECTORY;
                self.side_entries[self.side_count] = entry;
                self.side_count += 1;
            }
            idx += 1;
        }
    }

    fn redraw(&self) {
        libdunit::gui_clear(WINDOW_ID);
        libdunit::gui_draw_rect(WINDOW_ID, 0, 0, CONTENT_W, CONTENT_H, BG);
        libdunit::gui_draw_rect(WINDOW_ID, 0, 0, CONTENT_W, 42, TOOLBAR);
        libdunit::gui_draw_rect(WINDOW_ID, 0, 42, SIDEBAR_W as u32, CONTENT_H - 42, SIDEBAR);
        libdunit::gui_draw_rect(WINDOW_ID, SIDEBAR_W, 42, 2, CONTENT_H - 42, 0x26313a);
        libdunit::gui_draw_rect(WINDOW_ID, 0, 40, CONTENT_W, 2, ACCENT);

        draw_button(12, 10, 52, "Back", self.history_len > 0);
        draw_button(72, 10, 42, "Up", self.path_len > 1);
        libdunit::gui_draw_rect(WINDOW_ID, 124, 10, CONTENT_W - 138, 22, 0x10161c);
        libdunit::gui_draw_text(WINDOW_ID, 134, 17, self.path());

        libdunit::gui_draw_text(WINDOW_ID, 16, 62, "Root");
        draw_place(16, 88, "/", self.path() == "/");
        let mut side = 0usize;
        while side < self.side_count {
            let y = 114 + side as i32 * 24;
            let active = path_is_root_child(self.path(), self.side_entries[side].name());
            draw_place(16, y, self.side_entries[side].name(), active);
            side += 1;
        }

        libdunit::gui_draw_rect(WINDOW_ID, SIDEBAR_W + 2, 42, CONTENT_W - SIDEBAR_W as u32 - 2, CONTENT_H - 68, SURFACE);
        libdunit::gui_draw_text(WINDOW_ID, GRID_X, 58, "Files");

        let mut idx = 0usize;
        while idx < self.entry_count {
            self.draw_entry(idx);
            idx += 1;
        }

        let mut summary = [0u8; 96];
        let mut len = 0usize;
        append_usize_label(&mut summary, &mut len, b"files", self.file_count);
        append_bytes(&mut summary, &mut len, b"  ");
        append_usize_label(&mut summary, &mut len, b"dirs", self.dir_count);
        append_bytes(&mut summary, &mut len, b"  ");
        append_bytes(&mut summary, &mut len, &self.status[..self.status_len]);
        libdunit::gui_draw_rect(WINDOW_ID, SIDEBAR_W, CONTENT_H as i32 - 26, CONTENT_W - SIDEBAR_W as u32, 26, 0x10161c);
        libdunit::gui_draw_text(WINDOW_ID, GRID_X, CONTENT_H as i32 - 16, line(&summary, len));
        libdunit::gui_set_status(line(&summary, len));
    }

    fn draw_entry(&self, index: usize) {
        let entry = self.entries[index];
        let col = (index % 6) as i32;
        let row = (index / 6) as i32;
        let x = GRID_X + col * TILE_W;
        let y = GRID_Y + row * TILE_H;
        if y + TILE_H > CONTENT_H as i32 - 30 {
            return;
        }
        let icon_color = match entry.file_type {
            libdunit::FILE_TYPE_DIRECTORY => ACCENT,
            libdunit::FILE_TYPE_DEVICE => RED,
            _ if is_executable(entry.name()) => YELLOW,
            _ => BLUE,
        };
        libdunit::gui_draw_rect(WINDOW_ID, x + 10, y + 2, 48, 38, 0x0e141a);
        libdunit::gui_draw_rect(WINDOW_ID, x + 12, y + 4, 44, 34, icon_color);
        if entry.file_type == libdunit::FILE_TYPE_DIRECTORY {
            libdunit::gui_draw_rect(WINDOW_ID, x + 12, y, 18, 8, icon_color);
            libdunit::gui_draw_rect(WINDOW_ID, x + 16, y + 9, 36, 3, 0x77c98d);
        } else {
            libdunit::gui_draw_rect(WINDOW_ID, x + 46, y + 8, 8, 8, 0xd7e0e8);
        }
        draw_short_name(x + 4, y + 48, entry.name());
        if entry.file_type == libdunit::FILE_TYPE_DIRECTORY {
            libdunit::gui_draw_text(WINDOW_ID, x + 10, y + 62, "Folder");
        } else {
            let mut out = [0u8; 32];
            let mut len = 0usize;
            append_size(&mut out, &mut len, entry.size);
            libdunit::gui_draw_text(WINDOW_ID, x + 8, y + 62, line(&out, len));
        }
    }

    fn handle_pointer(&mut self, x: i32, y: i32) {
        if inside(x, y, 12, 10, 52, 22) {
            self.go_back();
            self.redraw();
            return;
        }
        if inside(x, y, 72, 10, 42, 22) {
            self.go_parent();
            self.redraw();
            return;
        }
        if inside(x, y, 12, 84, 108, 22) {
            if self.path() != "/" {
                self.push_history();
                self.set_path("/");
                self.load();
                self.redraw();
            }
            return;
        }
        let mut idx = 0usize;
        while idx < self.side_count {
            let py = 110 + idx as i32 * 24;
            if inside(x, y, 12, py, 108, 22) {
                let mut next = [0u8; MAX_PATH];
                let mut len = 0usize;
                append_bytes(&mut next, &mut len, b"/");
                append_bytes(&mut next, &mut len, self.side_entries[idx].name().as_bytes());
                let path = core::str::from_utf8(&next[..len]).unwrap_or("/");
                if self.path() != path {
                    self.push_history();
                    self.set_path(path);
                    self.load();
                    self.redraw();
                }
                return;
            }
            idx += 1;
        }
        if x >= GRID_X && y >= GRID_Y {
            let col = (x - GRID_X) / TILE_W;
            let row = (y - GRID_Y) / TILE_H;
            if col >= 0 && row >= 0 && col < 6 {
                let index = (row as usize) * 6 + col as usize;
                if index < self.entry_count {
                    self.open_entry(index);
                    self.redraw();
                }
            }
        }
    }
}

#[no_mangle]
pub extern "C" fn _start(argc: usize, argv: libdunit::RawArgv, _envp: libdunit::RawEnvp) -> ! {
    if argc > 1 {
        if let Some(arg) = unsafe { libdunit::argv_get(argc, argv, 1) } {
            if arg == "--smoke" {
                smoke();
            }
        }
    }

    libdunit::println("gui_file_manager: start");
    libdunit::gui_create_window(WINDOW_ID, "File Manager", WIDTH, HEIGHT);
    libdunit::gui_set_title(WINDOW_ID, "File Manager");
    libdunit::gui_set_status("gui_file_manager: running");

    let mut app = FileManager::new();
    app.init();
    app.redraw();

    loop {
        let mut event = libdunit::GuiMessage::new(0);
        let received = libdunit::gui_recv_event(&mut event);
        if received < 0 {
            libdunit::yield_now();
            continue;
        }
        if event.window_id != WINDOW_ID {
            continue;
        }
        match event.kind {
            libdunit::GUI_MSG_POINTER_EVENT => app.handle_pointer(event.a, event.b),
            libdunit::GUI_MSG_KEY_EVENT => match event.a as u8 {
                b'h' | 8 => {
                    app.go_back();
                    app.redraw();
                }
                b'u' => {
                    app.go_parent();
                    app.redraw();
                }
                _ => {}
            },
            libdunit::GUI_MSG_CLOSE_EVENT => {
                send_exit();
                libdunit::exit(0);
            }
            _ => {}
        }
    }
}

fn smoke() -> ! {
    let mut entries = [libdunit::DirEntry::empty(); 16];
    let count = libdunit::readdir("/", &mut entries);
    if count < 0 {
        libdunit::println("gui_file_manager: smoke readdir failed");
        libdunit::exit(1);
    }
    let mut first_dir = [0u8; MAX_PATH];
    let mut first_len = 0usize;
    let mut idx = 0usize;
    while idx < count as usize && idx < entries.len() {
        if entries[idx].file_type == libdunit::FILE_TYPE_DIRECTORY {
            append_bytes(&mut first_dir, &mut first_len, b"/");
            append_bytes(&mut first_dir, &mut first_len, entries[idx].name().as_bytes());
            break;
        }
        idx += 1;
    }
    if first_len == 0 {
        libdunit::println("gui_file_manager: smoke found no directory");
        libdunit::exit(2);
    }
    let first_path = core::str::from_utf8(&first_dir[..first_len]).unwrap_or("/");
    let mut stat = libdunit::FileStat::default();
    if libdunit::stat(first_path, &mut stat) != 0 || stat.file_type != libdunit::FILE_TYPE_DIRECTORY {
        libdunit::println("gui_file_manager: smoke stat failed");
        libdunit::exit(3);
    }
    libdunit::print("gui_file_manager: smoke OK entries=");
    libdunit::print_usize(count as usize);
    libdunit::println("");
    libdunit::exit(0);
}

fn send_exit() {
    libdunit::gui_set_status("gui_file_manager: exiting");
    let mut message = libdunit::GuiMessage::new(libdunit::GUI_MSG_EXIT);
    message.window_id = WINDOW_ID;
    libdunit::gui_send(&message);
}

fn draw_button(x: i32, y: i32, w: u32, label: &str, enabled: bool) {
    libdunit::gui_draw_rect(WINDOW_ID, x, y, w, 22, if enabled { BUTTON } else { BUTTON_DIM });
    libdunit::gui_draw_text(WINDOW_ID, x + 10, y + 8, label);
}

fn draw_place(x: i32, y: i32, label: &str, active: bool) {
    libdunit::gui_draw_rect(WINDOW_ID, x - 4, y - 4, 108, 22, if active { ROW_HOVER } else { SIDEBAR });
    libdunit::gui_draw_rect(WINDOW_ID, x, y, 12, 12, if active { ACCENT } else { MUTED });
    libdunit::gui_draw_text(WINDOW_ID, x + 22, y + 2, label);
}

fn draw_short_name(x: i32, y: i32, name: &str) {
    let bytes = name.as_bytes();
    if bytes.len() <= 10 {
        libdunit::gui_draw_text(WINDOW_ID, x, y, name);
        return;
    }
    let mut out = [0u8; 13];
    let mut len = 0usize;
    append_bytes(&mut out, &mut len, &bytes[..9]);
    append_bytes(&mut out, &mut len, b"..");
    libdunit::gui_draw_text(WINDOW_ID, x, y, line(&out, len));
}

fn path_is_root_child(path: &str, child: &str) -> bool {
    let path_bytes = path.as_bytes();
    let child_bytes = child.as_bytes();
    if path_bytes.len() < child_bytes.len() + 1 || path_bytes.first().copied() != Some(b'/') {
        return false;
    }
    &path_bytes[1..1 + child_bytes.len()] == child_bytes
        && (path_bytes.len() == child_bytes.len() + 1 || path_bytes[1 + child_bytes.len()] == b'/')
}

fn is_executable(name: &str) -> bool {
    !contains(name, ".") || ends_with(name, ".app") || ends_with(name, ".elf")
}

fn contains(text: &str, needle: &str) -> bool {
    let hay = text.as_bytes();
    let ndl = needle.as_bytes();
    if ndl.is_empty() || ndl.len() > hay.len() {
        return false;
    }
    let mut idx = 0usize;
    while idx + ndl.len() <= hay.len() {
        if &hay[idx..idx + ndl.len()] == ndl {
            return true;
        }
        idx += 1;
    }
    false
}

fn ends_with(text: &str, suffix: &str) -> bool {
    let text = text.as_bytes();
    let suffix = suffix.as_bytes();
    text.len() >= suffix.len() && &text[text.len() - suffix.len()..] == suffix
}

fn inside(mx: i32, my: i32, x: i32, y: i32, w: i32, h: i32) -> bool {
    mx >= x && mx < x + w && my >= y && my < y + h
}

fn append_bytes(out: &mut [u8], len: &mut usize, value: &[u8]) {
    let mut idx = 0usize;
    while idx < value.len() && *len < out.len() {
        out[*len] = value[idx];
        *len += 1;
        idx += 1;
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
            value /= 10;
            count += 1;
        }
    }
    while count > 0 {
        count -= 1;
        append_bytes(out, len, &digits[count..count + 1]);
    }
}

fn append_usize_label(out: &mut [u8], len: &mut usize, label: &[u8], value: usize) {
    append_bytes(out, len, label);
    append_bytes(out, len, b" ");
    append_u64(out, len, value as u64);
}

fn append_size(out: &mut [u8], len: &mut usize, bytes: usize) {
    if bytes >= 1024 * 1024 {
        append_u64(out, len, (bytes / (1024 * 1024)) as u64);
        append_bytes(out, len, b"MiB");
    } else if bytes >= 1024 {
        append_u64(out, len, (bytes / 1024) as u64);
        append_bytes(out, len, b"KiB");
    } else {
        append_u64(out, len, bytes as u64);
        append_bytes(out, len, b"B");
    }
}

fn line(buf: &[u8], len: usize) -> &str {
    core::str::from_utf8(&buf[..len.min(buf.len())]).unwrap_or("<invalid>")
}
