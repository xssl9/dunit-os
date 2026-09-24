//! The layout engine: a constraint/flex pass over a [`Tree`] that assigns a
//! pixel [`Rect`] to every node. Two passes — a bottom-up *measure* that resolves
//! each node's desired size, then a top-down *arrange* that distributes space and
//! places children per container kind.
//!
//! Containers: `Row`/`Column` are flex lines (main-axis distribution with `grow`,
//! cross-axis alignment), `Stack` overlaps children, `Grid` is a fixed-column
//! table, and `Scroll` stacks content vertically at natural height (content may
//! exceed the viewport). Non-container elements are sized leaves.

use alloc::vec;
use alloc::vec::Vec;

use crate::attrs::{Align, Attrs, Sizing};
use crate::tree::{Kind, NodeId, Tree};

/// An axis-aligned rectangle in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// The computed layout: one rect per node, indexed by [`NodeId`].
#[derive(Debug, Clone)]
pub struct Layout {
    boxes: Vec<Rect>,
}

impl Layout {
    /// The rect assigned to `id`.
    pub fn rect(&self, id: NodeId) -> Rect {
        self.boxes[id.0 as usize]
    }
}

/// Lay out `tree` into a `avail_w * avail_h` viewport. The root fills the
/// viewport; every descendant is placed relative to it.
pub fn layout(tree: &Tree, avail_w: f32, avail_h: f32) -> Layout {
    let n = tree.len();
    let mut desired = vec![(0.0f32, 0.0f32); n];
    if n == 0 {
        return Layout { boxes: Vec::new() };
    }
    measure(tree, tree.root(), avail_w, avail_h, &mut desired);

    let mut boxes = vec![Rect::default(); n];
    let root_rect = Rect { x: 0.0, y: 0.0, w: avail_w, h: avail_h };
    arrange(tree, tree.root(), root_rect, &desired, &mut boxes);
    Layout { boxes }
}

// ---- measure ------------------------------------------------------------

fn measure(tree: &Tree, id: NodeId, avail_w: f32, avail_h: f32, desired: &mut Vec<(f32, f32)>) -> (f32, f32) {
    let node = tree.node(id);
    let a = &node.attrs;
    let inner_w = fmax(avail_w - a.padding.horizontal(), 0.0);
    let inner_h = fmax(avail_h - a.padding.vertical(), 0.0);

    let (content_w, content_h) = match &node.kind {
        Kind::Row => measure_line(tree, node.children.as_slice(), a, inner_w, inner_h, true, desired),
        Kind::Column => measure_line(tree, node.children.as_slice(), a, inner_w, inner_h, false, desired),
        Kind::Stack | Kind::Scroll => {
            let mut w = 0.0f32;
            let mut h = 0.0f32;
            for &c in &node.children {
                let (cw, ch) = measure(tree, c, inner_w, inner_h, desired);
                w = fmax(w, cw);
                h = fmax(h, ch);
            }
            (w, h)
        }
        Kind::Grid => measure_grid(tree, node.children.as_slice(), a, inner_w, inner_h, desired),
        Kind::Element(_) => (0.0, 0.0),
    };

    // Fold padding back in, then let explicit sizing override the content size.
    let outer_w = content_w + a.padding.horizontal();
    let outer_h = content_h + a.padding.vertical();
    let w = clamp_opt(resolve(a.width, avail_w, outer_w), a.min_w, a.max_w);
    let h = clamp_opt(resolve(a.height, avail_h, outer_h), a.min_h, a.max_h);
    desired[id.0 as usize] = (w, h);
    (w, h)
}

/// Measure a Row (`horizontal`) or Column line: main = sum + gaps, cross = max.
fn measure_line(
    tree: &Tree,
    children: &[NodeId],
    a: &Attrs,
    inner_w: f32,
    inner_h: f32,
    horizontal: bool,
    desired: &mut Vec<(f32, f32)>,
) -> (f32, f32) {
    let mut main = 0.0f32;
    let mut cross = 0.0f32;
    for &c in children {
        let (cw, ch) = measure(tree, c, inner_w, inner_h, desired);
        let (cmain, ccross) = if horizontal { (cw, ch) } else { (ch, cw) };
        main += cmain;
        cross = fmax(cross, ccross);
    }
    if children.len() > 1 {
        main += a.spacing * (children.len() as f32 - 1.0);
    }
    if horizontal {
        (main, cross)
    } else {
        (cross, main)
    }
}

