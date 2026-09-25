//! CPU painter for the DUI/DSS UI Runtime.
//!
//! [`Surface`] is an ARGB8888 pixel buffer (`0xAARRGGBB` per pixel, the format
//! the gui-v1 compositor blits) with clipped, alpha-blended [`Surface::fill_rect`]
//! and [`Surface::stroke_rect`] primitives and glyph-coverage blitting. [`paint`]
//! walks a laid-out [`dunit_ui`] tree and, using a caller-supplied per-node DSS
//! [`dunit_style::Style`] resolver and a [`dunit_text::Font`], draws each node's
//! background, border and text into the surface.
//!
//! The painter is pure mechanism: it does not resolve the cascade or lay out the
//! tree itself (callers pass the results in), which keeps it decoupled from the
//! widget/theme policy above it and trivially host-testable — paint into a
//! `Vec<u32>` and assert on pixels.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

mod paint;
mod surface;

pub use paint::{paint, StyleOf};
pub use surface::{pack_argb, Surface};
