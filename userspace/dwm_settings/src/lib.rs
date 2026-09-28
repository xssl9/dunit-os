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
    /// Title-bar text on a focused window (the label the compositor draws over
    /// the title bar). Was a hardcoded 0x00EAF2EC in the compositor.
    pub title_text_focused: u32,
    /// Title-bar / taskbar text on an unfocused window. Was 0x00A8B4AC.
    pub title_text_unfocused: u32,
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
            title_text_focused: 0xFFEAF2EC,
            title_text_unfocused: 0xFFA8B4AC,
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
            "title_text_focused" => &mut self.title_text_focused,
            "title_text_unfocused" => &mut self.title_text_unfocused,
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
    /// Point size of the TTF window/tray title text. Was a hardcoded 14.0.
    pub title_font_px: i32,
    /// Point size of the TTF taskbar-button text. Was a hardcoded 13.0.
    pub panel_font_px: i32,
}

impl Layout {
    /// The built-in baseline (identical to the pre-slice-9.2 compositor consts).
    pub const fn baseline() -> Self {
        Layout {
            title_h: 26,
            border: 2,
            panel_h: 28,
            // App-menu button width at the left of the panel. Holds the "Dunit"
            // wordmark (logo mark + label); the workspace pips start right after
            // it. Widened from the old 40px hamburger slot to fit the wordmark.
            launcher_w: 92,
            // Vestigial: the top-panel per-window taskbar was replaced by the
            // dock-as-task-switcher + centered focused title (REF-2). Kept for
            // config round-trip stability; no compositor consumer today.
            taskbtn_w: 120,
            taskbtn_gap: 4,
            menu_w: 130,
            menu_item_h: 26,
            dock_w: 48,
            ws_w: 22,
            max_windows: 8,
            title_font_px: 14,
            panel_font_px: 13,
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
            "title_font_px" => self.title_font_px = (n as i32).clamp(6, 64),
            "panel_font_px" => self.panel_font_px = (n as i32).clamp(6, 64),
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

/// Bounded inline string for filesystem paths in the config, kept `Copy` (and
/// heap-free) so `Settings` stays `Copy` like the rest of the schema. Paths are
/// short; anything past `CAP` bytes is truncated.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ConfigStr {
    buf: [u8; ConfigStr::CAP],
    len: usize,
}

impl ConfigStr {
    const CAP: usize = 128;

    /// Build from a `&str` at const time (baselines), truncating to `CAP`.
    pub const fn new(s: &str) -> Self {
        let src = s.as_bytes();
        let mut buf = [0u8; ConfigStr::CAP];
        let mut i = 0;
        while i < src.len() && i < ConfigStr::CAP {
            buf[i] = src[i];
            i += 1;
        }
        ConfigStr { buf, len: i }
    }

    /// Overwrite in place from a runtime `&str` (config apply), truncating to `CAP`.
    fn set(&mut self, s: &str) {
        let src = s.as_bytes();
        let n = if src.len() > ConfigStr::CAP { ConfigStr::CAP } else { src.len() };
        self.buf[..n].copy_from_slice(&src[..n]);
        self.len = n;
    }

    /// Borrow as `&str` (bytes originate from `&str`, so always valid UTF-8).
    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("")
    }
}

/// Asset paths (`[desktop]` table): everything the desktop loads *by path* lives
/// here, so it is fully config-driven. Baseline values are the in-tree assets; a
/// config may point any of them elsewhere in the VFS.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Desktop {
    /// Wallpaper BMP (1600x900, 24-bit) painted as the compositor backdrop.
    pub wallpaper: ConfigStr,
    /// Icon theme directory name under `/assets/icons/<name>/`; dock/launcher and
    /// the file-manager mimetypes resolve `<name>/<key>.rgba` from it.
    pub icon_theme: ConfigStr,
    /// TrueType font every GUI app loads for text (falls back to the embedded
    /// copy when the path is missing or unparseable).
    pub font: ConfigStr,
    /// Brand logo blitted into the app-menu wordmark, a 32x32 straight-alpha
    /// `R,G,B,A` raster (circular mask baked into the alpha). Absent/mis-sized →
    /// the compositor draws its flat accent mark instead.
    pub logo: ConfigStr,
}

impl Desktop {
    /// The built-in baseline: the assets shipped in-tree today.
    pub const fn baseline() -> Self {
        Desktop {
            wallpaper: ConfigStr::new("/assets/wallpapers/wallpaper.bmp"),
            icon_theme: ConfigStr::new("breeze"),
            font: ConfigStr::new("/assets/fonts/DejaVuSans.ttf"),
            logo: ConfigStr::new("/assets/images/logo.rgba"),
        }
    }

    /// Apply one `key`/`value` pair from the `[desktop]` table (string values;
    /// already unquoted by the caller). Unknown keys are ignored.
    fn apply(&mut self, key: &str, value: &str) {
        match key {
            "wallpaper" => self.wallpaper.set(value),
            "icon_theme" => self.icon_theme.set(value),
            "font" => self.font.set(value),
            "logo" => self.logo.set(value),
            _ => {}
        }
    }
}

/// Desktop widget card (concept §5): a translucent plasmoid painted on the
/// wallpaper (behind windows). `[widgets]` holds only the desktop-wide policy —
/// the master on/off and the default corner. WHICH widgets are actually drawn is
/// owned per-widget by `widgets/<name>.toml` (see [`WidgetCfg`]): each widget is
/// present only when its file exists and is enabled, so a widget is removed by
/// deleting its file (single source of truth, no `[widgets] clock/monitor` bool).
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Widgets {
    /// Master toggle: `false` hides the whole plasmoid regardless of per-widget files.
    pub enabled: bool,
    /// Default screen corner inherited by a widget whose file omits `corner`:
    /// 0 = top-left, 1 = top-right, 2 = bottom-left, 3 = bottom-right.
    pub corner: i32,
}

impl Widgets {
    /// The built-in baseline: plasmoid on, default corner top-right. The clock and
    /// monitor widgets ship as their own `widgets/*.toml` files, not bools here.
    pub const fn baseline() -> Self {
        Widgets {
            enabled: true,
            corner: 1,
        }
    }