fn measure_grid(
    tree: &Tree,
    children: &[NodeId],
    a: &Attrs,
    inner_w: f32,
    inner_h: f32,
    desired: &mut Vec<(f32, f32)>,
) -> (f32, f32) {
    let cols = a.columns.max(1) as usize;
    let cell_avail_w = (inner_w - a.spacing * (cols as f32 - 1.0)) / cols as f32;
    let mut col_w = 0.0f32;
    let rows = children.len().div_ceil(cols);
    let mut row_h = vec![0.0f32; rows];
    for (i, &c) in children.iter().enumerate() {
        let (cw, ch) = measure(tree, c, fmax(cell_avail_w, 0.0), inner_h, desired);
        col_w = fmax(col_w, cw);
        let r = i / cols;
        row_h[r] = fmax(row_h[r], ch);
    }
    let total_w = col_w * cols as f32 + a.spacing * (cols as f32 - 1.0);
    let mut total_h = 0.0f32;
    for (r, &h) in row_h.iter().enumerate() {
        total_h += h;
        if r > 0 {
            total_h += a.spacing;
        }
    }
    (total_w, total_h)
}

// ---- arrange ------------------------------------------------------------

fn arrange(tree: &Tree, id: NodeId, rect: Rect, desired: &[(f32, f32)], boxes: &mut Vec<Rect>) {
    boxes[id.0 as usize] = rect;
    let node = tree.node(id);
    let a = &node.attrs;
    let inner = Rect {
        x: rect.x + a.padding.left,
        y: rect.y + a.padding.top,
        w: fmax(rect.w - a.padding.horizontal(), 0.0),
        h: fmax(rect.h - a.padding.vertical(), 0.0),
    };
    match &node.kind {
        Kind::Row => arrange_line(tree, &node.children, a, inner, true, desired, boxes),
        Kind::Column => arrange_line(tree, &node.children, a, inner, false, desired, boxes),
        Kind::Stack => {
            for &c in &node.children {
                let ca = &tree.node(c).attrs;
                let w = resolve_fill(ca.width, inner.w);
                let h = resolve_fill(ca.height, inner.h);
                let child = Rect {
                    x: inner.x + cross_offset(a.main_align, inner.w, w),
                    y: inner.y + cross_offset(a.cross_align, inner.h, h),
                    w,
                    h,
                };
                arrange(tree, c, child, desired, boxes);
            }
        }
        Kind::Scroll => {
            // Stack content vertically at natural height; content may exceed the
            // viewport (scroll offset is modelled in a later slice).
            let mut y = inner.y;
            for &c in &node.children {
                let ca = &tree.node(c).attrs;
                let dh = desired[c.0 as usize].1;
                let w = resolve_fill(ca.width, inner.w);
                let child = Rect { x: inner.x, y, w, h: dh };
                arrange(tree, c, child, desired, boxes);
                y += dh + a.spacing;
            }
        }
        Kind::Grid => arrange_grid(tree, &node.children, a, inner, desired, boxes),
        Kind::Element(_) => {}
    }
}

/// Arrange a Row (`horizontal`) or Column flex line.
fn arrange_line(
    tree: &Tree,
    children: &[NodeId],
    a: &Attrs,
    inner: Rect,
    horizontal: bool,
    desired: &[(f32, f32)],
    boxes: &mut Vec<Rect>,
) {
    let n = children.len();
    if n == 0 {
        return;
    }
    let main_avail = if horizontal { inner.w } else { inner.h };
    let cross_avail = if horizontal { inner.h } else { inner.w };

    // Base main size and grow weight per child.
    let mut base = Vec::with_capacity(n);
    let mut grow = Vec::with_capacity(n);
    let mut total_base = 0.0f32;
    let mut total_grow = 0.0f32;
    for &c in children {
        let (dw, dh) = desired[c.0 as usize];
        let bmain = if horizontal { dw } else { dh };
        base.push(bmain);
        total_base += bmain;
        let g = tree.node(c).attrs.grow;
        grow.push(g);
        total_grow += g;
    }
    let gaps = if n > 1 { a.spacing * (n as f32 - 1.0) } else { 0.0 };
    let free = main_avail - total_base - gaps;

    // Grow children absorb positive free space; otherwise it feeds main-align.
    let mut mains = base;
    let mut leftover = fmax(free, 0.0);
    if total_grow > 0.0 && free > 0.0 {
        for i in 0..n {
            mains[i] += free * grow[i] / total_grow;
        }
        leftover = 0.0;
    }

    let (mut cursor, gap_extra) = distribute(a.main_align, leftover, n);
    let start = if horizontal { inner.x } else { inner.y };
    cursor += start;
    let step_gap = a.spacing + gap_extra;

    for (i, &c) in children.iter().enumerate() {
        let ca = &tree.node(c).attrs;
        let (dw, dh) = desired[c.0 as usize];
        let cross_desired = if horizontal { dh } else { dw };
        let cross_sizing = if horizontal { ca.height } else { ca.width };
        let cross = resolve_cross(cross_sizing, a.cross_align, cross_avail, cross_desired);

        // Clamp the arranged size to the child's own min/max on each axis so
        // stretching or growing can never violate a constraint.
        let (main_min, main_max, cross_min, cross_max) = if horizontal {
            (ca.min_w, ca.max_w, ca.min_h, ca.max_h)
        } else {
            (ca.min_h, ca.max_h, ca.min_w, ca.max_w)
        };
        let main = clamp_opt(mains[i], main_min, main_max);
        let cross = clamp_opt(cross, cross_min, cross_max);
        let cross_pos = cross_offset(a.cross_align, cross_avail, cross);

        let rect = if horizontal {
            Rect { x: cursor, y: inner.y + cross_pos, w: main, h: cross }
        } else {
            Rect { x: inner.x + cross_pos, y: cursor, w: cross, h: main }
        };
        arrange(tree, c, rect, desired, boxes);
        cursor += mains[i] + step_gap;
    }
}

