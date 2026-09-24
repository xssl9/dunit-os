//! The [`Font`] handle: owns the font bytes, exposes character→glyph mapping,
//! scaled metrics, single-glyph rasterization and simple left-to-right layout.

use alloc::vec::Vec;

use crate::outline::Outline;
use crate::parse::{Reader, Sfnt};
use crate::raster::GlyphBitmap;
use crate::{Result, TextError};

/// A resolved glyph index within a font (0 is the `.notdef` glyph).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct GlyphId(pub u16);

/// Un-scaled (font-unit) horizontal metrics for one glyph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GlyphMetrics {
    /// Horizontal advance width in font units.
    pub advance: u16,
    /// Left side bearing in font units.
    pub left_side_bearing: i16,
}

/// Scaled vertical line metrics in pixels for a chosen pixel size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LineMetrics {
    /// Distance from baseline up to the top of typical glyphs (positive).
    pub ascent: f32,
    /// Distance from baseline down to the bottom (negative or zero).
    pub descent: f32,
    /// Recommended extra spacing between lines.
    pub line_gap: f32,
}

impl LineMetrics {
    /// Baseline-to-baseline advance for stacked lines.
    pub fn line_height(&self) -> f32 {
        self.ascent - self.descent + self.line_gap
    }
}

/// One positioned glyph produced by [`Font::layout_line`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LaidGlyph {
    pub glyph: GlyphId,
    /// Pen x position (pixels) of the glyph origin, relative to line start.
    pub x: f32,
    /// Source character (for callers that want to re-map or debug).
    pub ch: char,
}

/// A decoded character-map subtable, reduced to the lookups we need.
enum Cmap {
    /// Segment-mapped format 4 (BMP). Parallel arrays per segment.
    Format4 {
        end_code: Vec<u16>,
        start_code: Vec<u16>,
        id_delta: Vec<i16>,
        id_range_offset: Vec<u16>,
        /// The glyphIdArray that id_range_offset indexes into.
        glyph_id_array: Vec<u16>,
    },
    /// Segmented coverage format 12 (full Unicode). Sorted groups.
    Format12 {
        // (start_char, end_char, start_glyph)
        groups: Vec<(u32, u32, u32)>,
    },
}

/// A parsed TrueType font that owns its backing bytes.
pub struct Font {
    data: Vec<u8>,
    units_per_em: u16,
    num_glyphs: u16,
    long_loca: bool,
    num_h_metrics: u16,
    ascent: i16,
    descent: i16,
    line_gap: i16,
    // Byte ranges (start, end) within `data` for the tables read on demand.
    glyf: (usize, usize),
    loca: (usize, usize),
    hmtx: (usize, usize),
    cmap: Cmap,
}

impl Font {
    /// Parse a TrueType (`glyf`-outline) font from owned bytes.
    pub fn parse(data: Vec<u8>) -> Result<Self> {
        let sfnt = Sfnt::parse(&data)?;

        let head = sfnt.require(b"head")?;
        let mut hr = Reader::new(head);
        hr.skip(18)?; // version..fontRevision..checkSumAdjustment..magic..flags
        let units_per_em = hr.u16()?;
        // skip created(8)+modified(8)+xMin/yMin/xMax/yMax(8)+macStyle(2)
        // +lowestRecPPEM(2)+fontDirectionHint(2) = 30 bytes to indexToLocFormat
        hr.skip(30)?;
        let index_to_loc_format = hr.i16()?;
        let long_loca = index_to_loc_format == 1;

        let maxp = sfnt.require(b"maxp")?;
        let mut mr = Reader::new(maxp);
        mr.skip(4)?; // version (fixed)
        let num_glyphs = mr.u16()?;

        let hhea = sfnt.require(b"hhea")?;
        let mut ar = Reader::new(hhea);
        ar.skip(4)?; // version
        let ascent = ar.i16()?;
        let descent = ar.i16()?;
        let line_gap = ar.i16()?;
        ar.skip(24)?; // advanceWidthMax..reserved..metricDataFormat
        let num_h_metrics = ar.u16()?;

        let glyf = sfnt.require_range(b"glyf")?;
        let loca = sfnt.require_range(b"loca")?;
        let hmtx = sfnt.require_range(b"hmtx")?;
        let cmap = parse_cmap(sfnt.require(b"cmap")?)?;

        Ok(Font {
            data,
            units_per_em,
            num_glyphs,
            long_loca,
            num_h_metrics,
            ascent,
            descent,
            line_gap,
            glyf,
            loca,
            hmtx,
            cmap,
        })
    }

    /// Font design units per em (the coordinate space of glyph outlines).
    pub fn units_per_em(&self) -> u16 {
        self.units_per_em
    }

