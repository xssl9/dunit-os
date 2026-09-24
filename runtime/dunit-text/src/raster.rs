//! Anti-aliased glyph rasterizer: turns a flattened [`Outline`] (font units)
//! into an 8-bit alpha coverage mask at a requested scale, using the signed-area
//! accumulation method (à la font-rs / stb_truetype). Pure `no_std`, so all the
//! float math (floor/ceil/abs) is hand-rolled — `core` offers none of it.

use alloc::vec;
use alloc::vec::Vec;

use crate::outline::{Outline, Point};

/// A rasterized glyph: an 8-bit coverage mask plus its placement relative to the
/// pen origin on the baseline. `left` is the x offset (pixels, right-positive) of
/// the mask's left column; `top` is the y offset (pixels, up-positive) of the
/// mask's top row above the baseline.
#[derive(Debug, Clone)]
pub struct GlyphBitmap {
    pub width: usize,
    pub height: usize,
    pub left: i32,
    pub top: i32,
    /// Row-major coverage, `width * height` bytes, 0 = transparent, 255 = solid.
    pub coverage: Vec<u8>,
}

impl GlyphBitmap {
    /// Coverage at `(x, y)` (0 outside the mask).
    pub fn at(&self, x: usize, y: usize) -> u8 {
        if x < self.width && y < self.height {
            self.coverage[y * self.width + x]
        } else {
            0
        }
    }
}

/// The scanline coverage accumulator.
pub struct Raster {
    w: usize,
    h: usize,
    /// Row stride: `w + 1`. The extra column absorbs the `+1` right-edge spill so
    /// coverage never bleeds into the next row's first cell.
    stride: usize,
    a: Vec<f32>,
}

impl Raster {
    /// Rasterize `outline` scaled by `scale` (pixels per font unit). Returns
    /// `None` when the outline has no area (empty / degenerate bbox).
    pub fn render(outline: &Outline, scale: f32) -> Option<GlyphBitmap> {
        if outline.is_empty() || scale <= 0.0 {
            return None;
        }
        // Scaled bounding box, padded by one pixel so edge coverage and the
        // accumulator's +1 spill column always stay in bounds.
        let x0 = outline.x_min * scale;
        let x1 = outline.x_max * scale;
        let y0 = outline.y_min * scale;
        let y1 = outline.y_max * scale;
        let left = floorf(x0) as i32 - 1;
        let right = ceilf(x1) as i32 + 1;
        let bottom = floorf(y0) as i32 - 1;
        let top = ceilf(y1) as i32 + 1;
        let w = (right - left) as usize;
        let h = (top - bottom) as usize;
        if w == 0 || h == 0 {
            return None;
        }

        let stride = w + 1;
        let mut r = Raster { w, h, stride, a: vec![0.0; stride * h + 1] };

        // Emit every contour edge in bitmap space (y flipped: row 0 = top).
        let lf = left as f32;
        let tf = top as f32;
        for contour in &outline.contours {
            if contour.len() < 2 {
                continue;
            }
            let map = |p: &Point| Point { x: p.x * scale - lf, y: tf - p.y * scale };
            let mut prev = map(&contour[0]);
            for p in &contour[1..] {
                let cur = map(p);
                r.line(prev, cur);
                prev = cur;
            }
            // Close the contour back to its start.
            let start = map(&contour[0]);
            r.line(prev, start);
        }

        Some(GlyphBitmap { width: w, height: h, left, top, coverage: r.accumulate() })
    }

    /// Accumulate the signed-area contribution of one edge into `self.a`.
    fn line(&mut self, p0: Point, p1: Point) {
        // Orient the edge downward; `dir` carries the winding sign.
        let (dir, p0, p1) = if p0.y < p1.y { (1.0, p0, p1) } else { (-1.0, p1, p0) };
        if p0.y == p1.y {
            return; // horizontal edges contribute no vertical coverage
        }
        let dxdy = (p1.x - p0.x) / (p1.y - p0.y);
        let mut x = p0.x;
        if p0.y < 0.0 {
            x -= p0.y * dxdy; // advance to the y = 0 scanline
        }
        let y_start = fmaxf(p0.y, 0.0) as usize;
        let y_end = (ceilf(p1.y) as usize).min(self.h);
        for y in y_start..y_end {
            let linestart = y * self.stride;
            let dy = fminf((y + 1) as f32, p1.y) - fmaxf(y as f32, p0.y);
            let xnext = x + dxdy * dy;
            let d = dy * dir;
            let (xa, xb) = if x < xnext { (x, xnext) } else { (xnext, x) };
            let xa_floor = floorf(xa);
            let xai = xa_floor as i32;
            let xb_ceil = ceilf(xb);
            let xbi = xb_ceil as i32;
            if xbi <= xai + 1 {
                // Edge stays within a single pixel column on this scanline.
                let xmf = 0.5 * (x + xnext) - xa_floor;
                let idx = linestart as i32 + xai;
                self.add(idx, d - d * xmf);
                self.add(idx + 1, d * xmf);
            } else {
                // Edge spans multiple columns: split the area across them.
                let s = 1.0 / (xb - xa);
                let x0f = xa - xa_floor;
                let a_m = 1.0 - x0f;
                let x1f = xb - xb_ceil + 1.0;
                let am = 0.5 * s * a_m * a_m;
                let bm = s * (1.0 - 0.5 * x1f) * x1f;
                let base = linestart as i32 + xai;
                self.add(base, d * am);
                if xbi == xai + 2 {
                    self.add(base + 1, d * (1.0 - am - bm));
                } else {
                    let a0 = s * (1.5 - x0f);
                    self.add(base + 1, d * (a0 - am));
                    let mut xi = xai + 2;
                    while xi < xbi - 1 {
                        self.add(linestart as i32 + xi, d * s);
                        xi += 1;
                    }
                    let a1 = a0 + (xbi - xai - 3) as f32 * s;
                    self.add(linestart as i32 + (xbi - 1), d * (1.0 - a1 - bm));
                }
                self.add(linestart as i32 + xbi, d * bm);
            }
            x = xnext;
        }
    }

    #[inline]
    fn add(&mut self, idx: i32, v: f32) {
        if idx >= 0 {
            if let Some(cell) = self.a.get_mut(idx as usize) {
                *cell += v;
            }
        }
    }

    /// Integrate the accumulator per row into final 0..=255 coverage.
    fn accumulate(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.w * self.h);
        for y in 0..self.h {
            let mut acc = 0.0f32;
            let row = y * self.stride;
            for x in 0..self.w {
                acc += self.a[row + x];
                let mut a = absf(acc);
                if a > 1.0 {
                    a = 1.0;
                }
                out.push((a * 255.0 + 0.5) as u8);
            }
        }
        out
    }
}

// ---- hand-rolled float helpers (no libm in `no_std`) --------------------

#[inline]
fn floorf(x: f32) -> f32 {
    let t = x as i32 as f32;
    if t > x { t - 1.0 } else { t }
}

#[inline]
fn ceilf(x: f32) -> f32 {
    let t = x as i32 as f32;
    if t < x { t + 1.0 } else { t }
}

#[inline]
fn absf(x: f32) -> f32 {
    if x < 0.0 { -x } else { x }
}

#[inline]
fn fminf(a: f32, b: f32) -> f32 {
    if a < b { a } else { b }
}

#[inline]
fn fmaxf(a: f32, b: f32) -> f32 {
    if a > b { a } else { b }
}