    /// Apply one `key`/`value` pair from the `[widgets]` table. `enabled` accepts
    /// `true`/`false`/`1`/`0`; `corner` is a non-negative integer clamped to 0..3.
    /// Unknown keys and malformed values are ignored (field keeps its baseline).
    fn apply(&mut self, key: &str, value: &str) {
        match key {
            "enabled" => {
                if let Some(b) = parse_bool(value) {
                    self.enabled = b;
                }
            }
            "corner" => {
                if let Some(n) = parse_uint(value) {
                    self.corner = (n as i32).clamp(0, 3);
                }
            }
            _ => {}
        }
    }
}

/// Screen edge a shell strip (panel / tray) docks to. Fieldless, so it stays
/// `Copy` and `Settings` with it. Serialized as its lowercase name.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Top,
    Bottom,
    Left,
    Right,
}

impl Edge {
    /// Top/Bottom strips run horizontally (full width); Left/Right run vertically.
    pub const fn is_horizontal(self) -> bool {
        matches!(self, Edge::Top | Edge::Bottom)
    }

    /// TOML spelling (matches `parse`).
    pub const fn as_str(self) -> &'static str {
        match self {
            Edge::Top => "top",
            Edge::Bottom => "bottom",
            Edge::Left => "left",
            Edge::Right => "right",
        }
    }

    /// Parse a `pos = "..."` value; unknown spellings return `None` (field keeps
    /// its baseline edge).
    fn parse(value: &str) -> Option<Edge> {
        match value {
            "top" => Some(Edge::Top),
            "bottom" => Some(Edge::Bottom),
            "left" => Some(Edge::Left),
            "right" => Some(Edge::Right),
            _ => None,
        }
    }
}

/// Panel placement (`[panel]` table). The panel is the desktop's shell bar
/// (launcher button, workspace switcher, focused-window title, and — when the
/// tray shares its edge — the tray readout). Its *thickness* stays `[layout]
/// panel_h` (single source of truth); this table only picks which screen edge it
/// docks to, so the panel can move to any side without a rebuild.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Panel {
    pub pos: Edge,
}

impl Panel {
    pub const fn baseline() -> Self {
        Panel { pos: Edge::Top }
    }

    fn apply(&mut self, key: &str, value: &str) {
        if key == "pos" {
            if let Some(e) = Edge::parse(value) {
                self.pos = e;
            }
        }
    }
}

/// System-tray placement + thickness (`[tray]` table). The tray shows live RAM /
/// process / uptime. When it shares the panel's edge it renders INSIDE the panel
/// strip (end-aligned — the classic top-panel look); on any other edge it takes
/// its own reserved strip `size` px thick. Baseline top/28 reproduces the
/// pre-Phase-5 in-panel tray exactly.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Tray {
    pub pos: Edge,
    pub size: i32,
}

impl Tray {
    pub const fn baseline() -> Self {
        Tray { pos: Edge::Top, size: 28 }
    }

    fn apply(&mut self, key: &str, value: &str) {
        match key {
            "pos" => {
                if let Some(e) = Edge::parse(value) {
                    self.pos = e;
                }
            }
            "size" => {
                if let Some(n) = parse_uint(value) {
                    self.size = (n as i32).clamp(12, 200);
                }
            }
            _ => {}
        }
    }
}

/// Highest config schema this build understands. A `[meta] schema_version`
/// greater than this means the file was written by a NEWER desktop and may use
/// keys/semantics we cannot honour, so `load_config` rejects it (→ last-known-
/// good) rather than half-applying an unknown layout. An older version is
/// forward-migrated (see `migrate_config`). Absent `[meta]` ⇒ treated as the
/// current version (old hand-written files stay valid).
pub const SCHEMA_VERSION: u32 = 1;

/// True iff this build can load a document declaring schema version `v`. The
/// single predicate `load_config` and the compositor's startup guard both use,
/// so "which versions are accepted" has one definition. Version 0 is not a real
/// schema (a malformed/empty `schema_version`), so it is rejected.
pub const fn version_supported(v: u32) -> bool {
    v >= 1 && v <= SCHEMA_VERSION
}

/// Config document metadata (`[meta]` table). Currently just the schema version,
/// which drives compatibility handling in `load_config`. Kept as its own typed
/// section so the version travels through the same parse/serialize/round-trip
/// path as every other table (no special-case string handling).
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Meta {
    pub schema_version: u32,
}

impl Meta {
    pub const fn baseline() -> Self {
        Meta { schema_version: SCHEMA_VERSION }
    }

    fn apply(&mut self, key: &str, value: &str) {
        if key == "schema_version" {
            if let Some(n) = parse_uint(value) {
                self.schema_version = n;
            }
        }
    }
}

/// Which keyboard modifier arms the Alt-Tab-style window switcher (`[shortcuts]
/// switch_mod`). The switch KEY is always Tab (a UI invariant); only the held
/// modifier is user policy, so a user who prefers a Super-Tab switcher (GNOME
/// style) flips this without a rebuild. `mask()` returns the `libdunit::KEYMOD_*`
/// bit so the compositor tests the held modifier against config, not a constant.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SwitchMod {
    Alt,
    Super,
}

impl SwitchMod {
    pub const fn as_str(self) -> &'static str {
        match self {
            SwitchMod::Alt => "alt",
            SwitchMod::Super => "super",
        }
    }

    fn parse(value: &str) -> Option<SwitchMod> {
        match value {
            "alt" => Some(SwitchMod::Alt),
            "super" => Some(SwitchMod::Super),
            _ => None,
        }
    }

    /// The `KEYMOD_*` bit (mirrors libdunit: ALT = 1<<2, SUPER = 1<<3) this
    /// modifier maps to, so the switcher tests `ev.mods & switch_mod.mask()`.
    pub const fn mask(self) -> u8 {
        match self {
            SwitchMod::Alt => 1 << 2,
            SwitchMod::Super => 1 << 3,
        }
    }
}

