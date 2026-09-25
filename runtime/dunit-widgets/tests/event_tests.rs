//! Hit-testing, event-path and focus-navigation tests over a laid-out tree.

use dunit_ui::{layout, parse};
use dunit_widgets::{event_path, focus_next, focus_order, focus_prev, hit_test};

/// A tree with explicit sizes so layout produces real, testable rects.
fn scene() -> dunit_ui::tree::Tree {
    parse(
        r#"Column spacing=0 {
            Button #a width=100 height=20
            Row spacing=0 {
                Text #t width=40 height=20
                Input #i width=100 height=20
            }
            Slider #s width=100 height=20
        }"#,
    )
    .expect("parse")
}

#[test]
fn hit_test_finds_deepest_node() {
    let tree = scene();
    let lay = layout(&tree, 300.0, 200.0);

    let a = tree.by_name("a").unwrap();
    let t = tree.by_name("t").unwrap();
    let i = tree.by_name("i").unwrap();

    // Inside the first button.
    assert_eq!(hit_test(&tree, &lay, 20.0, 10.0), Some(a));
    // Row row: left cell is the text, past 40px is the input.
    assert_eq!(hit_test(&tree, &lay, 20.0, 25.0), Some(t));
    assert_eq!(hit_test(&tree, &lay, 60.0, 25.0), Some(i));
    // Right of every child but still inside the column -> the container.
    assert_eq!(hit_test(&tree, &lay, 150.0, 10.0), Some(tree.root()));
    // Fully outside the root.
    assert_eq!(hit_test(&tree, &lay, 400.0, 400.0), None);
}

#[test]
fn event_path_is_root_to_target() {
    let tree = scene();
    let i = tree.by_name("i").unwrap();
    let path = event_path(&tree, i);
    // Column (root) -> Row -> Input.
    assert_eq!(path.first(), Some(&tree.root()));
    assert_eq!(path.last(), Some(&i));
    assert_eq!(path.len(), 3);
}

#[test]
fn focus_order_skips_non_focusable_and_cycles() {
    let tree = scene();
    let a = tree.by_name("a").unwrap();
    let i = tree.by_name("i").unwrap();
    let s = tree.by_name("s").unwrap();

    // Text is not focusable; order follows document order.
    let order = focus_order(&tree);
    assert_eq!(order, [a, i, s]);

    assert_eq!(focus_next(&order, Some(a)), Some(i));
    assert_eq!(focus_next(&order, Some(s)), Some(a)); // wraps
    assert_eq!(focus_next(&order, None), Some(a));

    assert_eq!(focus_prev(&order, Some(i)), Some(a));
    assert_eq!(focus_prev(&order, Some(a)), Some(s)); // wraps
    assert_eq!(focus_prev(&order, None), Some(s));
}
