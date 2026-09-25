//! End-to-end painting: DUI tree + DSS cascade + font -> pixels.

use dunit_render::{pack_argb, paint, Surface};
use dunit_style::cascade::{Cascade, NodeStyle};
use dunit_style::value::Color;
use dunit_style::parse as parse_dss;
use dunit_text::Font;
use dunit_ui::{layout, parse as parse_dui};

fn load_font() -> Font {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/fonts/DejaVuSans.ttf");
    let bytes = std::fs::read(path).expect("bundled DejaVuSans.ttf must be readable");
    Font::parse(bytes).expect("DejaVuSans.ttf must parse")
}

#[test]
fn paints_background_border_and_text() {
    let font = load_font();
    let tree = parse_dui(r#"Column#root { Button#ok width=120 height=40 "OK" }"#).expect("dui");

    let sheet = parse_dss(
        r#"
        Column { background: #202020; }
        Button {
            background: #3050ff;
            color: #ffffff;
            border-width: 2;
            border-color: #000000;
            font-size: 16;
            padding: 4;
        }
        "#,
    )
    .expect("dss");
    let mut cas = Cascade::new();
    cas.push(sheet);

    let lay = layout(&tree, 200.0, 60.0);

    let mut buf = vec![0u32; 200 * 60];
    let mut surface = Surface::new(&mut buf, 200, 60);
    let style_of = |id| cas.resolve(&NodeStyle::element(tree.node(id).kind.tag()));
    paint(&tree, &lay, &style_of, &font, &mut surface);

    // Column fills the whole viewport behind the button.
    assert_eq!(surface.get(160, 50), pack_argb(Color::rgb(0x20, 0x20, 0x20)));
    // Button interior is its blue background (clear of border and glyphs).
    assert_eq!(surface.get(80, 30), pack_argb(Color::rgb(0x30, 0x50, 0xff)));
    // Left edge of the button is the black border.
    assert_eq!(surface.get(0, 20), pack_argb(Color::rgb(0, 0, 0)));

    // The "OK" label inked white pixels somewhere in the content box.
    let mut ink = 0;
    for y in 4..36 {
        for x in 4..120 {
            let p = surface.get(x, y);
            let (r, g, b) = ((p >> 16) & 0xff, (p >> 8) & 0xff, p & 0xff);
            if r > 200 && g > 200 && b > 200 {
                ink += 1;
            }
        }
    }
    assert!(ink > 0, "expected white text pixels, found none");
}

#[test]
fn empty_tree_paints_nothing() {
    let font = load_font();
    let tree = parse_dui("Column").expect("dui");
    let lay = layout(&tree, 10.0, 10.0);
    let mut buf = vec![0u32; 100];
    let mut surface = Surface::new(&mut buf, 10, 10);
    // No background declared -> surface stays zeroed.
    let style_of = |id| Cascade::new().resolve(&NodeStyle::element(tree.node(id).kind.tag()));
    paint(&tree, &lay, &style_of, &font, &mut surface);
    assert!(buf.iter().all(|&p| p == 0));
}
