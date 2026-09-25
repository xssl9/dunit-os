//! Intrinsic sizing tests using a deterministic mock text measurer.

use dunit_style::cascade::{Cascade, NodeStyle};
use dunit_style::parse;
use dunit_widgets::{intrinsic_size, TextMeasure, Widget};

/// A monospace-ish mock: each char is `font/2` px wide, lines are `font+4` tall.
/// Both are exact in f32 for the font sizes used here, so equality holds.
struct Mono;

impl TextMeasure for Mono {
    fn measure(&self, text: &str, font_size: f32) -> (f32, f32) {
        (text.chars().count() as f32 * (font_size * 0.5), font_size + 4.0)
    }
}

/// Resolve a `Style` for `tag` from a small stylesheet.
fn style_for(tag: &str, css: &str) -> dunit_style::Style {
    let sheet = parse(css).expect("stylesheet parses");
    let mut cas = Cascade::new();
    cas.push(sheet);
    cas.resolve(&NodeStyle::element(tag))
}

#[test]
fn button_adds_padding_and_border() {
    // padding: 6 10 -> vertical 6, horizontal 10 (CSS 2-value shorthand).
    let style = style_for("Button", "Button { font-size: 14; padding: 6 10; border-width: 1; }");
    let (w, h) = intrinsic_size(Widget::Button, Some("OK"), &style, &Mono);
    // text: 2 * 7 = 14 wide, 18 tall.
    // frame: pad_h = 2*10 = 20, pad_v = 2*6 = 12; border adds 2 on each axis.
    assert_eq!(w, 14.0 + 20.0 + 2.0);
    assert_eq!(h, 18.0 + 12.0 + 2.0);
}

#[test]
fn text_is_bare_content() {
    let style = style_for("Text", "Text { font-size: 14; }");
    let (w, h) = intrinsic_size(Widget::Text, Some("Hi"), &style, &Mono);
    assert_eq!(w, 14.0);
    assert_eq!(h, 18.0);
}

#[test]
fn empty_text_still_reserves_a_line() {
    let style = style_for("Text", "Text { font-size: 14; }");
    let (w, h) = intrinsic_size(Widget::Text, None, &style, &Mono);
    assert_eq!(w, 0.0);
    assert_eq!(h, 18.0);
}

#[test]
fn tracks_use_minimum_width() {
    // No matching rule -> empty style, defaults apply (font 14).
    let style = style_for("Nope", "Button { font-size: 99; }");
    assert_eq!(intrinsic_size(Widget::Slider, None, &style, &Mono), (120.0, 16.0));
    assert_eq!(intrinsic_size(Widget::Progress, None, &style, &Mono), (120.0, 6.0));
    // Input fits content but never below the track minimum.
    assert_eq!(intrinsic_size(Widget::Input, Some("x"), &style, &Mono).0, 120.0);
}