fn arrange_grid(
    tree: &Tree,
    children: &[NodeId],
    a: &Attrs,
    inner: Rect,
    desired: &[(f32, f32)],
    boxes: &mut Vec<Rect>,
) {
    let n = children.len();
    if n == 0 {
        return;
    }
    let cols = a.columns.max(1) as usize;
    let rows = n.div_ceil(cols);
    let col_w = (inner.w - a.spacing * (cols as f32 - 1.0)) / cols as f32;
    let mut row_h = vec![0.0f32; rows];
    for (i, &c) in children.iter().enumerate() {
        row_h[i / cols] = fmax(row_h[i / cols], desired[c.0 as usize].1);
    }

    let mut y = inner.y;
    for r in 0..rows {
        let mut x = inner.x;
        for cc in 0..cols {
            let idx = r * cols + cc;
            if idx >= n {
                break;
            }
            let rect = Rect { x, y, w: fmax(col_w, 0.0), h: row_h[r] };
            arrange(tree, children[idx], rect, desired, boxes);
            x += col_w + a.spacing;
        }
        y += row_h[r] + a.spacing;
    }
}

// ---- sizing helpers -----------------------------------------------------

/// Resolve a sizing against available space, defaulting to `content` (Auto/Grow
/// fall back to content when measuring).
fn resolve(s: Sizing, avail: f32, content: f32) -> f32 {
    match s {
        Sizing::Auto | Sizing::Grow(_) => content,
        Sizing::Px(v) => v,
        Sizing::Pct(p) => p * avail,
    }
}

/// Resolve a sizing where Auto/Grow fill the available space (Stack children).
fn resolve_fill(s: Sizing, avail: f32) -> f32 {
    match s {
        Sizing::Auto | Sizing::Grow(_) => avail,
        Sizing::Px(v) => v,
        Sizing::Pct(p) => p * avail,
    }
}

/// Cross-axis size for a flex child: stretch fills the line unless the child has
/// an explicit size.
fn resolve_cross(s: Sizing, cross_align: Align, avail: f32, desired: f32) -> f32 {
    match s {
        Sizing::Px(v) => v,
        Sizing::Pct(p) => p * avail,
        Sizing::Auto | Sizing::Grow(_) => {
            if cross_align == Align::Stretch {
                avail
            } else {
                desired
            }
        }
    }
}

/// Offset of a `size`-wide item within `avail` for a simple alignment.
fn cross_offset(align: Align, avail: f32, size: f32) -> f32 {
    match align {
        Align::Center | Align::SpaceAround | Align::SpaceBetween => fmax(avail - size, 0.0) * 0.5,
        Align::End => fmax(avail - size, 0.0),
        Align::Start | Align::Stretch => 0.0,
    }
}

/// Starting offset and inter-item extra gap for distributing `leftover` main
/// space among `n` items under a main-axis alignment.
fn distribute(align: Align, leftover: f32, n: usize) -> (f32, f32) {
    if leftover <= 0.0 || n == 0 {
        return (0.0, 0.0);
    }
    match align {
        Align::Start | Align::Stretch => (0.0, 0.0),
        Align::Center => (leftover * 0.5, 0.0),
        Align::End => (leftover, 0.0),
        Align::SpaceBetween => {
            if n > 1 {
                (0.0, leftover / (n as f32 - 1.0))
            } else {
                (leftover * 0.5, 0.0)
            }
        }
        Align::SpaceAround => {
            let gap = leftover / n as f32;
            (gap * 0.5, gap)
        }
    }
}

fn clamp_opt(v: f32, min: Option<f32>, max: Option<f32>) -> f32 {
    let mut r = v;
    if let Some(mx) = max {
        r = fmin(r, mx);
    }
    if let Some(mn) = min {
        r = fmax(r, mn);
    }
    r
}

#[inline]
fn fmax(a: f32, b: f32) -> f32 {
    if a > b {
        a
    } else {
        b
    }
}

#[inline]
fn fmin(a: f32, b: f32) -> f32 {
    if a < b {
        a
    } else {
        b
    }
}

