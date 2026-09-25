//! Per-widget interaction state and value.
//!
//! [`Interaction`] tracks the transient states DSS selectors key off
//! (`:hover`, `:focus`, `:active`, `:disabled`, `:selected`) and produces the
//! state-name list the [`dunit_style`] cascade consumes. [`WidgetValue`] holds
//! the mutable value of value-bearing widgets.

use alloc::string::String;
use alloc::vec::Vec;

/// The transient interaction state of a widget. Each flag maps to a DSS state
/// selector of the same name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Interaction {
    pub hovered: bool,
    pub focused: bool,
    /// Pressed / actuated (`:active`).
    pub pressed: bool,
    pub disabled: bool,
    pub selected: bool,
}

impl Interaction {
    pub fn new() -> Interaction {
        Interaction::default()
    }

    /// The active state names, ordered, for building a
    /// [`dunit_style::cascade::NodeStyle`]. A disabled widget reports only
    /// `disabled` (hover/press are suppressed while disabled).
    pub fn state_names(&self) -> Vec<&'static str> {
        let mut v = Vec::new();
        if self.disabled {
            v.push("disabled");
            if self.selected {
                v.push("selected");
            }
            if self.focused {
                v.push("focus");
            }
            return v;
        }
        if self.hovered {
            v.push("hover");
        }
        if self.focused {
            v.push("focus");
        }
        if self.pressed {
            v.push("active");
        }
        if self.selected {
            v.push("selected");
        }
        v
    }
}

/// The mutable value of a value-bearing widget.
#[derive(Debug, Clone, PartialEq)]
pub enum WidgetValue {
    /// The contents of a text `Input`.
    Text(String),
    /// A `Slider`/`Progress` fraction, clamped to `0.0..=1.0`.
    Fraction(f32),
    /// The selected index of a `List`/`Menu`, or `None` if nothing is selected.
    Selection(Option<usize>),
}

impl WidgetValue {
    /// A fraction value, clamped into `0.0..=1.0` on construction.
    pub fn fraction(f: f32) -> WidgetValue {
        WidgetValue::Fraction(clamp01(f))
    }

    /// The fraction, if this is a fraction value.
    pub fn as_fraction(&self) -> Option<f32> {
        match self {
            WidgetValue::Fraction(f) => Some(*f),
            _ => None,
        }
    }

    /// The text, if this is a text value.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            WidgetValue::Text(s) => Some(s.as_str()),
            _ => None,
        }
    }

    /// The selected index, if this is a selection value.
    pub fn as_selection(&self) -> Option<usize> {
        match self {
            WidgetValue::Selection(s) => *s,
            _ => None,
        }
    }
}

fn clamp01(f: f32) -> f32 {
    if f < 0.0 {
        0.0
    } else if f > 1.0 {
        1.0
    } else {
        f
    }
}
