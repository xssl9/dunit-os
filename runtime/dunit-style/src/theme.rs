//! The Green Tea reference theme: a monochrome dark stylesheet expressed in DSS.
//!
//! This is the appearance baseline for the M4 desktop — a restrained near-black
//! green background with pale green-grey text, one muted accent, and short
//! ease-out transitions on interactive surfaces. It is a plain DSS document so
//! it goes through the same [`crate::parse`] path as any user theme.

/// The Green Tea theme as a DSS source document. Parse it with
/// [`crate::parse`] and push it as a cascade layer above the defaults.
pub const GREEN_TEA_DSS: &str = r#"
// ---- Green Tea: monochrome dark reference theme ------------------------
$bg:        #0c110c;   // window / desktop base
$surface:   #121a12;   // panels, cards
$surface-2: #182218;   // raised / hover surface
$line:      #243024;   // hairline borders
$fg:        #d6e4d6;   // primary text
$fg-dim:    #8fa68f;   // secondary text
$accent:    #6fbf73;   // single muted green accent
$on-accent: #08120a;   // text on the accent

// Base surfaces
Window {
    background: $bg;
    color: $fg;
    font-size: 14;
}

Panel {
    background: $surface;
    color: $fg;
    border-color: $line;
    border-width: 1;
    padding: 6 10;
}

Text {
    color: $fg;
    font-size: 14;
}

.muted { color: $fg-dim; }

// Buttons
Button {
    background: $surface-2;
    color: $fg;
    border-color: $line;
    border-width: 1;
    radius: 6;
    padding: 6 14;
    transition: background 160ms ease-out, border-color 160ms ease-out;
}
Button:hover  { background: #1f2b1f; border-color: #33422f; }
Button:active { background: #172017; }
Button:disabled { color: $fg-dim; border-color: $line; }

.primary {
    background: $accent;
    color: $on-accent;
    border-color: $accent;
}
.primary:hover { background: #7ecb82; }

// Inputs
Input {
    background: $bg;
    color: $fg;
    border-color: $line;
    border-width: 1;
    radius: 4;
    padding: 6 8;
    transition: border-color 120ms ease-out;
}
Input:focus { border-color: $accent; }

// Selection in lists / menus
List { background: $surface; color: $fg; }
.selected { background: $surface-2; color: $fg; }

// Progress / slider track and fill
Progress { background: $surface-2; color: $accent; radius: 3; }
Slider   { background: $surface-2; color: $accent; radius: 3; }
"#;
