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

use gui_protocol_v1::wire::{Request, FEATURE_ARGB8888, MAGIC};

use dunit_render::Surface;
use dunit_style::value::Color;
use dunit_text::Font;

// Control-message magics shared with the compositor.
const CTRL_MAGIC: u32 = 0x3150_4143; // "CAP1" — capability announce
const INPUT_MAGIC: u32 = 0x3150_4E49; // "INP1" — pointer input
#[allow(dead_code)] // reserved for the intra-window drag slice (E5)
const IN_MOVE: u8 = 1;
const IN_DOWN: u8 = 2;
#[allow(dead_code)] // reserved for the intra-window drag slice (E5)
const IN_UP: u8 = 3;
const IN_KEY: u8 = 5;
const IN_QUIT: u8 = 9;

// Cooked key bytes the compositor forwards in the IN_KEY `button` field.
const KEY_BACKSPACE: u8 = 0x08;
const KEY_DEL: u8 = 0x7f;
const KEY_ENTER_LF: u8 = 0x0a;
const KEY_ENTER_CR: u8 = 0x0d;
const KEY_ESC: u8 = 0x1b;

const SURFACE: u64 = 1;
const BUFFER: u64 = 2;
const W: u32 = 520;
const H: u32 = 380;
const FMT_XRGB8888: u32 = 1;
const FMT_ARGB8888: u32 = 2;

// --- Grid geometry (client-local pixels) ---
const PAD: i32 = 10;
const HEADER_H: i32 = 30;
const STATUS_H: i32 = 24;
/// Fixed cell size; the column count and row count derive from the live surface
/// size at runtime (see `Files::cols`/`rows`), since the compositor can resize
/// us via a server-pushed CONFIGURE (maximize/restore).
const CELL_W: i32 = 100;
const CELL_H: i32 = 72;
const ICON: i32 = 32;


// --- Palette (config-resolved ARGB via Color) ---
/// Resolved color scheme, built once from `FilesCfg` (config→behavior). Each
/// field is a straight-alpha ARGB `Color`; the chrome colors are forced opaque
/// and the selection wash keeps its configured alpha so it blends over the grid.
#[derive(Clone, Copy)]
struct Palette {
    /// Opaque base fill used for dialog input fields. The window's per-frame base
    /// clear carries the configured `bg_alpha` separately (see `Files::clear_bg`).
    bg: Color,
    header_bg: Color,
    status_bg: Color,
    accent: Color,
    label: Color,
    muted: Color,
    /// Selection wash painted behind the icon+label of the active cell.
    sel_bg: Color,
}

/// Unpack a `0xAARRGGBB` config word into a straight-alpha `Color`.
fn col(argb: u32) -> Color {
    Color::rgba(
        ((argb >> 16) & 0xFF) as u8,
        ((argb >> 8) & 0xFF) as u8,
        (argb & 0xFF) as u8,
        ((argb >> 24) & 0xFF) as u8,
    )
}

/// The font, embedded in the ELF (M4 still ships assets in-image).
static FONT_BYTES: &[u8] = include_bytes!("../../../../assets/fonts/DejaVuSans.ttf");

/// Load the configured TTF (`[desktop] font`) from the VFS, falling back to the
/// embedded `FONT_BYTES` on any error — the desktop font is a live config knob.
fn load_font() -> Result<Font, ()> {
    let cfg = dwm_settings::load();
    if let Some(bytes) = libdunit::read_binary(cfg.desktop.font.as_str(), 4 * 1024 * 1024) {
        if let Ok(f) = Font::parse(bytes) {
            return Ok(f);
        }
    }
    Font::parse(FONT_BYTES.to_vec()).map_err(|_| ())
}

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

