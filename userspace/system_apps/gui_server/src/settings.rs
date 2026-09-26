//! DWM settings loaded from a TOML document (slice 9).
//!
//! The compositor's theme is data, not hardcoded constants: at desktop start
//! `load()` reads `/system/share/dwm/default.toml` from the (embedded) VFS and
//! overlays any recognised keys onto the built-in Green Tea baseline. A missing
//! or unparseable file is not fatal — the baseline is the last-known-good
//! default, so the desktop always comes up. Unknown keys are ignored so newer
//! configs stay loadable on older builds.
//!
//! The parser is a deliberately tiny TOML subset: `# ` / `//` comments, blank
//! lines, `[section]` headers (recorded but not required), and `key = "value"`
//! / `key = value` pairs. That covers flat theme/layout tables without pulling
//! in a full TOML crate.

use alloc::string::String;
use alloc::vec::Vec;

/// Compositor palette (ARGB8888). Field names mirror the `[theme]` TOML keys.
#[derive(Clone, Copy)]
pub struct Theme {
    pub desktop: u32,
    pub title_focused: u32,
    pub title_unfocused: u32,
    pub border_focused: u32,
    pub border_unfocused: u32,
    pub close: u32,
    pub panel: u32,
    pub panel_text: u32,
    pub launcher: u32,
    pub taskbtn: u32,
    pub taskbtn_focused: u32,
    pub menu: u32,
    pub menu_hover: u32,
}

impl Theme {
    /// The built-in Green Tea baseline (identical to the pre-slice-9 constants).
    pub const fn baseline() -> Self {
        Theme {
            desktop: 0xFF1E1E2E,
            title_focused: 0xFF313244,
            title_unfocused: 0xFF232331,
            border_focused: 0xFFA6E3A1,
            border_unfocused: 0xFF45475A,
            close: 0xFFF38BA8,
            panel: 0xFF181825,
            panel_text: 0xFFCDD6F4,
            launcher: 0xFFA6E3A1,
            taskbtn: 0xFF313244,
            taskbtn_focused: 0xFF45475A,
            menu: 0xFF11111B,
            menu_hover: 0xFF45475A,
        }
    }

    /// Apply one `key`/`value` pair from the `[theme]` table. Unknown keys and
    /// malformed colors are ignored (the field keeps its baseline value).
    fn apply(&mut self, key: &str, value: &str) {
        let Some(color) = parse_color(value) else {
            return;
        };
        let slot = match key {
            "desktop" => &mut self.desktop,
            "title_focused" => &mut self.title_focused,
            "title_unfocused" => &mut self.title_unfocused,
            "border_focused" => &mut self.border_focused,
            "border_unfocused" => &mut self.border_unfocused,
            "close" => &mut self.close,
            "panel" => &mut self.panel,
            "panel_text" => &mut self.panel_text,
            "launcher" => &mut self.launcher,
            "taskbtn" => &mut self.taskbtn,
            "taskbtn_focused" => &mut self.taskbtn_focused,
            "menu" => &mut self.menu,
            "menu_hover" => &mut self.menu_hover,
            _ => return,
        };
        *slot = color;
    }
}

/// Compositor geometry (pixels). Field names mirror the `[layout]` TOML keys.
/// `max_windows` is a safety cap on concurrently composited windows, not a pixel.
#[derive(Clone, Copy)]
pub struct Layout {
    pub title_h: i32,
    pub border: i32,
    pub panel_h: i32,
    pub launcher_w: i32,
    pub taskbtn_w: i32,
    pub taskbtn_gap: i32,
    pub menu_w: i32,
    pub menu_item_h: i32,
    pub max_windows: usize,
}

impl Layout {
    /// The built-in baseline (identical to the pre-slice-9.2 compositor consts).
    pub const fn baseline() -> Self {
        Layout {
            title_h: 26,
            border: 2,
            panel_h: 28,
            launcher_w: 40,
            taskbtn_w: 120,
            taskbtn_gap: 4,
            menu_w: 130,
            menu_item_h: 26,
            max_windows: 8,
        }
    }

    /// Apply one `key`/`value` pair from the `[layout]` table. Unknown keys and
    /// non-numeric values are ignored (the field keeps its baseline value).
    /// `max_windows` is clamped to a sane range so a bad config cannot ask for
    /// zero (a dead desktop) or an unbounded spawn count.
    fn apply(&mut self, key: &str, value: &str) {
        let Some(n) = parse_uint(value) else {
            return;
        };
        match key {
            "title_h" => self.title_h = n as i32,
            "border" => self.border = n as i32,
            "panel_h" => self.panel_h = n as i32,
            "launcher_w" => self.launcher_w = n as i32,
            "taskbtn_w" => self.taskbtn_w = n as i32,
            "taskbtn_gap" => self.taskbtn_gap = n as i32,
            "menu_w" => self.menu_w = n as i32,
            "menu_item_h" => self.menu_item_h = n as i32,
            "max_windows" => self.max_windows = (n as usize).clamp(1, 32),
            _ => {}
        }
    }
}

