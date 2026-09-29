#![no_std]
#![no_main]

//! Untrusted terminal client for the M4 userspace DWM (Stack B).
//!
//! The last legacy GUI app reborn on the userspace stack. It owns a PTY, spawns
//! the `dsh` shell as the slave, and bridges three streams: keystrokes the
//! compositor routes to the focused window (IN_KEY) are written into the pty
//! master; the shell's stdout is drained from the master into an on-screen
//! scrollback; that scrollback is painted into our own shared buffer via the M4
//! UI Runtime (DUI + DSS + TTF). Same capability + wire-protocol path as
//! gui_client/gui_stat — the compositor owns display/input, we own our surface.

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
const INPUT_MAGIC: u32 = 0x3150_4E49; // "INP1" — input events
const IN_KEY: u8 = 5;
const IN_SCROLL: u8 = 6;
const IN_QUIT: u8 = 9;

// Modifier bit in the IN_KEY `mods` field (mirrors libdunit::KEYMOD_CTRL).
const KEYMOD_CTRL: u8 = 1 << 1;

// PS/2 scancodes (sc & 0x7F) for keys the kernel leaves un-cooked (ascii = 0).
const SC_BACKSPACE: u8 = 0x0E;
const SC_HOME: u8 = 0x47;
const SC_UP: u8 = 0x48;
const SC_LEFT: u8 = 0x4B;
const SC_RIGHT: u8 = 0x4D;
const SC_END: u8 = 0x4F;
const SC_DOWN: u8 = 0x50;
const SC_DELETE: u8 = 0x53;
// Page keys are handled locally for scrollback (never forwarded to the pty).
const SC_PGUP: u8 = 0x49;
const SC_PGDN: u8 = 0x51;

/// Translate a widened IN_KEY event (scancode + mods + cooked ASCII) into the
/// byte sequence to feed the pty. Returns the number of bytes written to `out`.
///
/// - Ctrl + letter -> the control code (`c & 0x1F`): Ctrl-C=0x03, Ctrl-D=0x04,
///   Ctrl-L=0x0C, etc.
/// - Any other cooked byte (printable, Enter `\n`, Tab `\t`) -> itself.
/// - Backspace (no ASCII) -> 0x08.
/// - Arrows / Home / End / Delete (no ASCII) -> the usual xterm ESC sequences,
///   which `dsh` parses for line editing and history.
fn encode_key(scancode: u8, mods: u8, ascii: u8, out: &mut [u8; 4]) -> usize {
    if mods & KEYMOD_CTRL != 0 && ascii.is_ascii_alphabetic() {
        out[0] = ascii & 0x1F;
        return 1;
    }
    if ascii != 0 {
        out[0] = ascii;
        return 1;
    }
    // Extended / navigation keys arrive with ascii == 0.
    let esc: &[u8] = match scancode {
        SC_BACKSPACE => &[0x08],
        SC_UP => &[0x1B, b'[', b'A'],
        SC_DOWN => &[0x1B, b'[', b'B'],
        SC_RIGHT => &[0x1B, b'[', b'C'],
        SC_LEFT => &[0x1B, b'[', b'D'],
        SC_HOME => &[0x1B, b'[', b'H'],
        SC_END => &[0x1B, b'[', b'F'],
        SC_DELETE => &[0x1B, b'[', b'3', b'~'],
        _ => &[],
    };
    out[..esc.len()].copy_from_slice(esc);
    esc.len()
}

const SURFACE: u64 = 1;
const BUFFER: u64 = 2;
const W: u32 = 560;
const H: u32 = 360;
const FMT_XRGB8888: u32 = 1;
/// ARGB8888: the buffer carries a straight-alpha channel. Selected instead of
/// XRGB when `[terminal] bg_alpha < 255`, so the compositor blends the terminal's
/// translucent background over the desktop. Must match the protocol's
/// `FORMAT_ARGB8888`; the compositor validates the imported buffer's format
/// against the CreateSurface format on every commit.
const FMT_ARGB8888: u32 = 2;

/// Total scrollback cap. The number of visible rows is derived from the live
/// surface height at runtime (see `visible_rows_for`), since the compositor can
/// resize us via a server-pushed CONFIGURE (maximize/restore).
const SCROLL_CAP: usize = 200;
/// Poll cadence: block on compositor IPC at most this long, then drain the pty
/// and repaint. The compositor blits our buffer every tick regardless.
const POLL_MS: u64 = 40;

