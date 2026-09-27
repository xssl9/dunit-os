#![no_std]
#![no_main]

//! Dolphin-style file-manager client for the M4 userspace DWM (Stack B).
//!
//! Reborn from the plain text-list `gui_files` into an icon-grid browser: a
//! breadcrumb path header, a grid of per-type Breeze-Chameleon icons with TTF
//! labels, click-to-select / click-again-to-open navigation, and a status bar.
//! It lists the VFS with `libdunit::readdir` and paints its own shared buffer
//! directly with `dunit_render::Surface` primitives (fills, `blit_image` for
//! icons, `blit_glyph` for text) — the compositor blits that buffer each tick.

use core::panic::PanicInfo;

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use gui_protocol_v1::wire::{Request, FEATURE_ARGB8888};

use dunit_render::Surface;
use dunit_style::value::Color;
use dunit_text::Font;

// Control-message magics shared with the compositor.
const CTRL_MAGIC: u32 = 0x3150_4143; // "CAP1" — capability announce
const INPUT_MAGIC: u32 = 0x3150_4E49; // "INP1" — pointer input
const IN_DOWN: u8 = 2;
const IN_QUIT: u8 = 9;

const SURFACE: u64 = 1;
const BUFFER: u64 = 2;
const W: u32 = 520;
const H: u32 = 380;
const FMT_XRGB8888: u32 = 1;

// --- Grid geometry (client-local pixels) ---
const PAD: i32 = 10;
const HEADER_H: i32 = 30;
const STATUS_H: i32 = 24;
const COLS: i32 = 5;
const CELL_W: i32 = (W as i32 - 2 * PAD) / COLS; // 100
const CELL_H: i32 = 72;
const ICON: i32 = 32;
/// Rows that fit between the header and status bar, and the resulting cell cap.
const ROWS: i32 = (H as i32 - HEADER_H - STATUS_H) / CELL_H; // 4
const CAP: usize = (COLS * ROWS) as usize; // 20 visible cells


// --- Palette (opaque ARGB via Color) ---
const BG: Color = Color::rgb(0x12, 0x18, 0x20);
const HEADER_BG: Color = Color::rgb(0x0e, 0x14, 0x1b);
const STATUS_BG: Color = Color::rgb(0x0e, 0x14, 0x1b);
const ACCENT: Color = Color::rgb(0x2f, 0x8f, 0x5a); // Green Tea accent
const LABEL: Color = Color::rgb(0xcd, 0xd6, 0xf4);
const MUTED: Color = Color::rgb(0x8a, 0x94, 0xa8);
/// Selection wash painted behind the icon+label of the active cell.
const SEL_BG: Color = Color::rgba(0x2f, 0x8f, 0x5a, 0x50);

/// The font, embedded in the ELF (M4 still ships assets in-image).
static FONT_BYTES: &[u8] = include_bytes!("../../../../assets/fonts/DejaVuSans.ttf");

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    libdunit::println("gui_files: PANIC");
    libdunit::exit(101)
}

fn u64_at(p: &[u8], off: usize) -> u64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&p[off..off + 8]);
    u64::from_le_bytes(a)
}

fn round_i32(v: f32) -> i32 {
    if v <= 0.0 {
        0
    } else {
        (v + 0.5) as i32
    }
}

/// Total advance width of `text` at `px`.
fn text_width(font: &Font, text: &str, px: f32) -> f32 {
    font.layout_line(text, px).1
}

/// Draw one line of `text` with its left edge at `x` and baseline at `baseline`.
fn draw_text(s: &mut Surface, font: &Font, x: i32, baseline: i32, px: f32, text: &str, color: Color) {
    let (glyphs, _adv) = font.layout_line(text, px);
    for g in glyphs {
        if let Some(bmp) = font.rasterize(g.glyph, px) {
            let ox = x + round_i32(g.x) + bmp.left;
            let oy = baseline - bmp.top;
            s.blit_glyph(&bmp, ox, oy, color);
        }
    }
}


/// Shorten `name` with a trailing ellipsis so it fits within `max_w` px.
fn fit_label(font: &Font, name: &str, px: f32, max_w: f32) -> String {
    if text_width(font, name, px) <= max_w {
        return String::from(name);
    }
    let mut out = String::new();
    for c in name.chars() {
        let mut trial = out.clone();
        trial.push(c);
        trial.push('…');
        if text_width(font, &trial, px) > max_w {
            out.push('…');
            return out;
        }
        out.push(c);
    }
    out.push('…');
    out
}