/// Resolved DWM settings. Extends with `[layout]` etc. in later sub-slices.
#[derive(Clone, Copy)]
pub struct Settings {
    pub theme: Theme,
    pub layout: Layout,
    /// True when a config file was found and read (parse still best-effort).
    pub from_file: bool,
    /// Count of recognised keys applied over the baseline (0 for pure default).
    pub applied: u32,
}

impl Settings {
    pub const fn defaults() -> Self {
        Settings {
            theme: Theme::baseline(),
            layout: Layout::baseline(),
            from_file: false,
            applied: 0,
        }
    }
}

const CONFIG_PATH: &str = "/system/share/dwm/default.toml";

/// Read + parse the system DWM config, overlaying it on the baseline. Never
/// fails: an absent/unreadable/garbage file yields the pure baseline.
pub fn load() -> Settings {
    let mut settings = Settings::defaults();
    let Some(text) = read_file(CONFIG_PATH) else {
        return settings;
    };
    settings.from_file = true;
    parse_into(&text, &mut settings);
    settings
}

/// Overlay a TOML document onto `settings`. Public for headless unit exercise.
pub fn parse_into(text: &str, settings: &mut Settings) {
    let mut section = String::new();
    for raw in text.lines() {
        let line = strip_comment(raw).trim();
        if line.is_empty() {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            section.clear();
            section.push_str(name.trim());
            continue;
        }
        let Some(eq) = line.find('=') else {
            continue;
        };
        let key = line[..eq].trim();
        let value = unquote(line[eq + 1..].trim());
        if section == "theme" {
            settings.theme.apply(key, value);
            settings.applied += 1;
        } else if section == "layout" {
            settings.layout.apply(key, value);
            settings.applied += 1;
        }
    }
}

/// Drop a trailing `#`/`//` comment, but only outside a quoted string so a `#`
/// inside a color literal survives.
fn strip_comment(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut in_str = false;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => in_str = !in_str,
            b'#' if !in_str => return &line[..i],
            b'/' if !in_str && i + 1 < bytes.len() && bytes[i + 1] == b'/' => return &line[..i],
            _ => {}
        }
        i += 1;
    }
    line
}

fn unquote(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(value)
}

/// Parse `#RRGGBB` / `#AARRGGBB` (and bare `RRGGBB`/`AARRGGBB`) into ARGB8888.
/// A 6-digit value is forced fully opaque.
fn parse_color(value: &str) -> Option<u32> {
    let hex = value.strip_prefix('#').unwrap_or(value);
    let hex = hex.strip_prefix("0x").or_else(|| hex.strip_prefix("0X")).unwrap_or(hex);
    if hex.len() != 6 && hex.len() != 8 {
        return None;
    }
    let mut acc: u32 = 0;
    for c in hex.chars() {
        acc = (acc << 4) | c.to_digit(16)?;
    }
    if hex.len() == 6 {
        acc |= 0xFF00_0000;
    }
    Some(acc)
}

/// Parse a non-negative decimal integer (e.g. a `[layout]` pixel count). Returns
/// `None` for an empty string or any non-digit character, so a malformed value
/// leaves the field at its baseline. Saturates rather than overflowing.
fn parse_uint(value: &str) -> Option<u32> {
    if value.is_empty() {
        return None;
    }
    let mut acc: u32 = 0;
    for c in value.chars() {
        let d = c.to_digit(10)?;
        acc = acc.saturating_mul(10).saturating_add(d);
    }
    Some(acc)
}

/// Slurp a whole VFS file into a String (best-effort). `None` on open error or
/// non-UTF-8 content; a partial read still returns what was decoded.
fn read_file(path: &str) -> Option<String> {
    let fd = libdunit::open(path, libdunit::OPEN_READ);
    if fd < 0 {
        return None;
    }
    let fd = fd as usize;
    let mut data: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 512];
    loop {
        let n = libdunit::read(fd, &mut chunk);
        if n <= 0 {
            break;
        }
        data.extend_from_slice(&chunk[..n as usize]);
        if data.len() > 64 * 1024 {
            break; // config sanity cap
        }
    }
    libdunit::close(fd);
    String::from_utf8(data).ok()
}