// APPEND_MARKER

/// The window's text font, embedded in the ELF (M4 still ships assets in-image).
/// Monospace so the cell-grid columns line up even on the built-in fallback path.
static FONT_BYTES: &[u8] = include_bytes!("../../../../assets/fonts/DejaVuSansMono.ttf");

/// Load the terminal's TTF, falling back to the embedded `FONT_BYTES` on any
/// error. Prefers the per-terminal `[terminal] font` when set; otherwise the
/// shared `[desktop] font`. Both are live config knobs.
fn load_font(tcfg: &dwm_settings::TerminalCfg) -> Result<Font, ()> {
    let cfg = dwm_settings::load();
    let path = if tcfg.font.as_str().is_empty() {
        cfg.desktop.font.as_str()
    } else {
        tcfg.font.as_str()
    };
    if let Some(bytes) = libdunit::read_binary(path, 4 * 1024 * 1024) {
        if let Ok(f) = Font::parse(bytes) {
            return Ok(f);
        }
    }
    Font::parse(FONT_BYTES.to_vec()).map_err(|_| ())
}

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    libdunit::println("gui_terminal: PANIC");
    libdunit::exit(101)
}

fn u64_at(p: &[u8], off: usize) -> u64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&p[off..off + 8]);
    u64::from_le_bytes(a)
}

/// Fixed layout metric: the point size the scrollback renders at, paired with
/// `ROW_PX`. This is a rendering invariant (not user palette), so it stays a
/// const; the *colors* below are config-driven via `TerminalCfg`.
const FONT_PX: f32 = 13.0;
const ROW_PX: i32 = 16;
/// Padding from the surface edge to the first cell (both axes).
const PAD_X: i32 = 8;
const PAD_Y: i32 = 8;

/// Visible text rows that fit in a surface `h` px tall: an 8px top pad, then one
/// `ROW_PX`-tall row each. At least one row so a tiny surface still paints.
fn visible_rows_for(h: u32) -> usize {
    (((h as i32 - PAD_Y) / ROW_PX).max(1)) as usize
}

/// Columns that fit in a surface `w` px wide at the monospace `cell_w`.
fn cols_for(w: u32, cell_w: i32) -> usize {
    (((w as i32 - 2 * PAD_X) / cell_w).max(1)) as usize
}

/// Monospace cell width in px: the advance of a representative glyph at `FONT_PX`.
/// For a monospace font every glyph shares this advance, so it defines the grid.
fn mono_cell_w(font: &Font) -> i32 {
    let (_g, adv) = font.layout_line("M", FONT_PX);
    ((adv + 0.5) as i32).max(1)
}

/// The live text palette, resolved from `[terminal]` config (`fg`, `ansi[8]`,
/// `ansi_bright[8]`). Carried in `Term` so SGR (`ESC[…m`) selects configured
/// colors instead of hardcoded constants — the single source of truth is the
/// TOML (falling back to `TerminalCfg::baseline`).
#[derive(Clone, Copy)]
struct Palette {
    fg: u32,
    ansi: [u32; 8],
    ansi_bright: [u32; 8],
}

/// ARGB8888 -> render Color.
fn col(argb: u32) -> Color {
    Color::rgba(
        ((argb >> 16) & 0xFF) as u8,
        ((argb >> 8) & 0xFF) as u8,
        (argb & 0xFF) as u8,
        ((argb >> 24) & 0xFF) as u8,
    )
}

/// One screen cell: a glyph plus its SGR foreground color. The background is the
/// surface-wide `bg` (config `[terminal] bg`/`bg_alpha`), so cells carry only what
/// the shell actually controls per character.
#[derive(Clone, Copy)]
struct Cell {
    ch: char,
    fg: u32,
}

impl Cell {
    fn blank(fg: u32) -> Self {
        Cell { ch: ' ', fg }
    }
}

/// A fresh row of `cols` blank cells in the default foreground.
fn blank_row(cols: usize, fg: u32) -> Vec<Cell> {
    let mut r: Vec<Cell> = Vec::new();
    r.resize(cols, Cell::blank(fg));
    r
}

/// ESC-sequence parser state for interpreting the shell's stdout byte stream.
enum Esc {
    Normal,
    Esc,
    Csi,
}