/// Keyboard shortcut policy (`[shortcuts]` table). Which global gestures the
/// compositor honours and how they are triggered — user-facing policy, so it
/// lives in config rather than baked into the key-drain loop.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Shortcuts {
    /// Enable the Alt/Super-Tab window switcher overlay.
    pub switcher: bool,
    /// Modifier that arms the switcher (Tab is the fixed trigger key).
    pub switch_mod: SwitchMod,
}

impl Shortcuts {
    pub const fn baseline() -> Self {
        Shortcuts { switcher: true, switch_mod: SwitchMod::Alt }
    }

    fn apply(&mut self, key: &str, value: &str) {
        match key {
            "switcher" => {
                if let Some(b) = parse_bool(value) {
                    self.switcher = b;
                }
            }
            "switch_mod" => {
                if let Some(m) = SwitchMod::parse(value) {
                    self.switch_mod = m;
                }
            }
            _ => {}
        }
    }
}

/// Quick-settings applet policy (`[quicksettings]` table). The panel applet opens
/// a flyout that toggles live effect/widget flags. The flags themselves stay in
/// `[effects]`/`[widgets]` (single source of truth); this table only gates whether
/// the applet is offered.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct QuickSettings {
    pub enabled: bool,
}

impl QuickSettings {
    pub const fn baseline() -> Self {
        QuickSettings { enabled: true }
    }

    fn apply(&mut self, key: &str, value: &str) {
        if key == "enabled" {
            if let Some(b) = parse_bool(value) {
                self.enabled = b;
            }
        }
    }
}

/// Desktop notification policy (`[notifications]` table). Transient toasts posted
/// by the compositor (launch feedback) or by clients over the notify IPC channel.
/// `corner` uses the same 0=TL/1=TR/2=BL/3=BR convention as `[widgets] corner`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Notifications {
    pub enabled: bool,
    /// Auto-dismiss timeout in milliseconds (clamped to a sane range on parse).
    pub timeout_ms: u32,
    /// Screen corner toasts stack in (0=top-left, 1=top-right, 2=bottom-left,
    /// 3=bottom-right).
    pub corner: i32,
}

impl Notifications {
    pub const fn baseline() -> Self {
        Notifications { enabled: true, timeout_ms: 3000, corner: 1 }
    }

    fn apply(&mut self, key: &str, value: &str) {
        match key {
            "enabled" => {
                if let Some(b) = parse_bool(value) {
                    self.enabled = b;
                }
            }
            "timeout_ms" => {
                if let Some(n) = parse_uint(value) {
                    self.timeout_ms = n.clamp(500, 30_000);
                }
            }
            "corner" => {
                if let Some(n) = parse_uint(value) {
                    self.corner = (n as i32).clamp(0, 3);
                }
            }
            _ => {}
        }
    }
}

/// Window/session restore policy (`[session]` table). When `restore` is on, the
/// compositor persists each window's geometry (keyed by app id) to the session
/// store `SESSION_PATH` and re-applies it when the app opens again. The geometry
/// data lives in its own file (`[window.<id>]` tables); this table is just the
/// master toggle. RAM-only until DunitFS v2, then it survives reboots unchanged.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Session {
    pub restore: bool,
}

impl Session {
    pub const fn baseline() -> Self {
        Session { restore: true }
    }

    fn apply(&mut self, key: &str, value: &str) {
        if key == "restore" {
            if let Some(b) = parse_bool(value) {
                self.restore = b;
            }
        }
    }
}

/// Resolved DWM settings. Extends with `[layout]` etc. in later sub-slices.
#[derive(Clone, Copy)]
pub struct Settings {
    pub meta: Meta,
    pub theme: Theme,
    pub layout: Layout,
    pub effects: Effects,
    pub desktop: Desktop,
    pub widgets: Widgets,
    pub panel: Panel,
    pub tray: Tray,
    pub shortcuts: Shortcuts,
    pub quicksettings: QuickSettings,
    pub notifications: Notifications,
    pub session: Session,
    /// True when a config file was found and read (parse still best-effort).
    pub from_file: bool,
    /// Count of recognised keys applied over the baseline (0 for pure default).
    pub applied: u32,
}

impl Settings {
    pub const fn defaults() -> Self {
        Settings {
            meta: Meta::baseline(),
            theme: Theme::baseline(),
            layout: Layout::baseline(),
            effects: Effects::baseline(),
            desktop: Desktop::baseline(),
            widgets: Widgets::baseline(),
            panel: Panel::baseline(),
            tray: Tray::baseline(),
            shortcuts: Shortcuts::baseline(),
            quicksettings: QuickSettings::baseline(),
            notifications: Notifications::baseline(),
            session: Session::baseline(),
            from_file: false,
            applied: 0,
        }
    }
}

const CONFIG_PATH: &str = "/system/share/dwm/default.toml";

/// Upper bound on registered applications (safety cap, not a pixel count): a
/// malformed config cannot make the compositor allocate an unbounded registry.
pub const MAX_APPS: usize = 32;
/// Upper bound on virtual workspaces the switcher will lay out.
pub const MAX_WS: usize = 9;

/// One registered application — an `[application.<id>]` TOML table. This is the
/// data that replaces the compositor's old hardcoded `LAUNCH_APPS`/`app_title`:
/// adding an app is adding a table here, never editing gui_server. `exec`/`icon`/
/// `label` default to derivations of `id` so a minimal entry is just an `id`.
#[derive(Clone, PartialEq, Eq)]
pub struct AppEntry {
    /// Registry key from `[application.<id>]`; also the default exec/icon stem.
    pub id: String,
    /// Human-readable window/taskbar title.
    pub name: String,
    /// ELF to spawn (resolved by `libdunit::spawn` against /app). Defaults to `id`.
    pub exec: String,
    /// Icon: a stem resolved as `<icon_theme>/<icon>.rgba`, or an absolute VFS
    /// path if it contains a '/'. Defaults to `id`.
    pub icon: String,
    /// 1-4 char dock/menu initials fallback when no icon renders. Derived from
    /// `name` (uppercased) when omitted.
    pub label: String,
}

