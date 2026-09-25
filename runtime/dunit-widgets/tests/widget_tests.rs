//! Widget kind, interaction-state and value tests.

use dunit_widgets::{Interaction, Widget, WidgetValue};

#[test]
fn tag_mapping_and_roles() {
    assert_eq!(Widget::from_tag("Button"), Some(Widget::Button));
    assert_eq!(Widget::from_tag("Label"), Some(Widget::Text));
    assert_eq!(Widget::from_tag("Toast"), Some(Widget::Notification));
    // Containers and unknown tags are not widgets.
    assert_eq!(Widget::from_tag("Row"), None);
    assert_eq!(Widget::from_tag("Frobnicate"), None);

    assert_eq!(Widget::Button.tag(), "Button");
    assert!(Widget::Button.focusable());
    assert!(Widget::Slider.focusable());
    assert!(!Widget::Text.focusable());
    assert!(!Widget::Progress.focusable());

    assert!(Widget::Input.has_value());
    assert!(Widget::Slider.has_value());
    assert!(!Widget::Text.has_value());
}

#[test]
fn state_names_reflect_interaction() {
    let mut i = Interaction::new();
    i.hovered = true;
    i.focused = true;
    assert_eq!(i.state_names(), ["hover", "focus"]);

    // Disabled suppresses hover/press but keeps focus/selection visibility.
    let mut d = Interaction::new();
    d.disabled = true;
    d.hovered = true;
    d.pressed = true;
    assert_eq!(d.state_names(), ["disabled"]);

    let mut ds = Interaction::new();
    ds.disabled = true;
    ds.selected = true;
    ds.focused = true;
    assert_eq!(ds.state_names(), ["disabled", "selected", "focus"]);
}

#[test]
fn widget_value_helpers() {
    // Fractions clamp on construction.
    assert_eq!(WidgetValue::fraction(1.5).as_fraction(), Some(1.0));
    assert_eq!(WidgetValue::fraction(-0.2).as_fraction(), Some(0.0));
    assert_eq!(WidgetValue::fraction(0.4).as_fraction(), Some(0.4));

    let t = WidgetValue::Text("hi".into());
    assert_eq!(t.as_text(), Some("hi"));
    assert_eq!(t.as_fraction(), None);

    assert_eq!(WidgetValue::Selection(Some(3)).as_selection(), Some(3));
    assert_eq!(WidgetValue::Selection(None).as_selection(), None);
}
