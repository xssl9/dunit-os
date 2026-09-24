//! Parser + retained-tree tests for the DUI layer.

use dunit_ui::tree::Kind;
use dunit_ui::{parse, Sizing, UiError};

const DOC: &str = r#"
// a small window
Column #root spacing=8 padding=16 {
    Text "Hello, world" size=20
    Row #buttons spacing=4 align=space-between {
        Button #ok "OK" grow=1
        Button #cancel "Cancel" width=80
    }
}
"#;

#[test]
fn parses_structure_and_names() {
    let tree = parse(DOC).expect("document should parse");
    let root = tree.node(tree.root());
    assert_eq!(root.kind, Kind::Column);
    assert_eq!(root.name.as_deref(), Some("root"));
    assert_eq!(root.children.len(), 2);

    // #name index resolves.
    let ok = tree.by_name("ok").expect("#ok should be indexed");
    let ok_node = tree.node(ok);
    assert_eq!(ok_node.kind, Kind::Element("Button".into()));
    assert_eq!(ok_node.text.as_deref(), Some("OK"));
    assert_eq!(ok_node.attrs.grow, 1.0);

    // A container tag resolves to a container kind.
    let buttons = tree.by_name("buttons").expect("#buttons");
    assert_eq!(tree.node(buttons).kind, Kind::Row);
    assert_eq!(tree.node(buttons).children.len(), 2);
}

#[test]
fn parses_layout_attributes() {
    let tree = parse(DOC).expect("parse");
    let root = tree.node(tree.root());
    assert_eq!(root.attrs.spacing, 8.0);
    assert_eq!(root.attrs.padding.left, 16.0);
    assert_eq!(root.attrs.padding.vertical(), 32.0);

    let cancel = tree.node(tree.by_name("cancel").unwrap());
    assert_eq!(cancel.attrs.width, Sizing::Px(80.0));
    // Raw attributes are preserved for later layers.
    assert_eq!(cancel.attrs.raw.get("width").map(String::as_str), Some("80"));
}

#[test]
fn shorthand_text_and_siblings_disambiguate() {
    // Three leaf siblings, no braces on the leaves: the parser must not treat a
    // sibling tag as an attribute of the previous node.
    let tree = parse(r#"Row { Text "a" Text "b" Text "c" }"#).expect("parse");
    assert_eq!(tree.node(tree.root()).children.len(), 3);
}

#[test]
fn percentage_and_grow_sizing() {
    let tree = parse(r#"Row { Stack width=50% Stack width=grow }"#).unwrap();
    let kids = &tree.node(tree.root()).children;
    assert_eq!(tree.node(kids[0]).attrs.width, Sizing::Pct(0.5));
    assert_eq!(tree.node(kids[1]).attrs.width, Sizing::Grow(1.0));
    // `width=grow` also seeds the flex weight.
    assert_eq!(tree.node(kids[1]).attrs.grow, 1.0);
}

#[test]
fn rejects_empty_document() {
    assert_eq!(parse("   // only a comment\n").unwrap_err(), UiError::NoRoot);
}

#[test]
fn rejects_unterminated_block() {
    match parse("Column { Text \"x\"") {
        Err(UiError::UnexpectedEof(_)) => {}
        other => panic!("expected UnexpectedEof, got {:?}", other),
    }
}

#[test]
fn rejects_trailing_content() {
    match parse("Row {} Column {}") {
        Err(UiError::Unexpected { .. }) => {}
        other => panic!("expected Unexpected, got {:?}", other),
    }
}