/// The data-driven desktop application model: the registry plus the ordered
/// dock / launcher / autostart lists (indices into `apps`) and the workspace
/// count. Built from `[application.*]`, `[dock]`, `[launcher]`, `[startup]` and
/// `[workspaces]`. Replaces the compositor's hardcoded `LAUNCH_APPS`/`WS_COUNT`.
#[derive(Clone)]
pub struct Applications {
    pub apps: Vec<AppEntry>,
    /// Dock icons, in order, as indices into `apps`.
    pub dock: Vec<usize>,
    /// Launcher-menu entries, in order, as indices into `apps`.
    pub launcher: Vec<usize>,
    /// Autostart spawn list, in order, as indices into `apps` (may repeat).
    pub startup: Vec<usize>,
    /// Number of virtual workspaces.
    pub workspaces: usize,
    /// Test-only: `[startup] self_test = true` makes the compositor drive its
    /// runtime windows through a headless resize self-exercise (Phase 3) — no
    /// mouse exists in the automated harness, so a title-bar maximize chip can't
    /// be clicked. NEVER set in `default.toml`: a real desktop must not auto-
    /// maximize its windows; only `test.toml` opts in.
    pub self_test: bool,
}

impl AppEntry {
    /// Build a registry entry; `exec`/`icon` default to `id`, `label` to `label`.
    fn new(id: &str, name: &str, label: &str) -> AppEntry {
        AppEntry {
            id: String::from(id),
            name: String::from(name),
            exec: String::from(id),
            icon: String::from(id),
            label: String::from(label),
        }
    }
}

impl Applications {
    /// The built-in baseline: the exact set the compositor used to hardcode in
    /// `LAUNCH_APPS`/`app_title`/`WS_COUNT` (the last-known-good default).
    pub fn baseline() -> Applications {
        let apps = alloc::vec![
            AppEntry::new("gui_client", "Window", "WIN"),
            AppEntry::new("gui_calc", "Calculator", "CALC"),
            AppEntry::new("gui_stat", "System Monitor", "STAT"),
            AppEntry::new("gui_files", "Files", "FILE"),
            AppEntry::new("gui_terminal", "Terminal", "TERM"),
            AppEntry::new("gui_settings", "Settings", "SET"),
        ];
        Applications {
            dock: alloc::vec![0, 1, 2, 3, 4, 5],
            launcher: alloc::vec![0, 1, 2, 3, 4, 5],
            // Clean-boot baseline: NO autostart. Matches `default.toml`'s empty
            // `[startup]`, so a missing/garbage config falls back to the same
            // windowless desktop rather than resurrecting hardcoded test windows.
            startup: alloc::vec![],
            workspaces: 5,
            apps,
            // Never self-exercise on the real desktop; only test.toml sets this.
            self_test: false,
        }
    }
}

/// The whole resolved desktop configuration: scalar `settings` (theme/layout/
/// effects/desktop) plus the `apps` model, and a `valid` flag for last-known-good
/// handling. A live reload keeps its previous `Config` when a candidate is
/// `!valid` (garbage/empty file), so a bad edit never wipes a working desktop.
#[derive(Clone)]
pub struct Config {
    pub settings: Settings,
    pub apps: Applications,
    pub valid: bool,
}

impl Config {
    /// The pure built-in default (no config file): baselines, marked valid.
    pub fn defaults() -> Config {
        Config {
            settings: Settings::defaults(),
            apps: Applications::baseline(),
            valid: true,
        }
    }
}

/// Serialize `settings` to the TOML dialect and write it back to `CONFIG_PATH`
/// (slice C: the write half of the GUI<->TOML round-trip). Returns whether the
/// write succeeded. The file is an `Owned` MemFS node so this persists for the
/// session (RAM only until DunitFS v2). The caller then signals the compositor
/// to reload. Never panics: a write error just returns `false`.
pub fn save(settings: &Settings) -> bool {
    let mut text = to_toml(settings);
    // Preserve everything the settings app does not own — the `[application.*]`,
    // `[dock]`, `[launcher]`, `[startup]` and `[workspaces]` tables live only in
    // the file, so a settings-only rewrite must carry them over verbatim or a
    // save would silently wipe the app registry. `to_toml` re-emits the core
    // theme/layout/effects/desktop tables; we append the rest as-is.
    if let Some(existing) = read_file(CONFIG_PATH) {
        let extra = extra_sections(&existing);
        if !extra.is_empty() {
            if !text.ends_with('\n') {
                text.push('\n');
            }
            text.push('\n');
            text.push_str(&extra);
        }
    }
    libdunit::write_string(CONFIG_PATH, &text).is_ok()
}

/// Collect the raw text of every top-level table that `to_toml` does NOT emit
/// (anything whose header's first segment is not one of the typed scalar tables
/// meta/theme/layout/effects/desktop/widgets/panel/tray/shortcuts/quicksettings/
/// notifications/session), so `save` can round-trip user-authored
/// `[application.*]`/`[dock]`/etc. tables it has no typed knowledge of. Preamble
/// before the first `[header]` is dropped.
fn extra_sections(text: &str) -> String {
    let mut out = String::new();
    let mut keep = false;
    for raw in text.lines() {
        let trimmed = strip_comment(raw).trim();
        if let Some(name) = trimmed.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            let base = name.trim().split('.').next().unwrap_or("").trim();
            keep = !matches!(
                base,
                "theme"
                    | "layout"
                    | "effects"
                    | "desktop"
                    | "widgets"
                    | "panel"
                    | "tray"
                    | "meta"
                    | "shortcuts"
                    | "quicksettings"
                    | "notifications"
                    | "session"
            );
        }
        if keep {
            out.push_str(raw);
            out.push('\n');
        }
    }
    out
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
        } else if section == "desktop" {
            settings.desktop.apply(key, value);
            settings.applied += 1;
        } else if section == "widgets" {
            settings.widgets.apply(key, value);
            settings.applied += 1;
        } else if section == "panel" {
            settings.panel.apply(key, value);
            settings.applied += 1;
        } else if section == "tray" {
            settings.tray.apply(key, value);
            settings.applied += 1;
        } else if section == "meta" {
            settings.meta.apply(key, value);
            settings.applied += 1;
        } else if section == "shortcuts" {
            settings.shortcuts.apply(key, value);
            settings.applied += 1;
        } else if section == "quicksettings" {
            settings.quicksettings.apply(key, value);
            settings.applied += 1;
        } else if section == "notifications" {
            settings.notifications.apply(key, value);
            settings.applied += 1;
        } else if section == "session" {
            settings.session.apply(key, value);
            settings.applied += 1;
        }
    }
}

