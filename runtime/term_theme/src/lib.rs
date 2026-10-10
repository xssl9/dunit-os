#![no_std]
//! Terminal color-theme presets — the single source of truth for terminal
//! palettes across the whole OS.
//!
//! Both terminals consume this crate:
//!   * the kernel framebuffer terminal (`kernel/src/terminal.rs`), and
//!   * the userspace GUI terminal (`dwm_settings::TerminalCfg` -> `gui_terminal`).
//!
//! A [`TermTheme`] is pure data: a default foreground/background plus the 16
//! ANSI slots (8 normal + 8 bright) an SGR sequence selects. Colors are
//! `0xAARRGGBB` (alpha always `0xFF` — terminals treat `bg` opacity separately
//! via their own `bg_alpha`). WHICH theme is active is policy: it lives in each
//! terminal's config as `[terminal] theme = "<name>"`, resolved here with
//! [`resolved_or_default`]. Names are matched case-insensitively with `-` and
//! `_` treated as the same separator, so `"tokyo-night"` == `"tokyo_night"`.

/// One terminal palette. `ansi`/`ansi_bright` are indexed by the ANSI color
/// number 0..=7 (black, red, green, yellow, blue, magenta, cyan, white).
#[derive(Clone, Copy)]
pub struct TermTheme {
    /// Stable lookup key (lowercase, `_`-separated).
    pub name: &'static str,
    /// Default text color (SGR 39 / reset restores this).
    pub fg: u32,
    /// Default background fill.
    pub bg: u32,
    /// Normal-intensity ANSI slots 0..=7 (SGR 30..=37).
    pub ansi: [u32; 8],
    /// Bright/bold ANSI slots 0..=7 (SGR 90..=97, or 1 + 30..=37).
    pub ansi_bright: [u32; 8],
}

/// Case/`-`/`_`-insensitive ASCII name compare.
fn name_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        let norm = |c: u8| -> u8 {
            match c {
                b'A'..=b'Z' => c + 32, // to lowercase
                b'-' => b'_',          // unify separators
                other => other,
            }
        };
        if norm(a[i]) != norm(b[i]) {
            return false;
        }
        i += 1;
    }
    true
}

/// Resolve a theme by name, or `None` if the name is unknown.
pub fn by_name(name: &str) -> Option<&'static TermTheme> {
    let mut i = 0;
    while i < THEMES.len() {
        if name_eq(THEMES[i].name, name) {
            return Some(&THEMES[i]);
        }
        i += 1;
    }
    None
}

/// Resolve a theme by name, falling back to [`DEFAULT`] for an empty/unknown
/// name — the safe path every terminal uses so a typo never blanks the palette.
pub fn resolved_or_default(name: &str) -> &'static TermTheme {
    match by_name(name) {
        Some(t) => t,
        None => DEFAULT,
    }
}

/// The baseline theme (index 0, `green_tea`) — matches the historical hardcoded
/// `TerminalCfg::baseline`, so an unset `theme` key is a no-op visual change.
pub const DEFAULT: &TermTheme = &THEMES[0];

