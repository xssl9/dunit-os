//! Intrinsic content sizing for widgets.
//!
//! Layout ([`dunit_ui::layout_measured`]) asks for the content size of each
//! element leaf; [`intrinsic_size`] answers using the widget kind, its text and
//! its resolved DSS [`Style`] (font size, padding, border). Text metrics come
//! from a [`TextMeasure`] source so the sizing logic stays decoupled from font
//! loading; [`FontMeasure`] adapts a [`dunit_text::Font`].

use dunit_style::value::Value;
use dunit_style::Style;
use dunit_text::Font;

use crate::kind::Widget;

/// Default minimum track length for sliders/progress/inputs, in pixels.
const MIN_TRACK: f32 = 120.0;
/// Default font size when a style sets none, in pixels.
const DEFAULT_FONT_PX: f32 = 14.0;

/// A source of single-line text measurements in pixels.
pub trait TextMeasure {
    /// The `(width, height)` of `text` rendered on one line at `font_size` px.
    fn measure(&self, text: &str, font_size: f32) -> (f32, f32);
}

/// A [`TextMeasure`] backed by a real [`dunit_text::Font`].
pub struct FontMeasure<'a> {
    pub font: &'a Font,
}

impl TextMeasure for FontMeasure<'_> {
    fn measure(&self, text: &str, font_size: f32) -> (f32, f32) {
        let (_glyphs, width) = self.font.layout_line(text, font_size);
        let height = self.font.line_metrics(font_size).line_height();
        (width, height)
    }
}

/// Intrinsic content size of a widget: its text (if any) plus the padding and
/// border declared in `style`. Returns the box layout should reserve for the
/// widget's own content, before any DUI-attribute padding is folded in.
pub fn intrinsic_size(
    widget: Widget,
    text: Option<&str>,
    style: &Style,
    measure: &dyn TextMeasure,
) -> (f32, f32) {
    let font = style.number("font-size").unwrap_or(DEFAULT_FONT_PX);
    let (pad_h, pad_v) = pad_edges(style);
    let border = style.number("border-width").unwrap_or(0.0);
    let frame_w = pad_h + 2.0 * border;
    let frame_h = pad_v + 2.0 * border;

    let (cw, ch) = match widget {
        Widget::Text => text_size(text, font, measure),
        Widget::Button | Widget::Notification => text_size(text, font, measure),
        Widget::Icon => {
            let s = style.number("icon-size").unwrap_or(font);
            (s, s)
        }
        Widget::Input => {
            // Fit the placeholder/content but never below the minimum track.
            let (tw, th) = text_size(text, font, measure);
            (fmax(tw, MIN_TRACK), th)
        }
        Widget::Slider => (MIN_TRACK, fmax(font, 16.0)),
        Widget::Progress => (MIN_TRACK, style.number("thickness").unwrap_or(6.0)),
        Widget::List | Widget::Menu => {
            // Rows are supplied as children by the caller; on their own these
            // reserve one line at the minimum track width.
            let line = measure.measure("W", font).1;
            (MIN_TRACK, line)
        }
    };
    (cw + frame_w, ch + frame_h)
}

/// Text box for an optional string; empty/absent text still reserves one line's
/// height so an empty label or button keeps its vertical rhythm.
fn text_size(text: Option<&str>, font: f32, measure: &dyn TextMeasure) -> (f32, f32) {
    match text {
        Some(t) if !t.is_empty() => measure.measure(t, font),
        _ => (0.0, measure.measure("", font).1),
    }
}

/// Horizontal (`left+right`) and vertical (`top+bottom`) padding from the
/// `padding` style property, following CSS 1/2/4-value shorthand.
fn pad_edges(style: &Style) -> (f32, f32) {
    let v = match style.get("padding") {
        Some(v) => v,
        None => return (0.0, 0.0),
    };
    match v {
        Value::Number(n) => (2.0 * n, 2.0 * n),
        Value::List(items) => {
            let nums: alloc::vec::Vec<f32> =
                items.iter().filter_map(Value::as_number).collect();
            match nums.as_slice() {
                [a] => (2.0 * a, 2.0 * a),
                [v, h] => (2.0 * h, 2.0 * v),
                [t, r, b, l] => (r + l, t + b),
                _ => (0.0, 0.0),
            }
        }
        _ => (0.0, 0.0),
    }
}

#[inline]
fn fmax(a: f32, b: f32) -> f32 {
    if a > b {
        a
    } else {
        b
    }
}
