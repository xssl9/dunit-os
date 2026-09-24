//! Glyph outline extraction: decode the `glyf` table into flattened quadratic
//! contours expressed as polylines in font units, ready for the rasterizer.

use alloc::vec::Vec;

use crate::font::{Font, GlyphId};
use crate::parse::Reader;

/// A point in font units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

/// A decoded glyph outline: one polyline per contour (already flattened from
/// quadratic B-splines) plus the outline bounding box, all in font units.
#[derive(Debug, Clone, Default)]
pub struct Outline {
    pub contours: Vec<Vec<Point>>,
    pub x_min: f32,
    pub y_min: f32,
    pub x_max: f32,
    pub y_max: f32,
}

impl Outline {
    pub fn is_empty(&self) -> bool {
        self.contours.iter().all(|c| c.len() < 2)
    }

    fn recompute_bounds(&mut self) {
        let mut first = true;
        for c in &self.contours {
            for p in c {
                if first {
                    self.x_min = p.x;
                    self.x_max = p.x;
                    self.y_min = p.y;
                    self.y_max = p.y;
                    first = false;
                } else {
                    self.x_min = self.x_min.min(p.x);
                    self.x_max = self.x_max.max(p.x);
                    self.y_min = self.y_min.min(p.y);
                    self.y_max = self.y_max.max(p.y);
                }
            }
        }
    }
}

// TrueType simple-glyph flag bits.
const ON_CURVE: u8 = 0x01;
const X_SHORT: u8 = 0x02;
const Y_SHORT: u8 = 0x04;
const REPEAT: u8 = 0x08;
const X_SAME_POS: u8 = 0x10;
const Y_SAME_POS: u8 = 0x20;

/// Decode the outline for `glyph`, recursing through composite components.
pub fn outline(font: &Font, glyph: GlyphId) -> Outline {
    let mut out = Outline::default();
    append_glyph(font, glyph.0, &mut out, 0, Affine::IDENTITY);
    out.recompute_bounds();
    out
}

/// A 2x3 affine transform (component placement in composite glyphs).
#[derive(Clone, Copy)]
struct Affine {
    a: f32,
    b: f32,
    c: f32,
    d: f32,
    e: f32,
    f: f32,
}

impl Affine {
    const IDENTITY: Affine = Affine { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: 0.0, f: 0.0 };

    fn apply(&self, x: f32, y: f32) -> Point {
        Point { x: self.a * x + self.c * y + self.e, y: self.b * x + self.d * y + self.f }
    }

    /// self ∘ other (apply `other` first, then `self`).
    fn concat(&self, o: &Affine) -> Affine {
        Affine {
            a: self.a * o.a + self.c * o.b,
            b: self.b * o.a + self.d * o.b,
            c: self.a * o.c + self.c * o.d,
            d: self.b * o.c + self.d * o.d,
            e: self.a * o.e + self.c * o.f + self.e,
            f: self.b * o.e + self.d * o.f + self.f,
        }
    }
}

fn append_glyph(font: &Font, gid: u16, out: &mut Outline, depth: u8, xf: Affine) {
    if depth > 8 {
        return; // guard against pathological/cyclic composites
    }
    let data = match font.glyph_slice(gid) {
        Some(d) => d,
        None => return, // empty glyph (space etc.)
    };
    let mut r = Reader::new(data);
    let num_contours = r.i16().unwrap_or(0);
    if num_contours >= 0 {
        decode_simple(data, num_contours as usize, out, xf);
    } else {
        decode_composite(font, data, out, depth, xf);
    }
}

fn decode_simple(data: &[u8], num_contours: usize, out: &mut Outline, xf: Affine) {
    let _ = decode_simple_inner(data, num_contours, out, xf);
}