/// A file-type bucket, each mapped to one Breeze mimetype icon.
#[derive(Clone, Copy, PartialEq)]
enum IconKind {
    Folder,
    Text,
    Exec,
    Image,
    Archive,
    Unknown,
}

/// The extension after the last dot (empty if none).
fn ext_of(name: &str) -> &str {
    match name.rfind('.') {
        Some(i) if i + 1 < name.len() => &name[i + 1..],
        _ => "",
    }
}

/// Bucket a listing entry. `exec_dir` marks `/app`, whose extensionless files
/// are program binaries and get the executable icon.
fn classify(name: &str, is_dir: bool, exec_dir: bool) -> IconKind {
    if is_dir {
        return IconKind::Folder;
    }
    match ext_of(name) {
        "txt" | "toml" | "md" | "rs" | "cfg" | "conf" | "log" | "json" | "c" | "h" => IconKind::Text,
        "rgba" | "bmp" | "png" | "jpg" | "jpeg" | "gif" | "ppm" => IconKind::Image,
        "zip" | "gz" | "tar" | "xz" | "iso" => IconKind::Archive,
        "" if exec_dir => IconKind::Exec,
        _ => IconKind::Unknown,
    }
}


/// The six mimetype icons, loaded once from the VFS (each 32x32 RGBA, or `None`
/// if the asset is missing/wrong-sized — the cell then draws a colored chip).
struct Icons {
    folder: Option<Vec<u8>>,
    text: Option<Vec<u8>>,
    exec: Option<Vec<u8>>,
    image: Option<Vec<u8>>,
    archive: Option<Vec<u8>>,
    unknown: Option<Vec<u8>>,
}

/// Slurp a 32x32 RGBA icon, or `None` unless it is exactly `ICON*ICON*4` bytes.
fn read_icon(path: &str) -> Option<Vec<u8>> {
    let fd = libdunit::open(path, libdunit::OPEN_READ);
    if fd < 0 {
        return None;
    }
    let fd = fd as usize;
    let want = (ICON * ICON * 4) as usize;
    let mut data: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 512];
    loop {
        let n = libdunit::read(fd, &mut chunk);
        if n <= 0 {
            break;
        }
        data.extend_from_slice(&chunk[..n as usize]);
        if data.len() > want {
            break;
        }
    }
    libdunit::close(fd);
    if data.len() == want {
        Some(data)
    } else {
        None
    }
}

impl Icons {
    fn load() -> Icons {
        Icons {
            folder: read_icon("/assets/icons/breeze/mime_folder.rgba"),
            text: read_icon("/assets/icons/breeze/mime_text.rgba"),
            exec: read_icon("/assets/icons/breeze/mime_exec.rgba"),
            image: read_icon("/assets/icons/breeze/mime_image.rgba"),
            archive: read_icon("/assets/icons/breeze/mime_archive.rgba"),
            unknown: read_icon("/assets/icons/breeze/mime_unknown.rgba"),
        }
    }

    fn get(&self, k: IconKind) -> Option<&[u8]> {
        match k {
            IconKind::Folder => self.folder.as_deref(),
            IconKind::Text => self.text.as_deref(),
            IconKind::Exec => self.exec.as_deref(),
            IconKind::Image => self.image.as_deref(),
            IconKind::Archive => self.archive.as_deref(),
            IconKind::Unknown => self.unknown.as_deref(),
        }
    }
}


/// One listing entry (owned, so it survives across repaints).
struct Item {
    name: String,
    kind: IconKind,
}

impl Item {
    fn is_dir(&self) -> bool {
        self.kind == IconKind::Folder
    }
}

/// File-manager model: current directory, its (capped) entries and selection.
struct Files {
    path: String,
    items: Vec<Item>,
    /// Selected *item* index (never the ".." cell), or `None`.
    selected: Option<usize>,
    /// Full entry count before the on-screen cap, for the status bar.
    total: usize,
}

impl Files {
    fn new() -> Files {
        let mut f = Files { path: String::from("/"), items: Vec::new(), selected: None, total: 0 };
        f.reload();
        f
    }

