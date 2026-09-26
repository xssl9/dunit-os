#![no_std]
#![no_main]

//! Untrusted file-manager client for the M4 userspace DWM (Stack B).
//!
//! The legacy kernel-GUI `gui_file_manager` reborn as a gui-v1 client. Same
//! capability + wire-protocol path as `gui_client`, painted through the same M4
//! UI Runtime (DUI + DSS + TTF text). It lists the VFS with `libdunit::readdir`
//! and turns compositor pointer events into navigation: the first row goes to
//! the parent directory, each following row opens that entry (directories only).

use core::panic::PanicInfo;

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use gui_protocol_v1::wire::{Request, FEATURE_ARGB8888};

use dunit_render::{paint, Surface};
use dunit_style::cascade::{Cascade, NodeStyle};
use dunit_style::parse as parse_dss;
use dunit_text::Font;
use dunit_ui::layout::layout_measured;
use dunit_ui::parse as parse_dui;
use dunit_ui::tree::{Kind, NodeId};
use dunit_widgets::{intrinsic_size, FontMeasure, Widget};

// Control-message magics shared with the compositor.
const CTRL_MAGIC: u32 = 0x3150_4143; // "CAP1" — capability announce
const INPUT_MAGIC: u32 = 0x3150_4E49; // "INP1" — pointer input
const IN_DOWN: u8 = 2;
const IN_QUIT: u8 = 9;

const SURFACE: u64 = 1;
const BUFFER: u64 = 2;
const W: u32 = 460;
const H: u32 = 340;
const FMT_XRGB8888: u32 = 1;

/// Cap on listed entries so one huge directory cannot blow the DUI/layout budget.
const MAX_ROWS: usize = 22;

/// The window's text font, embedded in the ELF (M4 still ships assets in-image).
static FONT_BYTES: &[u8] = include_bytes!("../../../../assets/fonts/DejaVuSans.ttf");

// APPEND_MARKER

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

/// One listing entry (owned, so it survives across repaints).
struct Item {
    name: String,
    is_dir: bool,
}

/// File-manager model: the current directory path and its listed entries.
struct Files {
    path: String,
    items: Vec<Item>,
}

impl Files {
    fn new() -> Files {
        let mut f = Files { path: String::from("/"), items: Vec::new() };
        f.reload();
        f
    }

