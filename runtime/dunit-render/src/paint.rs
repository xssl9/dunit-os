//! Painting a laid-out DUI tree into a [`Surface`].

use dunit_style::value::{Color, Value};
use dunit_style::Style;
use dunit_text::Font;
use dunit_ui::layout::Layout;
use dunit_ui::tree::{NodeId, Tree};
use dunit_ui::Rect;

use crate::surface::Surface;

/// A resolver from a node to its already-cascaded DSS style. Callers build these
/// from [`dunit_style::cascade`]; the painter treats the result as read-only.
pub type StyleOf<'a> = &'a dyn Fn(NodeId) -> Style;

/// Default text/border color when a style declares none: opaque black.
const DEFAULT_INK: Color = Color::rgb(0, 0, 0);
/// Default font size, matching the widget measurer.
const DEFAULT_FONT_PX: f32 = 14.0;

/// Paint the whole tree into `surface`, in document order (parents before
/// children, so children paint on top). Each node contributes its background
/// fill, border stroke and text, as declared by its resolved [`Style`].
pub fn paint(tree: &Tree, layout: &Layout, style_of: StyleOf, font: &Font, surface: &mut Surface) {
    if tree.is_empty() {
        return;
    }
    paint_node(tree, layout, tree.root(), style_of, font, surface);
}

fn paint_node(
    tree: &Tree,
    layout: &Layout,
    id: NodeId,
    style_of: StyleOf,
    font: &Font,
    surface: &mut Surface,
) {
    let rect = layout.rect(id);
    let style = style_of(id);

    if let Some(bg) = style.color("background") {
        surface.fill_rect(rect.x, rect.y, rect.w, rect.h, bg);
    }

    let border = style.number("border-width").unwrap_or(0.0);
    if border > 0.0 {
        let bc = style.color("border-color").unwrap_or(DEFAULT_INK);
        surface.stroke_rect(rect.x, rect.y, rect.w, rect.h, border, bc);
    }

    if let Some(text) = node_text(tree, id) {
        paint_text(&style, rect, border, text, font, surface);
    }

    for &c in &tree.node(id).children {
        paint_node(tree, layout, c, style_of, font, surface);
    }
}

/// The non-empty text of a node, if any.
fn node_text(tree: &Tree, id: NodeId) -> Option<&str> {
    match tree.node(id).text.as_deref() {
        Some(t) if !t.is_empty() => Some(t),
        _ => None,
    }
}

/// Draw a single line of text inside `rect`'s content box (inset by border and
/// padding), on the font baseline.
fn paint_text(style: &Style, rect: Rect, border: f32, text: &str, font: &Font, surface: &mut Surface) {
    let px = style.number("font-size").unwrap_or(DEFAULT_FONT_PX);
    let ink = style.color("color").unwrap_or(DEFAULT_INK);
    let (pad_l, pad_t) = pad_left_top(style);
    let origin_x = rect.x + border + pad_l;
    let baseline = rect.y + border + pad_t + font.line_metrics(px).ascent;

    let (glyphs, _advance) = font.layout_line(text, px);
    for g in glyphs {
        if let Some(bmp) = font.rasterize(g.glyph, px) {
            let ox = round_i32(origin_x + g.x) + bmp.left;
            let oy = round_i32(baseline) - bmp.top;
            surface.blit_glyph(&bmp, ox, oy, ink);
        }
    }
}

/// The left and top padding declared by the `padding` shorthand (CSS 1/2/4).
fn pad_left_top(style: &Style) -> (f32, f32) {
    let v = match style.get("padding") {
        Some(v) => v,
        None => return (0.0, 0.0),
    };
    match v {
        Value::Number(n) => (*n, *n),
        Value::List(items) => {
            let nums: alloc::vec::Vec<f32> = items.iter().filter_map(Value::as_number).collect();
            match nums.as_slice() {
                [a] => (*a, *a),
                [vert, horiz] => (*horiz, *vert),
                [t, _r, _b, l] => (*l, *t),
                _ => (0.0, 0.0),
            }
        }
        _ => (0.0, 0.0),
    }
}

fn round_i32(v: f32) -> i32 {
    if v <= 0.0 {
        0
    } else {
        (v + 0.5) as i32
    }
}