/// Read + parse the whole desktop config — scalar `settings` *and* the
/// application model — with last-known-good semantics. Never fails: an absent
/// file yields the pure baseline (marked `valid`); a present-but-garbage file
/// yields `valid == false`, so a live reload can keep the previous good `Config`
/// instead of applying junk. This is the authoritative desktop-config entry
/// point; `load()` remains for callers that only need the scalar `Settings`.
pub fn load_config() -> Config {
    let mut settings = Settings::defaults();
    let Some(text) = read_file(CONFIG_PATH) else {
        return Config::defaults();
    };
    settings.from_file = true;
    parse_into(&text, &mut settings);

    let (mut apps, saw_apps) = parse_apps(&text);

    // Schema-version gate. A document from a NEWER build (version > ours) may use
    // keys/semantics we cannot honour, so it is rejected here → the reload path
    // keeps last-known-good rather than half-applying an unknown layout. An OLDER
    // (but valid) version is forward-migrated in place, then stamped current.
    let declared = settings.meta.schema_version;
    let version_ok = version_supported(declared);
    if version_ok && declared < SCHEMA_VERSION {
        migrate_config(&mut settings, &mut apps, declared);
        settings.meta.schema_version = SCHEMA_VERSION;
    }

    // A file counts as a usable config when it changed *something* — either a
    // recognised scalar key or an `[application.*]` table — AND its schema version
    // is one this build understands. A file that parsed to nothing (empty/garbage)
    // or targets a future schema is `!valid`, and the reload path keeps last-good.
    let valid = version_ok
        && apps.workspaces >= 1
        && apps.workspaces <= MAX_WS
        && !apps.apps.is_empty()
        && apps.apps.len() <= MAX_APPS
        && (settings.applied > 0 || saw_apps);

    Config { settings, apps, valid }
}

/// Forward-migrate a parsed document from an older schema `from` to
/// `SCHEMA_VERSION`. The hook exists so a future field rename / default change
/// lands in ONE place instead of scattering `if version < N` checks across the
/// compositor. v1 is the earliest schema, so there is nothing older to transform
/// yet; a v2 that (say) renamed a key would remap it here.
fn migrate_config(_settings: &mut Settings, _apps: &mut Applications, from: u32) {
    match from {
        // 0 is never a real schema (rejected before we get here); 1 is current.
        _ => {}
    }
}

/// Build the `Applications` model from a config document. Returns the model and
/// whether any `[application.<id>]` table was present. With none, the built-in
/// baseline registry is returned (an old theme-only config still yields the
/// standard desktop) and the caller judges validity from the scalar `applied`.
///
/// Two passes over the tiny-TOML lines: the first registers every
/// `[application.<id>]` (first-seen order) and applies its `name`/`exec`/`icon`/
/// `label` keys plus the raw `[dock]`/`[launcher]`/`[startup]` `entries` arrays
/// and `[workspaces].count`; the second resolves those id-lists into indices
/// (unknown ids dropped). Absent `[dock]`/`[launcher]` default to the whole
/// registry in order; absent `[startup]` autostarts nothing.
fn parse_apps(text: &str) -> (Applications, bool) {
    let mut apps: Vec<AppEntry> = Vec::new();
    let mut dock_ids: Vec<String> = Vec::new();
    let mut launcher_ids: Vec<String> = Vec::new();
    let mut startup_ids: Vec<String> = Vec::new();
    let (mut have_dock, mut have_launcher, mut have_startup) = (false, false, false);
    let mut workspaces: usize = 1;
    let mut self_test = false;

    let mut section = String::new(); // full header, e.g. "application.gui_files"
    for raw in text.lines() {
        let line = strip_comment(raw).trim();
        if line.is_empty() {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            section.clear();
            section.push_str(name.trim());
            // Register the id eagerly so even an empty `[application.x]` counts.
            if let Some(id) = section.strip_prefix("application.") {
                let id = id.trim();
                if !id.is_empty() && apps.len() < MAX_APPS && !apps.iter().any(|a| a.id == id) {
                    apps.push(AppEntry::new(id, id, ""));
                }
            }
            continue;
        }
        let Some(eq) = line.find('=') else {
            continue;
        };
        let key = line[..eq].trim();
        let rhs = line[eq + 1..].trim();

        if let Some(id) = section.strip_prefix("application.") {
            let id = id.trim();
            let value = unquote(rhs);
            if let Some(app) = apps.iter_mut().find(|a| a.id == id) {
                match key {
                    "name" => app.name = String::from(value),
                    "exec" => app.exec = String::from(value),
                    "icon" => app.icon = String::from(value),
                    "label" => app.label = String::from(value),
                    _ => {}
                }
            }
        } else if section == "dock" && key == "entries" {
            dock_ids = parse_str_array(rhs);
            have_dock = true;
        } else if section == "launcher" && key == "entries" {
            launcher_ids = parse_str_array(rhs);
            have_launcher = true;
        } else if section == "startup" && key == "entries" {
            startup_ids = parse_str_array(rhs);
            have_startup = true;
        } else if section == "startup" && key == "self_test" {
            self_test = parse_bool(rhs).unwrap_or(false);
        } else if section == "workspaces" && (key == "count" || key == "n") {
            if let Some(n) = parse_uint(rhs) {
                workspaces = (n as usize).clamp(1, MAX_WS);
            }
        }
    }

    if apps.is_empty() {
        // No registry in the file — fall back to the built-in desktop.
        return (Applications::baseline(), false);
    }

    // Fill any label left empty from the (possibly overridden) name.
    for app in apps.iter_mut() {
        if app.label.is_empty() {
            app.label = default_label(&app.name);
        }
    }

    let resolve = |ids: &[String], apps: &[AppEntry]| -> Vec<usize> {
        let mut out = Vec::new();
        for id in ids {
            if let Some(i) = apps.iter().position(|a| &a.id == id) {
                out.push(i);
            }
        }
        out
    };
    let all: Vec<usize> = (0..apps.len()).collect();
    let dock = if have_dock { resolve(&dock_ids, &apps) } else { all.clone() };
    let launcher = if have_launcher { resolve(&launcher_ids, &apps) } else { all.clone() };
    let startup = if have_startup { resolve(&startup_ids, &apps) } else { Vec::new() };

    (Applications { apps, dock, launcher, startup, workspaces, self_test }, true)
}

