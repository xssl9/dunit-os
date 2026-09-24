//! The DSS value model: colours, numbers, durations, keywords and short lists,
//! with the hand-rolled parsing DSS needs (no libm / std float parse under
//! `no_std`).

use alloc::string::String;
use alloc::vec::Vec;

/// A straight (non-premultiplied) 8-bit RGBA colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color {
        Color { r, g, b, a }
    }

    pub const fn rgb(r: u8, g: u8, b: u8) -> Color {
        Color { r, g, b, a: 0xff }
    }

    /// Parse a `#rgb`, `#rgba`, `#rrggbb` or `#rrggbbaa` hex colour. The leading
    /// `#` is optional. Returns `None` on any malformed input.
    pub fn parse(s: &str) -> Option<Color> {
        let h = s.strip_prefix('#').unwrap_or(s);
        let bytes = h.as_bytes();
        match bytes.len() {
            3 | 4 => {
                let r = nib(bytes[0])?;
                let g = nib(bytes[1])?;
                let b = nib(bytes[2])?;
                let a = if bytes.len() == 4 { nib(bytes[3])? } else { 0xf };
                // Expand each nibble to a full byte (0xf -> 0xff).
                Some(Color::rgba(r * 17, g * 17, b * 17, a * 17))
            }
            6 | 8 => {
                let r = hex2(bytes[0], bytes[1])?;
                let g = hex2(bytes[2], bytes[3])?;
                let b = hex2(bytes[4], bytes[5])?;
                let a = if bytes.len() == 8 { hex2(bytes[6], bytes[7])? } else { 0xff };
                Some(Color::rgba(r, g, b, a))
            }
            _ => None,
        }
    }
}

fn nib(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

fn hex2(hi: u8, lo: u8) -> Option<u8> {
    Some(nib(hi)? * 16 + nib(lo)?)
}

/// A resolved DSS declaration value.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// A colour (`#rrggbb`, ...).
    Color(Color),
    /// A bare number (pixels, unitless weights, ...).
    Number(f32),
    /// A time in milliseconds (`250ms`, `0.3s`).
    Duration(f32),
    /// A bareword such as `ease-in-out`, `bold`, `hidden`.
    Keyword(String),
    /// A whitespace-separated list (e.g. `padding: 8 16`).
    List(Vec<Value>),
}

impl Value {
    /// Interpret a single already-tokenised value word (variables must already
    /// be expanded by the parser). Tries colour, then duration, then number,
    /// falling back to a keyword.
    pub fn parse_word(s: &str) -> Value {
        if s.starts_with('#') {
            if let Some(c) = Color::parse(s) {
                return Value::Color(c);
            }
        }
        if let Some(ms) = parse_duration(s) {
            return Value::Duration(ms);
        }
        if let Some(n) = parse_number_with_px(s) {
            return Value::Number(n);
        }
        Value::Keyword(String::from(s))
    }

    /// The colour, if this value is one.
    pub fn as_color(&self) -> Option<Color> {
        match self {
            Value::Color(c) => Some(*c),
            _ => None,
        }
    }

    /// The number, if this value is a plain number.
    pub fn as_number(&self) -> Option<f32> {
        match self {
            Value::Number(n) => Some(*n),
            _ => None,
        }
    }

    /// The duration in milliseconds, if this value is a duration.
    pub fn as_duration(&self) -> Option<f32> {
        match self {
            Value::Duration(ms) => Some(*ms),
            _ => None,
        }
    }

    /// The keyword text, if this value is a keyword.
    pub fn as_keyword(&self) -> Option<&str> {
        match self {
            Value::Keyword(s) => Some(s.as_str()),
            _ => None,
        }
    }

    /// The list items, if this value is a list.
    pub fn as_list(&self) -> Option<&[Value]> {
        match self {
            Value::List(v) => Some(v.as_slice()),
            _ => None,
        }
    }
}

/// Parse a duration token (`250ms` or `0.3s`) into milliseconds.
fn parse_duration(s: &str) -> Option<f32> {
    if let Some(rest) = s.strip_suffix("ms") {
        return parse_f32(rest);
    }
    if let Some(rest) = s.strip_suffix('s') {
        // Guard against a lone "s" or things ending in "s" that are not numbers.
        return parse_f32(rest).map(|v| v * 1000.0);
    }
    None
}

/// Parse a number that may carry a trailing `px` unit.
fn parse_number_with_px(s: &str) -> Option<f32> {
    let body = s.strip_suffix("px").unwrap_or(s);
    parse_f32(body)
}

/// Parse an `f32` without libm/std float parsing. Accepts an optional sign, an
/// integer part and a fractional part.
pub fn parse_f32(s: &str) -> Option<f32> {
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