    /// Re-read the current directory into `items` (directories first, capped).
    fn reload(&mut self) {
        self.items.clear();
        let mut raw = [libdunit::DirEntry::empty(); 64];
        let n = libdunit::readdir(&self.path, &mut raw);
        if n < 0 {
            return;
        }
        let mut dirs: Vec<Item> = Vec::new();
        let mut files: Vec<Item> = Vec::new();
        for e in raw.iter().take(n as usize) {
            let is_dir = e.file_type == libdunit::FILE_TYPE_DIRECTORY;
            let item = Item { name: String::from(e.name()), is_dir };
            if is_dir {
                dirs.push(item);
            } else {
                files.push(item);
            }
        }
        for it in dirs.into_iter().chain(files.into_iter()) {
            if self.items.len() >= MAX_ROWS {
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
        // Drop the trailing slash unless we land back on root.
        let new_len = if cut <= 1 { 1 } else { cut - 1 };
        self.path.truncate(new_len);
        self.reload();
    }

    /// Open entry `idx`; directories become the new path, files are ignored.
    fn open(&mut self, idx: usize) {
        if idx >= self.items.len() || !self.items[idx].is_dir {
            return;
        }
        if self.path != "/" {
            self.path.push('/');
        }
        // `push_str` after the (possibly root) slash yields e.g. "/app".
        let name = self.items[idx].name.clone();
        self.path.push_str(&name);
        self.reload();
    }
}

// APPEND_MARKER2

/// Paint the listing and return each row's client-local rect for hit-testing.
/// Row 0 is the ".." parent row; row `k` (k>=1) maps to `files.items[k-1]`.
fn render(px: *mut u8, font: &Font, files: &Files) -> Vec<(i32, i32, i32, i32)> {
    // Build the DUI: a title showing the path, then one named Text row per line.
    let mut dui = String::new();
    dui.push_str("Column#win { Text#title \"");
    push_escaped(&mut dui, &files.path);
    dui.push_str("\" Text#row0 \".. (up)\" ");
    for (i, it) in files.items.iter().enumerate() {
        dui.push_str("Text#row");
        push_u32(&mut dui, (i + 1) as u32);
        dui.push_str(" \"");
        dui.push_str(if it.is_dir { "[D] " } else { "    " });
        push_escaped(&mut dui, &it.name);
        dui.push_str("\" ");
    }
    dui.push('}');

    let tree = match parse_dui(&dui) {
        Ok(t) => t,
        Err(_) => return Vec::new(),
    };
    let mut dss = String::new();
    dss.push_str("Column#win { background: #121820; padding: 8; }\n");
    dss.push_str("Text { color: #cdd6f4; font-size: 13; padding: 1; }\n");
    dss.push_str("Text#title { color: #2f8f5a; font-size: 16; padding: 4; }\n");
    dss.push_str("Text#row0 { color: #caa84a; }\n");
    let sheet = match parse_dss(&dss) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let mut cas = Cascade::new();
    cas.push(sheet);

    let style_of = |nid: NodeId| {
        let node = tree.node(nid);
        let tag = node.kind.tag();
        let ns = match node.name.as_deref() {
            Some(name) => NodeStyle { element: tag, id: Some(name), classes: &[], states: &[] },
            None => NodeStyle::element(tag),
        };
        cas.resolve(&ns)
    };

    let fm = FontMeasure { font };
    let measure_fn = |nid: NodeId| {
        let node = tree.node(nid);
        if let Kind::Element(tag) = &node.kind {
            if let Some(w) = Widget::from_tag(tag) {
                return intrinsic_size(w, node.text.as_deref(), &style_of(nid), &fm);
            }
        }
        (0.0, 0.0)
    };
    let lay = layout_measured(&tree, W as f32, H as f32, &measure_fn);

    let pixels = unsafe { core::slice::from_raw_parts_mut(px as *mut u32, (W * H) as usize) };
    let mut surface = Surface::new(pixels, W as usize, H as usize);
    paint(&tree, &lay, &style_of, font, &mut surface);

    // Collect row rects (client-local) so the input loop can map clicks to rows.
    let mut rects = Vec::new();
    for i in 0..=files.items.len() {
        let mut nm = String::from("row");
        push_u32(&mut nm, i as u32);
        if let Some(nid) = tree.by_name(&nm) {
            let r = lay.rect(nid);
            rects.push((r.x as i32, r.y as i32, r.w as i32, r.h as i32));
        } else {
            rects.push((0, 0, 0, 0));
        }
    }
    rects
}

/// Append `s` to a DUI string, dropping characters that would break parsing.
fn push_escaped(d: &mut String, s: &str) {
    for c in s.chars() {
        if c != '"' && c != '{' && c != '}' {
            d.push(c);
        }
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

// APPEND_MARKER3

#[no_mangle]
pub extern "C" fn _start() -> ! {
    // 1) Handshake: compositor pid + our client id (id is a tint hint only).
    let mut m = [0u8; 8];
    if libdunit::ipc_recv_blocking(&mut m, 0) < 8 {
        libdunit::println("gui_files: FAIL recv handshake");
        libdunit::exit(1);
    }
    let compositor = u32::from_le_bytes([m[0], m[1], m[2], m[3]]);

    // 2) Create + map our own shared buffer and paint the initial listing.
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
    let mut files = Files::new();
    let mut rows = render(px, &font, &files);

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

    // 9) Interactive loop: a pointer-down in row 0 goes to the parent, in any
    //    other row opens that entry. On navigation we repaint our shared buffer
    //    (the compositor blits it every tick), and refresh the row rects.
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
            let ly = i32::from_le_bytes([rx[12], rx[13], rx[14], rx[15]]);
            let mut acted = false;
            for (i, r) in rows.iter().enumerate() {
                if ly >= r.1 && ly < r.1 + r.3 && r.3 > 0 {
                    if i == 0 {
                        files.go_parent();
                    } else {
                        files.open(i - 1);
                    }
                    acted = true;
                    break;
                }
            }
            if acted {
                rows = render(px, &font, &files);
            }
        }
    }

    libdunit::handle_close(buf);
    libdunit::exit(0)
}

