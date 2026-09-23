//! Boundary, hard-limit and no-partial-state conformance (README §11.7, §11.8,
//! §11.12). These re-express the "требования к будущим headless/property tests"
//! that the behavioural/namespace suites only touched in passing: the exact
//! min/max/max+1 edges of every hard limit, the wire's fixed little-endian /
//! no-native-padding guarantee, buffer-backing budgets, damage-rect bounds and
//! count/array framing, and the rule that a recoverable error leaves no partial
//! object and is retried under a fresh serial.

mod common;
use common::*;
use gui_protocol_v1::server::Server;
use gui_protocol_v1::wire::{Header, Rect, DIM_MAX, MAX_DAMAGE_RECTS};
use gui_protocol_v1::{Opcode, Request};

// ---- builders -----------------------------------------------------------

fn create(obj: u64, serial: u64, w: u32, h: u32, fmt: u32) -> Vec<u8> {
    Request::CreateSurface { role: 1, width: w, height: h, format: fmt }.encode(obj, serial)
}
fn import_raw(obj: u64, serial: u64, w: u32, h: u32, stride: u32, offset: u64, fmt: u32) -> Vec<u8> {
    Request::ImportBuffer { width: w, height: h, stride, format: fmt, offset }.encode(obj, serial)
}
fn import(obj: u64, serial: u64, w: u32, h: u32) -> Vec<u8> {
    import_raw(obj, serial, w, h, w * 4, 0, 1)
}
fn tight_cap(w: u32, h: u32) -> u64 {
    (w * 4 * h) as u64
}
fn attach(obj: u64, serial: u64, buffer: u64, damage: Vec<Rect>) -> Vec<u8> {
    Request::AttachBuffer { buffer, damage }.encode(obj, serial)
}
fn rect(x: i32, y: i32, w: u32, h: u32) -> Rect {
    Rect { x, y, w, h }
}

// A conformant "created + acked" surface `sid`, returning nothing of note; used
// only to obtain a buffer target for damage tests.
fn create_ok(s: &mut Server, c: gui_protocol_v1::server::ConnId, sid: u64, serial: &mut u64, w: u32, h: u32) {
    let out = s.deliver(c, &create(sid, *serial, w, h, 1), None);
    assert_eq!(opcode(&out[0]), Opcode::Result, "CREATE ok");
    *serial += 1;
}

// ---- §11.7 damage rects: boundary, max, max+1, count/array framing --------

/// A damage rect flush with the buffer's far edge is in-bounds; one column past
/// it is OUT_OF_BOUNDS; a zero-area rect is OUT_OF_BOUNDS.
#[test]
fn damage_rect_boundaries() {
    let mut s = Server::new();
    let c = active_conn(&mut s);
    let mut serial = 2;
    create_ok(&mut s, c, 1, &mut serial, 16, 16);
    assert_eq!(opcode(&s.deliver(c, &import(2, serial, 16, 16), Some(tight_cap(16, 16)))[0]), Opcode::Result);
    serial += 1;

    // Exactly covering the 16x16 buffer is valid (half-open, x+w == width).
    let out = s.deliver(c, &attach(1, serial, 2, vec![rect(0, 0, 16, 16)]), None);
    assert_eq!(opcode(&out[0]), Opcode::Result, "edge-flush damage accepted");
}

/// One pixel past the edge and a zero-width rect both fault with OUT_OF_BOUNDS.
#[test]
fn damage_rect_out_of_bounds() {
    let mut s = Server::new();
    let c = active_conn(&mut s);
    let mut serial = 2;
    create_ok(&mut s, c, 1, &mut serial, 16, 16);
    s.deliver(c, &import(2, serial, 16, 16), Some(tight_cap(16, 16)));
    serial += 1;

    // x + w == 17 > 16.
    assert_eq!(error_of(&s.deliver(c, &attach(1, serial, 2, vec![rect(0, 0, 17, 1)]), None)[0]), (11, 0));
    serial += 1;
    // Zero-area rect is rejected before any state changes.
    assert_eq!(error_of(&s.deliver(c, &attach(1, serial, 2, vec![rect(0, 0, 0, 4)]), None)[0]), (11, 0));
    serial += 1;
    // The buffer is still Available: a valid attach now succeeds.
    assert_eq!(opcode(&s.deliver(c, &attach(1, serial, 2, vec![rect(1, 1, 2, 2)]), None)[0]), Opcode::Result);
}