/// On-screen terminal model: a real character CELL GRID (`rows`×`cols`) with a
/// cursor at (`crow`, `ccol`), plus a `scrollback` of rows that scrolled off the
/// top. `feed` interprets the shell's bytes like a VT — printable ASCII writes at
/// the cursor and advances it (wrapping at the right edge), `\r` returns to column
/// 0, `\n` is CR+LF, `\t` tabs, `\f`/`ESC[2J` clear, `\b` steps left, and the CSI
/// cursor/erase finals (`H`/`f`/`A`/`B`/`C`/`D`/`G`/`d`/`J`/`K`/`m`) move the cursor
/// and blank cells — so `dsh`'s `\r`+text+`ESC[K`+`ESC[nD` line editing renders with
/// a correct caret, like a raw terminal.
struct Term {
    scrollback: Vec<Vec<Cell>>,
    grid: Vec<Vec<Cell>>,
    cols: usize,
    rows: usize,
    crow: usize,
    ccol: usize,
    fg: u32,
    dirty: bool,
    /// Rows the view is lifted above the live bottom (0 = following new output).
    scroll: usize,
    esc: Esc,
    params: [u32; 8],
    nparams: usize,
    param: u32,
    has_param: bool,
    /// Config-resolved colors (default fg + the 16 ANSI slots).
    palette: Palette,
    /// Monospace cell width in px (grid column pitch).
    cell_w: i32,
    /// Logical current-line accumulator for the `[term]` serial echo — tracks the
    /// shell's line independent of on-screen column wrapping (see `flush_echo`).
    echo: String,
}

impl Term {
    fn new(palette: Palette, cols: usize, rows: usize, cell_w: i32) -> Self {
        let mut grid: Vec<Vec<Cell>> = Vec::new();
        for _ in 0..rows {
            grid.push(blank_row(cols, palette.fg));
        }
        Term {
            scrollback: Vec::new(),
            grid,
            cols,
            rows,
            crow: 0,
            ccol: 0,
            fg: palette.fg,
            dirty: true,
            scroll: 0,
            esc: Esc::Normal,
            params: [0; 8],
            nparams: 0,
            param: 0,
            has_param: false,
            palette,
            cell_w,
            echo: String::new(),
        }
    }

    /// Move the view up (`up = true`) or down through the scrollback by `rows`,
    /// clamped so it never scrolls past the top or below the live bottom.
    fn scroll_by(&mut self, rows: usize, up: bool) {
        let max = self.scrollback.len();
        self.scroll = if up {
            (self.scroll + rows).min(max)
        } else {
            self.scroll.saturating_sub(rows)
        };
        self.dirty = true;
    }

    /// Push the top grid row into scrollback and open a fresh blank bottom row —
    /// the screen scrolled up by one line.
    fn scroll_up(&mut self) {
        let row = self.grid.remove(0);
        self.scrollback.push(row);
        if self.scrollback.len() > SCROLL_CAP {
            let excess = self.scrollback.len() - SCROLL_CAP;
            self.scrollback.drain(0..excess);
        }
        self.grid.push(blank_row(self.cols, self.palette.fg));
        self.scroll = 0; // new output snaps the view back to the live bottom
    }

    /// Advance to the next row (scrolling at the bottom) and return to column 0.
    /// `flush` emits the logical echo line for a real `\n`; a column wrap passes
    /// false so the wrapped text stays one logical `[term]` echo.
    fn newline(&mut self, flush: bool) {
        if flush {
            self.flush_echo();
        }
        self.crow += 1;
        if self.crow >= self.rows {
            self.scroll_up();
            self.crow = self.rows - 1;
        }
        self.ccol = 0;
    }

    /// Emit the accumulated logical line to fd 1 as `[term] <line>` — the headless
    /// verification channel (our own stdout, not the pty) — then reset it.
    fn flush_echo(&mut self) {
        libdunit::write(1, b"[term] ");
        libdunit::write(1, self.echo.as_bytes());
        libdunit::write(1, b"\n");
        self.echo.clear();
    }

    /// Write one printable char at the cursor, wrapping at the right edge, and
    /// advance. The char also appends to the logical echo line.
    fn put_char(&mut self, c: char) {
        if self.ccol >= self.cols {
            self.newline(false);
        }
        self.grid[self.crow][self.ccol] = Cell { ch: c, fg: self.fg };
        self.ccol += 1;
        self.echo.push(c);
    }

