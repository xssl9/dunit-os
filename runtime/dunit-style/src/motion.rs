//! Declarative motion: transition descriptors, easing curves and value
//! interpolation for the animation clock. There is no scripting — a transition
//! only names a property, a duration and an easing; the compositor supplies the
//! clock and calls [`interpolate`] each frame.

use alloc::string::String;
use alloc::vec::Vec;

use crate::value::{Color, Value};

/// An easing curve mapping normalised time `t` in `0..=1` to eased progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Easing {
    #[default]
    Linear,
    EaseIn,
    EaseOut,
    EaseInOut,
}

impl Easing {
    /// Parse an easing keyword; unknown keywords fall back to `None`.
    pub fn parse(s: &str) -> Option<Easing> {
        Some(match s {
            "linear" => Easing::Linear,
            "ease-in" => Easing::EaseIn,
            "ease-out" => Easing::EaseOut,
            "ease" | "ease-in-out" => Easing::EaseInOut,
            _ => return None,
        })
    }

    /// Apply the curve to a normalised time, clamping to `0..=1`.
    pub fn apply(self, t: f32) -> f32 {
        let t = if t < 0.0 { 0.0 } else if t > 1.0 { 1.0 } else { t };
        match self {
            Easing::Linear => t,
            Easing::EaseIn => t * t,
            Easing::EaseOut => 1.0 - (1.0 - t) * (1.0 - t),
            // Smoothstep: 3t^2 - 2t^3.
            Easing::EaseInOut => t * t * (3.0 - 2.0 * t),
        }
    }
}

/// A single declared transition: interpolate `property` over `duration_ms`
/// using `easing`.
#[derive(Debug, Clone, PartialEq)]
pub struct Transition {
    pub property: String,
    pub duration_ms: f32,
    pub easing: Easing,
}

impl Transition {
    /// Decode a `transition` declaration value into zero or more transitions.
    /// A new transition starts at each non-easing keyword (the property name);
    /// a duration or bare number sets its duration and an easing keyword its
    /// curve, in any order after the property.
    pub fn parse_value(v: &Value) -> Vec<Transition> {
        let mut items: Vec<&Value> = Vec::new();
        match v {
            Value::List(l) => items.extend(l.iter()),
            single => items.push(single),
        }
        let mut out = Vec::new();
        let mut cur: Option<Transition> = None;
        for it in items {
            match it {
                Value::Duration(ms) => {
                    if let Some(t) = &mut cur {
                        t.duration_ms = *ms;
                    }
                }
                Value::Number(n) => {
                    if let Some(t) = &mut cur {
                        t.duration_ms = *n;
                    }
                }
                Value::Keyword(k) => {
                    if let Some(e) = Easing::parse(k) {
                        if let Some(t) = &mut cur {
                            t.easing = e;
                        }
                    } else {
                        if let Some(t) = cur.take() {
                            out.push(t);
                        }
                        cur = Some(Transition {
                            property: k.clone(),
                            duration_ms: 0.0,
                            easing: Easing::default(),
                        });
                    }
                }
                _ => {}
            }
        }
        if let Some(t) = cur.take() {
            out.push(t);
        }
        out
    }
}

/// Linear interpolation between two `f32`s.
pub fn lerp_f32(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Interpolate between two colours per channel (rounded to the nearest byte).
pub fn lerp_color(a: Color, b: Color, t: f32) -> Color {
    let ch = |x: u8, y: u8| -> u8 {
        let v = lerp_f32(x as f32, y as f32, t) + 0.5;
        let v = if v < 0.0 { 0.0 } else if v > 255.0 { 255.0 } else { v };
        v as u8
    };
    Color::rgba(ch(a.r, b.r), ch(a.g, b.g), ch(a.b, b.b), ch(a.a, b.a))
}

/// Interpolate between two values at eased progress `t` (already in `0..=1`).
/// Colours, numbers and durations blend continuously; anything else snaps at
/// the halfway point.
pub fn lerp_value(from: &Value, to: &Value, t: f32) -> Value {
    match (from, to) {
        (Value::Color(a), Value::Color(b)) => Value::Color(lerp_color(*a, *b, t)),
        (Value::Number(a), Value::Number(b)) => Value::Number(lerp_f32(*a, *b, t)),
        (Value::Duration(a), Value::Duration(b)) => Value::Duration(lerp_f32(*a, *b, t)),
        _ => {
            if t < 0.5 {
                from.clone()
            } else {
                to.clone()
            }
        }
    }
}

/// Interpolate from `from` to `to` at raw progress `t`, shaped by `easing`.
pub fn interpolate(from: &Value, to: &Value, t: f32, easing: Easing) -> Value {
    lerp_value(from, to, easing.apply(t))
}