    /// Whether a ".." parent cell precedes the entries (everywhere but root).
    fn has_parent(&self) -> bool {
        self.path != "/"
    }

    /// Re-read the current directory (directories first, capped to the grid).
    fn reload(&mut self) {
        self.items.clear();
        self.selected = None;
        let mut raw = [libdunit::DirEntry::empty(); 64];
        let n = libdunit::readdir(&self.path, &mut raw);
        if n < 0 {
            self.total = 0;
            return;
        }
        let exec_dir = self.path == "/app";
        let mut dirs: Vec<Item> = Vec::new();
        let mut files: Vec<Item> = Vec::new();
        for e in raw.iter().take(n as usize) {
            let is_dir = e.file_type == libdunit::FILE_TYPE_DIRECTORY;
            let name = String::from(e.name());
            let kind = classify(&name, is_dir, exec_dir);
            let item = Item { name, kind };
            if is_dir {
                dirs.push(item);
            } else {
                files.push(item);
            }
        }
        self.total = dirs.len() + files.len();
        let cap_items = CAP - self.has_parent() as usize;
        for it in dirs.into_iter().chain(files.into_iter()) {
            if self.items.len() >= cap_items {
                break;
            }
            self.items.push(it);
        }
    }

    /// Navigate to the parent directory (no-op at root).
    fn go_parent(&mut self) {
        if self.path == "/" {
            return;
        }
        let bytes = self.path.as_bytes();
        let mut cut = bytes.len();
        while cut > 1 && bytes[cut - 1] != b'/' {
            cut -= 1;
        }
        let new_len = if cut <= 1 { 1 } else { cut - 1 };
        self.path.truncate(new_len);
        self.reload();
    }

    /// Enter entry `idx` if it is a directory (files are ignored for now).
    fn open(&mut self, idx: usize) {
        if idx >= self.items.len() || !self.items[idx].is_dir() {
            return;
        }
        if self.path != "/" {
            self.path.push('/');
        }
        let name = self.items[idx].name.clone();
        self.path.push_str(&name);
        self.reload();
    }
}