    /// Number of glyphs in the font.
    pub fn num_glyphs(&self) -> u16 {
        self.num_glyphs
    }

    /// Scale factor mapping font units to pixels for a given em pixel size.
    pub fn scale_for_px(&self, px: f32) -> f32 {
        if self.units_per_em == 0 {
            0.0
        } else {
            px / self.units_per_em as f32
        }
    }

    /// Scaled vertical line metrics for the given pixel size.
    pub fn line_metrics(&self, px: f32) -> LineMetrics {
        let s = self.scale_for_px(px);
        LineMetrics {
            ascent: self.ascent as f32 * s,
            descent: self.descent as f32 * s,
            line_gap: self.line_gap as f32 * s,
        }
    }

    /// Map a Unicode scalar to a glyph index (0 = `.notdef` when unmapped).
    pub fn glyph_index(&self, ch: char) -> GlyphId {
        GlyphId(lookup_cmap(&self.cmap, ch as u32))
    }

    /// Un-scaled horizontal metrics for a glyph. Glyph indices at or beyond the
    /// `numberOfHMetrics` count share the last advance (the TrueType rule).
    pub fn glyph_metrics(&self, glyph: GlyphId) -> GlyphMetrics {
        let hmtx = &self.data[self.hmtx.0..self.hmtx.1];
        let n = self.num_h_metrics as usize;
        if n == 0 {
            return GlyphMetrics { advance: 0, left_side_bearing: 0 };
        }
        let gid = glyph.0 as usize;
        if gid < n {
            let o = gid * 4;
            let advance = be_u16(hmtx, o);
            let lsb = be_u16(hmtx, o + 2) as i16;
            GlyphMetrics { advance, left_side_bearing: lsb }
        } else {
            let advance = be_u16(hmtx, (n - 1) * 4);
            // Monospaced tail lsb array follows the hMetrics; default to 0.
            GlyphMetrics { advance, left_side_bearing: 0 }
        }
    }

    /// The raw `glyf` bytes for a glyph, or `None` when the glyph is empty
    /// (e.g. the space character has a zero-length `loca` entry).
    pub(crate) fn glyph_slice(&self, gid: u16) -> Option<&[u8]> {
        let loca = &self.data[self.loca.0..self.loca.1];
        let (start, end) = if self.long_loca {
            let o = gid as usize * 4;
            (be_u32(loca, o)? as usize, be_u32(loca, o + 4)? as usize)
        } else {
            // Short loca stores half-offsets.
            let o = gid as usize * 2;
            (be_u16_opt(loca, o)? as usize * 2, be_u16_opt(loca, o + 2)? as usize * 2)
        };
        if end <= start {
            return None; // empty glyph (no contours)
        }
        let glyf = &self.data[self.glyf.0..self.glyf.1];
        glyf.get(start..end)
    }

    /// Decode a glyph's outline into flattened quadratic contours (font units).
    pub fn outline(&self, glyph: GlyphId) -> Outline {
        crate::outline::outline(self, glyph)
    }

    /// Rasterize a glyph to an anti-aliased 8-bit alpha mask at `px` em size.
    /// Returns `None` for glyphs with no visible ink (spaces, control chars).
    pub fn rasterize(&self, glyph: GlyphId, px: f32) -> Option<GlyphBitmap> {
        let outline = self.outline(glyph);
        if outline.is_empty() {
            return None;
        }
        crate::raster::Raster::render(&outline, self.scale_for_px(px))
    }

    /// Lay out a single left-to-right line, returning positioned glyphs and the
    /// total advance width in pixels. No shaping/kerning in v1 — pure advances.
    pub fn layout_line(&self, text: &str, px: f32) -> (Vec<LaidGlyph>, f32) {
        let s = self.scale_for_px(px);
        let mut glyphs = Vec::new();
        let mut pen = 0.0f32;
        for ch in text.chars() {
            let g = self.glyph_index(ch);
            glyphs.push(LaidGlyph { glyph: g, x: pen, ch });
            pen += self.glyph_metrics(g).advance as f32 * s;
        }
        (glyphs, pen)
    }
}

// ---- byte helpers -------------------------------------------------------

fn be_u16(data: &[u8], o: usize) -> u16 {
    be_u16_opt(data, o).unwrap_or(0)
}

fn be_u16_opt(data: &[u8], o: usize) -> Option<u16> {
    let hi = *data.get(o)? as u16;
    let lo = *data.get(o + 1)? as u16;
    Some((hi << 8) | lo)
}

fn be_u32(data: &[u8], o: usize) -> Option<u32> {
    Some(((be_u16_opt(data, o)? as u32) << 16) | be_u16_opt(data, o + 2)? as u32)
}