/// Parse a single-line TOML string array (`["a", "b"]`) into its elements.
/// Tolerant: absent brackets are ignored, blank/empty elements are dropped.
fn parse_str_array(value: &str) -> Vec<String> {
    let t = value.trim();
    let t = t.strip_prefix('[').unwrap_or(t);
    let t = t.strip_suffix(']').unwrap_or(t);
    let mut out = Vec::new();
    for part in t.split(',') {
        let s = unquote(part.trim());
        if !s.is_empty() {
            out.push(String::from(s));
        }
    }
    out
}

/// Derive a 1-4 char uppercase dock/menu label from a display name (fallback for
/// an `[application.*]` table with no explicit `label`). Takes leading
/// alphanumerics of the first word; `?` when the name has none.
fn default_label(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            for u in c.to_uppercase() {
                if out.len() < 4 {
                    out.push(u);
                }
            }
        } else if !out.is_empty() {
            break;
        }
        if out.len() >= 4 {
            break;
        }
    }
    if out.is_empty() {
        out.push('?');
    }
    out
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

/// Append `key = "value"\n` for a string value (matches `unquote` on read).
fn push_string(out: &mut String, key: &str, value: &str) {
    out.push_str(key);
    out.push_str(" = \"");
    out.push_str(value);
    out.push_str("\"\n");
}

/// Serialize `Settings` back to the TOML dialect `parse_into` accepts, mirroring
/// `assets/dwm/default.toml`. Lossless for every recognised key: parsing the
/// output reproduces the same `theme`/`layout`/`effects` (see `roundtrip_ok`).
pub fn to_toml(s: &Settings) -> String {
    let mut out = String::new();
    out.push_str("# Dunit DWM settings (written by gui_settings; live in RAM this session).\n\n");

    out.push_str("[meta]\n");
    push_int(&mut out, "schema_version", s.meta.schema_version as i64);
    out.push('\n');

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
    push_color(&mut out, "title_text_focused", s.theme.title_text_focused);
    push_color(&mut out, "title_text_unfocused", s.theme.title_text_unfocused);
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
    push_int(&mut out, "title_font_px", s.layout.title_font_px as i64);
    push_int(&mut out, "panel_font_px", s.layout.panel_font_px as i64);
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
    out.push('\n');

    out.push_str("[desktop]\n");
    push_string(&mut out, "wallpaper", s.desktop.wallpaper.as_str());
    push_string(&mut out, "icon_theme", s.desktop.icon_theme.as_str());
    push_string(&mut out, "font", s.desktop.font.as_str());
    push_string(&mut out, "logo", s.desktop.logo.as_str());
    out.push('\n');

    out.push_str("[widgets]\n");
    push_bool(&mut out, "enabled", s.widgets.enabled);
    push_int(&mut out, "corner", s.widgets.corner as i64);
    out.push('\n');

    out.push_str("[panel]\n");
    push_string(&mut out, "pos", s.panel.pos.as_str());
    out.push('\n');

    out.push_str("[tray]\n");
    push_string(&mut out, "pos", s.tray.pos.as_str());
    push_int(&mut out, "size", s.tray.size as i64);
    out.push('\n');

    out.push_str("[shortcuts]\n");
    push_bool(&mut out, "switcher", s.shortcuts.switcher);
    push_string(&mut out, "switch_mod", s.shortcuts.switch_mod.as_str());
    out.push('\n');

    out.push_str("[quicksettings]\n");
    push_bool(&mut out, "enabled", s.quicksettings.enabled);
    out.push('\n');

    out.push_str("[notifications]\n");
    push_bool(&mut out, "enabled", s.notifications.enabled);
    push_int(&mut out, "timeout_ms", s.notifications.timeout_ms as i64);
    push_int(&mut out, "corner", s.notifications.corner as i64);
    out.push('\n');

    out.push_str("[session]\n");
    push_bool(&mut out, "restore", s.session.restore);

    out
}

/// True iff `to_toml` round-trips `s` through `parse_into` (meta/theme/layout/
/// effects/desktop/widgets/panel/tray/shortcuts/quicksettings/notifications/
/// session; the `from_file`/`applied` bookkeeping is not part of the value). A
/// cheap startup self-check that the serializer and parser stay in lockstep.
pub fn roundtrip_ok(s: &Settings) -> bool {
    let text = to_toml(s);
    let mut back = Settings::defaults();
    parse_into(&text, &mut back);
    back.meta == s.meta
        && back.theme == s.theme
        && back.layout == s.layout
        && back.effects == s.effects
        && back.desktop == s.desktop
        && back.widgets == s.widgets
        && back.panel == s.panel
        && back.tray == s.tray
        && back.shortcuts == s.shortcuts
        && back.quicksettings == s.quicksettings
        && back.notifications == s.notifications
        && back.session == s.session
}

// ===========================================================================
// Per-app / per-widget config (layered TOML). Each app and widget owns a small
// file under /system/share/dwm/{apps,widgets}/<id>.toml, so its user-facing
// knobs (prompt, palette, transparency, per-widget toggle) live beside the
// desktop policy instead of being baked into the app ELF. A missing/garbage
// file yields the typed baseline (last-known-good), exactly like `load_config`.
// The app/widget id is passed by the CALLER (the app knows its own identity,
// the compositor drives widget names off `[widgets]`), so no id is hardcoded
// here — this stays a generic path-driven loader.
// ===========================================================================

/// Directory holding per-app config files (`<id>.toml`).
pub const APPS_DIR: &str = "/system/share/dwm/apps";
/// Directory holding per-widget config files (`<name>.toml`).
pub const WIDGETS_DIR: &str = "/system/share/dwm/widgets";

