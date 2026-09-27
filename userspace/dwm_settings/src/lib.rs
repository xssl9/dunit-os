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

#![no_std]
extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

/// Compositor palette (ARGB8888). Field names mirror the `[theme]` TOML keys.
#[derive(Clone, Copy, PartialEq, Eq)]
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
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    pub title_h: i32,
    pub border: i32,
    pub panel_h: i32,
    pub launcher_w: i32,
    pub taskbtn_w: i32,
    pub taskbtn_gap: i32,
    pub menu_w: i32,
    pub menu_item_h: i32,
    pub dock_w: i32,
    pub ws_w: i32,
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
            dock_w: 48,
            ws_w: 22,
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
            "dock_w" => self.dock_w = n as i32,
            "ws_w" => self.ws_w = n as i32,
            "max_windows" => self.max_windows = (n as usize).clamp(1, 32),
            _ => {}
        }
    }
}

/// Compositor visual effects (concept §5: rounded corners, soft shadows, backdrop
/// blur, gradients, animations). Every effect can be tuned or switched off from
/// the `[effects]` TOML table, so a low-end target (or a user who wants the flat
/// look back) can disable them without a rebuild. Field names mirror the keys.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Effects {
    /// Rounded-corner radius (px) for windows/panel/menus/buttons; 0 = square.
    pub corner_radius: i32,
    /// Drop-shadow spread (px) under windows/menus; 0 = no shadow.
    pub shadow: i32,
    /// Shadow opacity at the silhouette edge (0..255).
    pub shadow_alpha: i32,
    /// Backdrop blur under the panel/menus (acrylic look).
    pub blur: bool,
    /// Blur kernel radius (px, per separable box pass).
    pub blur_radius: i32,
    /// Blur passes (2-3 ≈ Gaussian); more = smoother but costlier.
    pub blur_iters: i32,
    /// Panel tint opacity over the blurred backdrop (0..255).
    pub panel_alpha: i32,
    /// Menu tint opacity over the blurred backdrop (0..255).
    pub menu_alpha: i32,
    /// Vertical gradient fills on the panel/title bars/buttons.
    pub gradient: bool,
    /// Open/close/switch animations.
    pub anim: bool,
    /// Base animation duration (ms) at the compositor's ~60 Hz tick.
    pub anim_ms: i32,
}

impl Effects {
    /// The built-in baseline — the modern Green Tea look turned on (concept §5).
    pub const fn baseline() -> Self {
        Effects {
            corner_radius: 8,
            shadow: 7,
            shadow_alpha: 90,
            blur: true,
            blur_radius: 4,
            blur_iters: 2,
            panel_alpha: 205,
            menu_alpha: 225,
            gradient: true,
            anim: true,
            anim_ms: 140,
        }
    }

    /// Apply one `key`/`value` pair from the `[effects]` table. Bool keys accept
    /// `true`/`false`/`1`/`0`; the rest are non-negative integers. Unknown keys
    /// and malformed values are ignored (the field keeps its baseline value).
    fn apply(&mut self, key: &str, value: &str) {
        match key {
            "blur" => {
                if let Some(b) = parse_bool(value) {
                    self.blur = b;
                }
            }
            "gradient" => {
                if let Some(b) = parse_bool(value) {
                    self.gradient = b;
                }
            }
            "anim" | "animations" => {
                if let Some(b) = parse_bool(value) {
                    self.anim = b;
                }
            }
            _ => {
                let Some(n) = parse_uint(value) else {
                    return;
                };
                let n = n as i32;
                match key {
                    "corner_radius" => self.corner_radius = n.clamp(0, 64),
                    "shadow" => self.shadow = n.clamp(0, 32),
                    "shadow_alpha" => self.shadow_alpha = n.clamp(0, 255),
                    "blur_radius" => self.blur_radius = n.clamp(0, 16),
                    "blur_iters" => self.blur_iters = n.clamp(1, 4),
                    "panel_alpha" => self.panel_alpha = n.clamp(0, 255),
                    "menu_alpha" => self.menu_alpha = n.clamp(0, 255),
                    "anim_ms" => self.anim_ms = n.clamp(0, 2000),
                    _ => {}
                }
            }
        }
    }
}

/// Resolved DWM settings. Extends with `[layout]` etc. in later sub-slices.
#[derive(Clone, Copy)]
pub struct Settings {
    pub theme: Theme,
    pub layout: Layout,
    pub effects: Effects,
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
            effects: Effects::baseline(),
            from_file: false,
            applied: 0,
        }
    }
}

const CONFIG_PATH: &str = "/system/share/dwm/default.toml";

