//! The retained component tree: an arena of [`Node`]s with a stable string-ID
//! index. Parsing builds a fresh `Tree` off-screen; the caller swaps it in
//! atomically and can keep the previous tree as last-known-good on a parse error.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use crate::attrs::Attrs;

/// A stable handle to a node within one [`Tree`] (an arena index).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId(pub u32);

/// The structural kind of a node. The five container kinds drive layout; every
/// other element (`Text`, `Button`, …) is an opaque, sized leaf at this layer —
/// widget semantics arrive in a later slice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    Row,
    Column,
    Stack,
    Grid,
    Scroll,
    /// A non-container element, carrying its source tag (e.g. `"Text"`).
    Element(String),
}

impl Kind {
    /// Resolve a source tag name to a kind.
    pub fn from_tag(tag: &str) -> Kind {
        match tag {
            "Row" => Kind::Row,
            "Column" => Kind::Column,
            "Stack" => Kind::Stack,
            "Grid" => Kind::Grid,
            "Scroll" => Kind::Scroll,
            other => Kind::Element(String::from(other)),
        }
    }

    /// Whether this kind lays out children (vs. being a sized leaf).
    pub fn is_container(&self) -> bool {
        !matches!(self, Kind::Element(_))
    }

    /// The source tag string for this kind.
    pub fn tag(&self) -> &str {
        match self {
            Kind::Row => "Row",
            Kind::Column => "Column",
            Kind::Stack => "Stack",
            Kind::Grid => "Grid",
            Kind::Scroll => "Scroll",
            Kind::Element(s) => s,
        }
    }
}

/// One node in the retained tree.
#[derive(Debug, Clone)]
pub struct Node {
    pub id: NodeId,
    /// Stable identity from a `#name` in the source, if present. Used to match
    /// nodes across reloads so per-widget state can be preserved.
    pub name: Option<String>,
    pub kind: Kind,
    pub attrs: Attrs,
    /// Inline text content (`Kind "text"` shorthand), if any.
    pub text: Option<String>,
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
}

/// A retained component tree.
#[derive(Debug, Clone)]
pub struct Tree {
    nodes: Vec<Node>,
    root: NodeId,
    by_name: BTreeMap<String, NodeId>,
}

impl Tree {
    /// The root node id.
    pub fn root(&self) -> NodeId {
        self.root
    }

    /// Total node count.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Whether the tree has no nodes. A parsed tree always has at least a root,
    /// so this is only true for the reserved empty tree.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Borrow a node by id.
    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id.0 as usize]
    }

    /// Look up a node by its stable `#name`.
    pub fn by_name(&self, name: &str) -> Option<NodeId> {
        self.by_name.get(name).copied()
    }

    /// Iterate over every node in arena (creation) order.
    pub fn iter(&self) -> impl Iterator<Item = &Node> {
        self.nodes.iter()
    }
}

/// A mutable builder used by the parser to assemble a [`Tree`] before it is
/// frozen. Keeps the arena append-only so [`NodeId`]s stay valid as it grows.
pub struct TreeBuilder {
    nodes: Vec<Node>,
    by_name: BTreeMap<String, NodeId>,
}

impl TreeBuilder {
    pub fn new() -> TreeBuilder {
        TreeBuilder { nodes: Vec::new(), by_name: BTreeMap::new() }
    }

    /// Append a node and return its id. `name` duplicates keep the first binding
    /// (later duplicates are still reachable by id, just not by name).
    pub fn push(
        &mut self,
        name: Option<String>,
        kind: Kind,
        attrs: Attrs,
        text: Option<String>,
        parent: Option<NodeId>,
    ) -> NodeId {
        let id = NodeId(self.nodes.len() as u32);
        if let Some(n) = &name {
            self.by_name.entry(n.clone()).or_insert(id);
        }
        self.nodes.push(Node { id, name, kind, attrs, text, parent, children: Vec::new() });
        id
    }

    /// Record `child` as a child of `parent`.
    pub fn add_child(&mut self, parent: NodeId, child: NodeId) {
        self.nodes[parent.0 as usize].children.push(child);
    }

    /// Freeze the builder into an immutable [`Tree`] rooted at `root`.
    pub fn finish(self, root: NodeId) -> Tree {
        Tree { nodes: self.nodes, root, by_name: self.by_name }
    }
}

impl Default for TreeBuilder {
    fn default() -> Self {
        TreeBuilder::new()
    }
}
