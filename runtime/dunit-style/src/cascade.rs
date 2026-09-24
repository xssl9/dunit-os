//! The DSS cascade: resolve the effective [`Style`] for a node by layering
//! stylesheets in `defaults -> theme -> class -> state` order and, within a
//! layer, ordering matched rules by selector specificity and source order.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use crate::motion::Transition;
use crate::parse::{Selector, Stylesheet};
use crate::value::{Color, Value};

/// A description of the node being styled: its element tag, optional id, and its
/// active classes and states (e.g. `hover`, `focus`).
#[derive(Debug, Clone, Copy)]
pub struct NodeStyle<'a> {
    pub element: &'a str,
    pub id: Option<&'a str>,
    pub classes: &'a [&'a str],
    pub states: &'a [&'a str],
}

impl<'a> NodeStyle<'a> {
    /// A node with just an element tag (no id, classes or states).
    pub fn element(tag: &'a str) -> NodeStyle<'a> {
        NodeStyle { element: tag, id: None, classes: &[], states: &[] }
    }
}

/// Selector specificity, ordered ids > classes/states > elements (CSS-like).
/// The derived `Ord` compares fields top-to-bottom, which is exactly that order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Specificity {
    pub ids: u16,
    pub classes: u16,
    pub elements: u16,
}

impl Selector {
    /// Does this selector match `node`? An empty component is a wildcard; every
    /// listed class and state must be present on the node.
    pub fn matches(&self, node: &NodeStyle) -> bool {
        if let Some(e) = &self.element {
            if e != node.element {
                return false;
            }
        }
        if let Some(i) = &self.id {
            if node.id != Some(i.as_str()) {
                return false;
            }
        }
        for c in &self.classes {
            if !node.classes.contains(&c.as_str()) {
                return false;
            }
        }
        for s in &self.states {
            if !node.states.contains(&s.as_str()) {
                return false;
            }
        }
        true
    }

    /// The specificity of this selector.
    pub fn specificity(&self) -> Specificity {
        Specificity {
            ids: self.id.is_some() as u16,
            classes: (self.classes.len() + self.states.len()) as u16,
            elements: self.element.is_some() as u16,
        }
    }
}

/// The resolved style for a node: the winning value for each property.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Style {
    props: BTreeMap<String, Value>,
}

impl Style {
    /// The raw value for `property`, if set.
    pub fn get(&self, property: &str) -> Option<&Value> {
        self.props.get(property)
    }

    /// A colour-typed property.
    pub fn color(&self, property: &str) -> Option<Color> {
        self.props.get(property).and_then(Value::as_color)
    }

    /// A number-typed property.
    pub fn number(&self, property: &str) -> Option<f32> {
        self.props.get(property).and_then(Value::as_number)
    }

    /// A keyword-typed property.
    pub fn keyword(&self, property: &str) -> Option<&str> {
        self.props.get(property).and_then(Value::as_keyword)
    }

    /// The declared transitions (`transition: prop dur easing, ...`).
    pub fn transitions(&self) -> Vec<Transition> {
        match self.props.get("transition") {
            Some(v) => Transition::parse_value(v),
            None => Vec::new(),
        }
    }

    /// Number of resolved properties.
    pub fn len(&self) -> usize {
        self.props.len()
    }

    pub fn is_empty(&self) -> bool {
        self.props.is_empty()
    }
}

/// An ordered stack of stylesheets forming a cascade. Push in application order:
/// defaults first, then theme, then more specific overlays; later layers win
/// ties against earlier ones.
#[derive(Debug, Clone, Default)]
pub struct Cascade {
    layers: Vec<Stylesheet>,
}

impl Cascade {
    pub fn new() -> Cascade {
        Cascade { layers: Vec::new() }
    }

    /// Append a stylesheet as the next (higher-priority) layer.
    pub fn push(&mut self, sheet: Stylesheet) -> &mut Cascade {
        self.layers.push(sheet);
        self
    }

    /// Resolve the effective style for `node`. Matched rules are applied in
    /// ascending (specificity, layer, source-order) order so the strongest
    /// declaration for each property wins.
    pub fn resolve(&self, node: &NodeStyle) -> Style {
        // Gather every matching rule with its sort key.
        let mut matched: Vec<(Specificity, usize, u32, usize, usize)> = Vec::new();
        for (li, sheet) in self.layers.iter().enumerate() {
            for (ri, rule) in sheet.rules.iter().enumerate() {
                if rule.selector.matches(node) {
                    matched.push((rule.selector.specificity(), li, rule.order, li, ri));
                }
            }
        }
        matched.sort_by(|a, b| {
            a.0.cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2))
        });

        let mut props: BTreeMap<String, Value> = BTreeMap::new();
        for (_, _, _, li, ri) in matched {
            for decl in &self.layers[li].rules[ri].declarations {
                props.insert(decl.property.clone(), decl.value.clone());
            }
        }
        Style { props }
    }
}