/// Build `/assets/icons/<theme>/<name>.rgba` on the stack and load it. An empty
/// theme name falls back to `breeze` so a bad config still shows icons. Keeps the
/// icon theme a shared config knob (same `[desktop] icon_theme` the compositor
/// uses for the dock/launcher).
fn read_theme_icon(theme: &str, name: &str) -> Option<Vec<u8>> {
    const PRE: &[u8] = b"/assets/icons/";
    const SUF: &[u8] = b".rgba";
    let theme = if theme.is_empty() { "breeze" } else { theme };
    let n = PRE.len() + theme.len() + 1 + name.len() + SUF.len();
    let mut buf = [0u8; 192];
    if n > buf.len() {
        return None;
    }
    let mut o = 0;
    buf[o..o + PRE.len()].copy_from_slice(PRE);
    o += PRE.len();
    buf[o..o + theme.len()].copy_from_slice(theme.as_bytes());
    o += theme.len();
    buf[o] = b'/';
    o += 1;
    buf[o..o + name.len()].copy_from_slice(name.as_bytes());
    o += name.len();
    buf[o..o + SUF.len()].copy_from_slice(SUF);
    o += SUF.len();
    let path = core::str::from_utf8(&buf[..o]).ok()?;
    read_icon(path)
}

impl Icons {
    /// Load the six mimetype icons from `theme` (already resolved: the
    /// `[files] icon_theme` override, else the shared `[desktop] icon_theme`).
    fn load(theme: &str) -> Icons {
        Icons {
            folder: read_theme_icon(theme, "mime_folder"),
            text: read_theme_icon(theme, "mime_text"),
            exec: read_theme_icon(theme, "mime_exec"),
            image: read_theme_icon(theme, "mime_image"),
            archive: read_theme_icon(theme, "mime_archive"),
            unknown: read_theme_icon(theme, "mime_unknown"),
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
    /// Copy/cut clipboard: (absolute source path, `true` if cut).
    clip: Option<(String, bool)>,
    /// Live surface size in px. The compositor can resize us via a server-pushed
    /// CONFIGURE, so the grid geometry (columns/rows/cap) derives from these.
    w: i32,
    h: i32,
    /// Config-resolved color scheme (chrome + selection wash).
    pal: Palette,
    /// Per-frame base clear color, carrying the configured `bg_alpha` (a value
    /// < 255 lets the wallpaper show through the file-manager background).
    clear_bg: Color,
    /// Negotiated pixel format: `FMT_XRGB8888` when opaque, `FMT_ARGB8888` when
    /// the base carries alpha. Threaded into every ImportBuffer.
    fmt: u32,
}

impl Files {
    fn new(pal: Palette, clear_bg: Color, fmt: u32) -> Files {
        let mut f = Files {
            path: String::from("/"),
            items: Vec::new(),
            selected: None,
            total: 0,
            clip: None,
            w: W as i32,
            h: H as i32,
            pal,
            clear_bg,
            fmt,
        };
        f.reload();
        f
    }

    /// Columns that fit across the current surface width (at least one).
    fn cols(&self) -> i32 {
        ((self.w - 2 * PAD) / CELL_W).max(1)
    }

    /// Rows that fit between the header and status bar (at least one).
    fn rows(&self) -> i32 {
        ((self.h - HEADER_H - STATUS_H) / CELL_H).max(1)
    }

    /// Number of grid cells visible at the current size (columns × rows).
    fn cap(&self) -> usize {
        (self.cols() * self.rows()) as usize
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
        let cap_items = self.cap().saturating_sub(self.has_parent() as usize);
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

    /// Absolute path of a child `name` in the current directory.
    fn child_path(&self, name: &str) -> String {
        let mut p = self.path.clone();
        if p != "/" {
            p.push('/');
        }
        p.push_str(name);
        p
    }

    /// Absolute path of the selected entry, if any.
    fn selected_path(&self) -> Option<String> {
        self.selected.map(|i| self.child_path(&self.items[i].name))
    }

    /// Create a directory `name` in the current directory, then reload.
    fn make_dir(&mut self, name: &str) {
        if !name.is_empty() {
            libdunit::mkdir(&self.child_path(name));
            self.reload();
        }
    }

    /// Rename the selected entry to `new`, then reload.
    fn rename_selected(&mut self, new: &str) {
        if new.is_empty() {
            return;
        }
        if let Some(i) = self.selected {
            let old = self.child_path(&self.items[i].name);
            libdunit::rename(&old, &self.child_path(new));
            self.reload();
        }
    }

    /// Delete the selected entry (files only — the kernel refuses directories),
    /// then reload.
    fn delete_selected(&mut self) {
        if let Some(p) = self.selected_path() {
            libdunit::unlink(&p);
            self.reload();
        }
    }

    /// Put the selected entry on the clipboard (`cut` marks a move).
    fn clip_selected(&mut self, cut: bool) {
        if let Some(p) = self.selected_path() {
            self.clip = Some((p, cut));
        }
    }

    /// Paste the clipboard entry into the current directory. A cut is a rename
    /// (move); a copy duplicates the file's bytes. Reloads afterwards.
    fn paste(&mut self) {
        let (src, cut) = match self.clip.clone() {
            Some(c) => c,
            None => return,
        };
        let base = basename(&src);
        let dst = self.child_path(base);
        if src == dst {
            return;
        }
        if cut {
            libdunit::rename(&src, &dst);
            self.clip = None;
        } else {
            copy_file(&src, &dst);
        }
        self.reload();
    }
}

/// The final path component of `path` (after the last '/').
fn basename(path: &str) -> &str {
    match path.rfind('/') {
        Some(i) => &path[i + 1..],
        None => path,
    }
}

/// Copy the file at `src` to `dst` byte-for-byte (best-effort; directories and
/// read/create failures are silently skipped — the caller reloads regardless).
fn copy_file(src: &str, dst: &str) {
    let rf = libdunit::open(src, libdunit::OPEN_READ);
    if rf < 0 {
        return;
    }
    let rf = rf as usize;
    let wf = libdunit::open(dst, libdunit::OPEN_CREATE | libdunit::OPEN_WRITE | libdunit::OPEN_TRUNC);
    if wf < 0 {
        libdunit::close(rf);
        return;
    }
    let wf = wf as usize;
    let mut chunk = [0u8; 512];
    loop {
        let n = libdunit::read(rf, &mut chunk);
        if n <= 0 {
            break;
        }
        libdunit::write(wf, &chunk[..n as usize]);
    }
    libdunit::close(rf);
    libdunit::close(wf);
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

/// Paint the whole window: header breadcrumb, icon grid, status bar, and any
/// active overlay (context menu / text-entry / delete-confirm).
fn render(px: *mut u8, font: &Font, files: &Files, icons: &Icons, mode: &Mode) {
    let w = files.w;
    let h = files.h;
    let cols = files.cols();
    let pixels = unsafe { core::slice::from_raw_parts_mut(px as *mut u32, (w * h) as usize) };
    let mut s = Surface::new(pixels, w as usize, h as usize);

    // Straight-alpha base: `clear` writes the configured bg (with its alpha)
    // over every pixel — unlike `fill_rect`, which would blend over and drift
    // the alpha of the previous frame, breaking a translucent background.
    s.clear(files.clear_bg);

    // Header bar + breadcrumb path.
    s.fill_rect(0.0, 0.0, w as f32, HEADER_H as f32, files.pal.header_bg);
    let crumb = fit_label(font, &files.path, 15.0, (w - 2 * PAD) as f32);
    draw_text(&mut s, font, PAD, 20, 15.0, &crumb, files.pal.accent);

    // Grid of cells: an optional ".." parent cell, then the entries.
    let has_parent = files.has_parent();
    let cell_count = files.items.len() + has_parent as usize;
    for ci in 0..cell_count {
        let col = (ci as i32) % cols;
        let row = (ci as i32) / cols;
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
            s.fill_rect((cx + 4) as f32, (cy + 2) as f32, (CELL_W - 8) as f32, (CELL_H - 6) as f32, files.pal.sel_bg);
        }

        // Icon centered horizontally near the top of the cell.
        let ix = cx + (CELL_W - ICON) / 2;
        let iy = cy + 6;
        match icons.get(kind) {
            Some(rgba) => s.blit_image(ix, iy, rgba, ICON as usize, ICON as usize),
            None => s.fill_rect(ix as f32, iy as f32, ICON as f32, ICON as f32, files.pal.muted),
        }

        // Label centered under the icon, ellipsized to the cell width.
        let px_lbl = 12.0;
        let text = fit_label(font, &label, px_lbl, (CELL_W - 8) as f32);
        let tw = text_width(font, &text, px_lbl);
        let tx = cx + ((CELL_W as f32 - tw) / 2.0) as i32;
        let baseline = iy + ICON + 16;
        draw_text(&mut s, font, tx, baseline, px_lbl, &text, files.pal.label);
    }

    // Status bar: entry count (+ hidden overflow) and the selected name.
    s.fill_rect(0.0, (h - STATUS_H) as f32, w as f32, STATUS_H as f32, files.pal.status_bg);
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
    let sb = fit_label(font, &status, 12.0, (w - 2 * PAD) as f32);
    draw_text(&mut s, font, PAD, h - 7, 12.0, &sb, files.pal.muted);

    // Overlays on top of the base view.
    match mode {
        Mode::Browse => {}
        Mode::Menu { mx, my } => draw_menu(&mut s, font, files, *mx, *my),
        Mode::Text { rename, buf } => draw_text_box(&mut s, font, &files.pal, w, h, *rename, buf),
        Mode::Confirm => draw_confirm(&mut s, font, files),
    }
}


/// What a client-local pointer landed on in the grid.
enum Hit {
    Parent,
    Item(usize),
}

/// Map a client-local pointer `(lx, ly)` to a grid cell, if any.
fn hit_cell(files: &Files, lx: i32, ly: i32) -> Option<Hit> {
    if lx < PAD || ly < HEADER_H || ly >= files.h - STATUS_H {
        return None;
    }
    let cols = files.cols();
    let col = (lx - PAD) / CELL_W;
    let row = (ly - HEADER_H) / CELL_H;
    if col < 0 || col >= cols || row < 0 || row >= files.rows() {
        return None;
    }
    let ci = (row * cols + col) as usize;
    let has_parent = files.has_parent();
    if ci >= files.items.len() + has_parent as usize {
        return None;
    }
    if has_parent && ci == 0 {
        return Some(Hit::Parent);
    }
    Some(Hit::Item(ci - has_parent as usize))
}


/// The interaction mode: plain browsing, a context menu at `(mx, my)`, a
/// text-entry overlay (for New Folder / Rename), or a delete confirmation.
enum Mode {
    Browse,
    Menu { mx: i32, my: i32 },
    Text { rename: bool, buf: String },
    Confirm,
}

/// A context-menu command.
#[derive(Clone, Copy)]
enum Action {
    NewFolder,
    Rename,
    Delete,
    Copy,
    Cut,
    Paste,
}

/// The fixed context-menu rows, in display order.
const MENU_ACTIONS: [(Action, &str); 6] = [
    (Action::NewFolder, "New Folder"),
    (Action::Rename, "Rename"),
    (Action::Delete, "Delete"),
    (Action::Copy, "Copy"),
    (Action::Cut, "Cut"),
    (Action::Paste, "Paste"),
];

// --- Overlay geometry ---
const MENU_W: i32 = 140;
const MENU_ROW_H: i32 = 24;
const MENU_H: i32 = MENU_ROW_H * MENU_ACTIONS.len() as i32;
const DLG_W: i32 = 320;
const DLG_H: i32 = 96;

/// Whether `a` is applicable given the current selection / clipboard state.
fn action_enabled(a: Action, files: &Files) -> bool {
    match a {
        Action::NewFolder => true,
        Action::Rename | Action::Delete | Action::Copy | Action::Cut => files.selected.is_some(),
        Action::Paste => files.clip.is_some(),
    }
}

/// Clamp a menu opened at `(mx, my)` so it stays fully inside a `w`×`h` window.
fn menu_origin(w: i32, h: i32, mx: i32, my: i32) -> (i32, i32) {
    let ox = mx.min(w - MENU_W - 2).max(2);
    let oy = my.min(h - MENU_H - 2).max(2);
    (ox, oy)
}

/// The menu row a client-local pointer `(lx, ly)` landed on, if inside the menu.
fn menu_hit(w: i32, h: i32, mx: i32, my: i32, lx: i32, ly: i32) -> Option<usize> {
    let (ox, oy) = menu_origin(w, h, mx, my);
    if lx < ox || lx >= ox + MENU_W || ly < oy || ly >= oy + MENU_H {
        return None;
    }
    let row = ((ly - oy) / MENU_ROW_H) as usize;
    if row < MENU_ACTIONS.len() {
        Some(row)
    } else {
        None
    }
}

/// Draw the context menu at its clamped origin.
fn draw_menu(s: &mut Surface, font: &Font, files: &Files, mx: i32, my: i32) {
    let (ox, oy) = menu_origin(files.w, files.h, mx, my);
    s.fill_rect(ox as f32, oy as f32, MENU_W as f32, MENU_H as f32, files.pal.header_bg);
    s.stroke_rect(ox as f32, oy as f32, MENU_W as f32, MENU_H as f32, 1.0, files.pal.accent);
    for (i, (action, label)) in MENU_ACTIONS.iter().enumerate() {
        let ry = oy + i as i32 * MENU_ROW_H;
        let color = if action_enabled(*action, files) { files.pal.label } else { files.pal.muted };
        draw_text(s, font, ox + 10, ry + MENU_ROW_H - 8, 13.0, label, color);
    }
}

/// Draw a centered dialog box (bg + accent border) in a `w`×`h` window and
/// return its origin.
fn draw_dialog(s: &mut Surface, pal: &Palette, w: i32, h: i32) -> (i32, i32) {
    let ox = (w - DLG_W) / 2;
    let oy = (h - DLG_H) / 2;
    s.fill_rect(0.0, 0.0, w as f32, h as f32, Color::rgba(0, 0, 0, 0x70)); // scrim
    s.fill_rect(ox as f32, oy as f32, DLG_W as f32, DLG_H as f32, pal.header_bg);
    s.stroke_rect(ox as f32, oy as f32, DLG_W as f32, DLG_H as f32, 1.0, pal.accent);
    (ox, oy)
}

/// Draw the text-entry overlay (New Folder / Rename) with the current buffer.
fn draw_text_box(s: &mut Surface, font: &Font, pal: &Palette, w: i32, h: i32, rename: bool, buf: &str) {
    let (ox, oy) = draw_dialog(s, pal, w, h);
    let title = if rename { "Rename to:" } else { "New folder name:" };
    draw_text(s, font, ox + 14, oy + 26, 14.0, title, pal.accent);
    // Input field.
    let fx = ox + 14;
    let fy = oy + 38;
    let fw = DLG_W - 28;
    s.fill_rect(fx as f32, fy as f32, fw as f32, 22.0, pal.bg);
    s.stroke_rect(fx as f32, fy as f32, fw as f32, 22.0, 1.0, pal.muted);
    let shown = fit_label(font, buf, 13.0, (fw - 12) as f32);
    draw_text(s, font, fx + 6, fy + 16, 13.0, &shown, pal.label);
    // Caret after the text.
    let cw = text_width(font, &shown, 13.0);
    let cx = fx + 6 + round_i32(cw);
    s.fill_rect(cx as f32, (fy + 4) as f32, 1.0, 14.0, pal.label);
    draw_text(s, font, ox + 14, oy + DLG_H - 10, 11.0, "[Enter] ok   [Esc] cancel", pal.muted);
}

/// Draw the delete-confirmation overlay for the selected entry.
fn draw_confirm(s: &mut Surface, font: &Font, files: &Files) {
    let (ox, oy) = draw_dialog(s, &files.pal, files.w, files.h);
    draw_text(s, font, ox + 14, oy + 26, 14.0, "Delete this file?", files.pal.accent);
    let name = files
        .selected
        .and_then(|i| files.items.get(i))
        .map(|it| it.name.as_str())
        .unwrap_or("");
    let shown = fit_label(font, name, 13.0, (DLG_W - 28) as f32);
    draw_text(s, font, ox + 14, oy + 50, 13.0, &shown, files.pal.label);
    draw_text(s, font, ox + 14, oy + DLG_H - 10, 11.0, "[Enter] delete   [Esc] cancel", files.pal.muted);
}


/// Re-negotiate the surface at a new size after a server-pushed CONFIGURE: back
/// a FRESH kernel buffer of `new_w*new_h`, re-read the directory at the new cell
/// cap, render into the new buffer, then replay the buffer half of the handshake
/// with a NEW object id (the compositor's freshness gate rejects a re-used id).
/// Returns the new `(handle, mapped ptr)`; the OLD buffer handle is closed.
/// `serial` advances so every request keeps a unique, increasing client serial.
#[allow(clippy::too_many_arguments)]
fn resize_surface(
    compositor: u32,
    old_buf: u32,
    font: &Font,
    files: &mut Files,
    icons: &Icons,
    mode: &Mode,
    new_w: u32,
    new_h: u32,
    obj: u64,
    token: u64,
    serial: &mut u64,
) -> Option<(u32, *mut u8)> {
    let bytes = (new_w * new_h * 4) as usize;
    let nb = libdunit::handle_create_shared(bytes);
    if nb <= 0 {
        return None;
    }
    let nb = nb as u32;
    let mapped = libdunit::handle_map(nb, 0, bytes);
    if mapped <= 0 {
        libdunit::handle_close(nb);
        return None;
    }
    let npx = mapped as usize as *mut u8;
    // Adopt the new geometry and re-cap the listing to the new grid, then paint.
    files.w = new_w as i32;
    files.h = new_h as i32;
    files.reload();
    render(npx, font, files, icons, mode);

    // ACK the new configure token so the compositor accepts our next COMMIT.
    let ack = Request::AckConfigure { configure: token }.encode(SURFACE, *serial);
    *serial += 1;
    libdunit::ipc_send(compositor, &ack);

    // Transfer the fresh buffer (read-only) and announce {handle, size, object}.
    let dup = libdunit::handle_dup(
        nb,
        libdunit::RIGHT_READ | libdunit::RIGHT_MAP | libdunit::RIGHT_TRANSFER,
    );
    let ch = if dup > 0 {
        libdunit::handle_transfer(dup as u32, compositor)
    } else {
        -1
    };
    if ch <= 0 {
        libdunit::handle_close(nb);
        return None;
    }
    let mut ann = [0u8; 16];
    ann[0..4].copy_from_slice(&CTRL_MAGIC.to_le_bytes());
    ann[4..8].copy_from_slice(&(ch as u32).to_le_bytes());
    ann[8..12].copy_from_slice(&(bytes as u32).to_le_bytes());
    ann[12..16].copy_from_slice(&(obj as u32).to_le_bytes());
    libdunit::ipc_send(compositor, &ann);

    // IMPORT (fresh object id) / ATTACH / COMMIT (new token).
    let import = Request::ImportBuffer {
        width: new_w,
        height: new_h,
        stride: new_w * 4,
        format: files.fmt,
        offset: 0,
    }
    .encode(obj, *serial);
    *serial += 1;
    libdunit::ipc_send(compositor, &import);
    let attach = Request::AttachBuffer { buffer: obj, damage: Vec::new() }.encode(SURFACE, *serial);
    *serial += 1;
    libdunit::ipc_send(compositor, &attach);
    let commit = Request::Commit { configure: token, frame_callback: 1 }.encode(SURFACE, *serial);
    *serial += 1;
    libdunit::ipc_send(compositor, &commit);

    // Release the old backing buffer; the compositor now blits the new one.
    libdunit::handle_close(old_buf);
    Some((nb, npx))
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    // 0) Load this app's config (apps/gui_files.toml). A missing/garbage file
    // yields the built-in baseline. The resolved values below actually drive the
    // color scheme, icon theme and background transparency (config→behavior).
    let fcfg = dwm_settings::FilesCfg::load("gui_files");
    libdunit::println(&alloc::format!(
        "gui_files: cfg from_file={} icon_theme={} bg={:#010X} accent={:#010X} bg_alpha={}",
        fcfg.from_file as u32,
        if fcfg.icon_theme.as_str().is_empty() { "<desktop>" } else { fcfg.icon_theme.as_str() },
        fcfg.bg,
        fcfg.accent,
        fcfg.bg_alpha,
    ));

    // Resolve the color scheme. Chrome colors are forced opaque; the base clear
    // carries the configured `bg_alpha` (a value < 255 makes the window
    // translucent, so ImportBuffer must advertise ARGB rather than XRGB).
    let pal = Palette {
        bg: col(fcfg.bg | 0xFF00_0000),
        header_bg: col(fcfg.header_bg | 0xFF00_0000),
        status_bg: col(fcfg.status_bg | 0xFF00_0000),
        accent: col(fcfg.accent | 0xFF00_0000),
        label: col(fcfg.label | 0xFF00_0000),
        muted: col(fcfg.muted | 0xFF00_0000),
        sel_bg: col(fcfg.sel_bg),
    };
    let clear_bg = col((fcfg.bg_alpha << 24) | (fcfg.bg & 0x00FF_FFFF));
    let fmt = if fcfg.bg_alpha < 255 { FMT_ARGB8888 } else { FMT_XRGB8888 };
    // Icon theme: the per-app `[files] icon_theme` override, else the shared
    // `[desktop] icon_theme`. `read_theme_icon` still falls back to "breeze".
    let icon_theme = if fcfg.icon_theme.as_str().is_empty() {
        dwm_settings::load().desktop.icon_theme
    } else {
        fcfg.icon_theme
    };

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
    let mut px = mapped as usize as *mut u8;
    let font = match load_font() {
        Ok(f) => f,
        Err(_) => {
            libdunit::println("gui_files: FAIL font parse");
            libdunit::exit(7);
        }
    };
    let icons = Icons::load(icon_theme.as_str());
    let mut files = Files::new(pal, clear_bg, fmt);
    let mut mode = Mode::Browse;
    // The surface's live geometry. The compositor may resize us via a
    // server-pushed CONFIGURE, so track it rather than reusing the W/H consts.
    let mut cur_buf = buf;
    let mut cur_w = W;
    let mut cur_h = H;
    render(px, &font, &files, &icons, &mode);

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
        Request::CreateSurface { role: 1, width: W, height: H, format: fmt }.encode(SURFACE, 2);
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
        Request::ImportBuffer { width: W, height: H, stride: W * 4, format: fmt, offset: 0 }
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

    // 9) Interactive loop. Left-click selects / opens; the ".." cell navigates
    //    up. Right-click (button=1) opens a context menu whose rows drive the
    //    file operations (New Folder / Rename / Delete / Copy / Cut / Paste).
    //    Text-entry and delete-confirm overlays consume IN_KEY. Any state change
    //    repaints the buffer.
    let mut next_obj: u64 = 3; // fresh buffer object id per resize (> import id 2)
    let mut serial: u64 = 7; // client request serial, continues past the handshake
    loop {
        let n = libdunit::ipc_recv_blocking(&mut rx, 0);
        if n < 8 {
            continue;
        }
        let magic = u32::from_le_bytes([rx[0], rx[1], rx[2], rx[3]]);
        // Server-pushed CONFIGURE (opcode 0x8010): the compositor resized us
        // (maximize/restore). Re-negotiate a fresh buffer at the new geometry,
        // then keep browsing at the new grid size.
        if magic == MAGIC && n >= 48 && u16::from_le_bytes([rx[8], rx[9]]) == 0x8010 {
            let tok = u64_at(&rx, 24);
            let nw = u32::from_le_bytes([rx[32], rx[33], rx[34], rx[35]]);
            let nh = u32::from_le_bytes([rx[36], rx[37], rx[38], rx[39]]);
            if nw == 0 || nh == 0 || (nw == cur_w && nh == cur_h) {
                let ack = Request::AckConfigure { configure: tok }.encode(SURFACE, serial);
                serial += 1;
                libdunit::ipc_send(compositor, &ack);
            } else if let Some((nb, npx)) = resize_surface(
                compositor, cur_buf, &font, &mut files, &icons, &mode, nw, nh, next_obj, tok,
                &mut serial,
            ) {
                cur_buf = nb;
                px = npx;
                cur_w = nw;
                cur_h = nh;
                next_obj += 1;
                libdunit::println("gui_files: reconfigured OK");
            }
            continue;
        }
        if magic != INPUT_MAGIC {
            continue;
        }
        let kind = rx[4];
        if kind == IN_QUIT {
            break;
        }
        let lx = i32::from_le_bytes([rx[8], rx[9], rx[10], rx[11]]);
        let ly = i32::from_le_bytes([rx[12], rx[13], rx[14], rx[15]]);
        let button = i32::from_le_bytes([rx[16], rx[17], rx[18], rx[19]]);
        let mut dirty = false;

        if kind == IN_DOWN {
            // Snapshot the mode kind so we can freely reassign `mode` below.
            enum Mk {
                Browse,
                Menu(i32, i32),
                Text,
                Confirm,
            }
            let mk = match &mode {
                Mode::Browse => Mk::Browse,
                Mode::Menu { mx, my } => Mk::Menu(*mx, *my),
                Mode::Text { .. } => Mk::Text,
                Mode::Confirm => Mk::Confirm,
            };
            match mk {
                Mk::Browse => {
                    if button == 1 {
                        // Right-click: select the cell under the cursor (if any)
                        // so item-specific actions target it, then open the menu.
                        if let Some(Hit::Item(ii)) = hit_cell(&files, lx, ly) {
                            files.selected = Some(ii);
                        }
                        mode = Mode::Menu { mx: lx, my: ly };
                        dirty = true;
                    } else {
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
                    }
                }
                Mk::Menu(mx, my) => {
                    match menu_hit(files.w, files.h, mx, my, lx, ly) {
                        Some(row) => {
                            let (action, _) = MENU_ACTIONS[row];
                            if action_enabled(action, &files) {
                                match action {
                                    Action::NewFolder => {
                                        mode = Mode::Text { rename: false, buf: String::new() };
                                    }
                                    Action::Rename => {
                                        let cur = files
                                            .selected
                                            .map(|i| files.items[i].name.clone())
                                            .unwrap_or_default();
                                        mode = Mode::Text { rename: true, buf: cur };
                                    }
                                    Action::Delete => mode = Mode::Confirm,
                                    Action::Copy => {
                                        files.clip_selected(false);
                                        mode = Mode::Browse;
                                    }
                                    Action::Cut => {
                                        files.clip_selected(true);
                                        mode = Mode::Browse;
                                    }
                                    Action::Paste => {
                                        files.paste();
                                        mode = Mode::Browse;
                                    }
                                }
                            } else {
                                mode = Mode::Browse;
                            }
                            dirty = true;
                        }
                        None => {
                            // Click outside the menu dismisses it.
                            mode = Mode::Browse;
                            dirty = true;
                        }
                    }
                }
                Mk::Text => {
                    // A click outside the text field cancels the entry.
                    mode = Mode::Browse;
                    dirty = true;
                }
                Mk::Confirm => {}
            }
        } else if kind == IN_KEY {
            let ascii = rx[16];
            let mut transition: Option<Mode> = None;
            let mut do_mkdir: Option<String> = None;
            let mut do_rename: Option<String> = None;
            let mut do_delete = false;
            match &mut mode {
                Mode::Text { rename, buf } => {
                    if ascii == KEY_ESC {
                        transition = Some(Mode::Browse);
                    } else if ascii == KEY_ENTER_LF || ascii == KEY_ENTER_CR {
                        if *rename {
                            do_rename = Some(buf.clone());
                        } else {
                            do_mkdir = Some(buf.clone());
                        }
                        transition = Some(Mode::Browse);
                    } else if ascii == KEY_BACKSPACE || ascii == KEY_DEL {
                        buf.pop();
                        dirty = true;
                    } else if (0x20..0x7f).contains(&ascii) {
                        buf.push(ascii as char);
                        dirty = true;
                    }
                }
                Mode::Confirm => {
                    if ascii == KEY_ENTER_LF || ascii == KEY_ENTER_CR {
                        do_delete = true;
                        transition = Some(Mode::Browse);
                    } else if ascii == KEY_ESC {
                        transition = Some(Mode::Browse);
                    }
                }
                Mode::Menu { .. } => {
                    if ascii == KEY_ESC {
                        transition = Some(Mode::Browse);
                    }
                }
                Mode::Browse => {
                    if ascii == KEY_ESC && files.selected.is_some() {
                        files.selected = None;
                        dirty = true;
                    }
                }
            }
            if let Some(name) = do_mkdir {
                files.make_dir(&name);
                dirty = true;
            }
            if let Some(name) = do_rename {
                files.rename_selected(&name);
                dirty = true;
            }
            if do_delete {
                files.delete_selected();
                dirty = true;
            }
            if let Some(m) = transition {
                mode = m;
                dirty = true;
            }
        }

        if dirty {
            render(px, &font, &files, &icons, &mode);
        }
    }

    libdunit::handle_close(cur_buf);
    libdunit::exit(0)
}