/// Serialize `settings` to the TOML dialect and write it back to `CONFIG_PATH`
/// (slice C: the write half of the GUI<->TOML round-trip). Returns whether the
/// write succeeded. The file is an `Owned` MemFS node so this persists for the
/// session (RAM only until DunitFS v2). The caller then signals the compositor
/// to reload. Never panics: a write error just returns `false`.
pub fn save(settings: &Settings) -> bool {
    let text = to_toml(settings);
    libdunit::write_string(CONFIG_PATH, &text).is_ok()
}

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
        } else if section == "effects" {
            settings.effects.apply(key, value);
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

/// Parse a boolean toggle for the `[effects]` table: `true`/`false` (any case)
/// or `1`/`0`. Anything else returns `None` (field keeps its baseline value).
fn parse_bool(value: &str) -> Option<bool> {
    match value.trim() {
        "true" | "True" | "TRUE" | "1" | "on" | "yes" => Some(true),
        "false" | "False" | "FALSE" | "0" | "off" | "no" => Some(false),
        _ => None,
    }
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

// ---------------------------------------------------------------------------
// Serialization (slice C): Settings -> TOML, the inverse of `parse_into`.
//
// The settings app edits a live `Settings`, renders it back to this TOML dialect
// and writes it to `CONFIG_PATH`, then asks the compositor to reload — a
// GUI<->TOML round-trip in RAM. The invariant we care about: parsing our own
// output reproduces the same theme/layout/effects (`roundtrip_ok`). Colors are
// emitted `#AARRGGBB`, integers as decimals, bools as `true`/`false`.
// ---------------------------------------------------------------------------

/// Append a byte as two uppercase hex digits.
fn push_hex_byte(out: &mut String, v: u32) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    out.push(HEX[((v >> 4) & 0xF) as usize] as char);
    out.push(HEX[(v & 0xF) as usize] as char);
}

/// Append a non-negative integer in decimal (no separators, no sign).
fn push_uint(out: &mut String, mut v: u64) {
    if v == 0 {
        out.push('0');
        return;
    }
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    while v > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    for &b in &buf[i..] {
        out.push(b as char);
    }
}

/// Append `key = "#AARRGGBB"\n` for an ARGB8888 color (matches `parse_color`).
fn push_color(out: &mut String, key: &str, argb: u32) {
    out.push_str(key);
    out.push_str(" = \"#");
    push_hex_byte(out, (argb >> 24) & 0xFF);
    push_hex_byte(out, (argb >> 16) & 0xFF);
    push_hex_byte(out, (argb >> 8) & 0xFF);
    push_hex_byte(out, argb & 0xFF);
    out.push_str("\"\n");
}

/// Append `key = <n>\n`. Values are conceptually non-negative (`parse_uint`
/// cannot read a sign), so a stray negative is clamped to 0 to stay parseable.
fn push_int(out: &mut String, key: &str, v: i64) {
    out.push_str(key);
    out.push_str(" = ");
    push_uint(out, v.max(0) as u64);
    out.push('\n');
}

/// Append `key = true\n` / `key = false\n`.
fn push_bool(out: &mut String, key: &str, b: bool) {
    out.push_str(key);
    out.push_str(if b { " = true\n" } else { " = false\n" });
}

/// Serialize `Settings` back to the TOML dialect `parse_into` accepts, mirroring
/// `assets/dwm/default.toml`. Lossless for every recognised key: parsing the
/// output reproduces the same `theme`/`layout`/`effects` (see `roundtrip_ok`).
pub fn to_toml(s: &Settings) -> String {
    let mut out = String::new();
    out.push_str("# Dunit DWM settings (written by gui_settings; live in RAM this session).\n\n");

    out.push_str("[theme]\n");
    push_color(&mut out, "desktop", s.theme.desktop);
    push_color(&mut out, "title_focused", s.theme.title_focused);
    push_color(&mut out, "title_unfocused", s.theme.title_unfocused);
    push_color(&mut out, "border_focused", s.theme.border_focused);
    push_color(&mut out, "border_unfocused", s.theme.border_unfocused);
    push_color(&mut out, "close", s.theme.close);
    push_color(&mut out, "panel", s.theme.panel);
    push_color(&mut out, "panel_text", s.theme.panel_text);
    push_color(&mut out, "launcher", s.theme.launcher);
    push_color(&mut out, "taskbtn", s.theme.taskbtn);
    push_color(&mut out, "taskbtn_focused", s.theme.taskbtn_focused);
    push_color(&mut out, "menu", s.theme.menu);
    push_color(&mut out, "menu_hover", s.theme.menu_hover);
    out.push('\n');

    out.push_str("[layout]\n");
    push_int(&mut out, "title_h", s.layout.title_h as i64);
    push_int(&mut out, "border", s.layout.border as i64);
    push_int(&mut out, "panel_h", s.layout.panel_h as i64);
    push_int(&mut out, "launcher_w", s.layout.launcher_w as i64);
    push_int(&mut out, "taskbtn_w", s.layout.taskbtn_w as i64);
    push_int(&mut out, "taskbtn_gap", s.layout.taskbtn_gap as i64);
    push_int(&mut out, "menu_w", s.layout.menu_w as i64);
    push_int(&mut out, "menu_item_h", s.layout.menu_item_h as i64);
    push_int(&mut out, "dock_w", s.layout.dock_w as i64);
    push_int(&mut out, "ws_w", s.layout.ws_w as i64);
    push_int(&mut out, "max_windows", s.layout.max_windows as i64);
    out.push('\n');

    out.push_str("[effects]\n");
    push_int(&mut out, "corner_radius", s.effects.corner_radius as i64);
    push_int(&mut out, "shadow", s.effects.shadow as i64);
    push_int(&mut out, "shadow_alpha", s.effects.shadow_alpha as i64);
    push_bool(&mut out, "blur", s.effects.blur);
    push_int(&mut out, "blur_radius", s.effects.blur_radius as i64);
    push_int(&mut out, "blur_iters", s.effects.blur_iters as i64);
    push_int(&mut out, "panel_alpha", s.effects.panel_alpha as i64);
    push_int(&mut out, "menu_alpha", s.effects.menu_alpha as i64);
    push_bool(&mut out, "gradient", s.effects.gradient);
    push_bool(&mut out, "anim", s.effects.anim);
    push_int(&mut out, "anim_ms", s.effects.anim_ms as i64);

    out
}

/// True iff `to_toml` round-trips `s` through `parse_into` (theme/layout/effects;
/// the `from_file`/`applied` bookkeeping is not part of the value). A cheap
/// startup self-check that the serializer and parser stay in lockstep.
pub fn roundtrip_ok(s: &Settings) -> bool {
    let text = to_toml(s);
    let mut back = Settings::defaults();
    parse_into(&text, &mut back);
    back.theme == s.theme && back.layout == s.layout && back.effects == s.effects
}
