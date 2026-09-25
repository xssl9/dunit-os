//! The ARGB8888 pixel surface and its drawing primitives.

use dunit_style::value::Color;
use dunit_text::GlyphBitmap;

/// Pack an DSS [`Color`] into a `0xAARRGGBB` pixel.
pub fn pack_argb(c: Color) -> u32 {
    ((c.a as u32) << 24) | ((c.r as u32) << 16) | ((c.g as u32) << 8) | (c.b as u32)
}

/// A mutable ARGB8888 pixel buffer. Pixels are `0xAARRGGBB`; `stride` is the
/// number of pixels per row (`>= width`), so a surface can address a sub-window
/// of a larger buffer.
pub struct Surface<'a> {
    pub pixels: &'a mut [u32],
    pub width: usize,
    pub height: usize,
    pub stride: usize,
}

impl<'a> Surface<'a> {
    /// A surface over a tightly packed `width * height` buffer (stride = width).
    pub fn new(pixels: &'a mut [u32], width: usize, height: usize) -> Surface<'a> {
        Surface { pixels, width, height, stride: width }
    }

    /// The pixel at `(x, y)`, or 0 if out of bounds (test/debug helper).
    pub fn get(&self, x: usize, y: usize) -> u32 {
        if x < self.width && y < self.height {
            self.pixels[y * self.stride + x]
        } else {
            0
        }
    }

    /// Alpha-blend `color` over the pixel at `(x, y)` (src-over). Out-of-bounds
    /// and fully transparent writes are no-ops.
    fn blend(&mut self, x: usize, y: usize, color: Color, cov: u8) {
        if x >= self.width || y >= self.height {
            return;
        }
        // Effective source alpha folds the color's own alpha with coverage.
        let a = (color.a as u32 * cov as u32) / 255;
        if a == 0 {
            return;
        }
        let idx = y * self.stride + x;
        if a == 255 {
            self.pixels[idx] = pack_argb(color);
            return;
        }
        let dst = self.pixels[idx];
        let inv = 255 - a;
        let dr = (dst >> 16) & 0xff;
        let dg = (dst >> 8) & 0xff;
        let db = dst & 0xff;
        let da = (dst >> 24) & 0xff;
        let r = (color.r as u32 * a + dr * inv) / 255;
        let g = (color.g as u32 * a + dg * inv) / 255;
        let b = (color.b as u32 * a + db * inv) / 255;
        // Resulting alpha: src over dst.
        let out_a = a + da * inv / 255;
        self.pixels[idx] = (out_a << 24) | (r << 16) | (g << 8) | b;
    }

    /// Fill the rect `(x, y, w, h)` (surface pixels, clipped) with `color`,
    /// alpha-blended. Fractional coordinates are rounded to the nearest pixel.
    pub fn fill_rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: Color) {
        if color.a == 0 || w <= 0.0 || h <= 0.0 {
            return;
        }
        let (x0, y0, x1, y1) = self.clip_rect(x, y, w, h);
        for py in y0..y1 {
            for px in x0..x1 {
                self.blend(px, py, color, 255);
            }
        }
    }

    /// Stroke a `width`-thick border just inside the rect `(x, y, w, h)`, so the
    /// border stays within the node's box (border-box semantics).
    pub fn stroke_rect(&mut self, x: f32, y: f32, w: f32, h: f32, width: f32, color: Color) {
        if color.a == 0 || width <= 0.0 || w <= 0.0 || h <= 0.0 {
            return;
        }
        // Clamp thickness so opposite edges never overlap past the box centre.
        let t = fmin(width, fmin(w, h) / 2.0);
        self.fill_rect(x, y, w, t, color); // top
        self.fill_rect(x, y + h - t, w, t, color); // bottom
        self.fill_rect(x, y, t, h, color); // left
        self.fill_rect(x + w - t, y, t, h, color); // right
    }

    /// Blit a glyph coverage mask in `color`, with the mask's top-left at pixel
    /// `(ox, oy)`. Coverage scales the color's alpha per pixel (anti-aliasing).
    pub fn blit_glyph(&mut self, glyph: &GlyphBitmap, ox: i32, oy: i32, color: Color) {
        for gy in 0..glyph.height {
            let py = oy + gy as i32;
            if py < 0 {
                continue;
            }
            for gx in 0..glyph.width {
                let px = ox + gx as i32;
                if px < 0 {
                    continue;
                }
                let cov = glyph.at(gx, gy);
                if cov != 0 {
                    self.blend(px as usize, py as usize, color, cov);
                }
            }
        }
    }

    /// Round `(x, y, w, h)` to integer pixels and clip to the surface.
    fn clip_rect(&self, x: f32, y: f32, w: f32, h: f32) -> (usize, usize, usize, usize) {
        let x0 = clamp_usize(round_i32(x), self.width);
        let y0 = clamp_usize(round_i32(y), self.height);
        let x1 = clamp_usize(round_i32(x + w), self.width);
        let y1 = clamp_usize(round_i32(y + h), self.height);
        (x0, y0, x1, y1)
    }
}

/// Round to nearest, ties toward +infinity; clamps below at 0.
fn round_i32(v: f32) -> i32 {
    if v <= 0.0 {
        0
    } else {
        (v + 0.5) as i32
    }
}

fn clamp_usize(v: i32, max: usize) -> usize {
    if v < 0 {
        0
    } else if v as usize > max {
        max
    } else {
        v as usize
    }
}

fn fmin(a: f32, b: f32) -> f32 {
    if a < b {
        a
    } else {
        b
    }
}