// ---- cmap ---------------------------------------------------------------

fn lookup_cmap(cmap: &Cmap, ch: u32) -> u16 {
    match cmap {
        Cmap::Format4 { end_code, start_code, id_delta, id_range_offset, glyph_id_array } => {
            if ch > 0xFFFF {
                return 0;
            }
            let c = ch as u16;
            let seg_count = end_code.len();
            for i in 0..seg_count {
                if c <= end_code[i] {
                    if c < start_code[i] {
                        return 0;
                    }
                    let ro = id_range_offset[i];
                    if ro == 0 {
                        return (c as i32 + id_delta[i] as i32) as u16;
                    }
                    // Index into glyph_id_array per the TrueType formula.
                    let idx = ro as usize / 2 + (c - start_code[i]) as usize;
                    let idx = idx.wrapping_sub(seg_count - i);
                    let g = glyph_id_array.get(idx).copied().unwrap_or(0);
                    if g == 0 {
                        return 0;
                    }
                    return (g as i32 + id_delta[i] as i32) as u16;
                }
            }
            0
        }
        Cmap::Format12 { groups } => {
            for &(start, end, start_glyph) in groups {
                if ch >= start && ch <= end {
                    return (start_glyph + (ch - start)) as u16;
                }
            }
            0
        }
    }
}

fn parse_cmap(cmap: &[u8]) -> Result<Cmap> {
    let num_tables = be_u16_opt(cmap, 2).ok_or(TextError::UnexpectedEof)?;
    let mut best4: Option<usize> = None;
    let mut best12: Option<usize> = None;
    for i in 0..num_tables as usize {
        let rec = 4 + i * 8;
        let offset = be_u32(cmap, rec + 4).ok_or(TextError::UnexpectedEof)? as usize;
        let format = be_u16_opt(cmap, offset).unwrap_or(0);
        match format {
            4 if best4.is_none() => best4 = Some(offset),
            12 if best12.is_none() => best12 = Some(offset),
            _ => {}
        }
    }
    if let Some(off) = best12 {
        return parse_cmap12(cmap, off);
    }
    if let Some(off) = best4 {
        return parse_cmap4(cmap, off);
    }
    Err(TextError::UnsupportedFormat("cmap format"))
}

fn parse_cmap4(cmap: &[u8], off: usize) -> Result<Cmap> {
    let seg_x2 = be_u16_opt(cmap, off + 6).ok_or(TextError::UnexpectedEof)? as usize;
    let seg_count = seg_x2 / 2;
    let end_base = off + 14;
    let start_base = end_base + seg_x2 + 2; // + reservedPad
    let delta_base = start_base + seg_x2;
    let range_base = delta_base + seg_x2;
    let gia_base = range_base + seg_x2;
    let mut end_code = Vec::with_capacity(seg_count);
    let mut start_code = Vec::with_capacity(seg_count);
    let mut id_delta = Vec::with_capacity(seg_count);
    let mut id_range_offset = Vec::with_capacity(seg_count);
    for i in 0..seg_count {
        end_code.push(be_u16_opt(cmap, end_base + i * 2).ok_or(TextError::UnexpectedEof)?);
        start_code.push(be_u16_opt(cmap, start_base + i * 2).ok_or(TextError::UnexpectedEof)?);
        id_delta.push(be_u16_opt(cmap, delta_base + i * 2).ok_or(TextError::UnexpectedEof)? as i16);
        id_range_offset.push(be_u16_opt(cmap, range_base + i * 2).ok_or(TextError::UnexpectedEof)?);
    }
    // The glyphIdArray is the remainder of the subtable after idRangeOffset.
    let mut glyph_id_array = Vec::new();
    let mut o = gia_base;
    while let Some(v) = be_u16_opt(cmap, o) {
        glyph_id_array.push(v);
        o += 2;
    }
    Ok(Cmap::Format4 { end_code, start_code, id_delta, id_range_offset, glyph_id_array })
}

fn parse_cmap12(cmap: &[u8], off: usize) -> Result<Cmap> {
    let n = be_u32(cmap, off + 12).ok_or(TextError::UnexpectedEof)? as usize;
    let mut groups = Vec::with_capacity(n);
    for i in 0..n {
        let g = off + 16 + i * 12;
        let start = be_u32(cmap, g).ok_or(TextError::UnexpectedEof)?;
        let end = be_u32(cmap, g + 4).ok_or(TextError::UnexpectedEof)?;
        let start_glyph = be_u32(cmap, g + 8).ok_or(TextError::UnexpectedEof)?;
        groups.push((start, end, start_glyph));
    }
    Ok(Cmap::Format12 { groups })
}