    /// Move the cursor one column left (non-destructive, like a raw terminal BS).
    fn backspace(&mut self) {
        if self.ccol > 0 {
            self.ccol -= 1;
        }
        self.echo.pop();
    }

    /// Advance to the next 8-column tab stop (clamped inside the row).
    fn tab(&mut self) {
        let next = ((self.ccol / 8) + 1) * 8;
        self.ccol = next.min(self.cols.saturating_sub(1));
        self.echo.push(' ');
    }

    /// Blank every cell and home the cursor (`\f` / `ESC[2J`+`ESC[H`).
    fn clear_screen(&mut self) {
        let fg = self.palette.fg;
        for row in self.grid.iter_mut() {
            for cell in row.iter_mut() {
                *cell = Cell::blank(fg);
            }
        }
        self.crow = 0;
        self.ccol = 0;
        self.echo.clear();
        self.scroll = 0;
    }

    /// Erase within the cursor row (EL): 0 = to end, 1 = to start, 2 = whole line.
    fn erase_line(&mut self, mode: u32) {
        let (a, b) = match mode {
            1 => (0, self.ccol + 1),
            2 => (0, self.cols),
            _ => (self.ccol, self.cols),
        };
        let fg = self.palette.fg;
        let end = b.min(self.cols);
        for cell in self.grid[self.crow].iter_mut().take(end).skip(a) {
            *cell = Cell::blank(fg);
        }
        if mode == 2 {
            self.echo.clear();
        }
    }
    // ERASE_RESIZE_MARKER

    /// Erase within the screen (ED): 0 = cursor→end, 1 = start→cursor, 2 = all.
    fn erase_display(&mut self, mode: u32) {
        let fg = self.palette.fg;
        match mode {
            2 => {
                for row in self.grid.iter_mut() {
                    for cell in row.iter_mut() {
                        *cell = Cell::blank(fg);
                    }
                }
                self.echo.clear();
            }
            1 => {
                for r in 0..self.crow {
                    for cell in self.grid[r].iter_mut() {
                        *cell = Cell::blank(fg);
                    }
                }
                self.erase_line(1);
            }
            _ => {
                self.erase_line(0);
                for r in (self.crow + 1)..self.rows {
                    for cell in self.grid[r].iter_mut() {
                        *cell = Cell::blank(fg);
                    }
                }
            }
        }
    }

    /// Resize the grid on a compositor CONFIGURE, keeping what fits: rows pushed
    /// off the top spill into scrollback; the cursor is clamped into range.
    fn resize(&mut self, cols: usize, rows: usize) {
        if cols == self.cols && rows == self.rows {
            return;
        }
        let fg = self.palette.fg;
        for row in self.grid.iter_mut() {
            row.resize(cols, Cell::blank(fg));
        }
        let mut removed_top = 0usize;
        while self.grid.len() > rows {
            let r = self.grid.remove(0);
            self.scrollback.push(r);
            removed_top += 1;
        }
        while self.grid.len() < rows {
            self.grid.push(blank_row(cols, fg));
        }
        if self.scrollback.len() > SCROLL_CAP {
            let excess = self.scrollback.len() - SCROLL_CAP;
            self.scrollback.drain(0..excess);
        }
        self.cols = cols;
        self.rows = rows;
        self.crow = self.crow.saturating_sub(removed_top).min(rows.saturating_sub(1));
        self.ccol = self.ccol.min(cols.saturating_sub(1));
        self.dirty = true;
    }

    /// Apply the collected SGR (`ESC [ … m`) params to the live foreground.
    fn apply_sgr(&mut self) {
        let n = if self.nparams == 0 { 1 } else { self.nparams };
        for i in 0..n {
            let p = if self.nparams == 0 { 0 } else { self.params[i] };
            match p {
                0 | 39 => self.fg = self.palette.fg,
                30..=37 => self.fg = self.palette.ansi[(p - 30) as usize],
                90..=97 => self.fg = self.palette.ansi_bright[(p - 90) as usize],
                _ => {} // bold/reverse/background — not modeled
            }
        }
    }

    fn push_param(&mut self) {
        if self.nparams < self.params.len() {
            self.params[self.nparams] = self.param;
            self.nparams += 1;
        }
        self.param = 0;
        self.has_param = false;
    }

