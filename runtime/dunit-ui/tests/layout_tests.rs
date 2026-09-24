//! Layout-engine tests for the DUI layer.

use dunit_ui::{layout, parse};

fn r(doc: &str, w: f32, h: f32) -> (dunit_ui::Tree, dunit_ui::Layout) {
    let tree = parse(doc).expect("parse");
    let lay = layout(&tree, w, h);
    (tree, lay)
}

#[test]
fn root_fills_viewport() {
    let (tree, lay) = r("Column {}", 800.0, 600.0);
    let rect = lay.rect(tree.root());
    assert_eq!((rect.x, rect.y, rect.w, rect.h), (0.0, 0.0, 800.0, 600.0));
}

#[test]
fn row_distributes_fixed_and_grow() {
    // 300px wide row, spacing 10: [100 fixed] [gap] [grow] => grow gets the rest.
    let (tree, lay) = r(
        r#"Row spacing=10 { Stack #a width=100 Stack #b width=grow }"#,
        300.0,
        50.0,
    );
    let a = lay.rect(tree.by_name("a").unwrap());
    let b = lay.rect(tree.by_name("b").unwrap());
    assert_eq!(a.x, 0.0);
    assert_eq!(a.w, 100.0);
    assert_eq!(b.x, 110.0); // 100 + spacing
    assert_eq!(b.w, 190.0); // 300 - 100 - 10
    // Cross axis stretches to the row height.
    assert_eq!(a.h, 50.0);
}

#[test]
fn two_grow_children_split_space() {
    let (tree, lay) = r(
        r#"Row { Stack #a grow=1 Stack #b grow=3 }"#,
        400.0,
        20.0,
    );
    let a = lay.rect(tree.by_name("a").unwrap());
    let b = lay.rect(tree.by_name("b").unwrap());
    assert_eq!(a.w, 100.0); // 1/4 of 400
    assert_eq!(b.w, 300.0); // 3/4 of 400
    assert_eq!(b.x, 100.0);
}

#[test]
fn column_stacks_vertically_with_padding() {
    let (tree, lay) = r(
        r#"Column padding=10 spacing=5 { Stack #a height=30 Stack #b height=40 }"#,
        200.0,
        200.0,
    );
    let a = lay.rect(tree.by_name("a").unwrap());
    let b = lay.rect(tree.by_name("b").unwrap());
    assert_eq!(a.y, 10.0); // top padding
    assert_eq!(a.x, 10.0); // left padding
    assert_eq!(a.w, 180.0); // stretched into inner width (200 - 20)
    assert_eq!(a.h, 30.0);
    assert_eq!(b.y, 45.0); // 10 + 30 + 5 spacing
    assert_eq!(b.h, 40.0);
}

#[test]
fn main_align_end_pushes_content() {
    // One 100px child in a 300px row aligned to the end.
    let (tree, lay) = r(
        r#"Row align=end { Stack #a width=100 }"#,
        300.0,
        20.0,
    );
    let a = lay.rect(tree.by_name("a").unwrap());
    assert_eq!(a.x, 200.0); // 300 - 100
}

#[test]
fn grid_places_cells_in_rows() {
    // 2 columns, 4 cells, 300px wide, spacing 20 => col width 140.
    let (tree, lay) = r(
        r#"Grid columns=2 spacing=20 {
            Stack #c0 height=50
            Stack #c1 height=50
            Stack #c2 height=50
            Stack #c3 height=50
        }"#,
        300.0,
        400.0,
    );
    let c0 = lay.rect(tree.by_name("c0").unwrap());
    let c1 = lay.rect(tree.by_name("c1").unwrap());
    let c2 = lay.rect(tree.by_name("c2").unwrap());
    assert_eq!(c0.w, 140.0); // (300 - 20) / 2
    assert_eq!(c0.x, 0.0);
    assert_eq!(c1.x, 160.0); // 140 + 20
    assert_eq!(c2.x, 0.0); // wrapped to next row
    assert_eq!(c2.y, 70.0); // 50 + 20 spacing
}

#[test]
fn stack_overlaps_children() {
    let (tree, lay) = r(
        r#"Stack { Stack #back Stack #front width=40 height=40 }"#,
        100.0,
        100.0,
    );
    let back = lay.rect(tree.by_name("back").unwrap());
    let front = lay.rect(tree.by_name("front").unwrap());
    // Auto child fills the stack; both share the same (start-aligned) origin.
    assert_eq!((back.x, back.y, back.w, back.h), (0.0, 0.0, 100.0, 100.0));
    assert_eq!((front.x, front.y), (0.0, 0.0));
    assert_eq!((front.w, front.h), (40.0, 40.0));
}

#[test]
fn min_max_constraints_clamp_size() {
    // Auto column content is 0 tall, but min-height forces 60.
    let (tree, lay) = r(
        r#"Column { Stack #a min-height=60 max-width=50 }"#,
        200.0,
        200.0,
    );
    let a = lay.rect(tree.by_name("a").unwrap());
    assert_eq!(a.h, 60.0);
    assert_eq!(a.w, 50.0); // stretched to 200 then clamped to max-width
}
