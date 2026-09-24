//! Dunit text stack — a self-contained TrueType parser and anti-aliased glyph
//! rasterizer for the Dunit UI Runtime (M4).
//!
//! This is the first M4 slice: text is ~80% of the target desktop, so a real
//! font rasterizer is the foundation every later layer (DUI layout, DSS themes,
//! widgets, the DWM shell) builds on. The crate is pure `no_std + alloc` with
//! zero external dependencies, so the same code runs in the host test suite and
//! inside the userspace GUI stack on `x86_64-unknown-none`.
//!
//! Scope of this slice:
//!
//! * [`parse`] — TrueType/OpenType table-directory parsing and the metric
//!   tables (`head`, `maxp`, `hhea`, `hmtx`, `cmap`, `loca`, `glyf`).
//! * [`outline`] — glyph outline extraction (simple and composite glyphs) into
//!   a flattened set of quadratic contours.
//! * [`raster`] — a signed-area coverage rasterizer producing an 8-bit
//!   anti-aliased alpha mask for a glyph at a requested pixel size.
//! * [`font`] — the [`font::Font`] handle tying it together: character → glyph
//!   mapping, scaled metrics, single-glyph rasterization and simple left-to-right
//!   text layout (positioned glyphs with horizontal advances).
//!
//! Only `glyf`-outline TrueType fonts are supported in v1 (the bundled DejaVu
//! family qualifies); CFF/`CFF2` PostScript outlines and hinting are out of
//! scope for this slice.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod parse;
pub mod outline;
pub mod raster;
pub mod font;

pub use font::{Font, GlyphId, GlyphMetrics, LaidGlyph, LineMetrics};
pub use raster::{GlyphBitmap, Raster};

/// Errors produced while parsing or rasterizing a font.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextError {
    /// The byte slice ended before a required field could be read.
    UnexpectedEof,
    /// A required table is missing from the font's table directory.
    MissingTable(&'static str),
    /// A table's version/format is not one this crate supports.
    UnsupportedFormat(&'static str),
    /// A referenced offset or index is outside the file/table bounds.
    OutOfBounds,
    /// The font uses PostScript (CFF) outlines, which v1 does not decode.
    CffUnsupported,
}

/// Convenience result alias for the crate.
pub type Result<T> = core::result::Result<T, TextError>;