fn decode_simple_inner(data: &[u8], num_contours: usize, out: &mut Outline, xf: Affine) -> Option<()> {
    if num_contours == 0 {
        return Some(());
    }
    let mut r = Reader::new(data);
    r.skip(10).ok()?; // numberOfContours(2) + bbox(8)
    let mut end_pts = Vec::with_capacity(num_contours);
    for _ in 0..num_contours {
        end_pts.push(r.u16().ok()?);
    }
    let num_points = *end_pts.last().unwrap() as usize + 1;
    let instr_len = r.u16().ok()? as usize;
    r.skip(instr_len).ok()?;

    // Flags with run-length repeat expansion.
    let mut flags = Vec::with_capacity(num_points);
    while flags.len() < num_points {
        let f = r.u8().ok()?;
        flags.push(f);
        if f & REPEAT != 0 {
            let count = r.u8().ok()?;
            for _ in 0..count {
                if flags.len() >= num_points {
                    break;
                }
                flags.push(f);
            }
        }
    }

    // X then Y delta-encoded coordinate streams.
    let mut xs = Vec::with_capacity(num_points);
    let mut acc: i32 = 0;
    for &f in &flags {
        acc += read_coord_delta(&mut r, f, X_SHORT, X_SAME_POS)?;
        xs.push(acc as f32);
    }
    let mut ys = Vec::with_capacity(num_points);
    acc = 0;
    for &f in &flags {
        acc += read_coord_delta(&mut r, f, Y_SHORT, Y_SAME_POS)?;
        ys.push(acc as f32);
    }

    // Split into contours and flatten each quadratic B-spline into a polyline.
    let mut start = 0usize;
    for &end in &end_pts {
        let end = end as usize;
        let mut pts = Vec::with_capacity(end - start + 1);
        for i in start..=end {
            pts.push((Point { x: xs[i], y: ys[i] }, flags[i] & ON_CURVE != 0));
        }
        flatten_contour(&pts, xf, out);
        start = end + 1;
    }
    Some(())
}

fn read_coord_delta(r: &mut Reader, flag: u8, short_bit: u8, same_bit: u8) -> Option<i32> {
    if flag & short_bit != 0 {
        let v = r.u8().ok()? as i32;
        Some(if flag & same_bit != 0 { v } else { -v })
    } else if flag & same_bit != 0 {
        Some(0) // coordinate unchanged from previous
    } else {
        Some(r.i16().ok()? as i32)
    }
}

#[inline]
fn mid(a: Point, b: Point) -> Point {
    Point { x: (a.x + b.x) * 0.5, y: (a.y + b.y) * 0.5 }
}

/// Flatten one contour of on/off-curve points into a transformed polyline and
/// push it onto `out`. TrueType contours are closed quadratic B-splines: an
/// off-curve point is a quadratic control point, and two consecutive off-curve
/// points imply an on-curve point at their midpoint.
fn flatten_contour(pts: &[(Point, bool)], xf: Affine, out: &mut Outline) {
    let n = pts.len();
    if n < 2 {
        return;
    }
    let mut poly: Vec<Point> = Vec::new();

    // Find a starting on-curve point.
    let on_start = (0..n).find(|&i| pts[i].1);

    match on_start {
        Some(s) => {
            let start = pts[s].0;
            poly.push(start);
            let mut cur = start;
            let mut pending: Option<Point> = None;
            for k in 1..=n {
                let (p, on) = pts[(s + k) % n];
                if on {
                    match pending.take() {
                        Some(c) => quad(cur, c, p, &mut poly),
                        None => poly.push(p),
                    }
                    cur = p;
                } else if let Some(c) = pending {
                    // Two off-curve points in a row: implied on-curve midpoint.
                    let implied = mid(c, p);
                    quad(cur, c, implied, &mut poly);
                    cur = implied;
                    pending = Some(p);
                } else {
                    pending = Some(p);
                }
            }
        }
        None => {
            // All points off-curve: on-curve points are the segment midpoints.
            let mut prev = mid(pts[n - 1].0, pts[0].0);
            poly.push(prev);
            for i in 0..n {
                let ctrl = pts[i].0;
                let next = mid(pts[i].0, pts[(i + 1) % n].0);
                quad(prev, ctrl, next, &mut poly);
                prev = next;
            }
        }
    }

    // Transform into the target space and store as a closed polyline.
    let contour: Vec<Point> = poly.iter().map(|p| xf.apply(p.x, p.y)).collect();
    if contour.len() >= 2 {
        out.contours.push(contour);
    }
}