    /// Parameter `i` with a fallback when absent OR zero (CSI convention: an
    /// omitted/zero cursor-motion parameter means 1).
    fn param_or(&self, i: usize, default: u32) -> u32 {
        let v = if i < self.nparams { self.params[i] } else { 0 };
        if v == 0 { default } else { v }
    }

    /// Raw parameter `i` (0 when absent) — for ED/EL, where 0 is a real mode.
    fn param_raw(&self, i: usize) -> u32 {
        if i < self.nparams { self.params[i] } else { 0 }
    }

    /// Handle a CSI final byte: SGR color, cursor motion, and erase.
    fn dispatch_csi(&mut self, final_byte: u8) {
        match final_byte {
            b'm' => self.apply_sgr(),
            b'A' => self.crow = self.crow.saturating_sub(self.param_or(0, 1) as usize),
            b'B' => self.crow = (self.crow + self.param_or(0, 1) as usize).min(self.rows - 1),
            b'C' => self.ccol = (self.ccol + self.param_or(0, 1) as usize).min(self.cols - 1),
            b'D' => self.ccol = self.ccol.saturating_sub(self.param_or(0, 1) as usize),
            b'G' => self.ccol = (self.param_or(0, 1) as usize - 1).min(self.cols - 1),
            b'd' => self.crow = (self.param_or(0, 1) as usize - 1).min(self.rows - 1),
            b'H' | b'f' => {
                self.crow = (self.param_or(0, 1) as usize - 1).min(self.rows - 1);
                self.ccol = (self.param_or(1, 1) as usize - 1).min(self.cols - 1);
            }
            b'J' => self.erase_display(self.param_raw(0)),
            b'K' => self.erase_line(self.param_raw(0)),
            _ => {} // other finals / private modes: consumed
        }
    }

    /// Feed one chunk of raw shell stdout through the ESC state machine.
    fn feed(&mut self, bytes: &[u8]) {
        for &b in bytes {
            match self.esc {
                Esc::Esc => {
                    self.esc = if b == b'[' { Esc::Csi } else { Esc::Normal };
                    continue;
                }
                Esc::Csi => {
                    match b {
                        b'0'..=b'9' => {
                            self.param =
                                self.param.saturating_mul(10).saturating_add((b - b'0') as u32);
                            self.has_param = true;
                        }
                        b';' => self.push_param(),
                        0x40..=0x7e => {
                            if self.has_param || self.nparams > 0 {
                                self.push_param();
                            }
                            self.dispatch_csi(b);
                            self.nparams = 0;
                            self.param = 0;
                            self.has_param = false;
                            self.esc = Esc::Normal;
                        }
                        _ => {} // '?' prefix and intermediates: keep scanning
                    }
                    continue;
                }
                Esc::Normal => {}
            }
            match b {
                0x1b => self.esc = Esc::Esc,
                b'\n' => self.newline(true),
                b'\r' => {
                    self.ccol = 0;
                    self.echo.clear();
                }
                b'\t' => self.tab(),
                0x0c => self.clear_screen(),
                0x08 => self.backspace(),
                0x20..=0x7e => self.put_char(b as char),
                _ => {}
            }
        }
        if !bytes.is_empty() {
            self.dirty = true;
        }
    }

}

/// Draw one line of text with its top-left at (x, y_top); returns the pen
/// advance in px. Mirrors the runtime painter's baseline math so it looks
/// identical to DUI-rendered text.
fn draw_text(surf: &mut Surface, font: &Font, x: i32, y_top: i32, color: Color, text: &str) -> f32 {
    let baseline = y_top as f32 + font.line_metrics(FONT_PX).ascent;
    let (glyphs, adv) = font.layout_line(text, FONT_PX);
    for g in glyphs {
        if let Some(bmp) = font.rasterize(g.glyph, FONT_PX) {
            let ox = (x as f32 + g.x + 0.5) as i32 + bmp.left;
            let oy = (baseline + 0.5) as i32 - bmp.top;
            surf.blit_glyph(&bmp, ox, oy, color);
        }
    }
    adv
}

