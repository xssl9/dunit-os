//! Dunit UI Runtime — the DUI layer.
//!
//! This is M4 slice 2. It provides three things the rest of the GUI stack builds
//! on:
//!
//! * [`parse`] — a small declarative component-tree markup (DUI). Deep UI trees
//!   are awkward in TOML/JSON and HTML is far too large, so DUI is a compact
//!   brace-based syntax: `Kind #id attr=value "text" { children }`.
//! * [`tree`] — a retained component [`tree::Tree`] backed by an arena, with
//!   stable string IDs so widget state can survive an atomic tree swap on reload.
//! * [`attrs`] — the layout-relevant attributes decoded from raw markup
//!   (sizing, constraints, spacing, padding, alignment, grid columns).
//! * [`layout`] — a constraint/flex layout engine for the container kinds
//!   `Row`, `Column`, `Stack`, `Grid` and `Scroll`, producing a pixel rect per
//!   node.
//!
//! Styling (DSS), widgets and motion are separate later slices; the DUI layer
//! deliberately treats non-container elements as opaque, sized leaves.
//!
//! Everything is pure `no_std + alloc` with zero dependencies, so the same code
//! runs in the host test suite and inside the userspace GUI stack on
//! `x86_64-unknown-none`.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod attrs;
pub mod layout;
pub mod parse;
pub mod tree;

pub use attrs::{Align, Attrs, Edges, Sizing};
pub use layout::{layout, layout_measured, Layout, Measurer, Rect};
pub use parse::parse;
pub use tree::{Kind, Node, NodeId, Tree};

/// Errors produced while parsing a DUI document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiError {
    /// The parser hit a token it did not expect. Carries a 1-based line number.
    Unexpected { line: u32, what: alloc::string::String },
    /// A string, block or attribute value was left unterminated at end of input.
    UnexpectedEof(alloc::string::String),
    /// The document did not contain exactly one root node.
    NoRoot,
}

/// Convenience result alias for the crate.
pub type Result<T> = core::result::Result<T, UiError>;