/// Every shipped palette. `green_tea` MUST stay first (it is [`DEFAULT`] and the
/// historical baseline). Order is otherwise cosmetic. Add a row to ship a theme.
pub const THEMES: &[TermTheme] = &[
    // 0 — Green Tea: the project baseline (green default text on near-black).
    TermTheme {
        name: "green_tea",
        fg: 0xFFA6_E3A1,
        bg: 0xFF0B_0F14,
        ansi: [
            0xFF45_475A, 0xFFF3_8BA8, 0xFFA6_E3A1, 0xFFF9_E2AF,
            0xFF89_B4FA, 0xFFCB_A6F7, 0xFF94_E2D5, 0xFFCD_D6F4,
        ],
        ansi_bright: [
            0xFF58_5B70, 0xFFEB_A0AC, 0xFFA6_E3A1, 0xFFFA_B387,
            0xFF89_DCEB, 0xFFF5_C2E7, 0xFF94_E2D5, 0xFFFF_FFFF,
        ],
    },
    // 1 — Catppuccin Mocha.
    TermTheme {
        name: "catppuccin_mocha",
        fg: 0xFFCD_D6F4,
        bg: 0xFF1E_1E2E,
        ansi: [
            0xFF45_475A, 0xFFF3_8BA8, 0xFFA6_E3A1, 0xFFF9_E2AF,
            0xFF89_B4FA, 0xFFF5_C2E7, 0xFF94_E2D5, 0xFFBA_C2DE,
        ],
        ansi_bright: [
            0xFF58_5B70, 0xFFF3_8BA8, 0xFFA6_E3A1, 0xFFF9_E2AF,
            0xFF89_B4FA, 0xFFF5_C2E7, 0xFF94_E2D5, 0xFFA6_ADC8,
        ],
    },
    // 2 — Catppuccin Latte (light).
    TermTheme {
        name: "catppuccin_latte",
        fg: 0xFF4C_4F69,
        bg: 0xFFEF_F1F5,
        ansi: [
            0xFF5C_5F77, 0xFFD2_0F39, 0xFF40_A02B, 0xFFDF_8E1D,
            0xFF1E_66F5, 0xFFEA_76CB, 0xFF17_9299, 0xFFAC_B0BE,
        ],
        ansi_bright: [
            0xFF6C_6F85, 0xFFD2_0F39, 0xFF40_A02B, 0xFFDF_8E1D,
            0xFF1E_66F5, 0xFFEA_76CB, 0xFF17_9299, 0xFFBC_C0CC,
        ],
    },
    // 3 — Dracula.
    TermTheme {
        name: "dracula",
        fg: 0xFFF8_F8F2,
        bg: 0xFF28_2A36,
        ansi: [
            0xFF21_222C, 0xFFFF_5555, 0xFF50_FA7B, 0xFFF1_FA8C,
            0xFFBD_93F9, 0xFFFF_79C6, 0xFF8B_E9FD, 0xFFF8_F8F2,
        ],
        ansi_bright: [
            0xFF62_72A4, 0xFFFF_6E6E, 0xFF69_FF94, 0xFFFF_FFA5,
            0xFFD6_ACFF, 0xFFFF_92DF, 0xFFA4_FFFF, 0xFFFF_FFFF,
        ],
    },
    // 4 — Gruvbox Dark.
    TermTheme {
        name: "gruvbox_dark",
        fg: 0xFFEB_DBB2,
        bg: 0xFF28_2828,
        ansi: [
            0xFF28_2828, 0xFFCC_241D, 0xFF98_971A, 0xFFD7_9921,
            0xFF45_8588, 0xFFB1_6286, 0xFF68_9D6A, 0xFFA8_9984,
        ],
        ansi_bright: [
            0xFF92_8374, 0xFFFB_4934, 0xFFB8_BB26, 0xFFFA_BD2F,
            0xFF83_A598, 0xFFD3_869B, 0xFF8E_C07C, 0xFFEB_DBB2,
        ],
    },
    // 5 — Nord.
    TermTheme {
        name: "nord",
        fg: 0xFFD8_DEE9,
        bg: 0xFF2E_3440,
        ansi: [
            0xFF3B_4252, 0xFFBF_616A, 0xFFA3_BE8C, 0xFFEB_CB8B,
            0xFF81_A1C1, 0xFFB4_8EAD, 0xFF88_C0D0, 0xFFE5_E9F0,
        ],
        ansi_bright: [
            0xFF4C_566A, 0xFFBF_616A, 0xFFA3_BE8C, 0xFFEB_CB8B,
            0xFF81_A1C1, 0xFFB4_8EAD, 0xFF8F_BCBB, 0xFFEC_EFF4,
        ],
    },
    // 6 — Solarized Dark.
    TermTheme {
        name: "solarized_dark",
        fg: 0xFF83_9496,
        bg: 0xFF00_2B36,
        ansi: [
            0xFF07_3642, 0xFFDC_322F, 0xFF85_9900, 0xFFB5_8900,
            0xFF26_8BD2, 0xFFD3_3682, 0xFF2A_A198, 0xFFEE_E8D5,
        ],
        ansi_bright: [
            0xFF00_2B36, 0xFFCB_4B16, 0xFF58_6E75, 0xFF65_7B83,
            0xFF83_9496, 0xFF6C_71C4, 0xFF93_A1A1, 0xFFFD_F6E3,
        ],
    },
    // 7 — Solarized Light.
    TermTheme {
        name: "solarized_light",
        fg: 0xFF65_7B83,
        bg: 0xFFFD_F6E3,
        ansi: [
            0xFF07_3642, 0xFFDC_322F, 0xFF85_9900, 0xFFB5_8900,
            0xFF26_8BD2, 0xFFD3_3682, 0xFF2A_A198, 0xFFEE_E8D5,
        ],
        ansi_bright: [
            0xFF00_2B36, 0xFFCB_4B16, 0xFF58_6E75, 0xFF65_7B83,
            0xFF83_9496, 0xFF6C_71C4, 0xFF93_A1A1, 0xFFFD_F6E3,
        ],
    },
    // 8 — Tokyo Night.
    TermTheme {
        name: "tokyo_night",
        fg: 0xFFC0_CAF5,
        bg: 0xFF1A_1B26,
        ansi: [
            0xFF15_161E, 0xFFF7_768E, 0xFF9E_CE6A, 0xFFE0_AF68,
            0xFF7A_A2F7, 0xFFBB_9AF7, 0xFF7D_CFFF, 0xFFA9_B1D6,
        ],
        ansi_bright: [
            0xFF41_4868, 0xFFF7_768E, 0xFF9E_CE6A, 0xFFE0_AF68,
            0xFF7A_A2F7, 0xFFBB_9AF7, 0xFF7D_CFFF, 0xFFC0_CAF5,
        ],
    },
    // 9 — Monokai.
    TermTheme {
        name: "monokai",
        fg: 0xFFF8_F8F2,
        bg: 0xFF27_2822,
        ansi: [
            0xFF27_2822, 0xFFF9_2672, 0xFFA6_E22E, 0xFFF4_BF75,
            0xFF66_D9EF, 0xFFAE_81FF, 0xFFA1_EFE4, 0xFFF8_F8F2,
        ],
        ansi_bright: [
            0xFF75_715E, 0xFFF9_2672, 0xFFA6_E22E, 0xFFF4_BF75,
            0xFF66_D9EF, 0xFFAE_81FF, 0xFFA1_EFE4, 0xFFF9_F8F5,
        ],
    },
];