/// Paint the cell grid into the mapped ARGB8888 buffer: fill the background,
/// then draw the visible window of the document (scrollback rows followed by the
/// live grid rows) as monospace cells, and finally a block caret at the cursor.
/// `w`/`h` are the surface's CURRENT size — the compositor can resize us via a
/// server-pushed CONFIGURE, so paint against the live geometry, not the consts.
/// `bg` is the config-resolved background as `0xAARRGGBB`: its alpha (from
/// `[terminal] bg_alpha`) is written straight into every background pixel via
/// `clear` — NOT `fill_rect`, which would blend over the previous frame and drift
/// the alpha toward opaque. Glyphs draw opaque on top, so text stays crisp while
/// the background lets the desktop show through when `bg_alpha < 255`.
fn render(px: *mut u8, font: &Font, term: &Term, w: u32, h: u32, bg: u32) {
    let pixels = unsafe { core::slice::from_raw_parts_mut(px as *mut u32, (w * h) as usize) };
    let mut surf = Surface::new(pixels, w as usize, h as usize);
    surf.clear(col(bg));

    // The document is `scrollback` (evicted rows) followed by the live `grid`.
    // `scroll` counts how many rows the view is lifted above the live bottom
    // (0 = pinned to the bottom, where the cursor lives).
    let sb = term.scrollback.len();
    let total = sb + term.rows;
    let visible = term.rows.min(total);
    let bottom = (total - 1).saturating_sub(term.scroll);
    let start = (bottom + 1).saturating_sub(visible);
    for (i, idx) in (start..=bottom).enumerate() {
        let y = PAD_Y + i as i32 * ROW_PX;
        let row = if idx < sb {
            &term.scrollback[idx]
        } else {
            &term.grid[idx - sb]
        };
        draw_row(&mut surf, font, term.cell_w, y, row);
    }

    // Block caret: only when pinned to the bottom (scroll == 0), drawn as a
    // translucent overlay so it reads as a cursor without re-rendering a glyph.
    if term.scroll == 0 {
        let cx = PAD_X + term.ccol as i32 * term.cell_w;
        let cy = PAD_Y + term.crow as i32 * ROW_PX;
        let c = col(term.fg);
        surf.fill_rect(
            cx as f32,
            cy as f32,
            term.cell_w as f32,
            ROW_PX as f32,
            Color::rgba(c.r, c.g, c.b, 150),
        );
    }
}

/// Draw one grid row of monospace cells with its top at `y`. Cells are grouped
/// into runs of identical foreground colour (so a whole colour span is laid out
/// in one `draw_text`, keeping glyph spacing natural) and each run is placed at
/// its starting column `c0 * cell_w`, which keeps every cell on the monospace
/// lattice. Trailing blank cells past the last glyph are skipped.
fn draw_row(surf: &mut Surface, font: &Font, cell_w: i32, y: i32, row: &[Cell]) {
    let last = match row.iter().rposition(|c| c.ch != ' ') {
        Some(i) => i,
        None => return, // empty row: nothing to paint (background already cleared)
    };
    let mut c0 = 0usize;
    while c0 <= last {
        let fg = row[c0].fg;
        let mut c1 = c0;
        while c1 <= last && row[c1].fg == fg {
            c1 += 1;
        }
        let mut run = String::new();
        for cell in &row[c0..c1] {
            run.push(cell.ch);
        }
        let x = PAD_X + c0 as i32 * cell_w;
        draw_text(surf, font, x, y, col(fg), &run);
        c0 = c1;
    }
}

// APPEND_START

