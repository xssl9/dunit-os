//! Surface primitive tests: fills, clipping, alpha blending and borders.

use dunit_render::{pack_argb, Surface};
use dunit_style::value::Color;

const W: usize = 10;
const H: usize = 10;

fn blank() -> [u32; W * H] {
    [0u32; W * H]
}

#[test]
fn fill_is_clipped_and_placed() {
    let mut buf = blank();
    let mut s = Surface::new(&mut buf, W, H);
    let red = Color::rgb(255, 0, 0);
    s.fill_rect(2.0, 2.0, 4.0, 4.0, red);

    assert_eq!(s.get(2, 2), pack_argb(red)); // top-left corner of the fill
    assert_eq!(s.get(5, 5), pack_argb(red)); // last filled pixel (x1/y1 exclusive)
    assert_eq!(s.get(6, 6), 0); // just outside
    assert_eq!(s.get(0, 0), 0); // untouched
}

#[test]
fn fill_clips_past_the_edge() {
    let mut buf = blank();
    let mut s = Surface::new(&mut buf, W, H);
    // Extends well past the surface; must not panic and must clip.
    s.fill_rect(8.0, 8.0, 100.0, 100.0, Color::rgb(1, 2, 3));
    assert_eq!(s.get(9, 9), pack_argb(Color::rgb(1, 2, 3)));
}

#[test]
fn alpha_blends_over_existing() {
    let mut buf = blank();
    let mut s = Surface::new(&mut buf, W, H);
    s.fill_rect(0.0, 0.0, W as f32, H as f32, Color::rgb(0, 0, 255)); // opaque blue
    s.fill_rect(0.0, 0.0, W as f32, H as f32, Color::rgba(255, 0, 0, 128)); // half red

    let px = s.get(4, 4);
    let r = (px >> 16) & 0xff;
    let b = px & 0xff;
    assert_eq!(r, 128); // 255 * 128/255
    assert_eq!(b, 127); // 255 * 127/255, the surviving blue
}

#[test]
fn stroke_touches_only_the_frame() {
    let mut buf = blank();
    let mut s = Surface::new(&mut buf, W, H);
    let green = Color::rgb(0, 255, 0);
    s.stroke_rect(0.0, 0.0, W as f32, H as f32, 2.0, green);

    let g = pack_argb(green);
    assert_eq!(s.get(0, 0), g); // corner
    assert_eq!(s.get(5, 0), g); // top edge
    assert_eq!(s.get(5, 9), g); // bottom edge
    assert_eq!(s.get(0, 5), g); // left edge
    assert_eq!(s.get(9, 5), g); // right edge
    assert_eq!(s.get(5, 5), 0); // interior untouched
}

#[test]
fn blit_image_places_and_blends_rgba() {
    let mut buf = blank();
    let mut s = Surface::new(&mut buf, W, H);
    // A 2x2 image: opaque red, opaque green, transparent, half-white (R,G,B,A).
    let img: [u8; 16] = [
        255, 0, 0, 255, // (0,0) opaque red
        0, 255, 0, 255, // (1,0) opaque green
        9, 9, 9, 0, //     (0,1) fully transparent -> skipped
        255, 255, 255, 128, // (1,1) half white over black
    ];
    s.blit_image(3, 4, &img, 2, 2);

    assert_eq!(s.get(3, 4), pack_argb(Color::rgb(255, 0, 0)));
    assert_eq!(s.get(4, 4), pack_argb(Color::rgb(0, 255, 0)));
    assert_eq!(s.get(3, 5), 0); // transparent pixel left the surface untouched
    // Half-white over opaque black: 255 * 128/255 = 128 per channel.
    let hw = s.get(4, 5);
    assert_eq!((hw >> 16) & 0xff, 128);
}

#[test]
fn blit_image_clips_out_of_bounds() {
    let mut buf = blank();
    let mut s = Surface::new(&mut buf, W, H);
    let img = [255u8; 4 * 4]; // 2x2 opaque white
    // Straddle the bottom-right corner: only (9,9) lands on the surface.
    s.blit_image(9, 9, &img, 2, 2);
    assert_eq!(s.get(9, 9), pack_argb(Color::rgb(255, 255, 255)));
    // A short slice must not panic (clips trailing rows).
    let short = [255u8; 5];
    s.blit_image(0, 0, &short, 2, 2);
    assert_eq!(s.get(0, 0), pack_argb(Color::rgb(255, 255, 255)));
}