/// Build "<dir>/<name>.toml".
fn config_path(dir: &str, name: &str) -> String {
    let mut p = String::from(dir);
    p.push('/');
    p.push_str(name);
    p.push_str(".toml");
    p
}

/// Overlay a single-line TOML color array (`["#...", ...]`) onto `out`, element
/// by element; a malformed entry leaves that slot at its baseline.
fn parse_color_array_into(rhs: &str, out: &mut [u32]) {
    for (i, s) in parse_str_array(rhs).iter().enumerate() {
        if i >= out.len() {
            break;
        }
        if let Some(c) = parse_color(s) {
            out[i] = c;
        }
    }
}

/// Per-terminal config (`apps/gui_terminal.toml`, table `[terminal]`). Mirrors
/// the palette/prompt knobs the terminal used to hardcode. `font` empty means
/// "inherit the desktop `[desktop] font`"; `bg_alpha` 255 keeps the opaque
/// XRGB fast path (values <255 request a translucent ARGB backdrop).
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct TerminalCfg {
    pub prompt: ConfigStr,
    pub font: ConfigStr,
    pub fg: u32,
    pub bg: u32,
    pub bg_alpha: u32,
    pub ansi: [u32; 8],
    pub ansi_bright: [u32; 8],
    /// True when a config file was actually found (diagnostics only).
    pub from_file: bool,
}

impl TerminalCfg {
    pub const fn baseline() -> Self {
        TerminalCfg {
            prompt: ConfigStr::new("dsh"),
            font: ConfigStr::new(""),
            fg: 0xFFA6_E3A1,
            bg: 0xFF0B_0F14,
            bg_alpha: 255,
            ansi: [
                0xFF45_475A, 0xFFF3_8BA8, 0xFFA6_E3A1, 0xFFF9_E2AF,
                0xFF89_B4FA, 0xFFCB_A6F7, 0xFF94_E2D5, 0xFFCD_D6F4,
            ],
            ansi_bright: [
                0xFF58_5B70, 0xFFEB_A0AC, 0xFFA6_E3A1, 0xFFFA_B387,
                0xFF89_DCEB, 0xFFF5_C2E7, 0xFF94_E2D5, 0xFFFF_FFFF,
            ],
            from_file: false,
        }
    }

    /// Load `apps/<app_id>.toml`, overlaying `[terminal]` onto the baseline.
    pub fn load(app_id: &str) -> Self {
        let mut c = Self::baseline();
        if let Some(text) = read_file(&config_path(APPS_DIR, app_id)) {
            c.from_file = true;
            c.parse(&text);
        }
        c
    }

    fn parse(&mut self, text: &str) {
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
            if section != "terminal" {
                continue;
            }
            let Some(eq) = line.find('=') else { continue };
            let key = line[..eq].trim();
            let rhs = line[eq + 1..].trim();
            match key {
                "prompt" => self.prompt.set(unquote(rhs)),
                "font" => self.font.set(unquote(rhs)),
                "fg" => if let Some(c) = parse_color(unquote(rhs)) { self.fg = c; },
                "bg" => if let Some(c) = parse_color(unquote(rhs)) { self.bg = c; },
                "bg_alpha" => if let Some(n) = parse_uint(rhs) { self.bg_alpha = n.min(255); },
                "ansi" => parse_color_array_into(rhs, &mut self.ansi),
                "ansi_bright" => parse_color_array_into(rhs, &mut self.ansi_bright),
                _ => {}
            }
        }
    }
}

/// Per-file-manager config (`apps/gui_files.toml`, table `[files]`). `icon_theme`
/// empty means "inherit the desktop `[desktop] icon_theme`". Colors mirror the
/// old hardcoded palette; `bg_alpha` 255 keeps the opaque fast path.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct FilesCfg {
    pub icon_theme: ConfigStr,
    pub bg: u32,
    pub header_bg: u32,
    pub status_bg: u32,
    pub accent: u32,
    pub label: u32,
    pub muted: u32,
    pub sel_bg: u32,
    pub bg_alpha: u32,
    pub from_file: bool,
}

impl FilesCfg {
    pub const fn baseline() -> Self {
        FilesCfg {
            icon_theme: ConfigStr::new(""),
            bg: 0xFF12_1820,
            header_bg: 0xFF0E_141B,
            status_bg: 0xFF0E_141B,
            accent: 0xFF2F_8F5A,
            label: 0xFFCD_D6F4,
            muted: 0xFF8A_94A8,
            sel_bg: 0x502F_8F5A,
            bg_alpha: 255,
            from_file: false,
        }
    }

    pub fn load(app_id: &str) -> Self {
        let mut c = Self::baseline();
        if let Some(text) = read_file(&config_path(APPS_DIR, app_id)) {
            c.from_file = true;
            c.parse(&text);
        }
        c
    }

    fn parse(&mut self, text: &str) {
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
            if section != "files" {
                continue;
            }
            let Some(eq) = line.find('=') else { continue };
            let key = line[..eq].trim();
            let rhs = line[eq + 1..].trim();
            match key {
                "icon_theme" => self.icon_theme.set(unquote(rhs)),
                "bg" => if let Some(c) = parse_color(unquote(rhs)) { self.bg = c; },
                "header_bg" => if let Some(c) = parse_color(unquote(rhs)) { self.header_bg = c; },
                "status_bg" => if let Some(c) = parse_color(unquote(rhs)) { self.status_bg = c; },
                "accent" => if let Some(c) = parse_color(unquote(rhs)) { self.accent = c; },
                "label" => if let Some(c) = parse_color(unquote(rhs)) { self.label = c; },
                "muted" => if let Some(c) = parse_color(unquote(rhs)) { self.muted = c; },
                "sel_bg" => if let Some(c) = parse_color(unquote(rhs)) { self.sel_bg = c; },
                "bg_alpha" => if let Some(n) = parse_uint(rhs) { self.bg_alpha = n.min(255); },
                _ => {}
            }
        }
    }
}