fn push_u32(d: &mut String, mut v: u32) {
    if v == 0 {
        d.push('0');
        return;
    }
    let mut tmp = [0u8; 10];
    let mut n = 0;
    while v > 0 {
        tmp[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
    }
    while n > 0 {
        n -= 1;
        d.push(tmp[n] as char);
    }
}

/// Paint the whole window: header breadcrumb, icon grid, status bar.
fn render(px: *mut u8, font: &Font, files: &Files, icons: &Icons) {
    let pixels = unsafe { core::slice::from_raw_parts_mut(px as *mut u32, (W * H) as usize) };
    let mut s = Surface::new(pixels, W as usize, H as usize);

    s.fill_rect(0.0, 0.0, W as f32, H as f32, BG);

    // Header bar + breadcrumb path.
    s.fill_rect(0.0, 0.0, W as f32, HEADER_H as f32, HEADER_BG);
    let crumb = fit_label(font, &files.path, 15.0, (W as i32 - 2 * PAD) as f32);
    draw_text(&mut s, font, PAD, 20, 15.0, &crumb, ACCENT);

    // Grid of cells: an optional ".." parent cell, then the entries.
    let has_parent = files.has_parent();
    let cell_count = files.items.len() + has_parent as usize;
    for ci in 0..cell_count {
        let col = (ci as i32) % COLS;
        let row = (ci as i32) / COLS;
        let cx = PAD + col * CELL_W;
        let cy = HEADER_H + row * CELL_H;

        let (label, kind, is_sel) = if has_parent && ci == 0 {
            (String::from(".."), IconKind::Folder, false)
        } else {
            let ii = ci - has_parent as usize;
            let it = &files.items[ii];
            (it.name.clone(), it.kind, files.selected == Some(ii))
        };

        if is_sel {
            s.fill_rect((cx + 4) as f32, (cy + 2) as f32, (CELL_W - 8) as f32, (CELL_H - 6) as f32, SEL_BG);
        }

        // Icon centered horizontally near the top of the cell.
        let ix = cx + (CELL_W - ICON) / 2;
        let iy = cy + 6;
        match icons.get(kind) {
            Some(rgba) => s.blit_image(ix, iy, rgba, ICON as usize, ICON as usize),
            None => s.fill_rect(ix as f32, iy as f32, ICON as f32, ICON as f32, MUTED),
        }

        // Label centered under the icon, ellipsized to the cell width.
        let px_lbl = 12.0;
        let text = fit_label(font, &label, px_lbl, (CELL_W - 8) as f32);
        let tw = text_width(font, &text, px_lbl);
        let tx = cx + ((CELL_W as f32 - tw) / 2.0) as i32;
        let baseline = iy + ICON + 16;
        draw_text(&mut s, font, tx, baseline, px_lbl, &text, LABEL);
    }

    // Status bar: entry count (+ hidden overflow) and the selected name.
    s.fill_rect(0.0, (H as i32 - STATUS_H) as f32, W as f32, STATUS_H as f32, STATUS_BG);
    let mut status = String::new();
    push_u32(&mut status, files.total as u32);
    status.push_str(" items");
    if files.items.len() < files.total {
        status.push_str(" (");
        push_u32(&mut status, files.items.len() as u32);
        status.push_str(" shown)");
    }
    if let Some(ii) = files.selected {
        if ii < files.items.len() {
            status.push_str("  \u{2022}  ");
            status.push_str(&files.items[ii].name);
        }
    }
    let sb = fit_label(font, &status, 12.0, (W as i32 - 2 * PAD) as f32);
    draw_text(&mut s, font, PAD, H as i32 - 7, 12.0, &sb, MUTED);
}


/// What a client-local pointer landed on in the grid.
enum Hit {
    Parent,
    Item(usize),
}

/// Map a client-local pointer `(lx, ly)` to a grid cell, if any.
fn hit_cell(files: &Files, lx: i32, ly: i32) -> Option<Hit> {
    if lx < PAD || ly < HEADER_H || ly >= H as i32 - STATUS_H {
        return None;
    }
    let col = (lx - PAD) / CELL_W;
    let row = (ly - HEADER_H) / CELL_H;
    if col < 0 || col >= COLS || row < 0 || row >= ROWS {
        return None;
    }
    let ci = (row * COLS + col) as usize;
    let has_parent = files.has_parent();
    if ci >= files.items.len() + has_parent as usize {
        return None;
    }
    if has_parent && ci == 0 {
        return Some(Hit::Parent);
    }
    Some(Hit::Item(ci - has_parent as usize))
}


#[no_mangle]
pub extern "C" fn _start() -> ! {
    // 1) Handshake: compositor pid + our client id (id is a tint hint only).
    let mut m = [0u8; 8];
    if libdunit::ipc_recv_blocking(&mut m, 0) < 8 {
        libdunit::println("gui_files: FAIL recv handshake");
        libdunit::exit(1);
    }
    let compositor = u32::from_le_bytes([m[0], m[1], m[2], m[3]]);

    // 2) Create + map our own shared buffer.
    let bytes = (W * H * 4) as usize;
    let buf = libdunit::handle_create_shared(bytes);
    if buf <= 0 {
        libdunit::println("gui_files: FAIL create buffer");
        libdunit::exit(2);
    }
    let buf = buf as u32;
    let mapped = libdunit::handle_map(buf, 0, bytes);
    if mapped <= 0 {
        libdunit::println("gui_files: FAIL map buffer");
        libdunit::exit(3);
    }
    let px = mapped as usize as *mut u8;
    let font = match Font::parse(FONT_BYTES.to_vec()) {
        Ok(f) => f,
        Err(_) => {
            libdunit::println("gui_files: FAIL font parse");
            libdunit::exit(7);
        }
    };
    let icons = Icons::load();
    let mut files = Files::new();
    render(px, &font, &files, &icons);

    let mut rx = [0u8; 256];

    // 3) HELLO -> WELCOME.
    let hello = Request::Hello {
        min_minor: 0,
        max_minor: 0,
        offered_features: FEATURE_ARGB8888,
        required_features: 0,
    }
    .encode(0, 1);
    libdunit::ipc_send(compositor, &hello);
    if libdunit::ipc_recv_blocking(&mut rx, 0) <= 0 {
        libdunit::println("gui_files: FAIL welcome");
        libdunit::exit(4);
    }

    // 4) CREATE_SURFACE -> RESULT + CONFIGURE (capture the configure token).
    let create =
        Request::CreateSurface { role: 1, width: W, height: H, format: FMT_XRGB8888 }.encode(SURFACE, 2);
    libdunit::ipc_send(compositor, &create);
    libdunit::ipc_recv_blocking(&mut rx, 0); // RESULT
    let n = libdunit::ipc_recv_blocking(&mut rx, 0); // CONFIGURE
    let token = if n >= 32 { u64_at(&rx, 24) } else { 0 };

    // 5) ACK_CONFIGURE -> RESULT.
    let ack = Request::AckConfigure { configure: token }.encode(SURFACE, 3);
    libdunit::ipc_send(compositor, &ack);
    libdunit::ipc_recv_blocking(&mut rx, 0);


    // 6) Transfer the buffer capability (read-only) and announce it.
    let dup = libdunit::handle_dup(
        buf,
        libdunit::RIGHT_READ | libdunit::RIGHT_MAP | libdunit::RIGHT_TRANSFER,
    );
    let ch = if dup > 0 {
        libdunit::handle_transfer(dup as u32, compositor)
    } else {
        -1
    };
    if ch <= 0 {
        libdunit::println("gui_files: FAIL cap transfer");
        libdunit::exit(5);
    }
    let mut ann = [0u8; 16];
    ann[0..4].copy_from_slice(&CTRL_MAGIC.to_le_bytes());
    ann[4..8].copy_from_slice(&(ch as u32).to_le_bytes());
    ann[8..12].copy_from_slice(&(bytes as u32).to_le_bytes());
    ann[12..16].copy_from_slice(&(BUFFER as u32).to_le_bytes());
    libdunit::ipc_send(compositor, &ann);

    // 7) IMPORT / ATTACH / COMMIT (each -> RESULT).
    let import =
        Request::ImportBuffer { width: W, height: H, stride: W * 4, format: FMT_XRGB8888, offset: 0 }
            .encode(BUFFER, 4);
    libdunit::ipc_send(compositor, &import);
    libdunit::ipc_recv_blocking(&mut rx, 0);
    let attach = Request::AttachBuffer { buffer: BUFFER, damage: alloc::vec::Vec::new() }.encode(SURFACE, 5);
    libdunit::ipc_send(compositor, &attach);
    libdunit::ipc_recv_blocking(&mut rx, 0);
    let commit = Request::Commit { configure: token, frame_callback: 1 }.encode(SURFACE, 6);
    libdunit::ipc_send(compositor, &commit);
    libdunit::ipc_recv_blocking(&mut rx, 0);

    // 8) FRAME_DONE from the compositor's composition tick.
    let n = libdunit::ipc_recv_blocking(&mut rx, 0);
    let ok = n >= 32 && u16::from_le_bytes([rx[8], rx[9]]) == 0x8021;
    if ok {
        libdunit::println("gui_files: surface presented OK");
    } else {
        libdunit::println("gui_files: FAIL no frame done");
        libdunit::handle_close(buf);
        libdunit::exit(6);
    }

    // 9) Interactive loop: pointer-down selects a cell; the ".." cell navigates
    //    up, and a second click on a selected directory enters it. Clicking
    //    empty space clears the selection. Any change repaints the buffer.
    loop {
        let n = libdunit::ipc_recv_blocking(&mut rx, 0);
        if n < 8 || u32::from_le_bytes([rx[0], rx[1], rx[2], rx[3]]) != INPUT_MAGIC {
            continue;
        }
        let kind = rx[4];
        if kind == IN_QUIT {
            break;
        }
        if kind == IN_DOWN {
            let lx = i32::from_le_bytes([rx[8], rx[9], rx[10], rx[11]]);
            let ly = i32::from_le_bytes([rx[12], rx[13], rx[14], rx[15]]);
            let mut dirty = false;
            match hit_cell(&files, lx, ly) {
                Some(Hit::Parent) => {
                    files.go_parent();
                    dirty = true;
                }
                Some(Hit::Item(ii)) => {
                    if files.selected == Some(ii) && files.items[ii].is_dir() {
                        files.open(ii);
                    } else {
                        files.selected = Some(ii);
                    }
                    dirty = true;
                }
                None => {
                    if files.selected.is_some() {
                        files.selected = None;
                        dirty = true;
                    }
                }
            }
            if dirty {
                render(px, &font, &files, &icons);
            }
        }
    }

    libdunit::handle_close(buf);
    libdunit::exit(0)
}