/// Re-negotiate the surface at a new size after a server-pushed CONFIGURE: back
/// a FRESH kernel buffer of `new_w*new_h`, render the scrollback into it at the
/// new geometry, then replay the buffer half of the handshake with a NEW object
/// id (the compositor's freshness gate rejects a re-used id). Returns the new
/// `(handle, mapped ptr)`; the OLD buffer handle is closed. `serial` advances so
/// every request keeps a unique, increasing client serial. This is the terminal
/// half of maximize/restore — the compositor owns the geometry, we re-buffer.
#[allow(clippy::too_many_arguments)]
fn resize_surface(
    compositor: u32,
    old_buf: u32,
    font: &Font,
    term: &Term,
    new_w: u32,
    new_h: u32,
    obj: u64,
    token: u64,
    serial: &mut u64,
    bg: u32,
    fmt: u32,
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
    render(npx, font, term, new_w, new_h, bg);

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
        format: fmt,
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
    // 0) Load this app's config (apps/gui_terminal.toml). A missing/garbage file
    // yields the built-in baseline. The resolved values drive the prompt (via the
    // shell's env), the font, the background (with alpha), and the text palette;
    // they also go to serial so a TOML edit is observable headless.
    let tcfg = dwm_settings::TerminalCfg::load("gui_terminal");
    libdunit::println(&alloc::format!(
        "gui_terminal: cfg from_file={} prompt={} fg={:#010X} bg={:#010X} bg_alpha={} font={}",
        tcfg.from_file as u32,
        tcfg.prompt.as_str(),
        tcfg.fg,
        tcfg.bg,
        tcfg.bg_alpha,
        if tcfg.font.as_str().is_empty() { "<desktop>" } else { tcfg.font.as_str() },
    ));
    // Resolve the presentation format from the configured background opacity.
    // `bg_alpha == 255` keeps the opaque XRGB fast path (compositor straight-copy);
    // anything less presents ARGB so the compositor blends our background over the
    // desktop. `bg` folds that alpha into the CONFIGURED background RGB and is
    // written into every background pixel each frame via `Surface::clear`.
    let bg = (tcfg.bg_alpha << 24) | (tcfg.bg & 0x00FF_FFFF);
    let fmt = if tcfg.bg_alpha < 255 { FMT_ARGB8888 } else { FMT_XRGB8888 };
    // The live text palette (default fg + ANSI slots) comes straight from config.
    let palette = Palette { fg: tcfg.fg, ansi: tcfg.ansi, ansi_bright: tcfg.ansi_bright };
    libdunit::println(&alloc::format!(
        "gui_terminal: argb bg_alpha={} fmt={}",
        tcfg.bg_alpha,
        if fmt == FMT_ARGB8888 { "argb8888" } else { "xrgb8888" },
    ));

    // 1) Handshake: compositor pid + our client id (id is a tint hint only).
    let mut m = [0u8; 8];
    if libdunit::ipc_recv_blocking(&mut m, 0) < 8 {
        libdunit::println("gui_terminal: FAIL recv handshake");
        libdunit::exit(1);
    }
    let compositor = u32::from_le_bytes([m[0], m[1], m[2], m[3]]);

    // 2) Create + map our own shared buffer and paint the initial (empty) frame.
    let bytes = (W * H * 4) as usize;
    let buf = libdunit::handle_create_shared(bytes);
    if buf <= 0 {
        libdunit::println("gui_terminal: FAIL create buffer");
        libdunit::exit(2);
    }
    let buf = buf as u32;
    let mapped = libdunit::handle_map(buf, 0, bytes);
    if mapped <= 0 {
        libdunit::println("gui_terminal: FAIL map buffer");
        libdunit::exit(3);
    }
    let mut px = mapped as usize as *mut u8;
    let font = match load_font(&tcfg) {
        Ok(f) => f,
        Err(_) => {
            libdunit::println("gui_terminal: FAIL font parse");
            libdunit::exit(7);
        }
    };
    let cell_w = mono_cell_w(&font);
    let mut term = Term::new(palette, cols_for(W, cell_w), visible_rows_for(H), cell_w);
    // The surface's live geometry. The compositor may resize us via a
    // server-pushed CONFIGURE, so track it rather than reusing the W/H consts.
    let mut cur_buf = buf;
    let mut cur_w = W;
    let mut cur_h = H;
    render(px, &font, &term, cur_w, cur_h, bg);

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
        libdunit::println("gui_terminal: FAIL welcome");
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
        libdunit::println("gui_terminal: FAIL cap transfer");
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
        libdunit::println("gui_terminal: surface presented OK");
    } else {
        libdunit::println("gui_terminal: FAIL no frame done");
        libdunit::handle_close(buf);
        libdunit::exit(6);
    }

    // 9) Spawn the shell as our pty slave: dsh's stdin/stdout are the pty rings.
    let pty = libdunit::pty_create();
    if pty <= 0 {
        libdunit::println("gui_terminal: FAIL pty create");
        libdunit::handle_close(buf);
        libdunit::exit(8);
    }
    let pty = pty as u32;
    // Hand the shell its prompt through the environment (desktop policy lives in
    // config, not baked into dsh): `[terminal] prompt` → `PROMPT=<value>`.
    let prompt_env = alloc::format!("PROMPT={}", tcfg.prompt.as_str());
    if libdunit::pty_spawn_env("dsh", pty, &[&prompt_env]) <= 0 {
        libdunit::println("gui_terminal: FAIL pty spawn");
        libdunit::pty_close(pty);
        libdunit::handle_close(buf);
        libdunit::exit(9);
    }

    // 10) Bridge loop: block on compositor IPC (short timeout). Forward IN_KEY
    //     bytes into the pty master; on any wake, drain the shell's stdout into
    //     the scrollback and repaint if it changed. A server-pushed CONFIGURE
    //     (maximize/restore) re-buffers us at the new geometry. Exit on IN_QUIT.
    let mut sbuf = [0u8; 256];
    let mut next_obj: u64 = 3; // fresh buffer object id per resize (> import id 2)
    let mut serial: u64 = 7; // client request serial, continues past the handshake
    loop {
        let n = libdunit::ipc_recv_blocking(&mut rx, POLL_MS);
        let magic = if n >= 8 {
            u32::from_le_bytes([rx[0], rx[1], rx[2], rx[3]])
        } else {
            0
        };
        // Server-pushed CONFIGURE (opcode 0x8010): the compositor resized us.
        // Re-negotiate a fresh buffer at the new geometry, then keep rendering
        // the scrollback into it (visible rows recomputed from the new height).
        if magic == MAGIC && n >= 48 && u16::from_le_bytes([rx[8], rx[9]]) == 0x8010 {
            let tok = u64_at(&rx, 24);
            let nw = u32::from_le_bytes([rx[32], rx[33], rx[34], rx[35]]);
            let nh = u32::from_le_bytes([rx[36], rx[37], rx[38], rx[39]]);
            if nw == 0 || nh == 0 || (nw == cur_w && nh == cur_h) {
                let ack = Request::AckConfigure { configure: tok }.encode(SURFACE, serial);
                serial += 1;
                libdunit::ipc_send(compositor, &ack);
            } else {
                // Reflow the grid to the new geometry FIRST — resize_surface
                // borrows the term immutably to render it into the fresh buffer,
                // so the cell grid must already match (nw, nh).
                term.resize(cols_for(nw, cell_w), visible_rows_for(nh));
                if let Some((nb, npx)) =
                    resize_surface(compositor, cur_buf, &font, &term, nw, nh, next_obj, tok, &mut serial, bg, fmt)
                {
                    cur_buf = nb;
                    px = npx;
                    cur_w = nw;
                    cur_h = nh;
                    next_obj += 1;
                    libdunit::println("gui_terminal: reconfigured OK");
                }
            }
        } else if magic == INPUT_MAGIC && n >= 8 {
            match rx[4] {
                IN_QUIT => break,
                IN_SCROLL => {
                    // Wheel delta (i32) in the lx field: positive = scroll up
                    // into the backlog, negative = back toward the live bottom.
                    let delta = i32::from_le_bytes([rx[8], rx[9], rx[10], rx[11]]);
                    if delta != 0 {
                        term.scroll_by(delta.unsigned_abs() as usize * 3, delta > 0);
                    }
                }
                IN_KEY => {
                    // Widened IN_KEY: scancode@rx[8..12], mods@rx[12..16],
                    // cooked ASCII@rx[16] (see gui_server::send_input). Decode
                    // into a pty byte sequence: control codes for Ctrl+letter,
                    // the cooked byte for printables/Enter/Tab, and xterm ESC
                    // sequences for the extended nav keys (arrows/Home/End/Del).
                    let scancode = rx[8];
                    let mods = rx[12];
                    let ascii = rx[16];
                    // PgUp/PgDn scroll the local view a page at a time and are
                    // never forwarded to the pty.
                    if scancode == SC_PGUP {
                        term.scroll_by(term.rows.saturating_sub(1), true);
                    } else if scancode == SC_PGDN {
                        term.scroll_by(term.rows.saturating_sub(1), false);
                    } else {
                        let mut seq = [0u8; 4];
                        let len = encode_key(scancode, mods, ascii, &mut seq);
                        if len > 0 {
                            libdunit::pty_write(pty, &seq[..len]);
                        }
                    }
                }
                _ => {}
            }
        }
        // Drain whatever the shell has produced since the last tick.
        loop {
            let r = libdunit::pty_read(pty, &mut sbuf);
            if r > 0 {
                term.feed(&sbuf[..r as usize]);
            } else {
                break; // EAGAIN / EOF / EPIPE — nothing more this tick
            }
        }
        if term.dirty {
            render(px, &font, &term, cur_w, cur_h, bg);
            term.dirty = false;
        }
    }

    libdunit::pty_close(pty);
    libdunit::handle_close(cur_buf);
    libdunit::exit(0)
}


