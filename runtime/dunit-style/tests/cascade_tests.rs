//! DSS cascade tests.

use dunit_style::cascade::NodeStyle;
use dunit_style::value::Color;
use dunit_style::{parse, Cascade, GREEN_TEA_DSS};

#[test]
fn specificity_and_state_win() {
    let sheet = parse(
        r#"
        Button          { color: #111111; }
        .primary        { color: #222222; }
        Button:hover    { color: #333333; }
        #ok             { color: #444444; }
        "#,
    )
    .unwrap();
    let mut cas = Cascade::new();
    cas.push(sheet);

    // Base Button: only the element rule applies.
    let base = cas.resolve(&NodeStyle::element("Button"));
    assert_eq!(base.color("color"), Some(Color::rgb(0x11, 0x11, 0x11)));

    // With id present, #ok (specificity 1,0,0) beats everything else.
    let with_id = NodeStyle {
        element: "Button",
        id: Some("ok"),
        classes: &["primary"],
        states: &["hover"],
    };
    assert_eq!(cas.resolve(&with_id).color("color"), Some(Color::rgb(0x44, 0x44, 0x44)));

    // No id, but hover active: the state rule (0,1,1) beats the class (0,1,0)
    // and element (0,0,1).
    let hovered = NodeStyle {
        element: "Button",
        id: None,
        classes: &["primary"],
        states: &["hover"],
    };
    assert_eq!(cas.resolve(&hovered).color("color"), Some(Color::rgb(0x33, 0x33, 0x33)));
}

#[test]
fn later_layer_overrides_equal_specificity() {
    let defaults = parse(r#"Button { color: #101010; }"#).unwrap();
    let theme = parse(r#"Button { color: #202020; }"#).unwrap();
    let mut cas = Cascade::new();
    cas.push(defaults).push(theme);
    let s = cas.resolve(&NodeStyle::element("Button"));
    assert_eq!(s.color("color"), Some(Color::rgb(0x20, 0x20, 0x20)));
}

#[test]
fn unmatched_node_has_empty_style() {
    let sheet = parse(r#"Button { color: #fff; }"#).unwrap();
    let mut cas = Cascade::new();
    cas.push(sheet);
    assert!(cas.resolve(&NodeStyle::element("Text")).is_empty());
}

#[test]
fn green_tea_theme_resolves() {
    let theme = parse(GREEN_TEA_DSS).expect("theme parses");
    let mut cas = Cascade::new();
    cas.push(theme);

    // A hovered primary button: .primary background wins over Button, and its
    // hover state raises it further.
    let btn = NodeStyle {
        element: "Button",
        id: None,
        classes: &["primary"],
        states: &["hover"],
    };
    let s = cas.resolve(&btn);
    assert_eq!(s.color("background"), Some(Color::rgb(0x7e, 0xcb, 0x82)));
    // Base button declares transitions.
    let plain = cas.resolve(&NodeStyle::element("Button"));
    assert_eq!(plain.transitions().len(), 2);
}