/// Exactly MAX_DAMAGE_RECTS in-bounds rects are accepted; one more is
/// LIMIT_EXCEEDED — the count check runs after per-rect bounds so every rect
/// here is individually valid (§11.7).
#[test]
fn damage_rect_count_limit() {
    let mut s = Server::new();
    let c = active_conn(&mut s);
    let mut serial = 2;

    // A fresh surface+buffer accepts exactly MAX_DAMAGE_RECTS rects.
    create_ok(&mut s, c, 1, &mut serial, 16, 16);
    s.deliver(c, &import(2, serial, 16, 16), Some(tight_cap(16, 16)));
    serial += 1;
    let max = MAX_DAMAGE_RECTS as usize;
    let ok: Vec<Rect> = (0..max).map(|_| rect(0, 0, 1, 1)).collect();
    assert_eq!(opcode(&s.deliver(c, &attach(1, serial, 2, ok), None)[0]), Opcode::Result);
    serial += 1;

    // A second fresh surface+buffer with one rect too many is LIMIT_EXCEEDED
    // (every rect is individually in-bounds, so only the count check fires).
    create_ok(&mut s, c, 3, &mut serial, 16, 16);
    s.deliver(c, &import(4, serial, 16, 16), Some(tight_cap(16, 16)));
    serial += 1;
    let too_many: Vec<Rect> = (0..=max).map(|_| rect(0, 0, 1, 1)).collect();
    assert_eq!(error_of(&s.deliver(c, &attach(3, serial, 4, too_many), None)[0]), (13, 0), "9 rects → LIMIT_EXCEEDED");
}

/// A `damage_count` header that disagrees with the rect array length is a wire
/// framing violation: MALFORMED and fatal, decided before any quota check.
#[test]
fn damage_count_array_mismatch_is_malformed() {
    let mut s = Server::new();
    let c = active_conn(&mut s);
    let mut serial = 2;
    create_ok(&mut s, c, 1, &mut serial, 16, 16);
    s.deliver(c, &import(2, serial, 16, 16), Some(tight_cap(16, 16)));
    serial += 1;

    // Encode a zero-rect attach, then claim 9 rects in the count field. The
    // payload (buffer:8, count:4, reserved:4) is 16 bytes; the decoder tries to
    // read 9 rects from the now-empty tail and rejects the packet.
    let mut pkt = attach(1, serial, 2, Vec::new());
    let count = 9u32.to_le_bytes(); // damage_count sits at payload byte 8 => packet byte 40
    pkt[40..44].copy_from_slice(&count);
    let out = s.deliver(c, &pkt, None);
    assert_eq!(error_of(&out[0]), (1, 1), "count/array mismatch → MALFORMED, fatal");
    assert!(s.deliver(c, &hello(99), None).is_empty(), "malformed request closes the connection");
}

// ---- §11.7 import backing bounds: end == cap, end-1, overflow --------------

/// The required backing is `stride*(h-1) + offset + w*4`; a cap equal to it is
/// accepted, one byte short is OUT_OF_BOUNDS, and an offset that overflows u64
/// is OUT_OF_BOUNDS rather than a panic.
#[test]
fn import_backing_end_boundaries() {
    let mut s = Server::new();
    let c = active_conn(&mut s);
    let mut serial = 2;

    // 16x16, stride 64, offset 0 => end = 64*15 + 64 = 1024.
    let need = 1024u64;
    assert_eq!(error_of(&s.deliver(c, &import_raw(1, serial, 16, 16, 64, 0, 1), Some(need - 1))[0]), (11, 0), "one byte short");
    serial += 1;
    assert_eq!(opcode(&s.deliver(c, &import_raw(1, serial, 16, 16, 64, 0, 1), Some(need))[0]), Opcode::Result, "end == cap ok");
    serial += 1;

    // Offset near u64::MAX (multiple of 4) overflows the end computation.
    let huge = u64::MAX & !3;
    assert_eq!(error_of(&s.deliver(c, &import_raw(2, serial, 16, 16, 64, huge, 1), Some(1 << 20))[0]), (11, 0), "offset overflow → OUT_OF_BOUNDS");
}

// ---- §11.12 hard limits: surface dims min / max / max+1 --------------------

/// Surface width/height accept the min and max dims and reject max+1 with
/// BAD_VALUE (the fresh-id watermark is untouched by the rejection, §11.8).
#[test]
fn surface_dim_edges() {
    let mut s = Server::new();
    let c = active_conn(&mut s);
    let mut serial = 2;

    // Min edge (1x1).
    assert_eq!(opcode(&s.deliver(c, &create(1, serial, 1, 1, 1), None)[0]), Opcode::Result);
    serial += 1;
    // Max edge (DIM_MAX square).
    assert_eq!(opcode(&s.deliver(c, &create(2, serial, DIM_MAX, DIM_MAX, 1), None)[0]), Opcode::Result);
    serial += 1;
    // Max+1 on width, then height.
    assert_eq!(error_of(&s.deliver(c, &create(3, serial, DIM_MAX + 1, 4, 1), None)[0]), (8, 0), "width max+1 → BAD_VALUE");
    serial += 1;
    assert_eq!(error_of(&s.deliver(c, &create(3, serial, 4, DIM_MAX + 1, 1), None)[0]), (8, 0), "height max+1 → BAD_VALUE");
    serial += 1;
    // The rejected id 3 was never consumed: it is still a valid fresh id.
    assert_eq!(opcode(&s.deliver(c, &create(3, serial, 4, 4, 1), None)[0]), Opcode::Result, "id reusable after rejection");
    assert_eq!(s.connection(c).unwrap().object_count(), 3);
}

