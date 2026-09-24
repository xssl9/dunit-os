//! DSS parser tests.

use dunit_style::value::{Color, Value};
use dunit_style::{parse, StyleError};

#[test]
fn parses_rules_selectors_and_values() {
    let sheet = parse(
        r#"
        // a comment
        Button#ok.primary:hover {
            color: #d6e4d6;
            padding: 6 14;
            radius: 6;
        }
        "#,
    )
    .expect("parse");
    assert_eq!(sheet.rules.len(), 1);
    let sel = &sheet.rules[0].selector;
    assert_eq!(sel.element.as_deref(), Some("Button"));
    assert_eq!(sel.id.as_deref(), Some("ok"));
    assert_eq!(sel.classes, ["primary"]);
    assert_eq!(sel.states, ["hover"]);

    let decls = &sheet.rules[0].declarations;
    assert_eq!(decls[0].property, "color");
    assert_eq!(decls[0].value, Value::Color(Color::rgb(0xd6, 0xe4, 0xd6)));
    // A two-number value becomes a list.
    assert_eq!(
        decls[1].value,
        Value::List(vec![Value::Number(6.0), Value::Number(14.0)])
    );
    assert_eq!(decls[2].value, Value::Number(6.0));
}

#[test]
fn expands_variables() {
    let sheet = parse(
        r#"
        $bg: #101410;
        Window { background: $bg; }
        "#,
    )
    .unwrap();
    assert_eq!(
        sheet.rules[0].declarations[0].value,
        Value::Color(Color::rgb(0x10, 0x14, 0x10))
    );
}

#[test]
fn selector_list_shares_block() {
    let sheet = parse(r#" #ok, #save { min-width: 96; } "#).unwrap();
    assert_eq!(sheet.rules.len(), 2);
    assert_eq!(sheet.rules[0].selector.id.as_deref(), Some("ok"));
    assert_eq!(sheet.rules[1].selector.id.as_deref(), Some("save"));
    assert_eq!(sheet.rules[0].declarations, sheet.rules[1].declarations);
}

#[test]
fn parses_durations_and_keywords() {
    let sheet = parse(r#"Button { transition: background 160ms ease-out; }"#).unwrap();
    let v = &sheet.rules[0].declarations[0].value;
    let list = v.as_list().expect("list");
    assert_eq!(list[0], Value::Keyword("background".into()));
    assert_eq!(list[1], Value::Duration(160.0));
    assert_eq!(list[2], Value::Keyword("ease-out".into()));
}

#[test]
fn rejects_unterminated_block() {
    match parse("Button { color: #fff;") {
        Err(StyleError::UnexpectedEof(_)) => {}
        other => panic!("expected UnexpectedEof, got {:?}", other),
    }
}
