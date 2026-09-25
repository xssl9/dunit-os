//! The event and focus model over a laid-out DUI tree.
//!
//! This layer is mechanism, not policy: it turns pointer coordinates into a
//! target node ([`hit_test`]), exposes the capture/bubble [`event_path`] an app
//! dispatches an [`Event`] along, and orders focusable widgets for keyboard
//! navigation ([`focus_order`], [`focus_next`], [`focus_prev`]). Deciding what a
//! click or key *does* is left to the app/widget behaviour above it.

use alloc::vec::Vec;

use dunit_ui::layout::Layout;
use dunit_ui::tree::{Kind, NodeId, Tree};
use dunit_ui::Rect;

use crate::kind::Widget;

/// A keyboard key relevant to widget interaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Enter,
    Space,
    Tab,
    /// Shift+Tab (reverse focus).
    ShiftTab,
    Escape,
    Left,
    Right,
    Up,
    Down,
    Backspace,
    /// A printable character (for text inputs).
    Char(char),
}

/// An input event in viewport pixel coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Event {
    PointerDown { x: f32, y: f32 },
    PointerUp { x: f32, y: f32 },
    PointerMove { x: f32, y: f32 },
    Key(Key),
}

/// Whether `r` contains the point `(x, y)` (left/top inclusive, right/bottom
/// exclusive).
fn contains(r: Rect, x: f32, y: f32) -> bool {
    x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h
}

/// The deepest, top-most node whose rect contains `(x, y)`, or `None` if the
/// point is outside the root. Later siblings paint over earlier ones, so they
/// win ties.
pub fn hit_test(tree: &Tree, layout: &Layout, x: f32, y: f32) -> Option<NodeId> {
    if tree.is_empty() {
        return None;
    }
    let root = tree.root();
    if !contains(layout.rect(root), x, y) {
        return None;
    }
    Some(hit_descend(tree, layout, root, x, y))
}

fn hit_descend(tree: &Tree, layout: &Layout, id: NodeId, x: f32, y: f32) -> NodeId {
    for &c in tree.node(id).children.iter().rev() {
        if contains(layout.rect(c), x, y) {
            return hit_descend(tree, layout, c, x, y);
        }
    }
    id
}

/// The path from the root down to `target`, i.e. capture order. Reverse it for
/// the bubble phase.
pub fn event_path(tree: &Tree, target: NodeId) -> Vec<NodeId> {
    let mut path = Vec::new();
    let mut cur = Some(target);
    while let Some(id) = cur {
        path.push(id);
        cur = tree.node(id).parent;
    }
    path.reverse();
    path
}

/// The focusable widgets in the tree, in document (pre-order) order. Callers
/// filter out currently-disabled widgets themselves.
pub fn focus_order(tree: &Tree) -> Vec<NodeId> {
    let mut out = Vec::new();
    if !tree.is_empty() {
        collect_focusable(tree, tree.root(), &mut out);
    }
    out
}

fn collect_focusable(tree: &Tree, id: NodeId, out: &mut Vec<NodeId>) {
    if is_focusable(tree, id) {
        out.push(id);
    }
    for &c in &tree.node(id).children {
        collect_focusable(tree, c, out);
    }
}

fn is_focusable(tree: &Tree, id: NodeId) -> bool {
    match &tree.node(id).kind {
        Kind::Element(tag) => Widget::from_tag(tag).is_some_and(Widget::focusable),
        _ => false,
    }
}

/// The next focusable node after `current` in `order`, cycling to the first.
/// With no current focus, returns the first entry.
pub fn focus_next(order: &[NodeId], current: Option<NodeId>) -> Option<NodeId> {
    step(order, current, 1)
}

/// The previous focusable node before `current`, cycling to the last. With no
/// current focus, returns the last entry.
pub fn focus_prev(order: &[NodeId], current: Option<NodeId>) -> Option<NodeId> {
    step(order, current, -1)
}

fn step(order: &[NodeId], current: Option<NodeId>, dir: i32) -> Option<NodeId> {
    let n = order.len();
    if n == 0 {
        return None;
    }
    match current {
        None => Some(if dir > 0 { order[0] } else { order[n - 1] }),
        Some(cur) => match order.iter().position(|&id| id == cur) {
            Some(i) => {
                let next = (i as i32 + dir).rem_euclid(n as i32) as usize;
                Some(order[next])
            }
            None => Some(if dir > 0 { order[0] } else { order[n - 1] }),
        },
    }
}
