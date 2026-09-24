//! Dunit Style — the DSS layer.
//!
//! This is M4 slice 3. It gives the GUI stack a bounded, CSS-like way to
//! describe appearance and motion without a full CSS engine:
//!
//! * [`parse`] — a small stylesheet parser (DSS). Rules are
//!   `selector { property: value; ... }`; selectors are a single compound of an
//!   optional element tag, `#id`, `.class` and `:state`; `$name: value;`
//!   declares a variable that `$name` references expand.
//! * [`value`] — the value model: [`value::Color`], numbers, durations,
//!   keywords and short value lists, with hex-colour and unit parsing.
//! * [`cascade`] — the ordered defaults -> theme -> class -> state cascade.
//!   [`cascade::Cascade`] resolves the [`cascade::Style`] for a node described
//!   by its element tag, id, classes and active states, honouring selector
//!   specificity and source order.
//! * [`motion`] — declarative transitions ([`motion::Transition`],
//!   [`motion::Easing`]) and value interpolation for the animation clock.
//! * [`theme`] — the monochrome Green Tea reference theme as a DSS document.
//!
//! Everything is pure `no_std + alloc` with zero dependencies, so the same code
//! runs in the host test suite and inside the userspace GUI stack on
//! `x86_64-unknown-none`.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod cascade;
pub mod motion;
pub mod parse;
pub mod theme;
pub mod value;

pub use cascade::{Cascade, Specificity, Style};
pub use motion::{Easing, Transition};
pub use parse::{parse, Declaration, Rule, Selector, Stylesheet};
pub use theme::GREEN_TEA_DSS;
pub use value::{Color, Value};

/// Errors produced while parsing a DSS document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StyleError {
    /// The parser hit a token it did not expect. Carries a 1-based line number.
    Unexpected { line: u32, what: alloc::string::String },
    /// A block, string or declaration was left unterminated at end of input.
    UnexpectedEof(alloc::string::String),
}

/// Convenience result alias for the crate.
pub type Result<T> = core::result::Result<T, StyleError>;