// ---- §11.12 hard limits: per-connection backing budget ---------------------

/// Four 64 MiB buffers exactly saturate the 256 MiB per-connection backing
/// budget; the fifth is LIMIT_EXCEEDED.
#[test]
fn per_connection_backing_budget() {
    let mut s = Server::new();
    let c = active_conn(&mut s);
    let mut serial = 2;

    // 4096x4096 ARGB, stride 16384, offset 0 => end = 16384*4096 = 64 MiB.
    let cap = 16384u64 * 4096;
    for id in 1..=4 {
        assert_eq!(opcode(&s.deliver(c, &import_raw(id, serial, DIM_MAX, DIM_MAX, 16384, 0, 1), Some(cap))[0]), Opcode::Result, "buffer {id} fits budget");
        serial += 1;
    }
    // Fifth pushes the connection past 256 MiB.
    assert_eq!(error_of(&s.deliver(c, &import_raw(5, serial, DIM_MAX, DIM_MAX, 16384, 0, 1), Some(cap))[0]), (13, 0), "budget exhausted → LIMIT_EXCEEDED");
}

// ---- §11.8 no partial state; retry under a fresh serial --------------------

/// A recoverable error creates no object and consumes no id; the same id
/// succeeds on retry with a fresh serial. Replaying the old serial is fatal
/// BAD_SERIAL — retries must advance the client serial (§2, §11.8).
#[test]
fn no_partial_object_and_serial_retry() {
    let mut s = Server::new();
    let c = active_conn(&mut s);

    // Bad format leaves no surface behind.
    assert_eq!(error_of(&s.deliver(c, &create(1, 2, 16, 16, 99), None)[0]), (8, 0), "bad format → BAD_VALUE");
    assert_eq!(s.connection(c).unwrap().object_count(), 0, "no partial surface");

    // Retry the same id under a fresh serial: it succeeds.
    assert_eq!(opcode(&s.deliver(c, &create(1, 3, 16, 16, 1), None)[0]), Opcode::Result, "retry with new serial");
    assert_eq!(s.connection(c).unwrap().object_count(), 1);

    // A too-small backing leaves no buffer and does not consume the id.
    assert_eq!(error_of(&s.deliver(c, &import(2, 4, 16, 16), Some(1))[0]), (11, 0), "backing too small → OUT_OF_BOUNDS");
    assert_eq!(s.connection(c).unwrap().object_count(), 1, "no partial buffer");
    assert_eq!(opcode(&s.deliver(c, &import(2, 5, 16, 16), Some(tight_cap(16, 16)))[0]), Opcode::Result, "id 2 reusable");

    // Replaying an already-seen serial is a fatal protocol error.
    let out = s.deliver(c, &create(6, 3, 16, 16, 1), None);
    assert_eq!(error_of(&out[0]), (7, 1), "stale serial → BAD_SERIAL, fatal");
    assert!(s.deliver(c, &hello(99), None).is_empty(), "connection closed on serial regression");
}

// ---- §11.12 fixed little-endian wire, no native padding --------------------

/// Every field is packed little-endian at a fixed offset with no implicit
/// padding: a 32-byte header + a 16-byte COMMIT payload, byte-exact, and a
/// lossless decode round-trip.
#[test]
fn wire_is_fixed_little_endian() {
    let obj = 0x1122_3344_5566_7788u64;
    let ser = 0x99AA_BBCC_DDEE_FF00u64;
    let cfg = 0x0102_0304_0506_0708u64;
    let pkt = Request::Commit { configure: cfg, frame_callback: 1 }.encode(obj, ser);

    // Header(32) + configure:u64 + frame_callback:u32 + reserved:u32 = 48, no padding.
    assert_eq!(pkt.len(), 48, "packed size, no native padding");
    // Magic is little-endian 0x49554744 == b"DGUI".
    assert_eq!(&pkt[0..4], b"DGUI");
    // object and serial are little-endian at their fixed header offsets.
    assert_eq!(&pkt[16..24], &obj.to_le_bytes());
    assert_eq!(&pkt[24..32], &ser.to_le_bytes());
    // configure payload word is little-endian immediately after the header.
    assert_eq!(&pkt[32..40], &cfg.to_le_bytes());

    // Header decodes to the same fields; payload decodes losslessly.
    let h = Header::decode(&pkt).unwrap();
    assert_eq!((h.object, h.serial), (obj, ser));
    assert_eq!(opcode(&pkt), Opcode::Commit);
    match Request::decode(Opcode::Commit, &pkt[32..]).unwrap() {
        Request::Commit { configure, frame_callback } => assert_eq!((configure, frame_callback), (cfg, 1)),
        other => panic!("round-trip changed the request: {other:?}"),
    }
}