/// Subdivide a quadratic Bézier (p0 control p1) into line segments appended to
/// `poly` (p0 is assumed already present as the last point). Step count scales
/// with the control-polygon chord length so large glyphs stay smooth.
fn quad(p0: Point, c: Point, p1: Point, poly: &mut Vec<Point>) {
    let chord = dist(p0, c) + dist(c, p1);
    let steps = ((chord / 40.0) as usize).clamp(2, 24);
    for i in 1..=steps {
        let t = i as f32 / steps as f32;
        let u = 1.0 - t;
        let x = u * u * p0.x + 2.0 * u * t * c.x + t * t * p1.x;
        let y = u * u * p0.y + 2.0 * u * t * c.y + t * t * p1.y;
        poly.push(Point { x, y });
    }
}

fn dist(a: Point, b: Point) -> f32 {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    sqrt(dx * dx + dy * dy)
}

/// Newton's-method square root (no libm in `no_std`).
fn sqrt(v: f32) -> f32 {
    if v <= 0.0 {
        return 0.0;
    }
    let mut g = v;
    for _ in 0..8 {
        g = 0.5 * (g + v / g);
    }
    g
}

// TrueType composite-glyph component flag bits.
const ARG_1_AND_2_ARE_WORDS: u16 = 0x0001;
const ARGS_ARE_XY_VALUES: u16 = 0x0002;
const WE_HAVE_A_SCALE: u16 = 0x0008;
const MORE_COMPONENTS: u16 = 0x0020;
const WE_HAVE_AN_X_AND_Y_SCALE: u16 = 0x0040;
const WE_HAVE_A_TWO_BY_TWO: u16 = 0x0080;

fn decode_composite(font: &Font, data: &[u8], out: &mut Outline, depth: u8, xf: Affine) {
    let _ = decode_composite_inner(font, data, out, depth, xf);
}

fn decode_composite_inner(font: &Font, data: &[u8], out: &mut Outline, depth: u8, xf: Affine) -> Option<()> {
    let mut r = Reader::new(data);
    r.skip(10).ok()?; // numberOfContours(2) + bbox(8)
    loop {
        let flags = r.u16().ok()?;
        let component_gid = r.u16().ok()?;

        // Arguments 1 & 2: either signed words or signed bytes.
        let (arg1, arg2) = if flags & ARG_1_AND_2_ARE_WORDS != 0 {
            (r.i16().ok()? as f32, r.i16().ok()? as f32)
        } else {
            (r.u8().ok()? as i8 as f32, r.u8().ok()? as i8 as f32)
        };

        // 2x2 linear part (defaults to identity).
        let (mut a, mut b, mut c, mut d) = (1.0f32, 0.0f32, 0.0f32, 1.0f32);
        if flags & WE_HAVE_A_SCALE != 0 {
            let s = f2dot14(&mut r)?;
            a = s;
            d = s;
        } else if flags & WE_HAVE_AN_X_AND_Y_SCALE != 0 {
            a = f2dot14(&mut r)?;
            d = f2dot14(&mut r)?;
        } else if flags & WE_HAVE_A_TWO_BY_TWO != 0 {
            a = f2dot14(&mut r)?;
            b = f2dot14(&mut r)?;
            c = f2dot14(&mut r)?;
            d = f2dot14(&mut r)?;
        }

        // Only XY-offset placement is supported (point-matching is rare and
        // needs the parent's decoded points; skip it gracefully).
        let (e, f) = if flags & ARGS_ARE_XY_VALUES != 0 {
            (arg1, arg2)
        } else {
            (0.0, 0.0)
        };

        let comp = Affine { a, b, c, d, e, f };
        append_glyph(font, component_gid, out, depth + 1, xf.concat(&comp));

        if flags & MORE_COMPONENTS == 0 {
            break;
        }
    }
    Some(())
}

/// Read an F2Dot14 fixed-point value (2 integer bits, 14 fraction bits).
fn f2dot14(r: &mut Reader) -> Option<f32> {
    let raw = r.i16().ok()?;
    Some(raw as f32 / 16384.0)
}