/// Per-widget config (`widgets/<name>.toml`, table `[widget]`). The compositor
/// draws desktop widgets (clock, monitor, …); each owns a small file. `enabled`
/// is the removal switch — a widget whose file is missing OR whose `enabled` is
/// false is not drawn. `accent` 0 / `corner` -1 mean "inherit the desktop theme
/// accent / `[widgets] corner`".
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct WidgetCfg {
    pub enabled: bool,
    pub accent: u32,
    pub corner: i32,
    /// True when a config file was actually found.
    pub from_file: bool,
}

impl WidgetCfg {
    pub const fn baseline() -> Self {
        WidgetCfg { enabled: true, accent: 0, corner: -1, from_file: false }
    }

    /// Load `widgets/<name>.toml`. A missing file yields `from_file=false` with
    /// the baseline — the caller decides whether "no file" means "not present".
    pub fn load(name: &str) -> Self {
        let mut c = Self::baseline();
        if let Some(text) = read_file(&config_path(WIDGETS_DIR, name)) {
            c.from_file = true;
            c.parse(&text);
        }
        c
    }

    fn parse(&mut self, text: &str) {
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
            if section != "widget" {
                continue;
            }
            let Some(eq) = line.find('=') else { continue };
            let key = line[..eq].trim();
            let rhs = line[eq + 1..].trim();
            match key {
                "enabled" => if let Some(b) = parse_bool(rhs) { self.enabled = b; },
                "accent" => if let Some(c) = parse_color(unquote(rhs)) { self.accent = c; },
                "corner" => if let Some(n) = parse_uint(rhs) { self.corner = n as i32; },
                _ => {}
            }
        }
    }
}

// ===========================================================================
// Session store (window geometry persistence). Separate from the scalar
// `[session]` policy toggle: this is the DATA the compositor persists so a
// window reopens where the user left it. One `[window.<app_id>]` table per app,
// keyed by the registry id (the same join key the dock/launcher use), written to
// `SESSION_PATH`. RAM-only until DunitFS v2 — then the identical code path
// survives reboots because the MemFS node gains disk backing, no edit needed.
// ===========================================================================

/// Writable session store: per-app window geometry as `[window.<id>]` tables.
/// Pre-created empty+owned in the kernel VFS (like `default.toml`) so the
/// compositor can rewrite it at runtime despite the `/system` mkdir guard.
pub const SESSION_PATH: &str = "/system/share/dwm/session.toml";

/// Upper bound on remembered windows (safety cap on a malformed store).
pub const MAX_SESSION: usize = 32;

/// One remembered window: its app id (join key) and last floating geometry plus
/// whether it was maximized. `Copy` so the compositor keeps a plain `Vec` mirror.
#[derive(Clone, Copy)]
pub struct WinGeom {
    pub id: ConfigStr,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub maximized: bool,
}

impl WinGeom {
    pub fn new(id: &str, x: i32, y: i32, w: i32, h: i32, maximized: bool) -> Self {
        let mut s = ConfigStr::new("");
        s.set(id);
        WinGeom { id: s, x, y, w, h, maximized }
    }
}

/// Read the session store into a flat list of remembered windows. Never fails: a
/// missing/empty/garbage file yields an empty list (fresh session). Bounded by
/// `MAX_SESSION` so a malformed store cannot make the compositor allocate wildly.
pub fn load_session() -> Vec<WinGeom> {
    let mut out: Vec<WinGeom> = Vec::new();
    let Some(text) = read_file(SESSION_PATH) else {
        return out;
    };
    let mut cur_id = String::new();
    let mut cur = WinGeom::new("", 0, 0, 0, 0, false);
    let mut have = false;
    let flush = |out: &mut Vec<WinGeom>, id: &str, g: &WinGeom| {
        if !id.is_empty() && g.w > 0 && g.h > 0 && out.len() < MAX_SESSION {
            let mut e = *g;
            e.id.set(id);
            out.push(e);
        }
    };
    for raw in text.lines() {
        let line = strip_comment(raw).trim();
        if line.is_empty() {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            if have {
                flush(&mut out, &cur_id, &cur);
            }
            cur_id.clear();
            cur = WinGeom::new("", 0, 0, 0, 0, false);
            have = false;
            // Table header `window.<id>`: the id after the first '.' is the join key.
            let hdr = name.trim();
            if let Some(rest) = hdr.strip_prefix("window.") {
                cur_id.push_str(rest.trim());
                have = true;
            }
            continue;
        }
        if !have {
            continue;
        }
        let Some(eq) = line.find('=') else { continue };
        let key = line[..eq].trim();
        let rhs = line[eq + 1..].trim();
        match key {
            "x" => if let Some(n) = parse_uint(rhs) { cur.x = n as i32; },
            "y" => if let Some(n) = parse_uint(rhs) { cur.y = n as i32; },
            "w" => if let Some(n) = parse_uint(rhs) { cur.w = n as i32; },
            "h" => if let Some(n) = parse_uint(rhs) { cur.h = n as i32; },
            "maximized" => if let Some(b) = parse_bool(rhs) { cur.maximized = b; },
            _ => {}
        }
    }
    if have {
        flush(&mut out, &cur_id, &cur);
    }
    out
}

/// Serialize the remembered-window list to `SESSION_PATH`. One `[window.<id>]`
/// table each; the newest entry per id wins (the caller upserts its mirror before
/// calling). Returns whether the write succeeded; never panics.
pub fn save_session(entries: &[WinGeom]) -> bool {
    let mut text = String::new();
    text.push_str("# Dunit DWM session (window geometry; live in RAM this session).\n");
    for e in entries.iter().take(MAX_SESSION) {
        let id = e.id.as_str();
        if id.is_empty() {
            continue;
        }
        text.push_str("\n[window.");
        text.push_str(id);
        text.push_str("]\n");
        push_int(&mut text, "x", e.x as i64);
        push_int(&mut text, "y", e.y as i64);
        push_int(&mut text, "w", e.w as i64);
        push_int(&mut text, "h", e.h as i64);
        push_bool(&mut text, "maximized", e.maximized);
    }
    libdunit::write_string(SESSION_PATH, &text).is_ok()
}
