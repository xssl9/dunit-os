//! Layout-relevant attributes decoded from raw DUI `key=value` pairs. Unknown or
//! non-layout attributes are preserved verbatim in [`Attrs::raw`] for later
//! layers (DSS styling, widget behaviour) to consume.

use alloc::collections::BTreeMap;
use alloc::string::String;

/// How a node is sized along one axis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Sizing {
    /// Size to content / intrinsic size (the default).
    Auto,
    /// A fixed pixel size.
    Px(f32),
    /// A fraction (0..=1) of the parent's inner size along this axis.
    Pct(f32),
    /// Flexibly share leftover main-axis space by this weight.
    Grow(f32),
}

impl Default for Sizing {
    fn default() -> Self {
        Sizing::Auto
    }
}

/// Alignment of children along an axis (or a single node within its slot).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Start,
    Center,
    End,
    /// Stretch children to fill the cross axis (default cross alignment).
    Stretch,
    /// Distribute free main-axis space between children.
    SpaceBetween,
    /// Distribute free main-axis space around children.
    SpaceAround,
}

impl Align {
    fn parse(s: &str) -> Option<Align> {
        Some(match s {
            "start" => Align::Start,
            "center" => Align::Center,
            "end" => Align::End,
            "stretch" => Align::Stretch,
            "space-between" => Align::SpaceBetween,
            "space-around" => Align::SpaceAround,
            _ => return None,
        })
    }
}

/// Per-edge insets (padding), in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Edges {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

impl Edges {
    pub fn all(v: f32) -> Edges {
        Edges { top: v, right: v, bottom: v, left: v }
    }

    /// CSS-style shorthand: 1 value = all, 2 = vertical/horizontal,
    /// 4 = top/right/bottom/left. Other counts are ignored (returns `None`).
    fn parse(s: &str) -> Option<Edges> {
        let mut it = s.split_whitespace().filter_map(parse_f32);
        let v: [f32; 4] = {
            let a = it.next()?;
            match (it.next(), it.next(), it.next()) {
                (None, _, _) => [a, a, a, a],
                (Some(b), None, _) => [a, b, a, b],
                (Some(b), Some(c), Some(d)) => [a, b, c, d],
                _ => return None,
            }
        };
        Some(Edges { top: v[0], right: v[1], bottom: v[2], left: v[3] })
    }

    pub fn horizontal(&self) -> f32 {
        self.left + self.right
    }

    pub fn vertical(&self) -> f32 {
        self.top + self.bottom
    }
}

/// Decoded layout attributes for one node.
#[derive(Debug, Clone, PartialEq)]
pub struct Attrs {
    pub width: Sizing,
    pub height: Sizing,
    pub min_w: Option<f32>,
    pub min_h: Option<f32>,
    pub max_w: Option<f32>,
    pub max_h: Option<f32>,
    /// Flex weight along the parent's main axis (0 = fixed).
    pub grow: f32,
    /// Gap between children (Row/Column/Grid).
    pub spacing: f32,
    pub padding: Edges,
    /// Main-axis distribution of children.
    pub main_align: Align,
    /// Cross-axis alignment of children.
    pub cross_align: Align,
    /// Fixed column count for `Grid` (0 falls back to 1).
    pub columns: u32,
    /// Every raw attribute as written, for downstream layers.
    pub raw: BTreeMap<String, String>,
}

impl Default for Attrs {
    fn default() -> Self {
        Attrs {
            width: Sizing::Auto,
            height: Sizing::Auto,
            min_w: None,
            min_h: None,
            max_w: None,
            max_h: None,
            grow: 0.0,
            spacing: 0.0,
            padding: Edges::default(),
            main_align: Align::Start,
            cross_align: Align::Stretch,
            columns: 0,
            raw: BTreeMap::new(),
        }
    }
}

impl Attrs {
    /// Fold one `key=value` pair into these attributes. Recognised layout keys
    /// are decoded; everything (including recognised keys) is also stored raw.
    pub fn apply(&mut self, key: &str, value: &str) {
        match key {
            "width" | "w" => self.width = parse_sizing(value),
            "height" | "h" => self.height = parse_sizing(value),
            "min-width" => self.min_w = parse_f32(value),
            "min-height" => self.min_h = parse_f32(value),
            "max-width" => self.max_w = parse_f32(value),
            "max-height" => self.max_h = parse_f32(value),
            "grow" => self.grow = parse_f32(value).unwrap_or(0.0),
            "spacing" | "gap" => self.spacing = parse_f32(value).unwrap_or(0.0),
            "padding" | "pad" => {
                if let Some(e) = Edges::parse(value) {
                    self.padding = e;
                }
            }
            "align" | "main-align" => {
                if let Some(a) = Align::parse(value) {
                    self.main_align = a;
                }
            }
            "cross-align" => {
                if let Some(a) = Align::parse(value) {
                    self.cross_align = a;
                }
            }
            "columns" | "cols" => {
                self.columns = parse_f32(value).map(|v| v as u32).unwrap_or(0);
            }
            _ => {}
        }
        // A `grow` sizing keyword on width/height also seeds the flex weight so
        // `width=grow` behaves like `grow=1` on a Row.
        if key == "width" || key == "height" {
            if let Sizing::Grow(g) = parse_sizing(value) {
                if self.grow == 0.0 {
                    self.grow = g;
                }
            }
        }
        self.raw.insert(String::from(key), String::from(value));
    }
}

/// Parse a sizing token: `auto`, `grow`/`grow(N)`, `NN%`, or a plain number (px).
fn parse_sizing(s: &str) -> Sizing {
    if s == "auto" {
        return Sizing::Auto;
    }
    if s == "grow" || s == "fill" {
        return Sizing::Grow(1.0);
    }
    if let Some(rest) = s.strip_suffix('%') {
        if let Some(v) = parse_f32(rest) {
            return Sizing::Pct(v / 100.0);
        }
    }
    match parse_f32(s) {
        Some(v) => Sizing::Px(v),
        None => Sizing::Auto,
    }
}

/// Parse an `f32` without libm/std float parsing (both absent under `no_std`).
/// Accepts an optional sign, integer part and fractional part.
pub(crate) fn parse_f32(s: &str) -> Option<f32> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let (neg, body) = match s.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, s.strip_prefix('+').unwrap_or(s)),
    };
    if body.is_empty() {
        return None;
    }
    let mut int_part: f64 = 0.0;
    let mut frac_part: f64 = 0.0;
    let mut frac_scale: f64 = 1.0;
    let mut seen_dot = false;
    let mut seen_digit = false;
    for c in body.chars() {
        match c {
            '0'..='9' => {
                seen_digit = true;
                let d = (c as u8 - b'0') as f64;
                if seen_dot {
                    frac_scale *= 0.1;
                    frac_part += d * frac_scale;
                } else {
                    int_part = int_part * 10.0 + d;
                }
            }
            '.' if !seen_dot => seen_dot = true,
            _ => return None,
        }
    }
    if !seen_digit {
        return None;
    }
    let v = (int_part + frac_part) as f32;
    Some(if neg { -v } else { v })
}
