//! Object namespace and per-request structural validation (spec §2, §8, §10):
//! fresh/stale/reused IDs, wrong type, cross-connection isolation, enum/range
//! checks and per-connection limits.

mod common;
use common::*;
use gui_protocol_v1::server::{ConnId, Server};
use gui_protocol_v1::{Opcode, Request};

fn create(id: u64, serial: u64, role: u32, w: u32, h: u32, format: u32) -> Vec<u8> {
    Request::CreateSurface { role, width: w, height: h, format }.encode(id, serial)
}

/// A conforming toplevel surface create.
fn ok_create(id: u64, serial: u64) -> Vec<u8> {
    create(id, serial, 1, 16, 16, 1)
}

fn is_result(s: &mut Server, c: ConnId, p: &[u8]) -> bool {
    opcode(&s.deliver(c, p, None)[0]) == Opcode::Result
}

#[test]
fn fresh_id_then_reuse_is_rejected() {
    let mut s = Server::new();
    let c = active_conn(&mut s);
    assert!(is_result(&mut s, c, &ok_create(1, 2)));
    // Reusing id 1 (<= watermark) is BAD_VALUE, recoverable.
    assert_eq!(error_of(&s.deliver(c, &ok_create(1, 3), None)[0]), (8, 0));
}

#[test]
fn ids_must_strictly_increase() {
    let mut s = Server::new();
    let c = active_conn(&mut s);
    assert!(is_result(&mut s, c, &ok_create(5, 2)));
    // id 3 <= watermark 5.
    assert_eq!(error_of(&s.deliver(c, &ok_create(3, 3), None)[0]), (8, 0));
}

#[test]
fn zero_id_create_is_bad_value() {
    let mut s = Server::new();
    let c = active_conn(&mut s);
    assert_eq!(error_of(&s.deliver(c, &ok_create(0, 2), None)[0]), (8, 0));
}

#[test]
fn stale_object_reference() {
    let mut s = Server::new();
    let c = active_conn(&mut s);
    // Destroy a surface that was never created.
    let d = Request::DestroySurface.encode(99, 2);
    assert_eq!(error_of(&s.deliver(c, &d, None)[0]), (5, 0), "STALE_OBJECT recoverable");
}

#[test]
fn wrong_object_type() {
    let mut s = Server::new();
    let c = active_conn(&mut s);
    assert!(is_result(&mut s, c, &ok_create(1, 2)));
    // id 1 is a surface; DESTROY_BUFFER expects a buffer.
    let d = Request::DestroyBuffer.encode(1, 3);
    assert_eq!(error_of(&s.deliver(c, &d, None)[0]), (6, 0), "WRONG_OBJECT_TYPE");
}

#[test]
fn namespaces_are_per_connection() {
    let mut s = Server::new();
    let a = active_conn(&mut s);
    let b = active_conn(&mut s);
    assert!(is_result(&mut s, a, &ok_create(1, 2)));
    // b never created id 1: destroying it is stale, and b may mint its own id 1.
    let d = Request::DestroySurface.encode(1, 2);
    assert_eq!(error_of(&s.deliver(b, &d, None)[0]), (5, 0));
    assert!(is_result(&mut s, b, &ok_create(1, 3)));
}

#[test]
fn role_and_format_and_dims() {
    let mut s = Server::new();
    let c = active_conn(&mut s);
    // Non-toplevel role.
    assert_eq!(error_of(&s.deliver(c, &create(1, 2, 2, 16, 16, 1), None)[0]), (9, 0));
    // Unknown format.
    assert_eq!(error_of(&s.deliver(c, &create(1, 3, 1, 16, 16, 99), None)[0]), (8, 0));
    // Zero dimension.
    assert_eq!(error_of(&s.deliver(c, &create(1, 4, 1, 0, 16, 1), None)[0]), (8, 0));
    // Oversized dimension (> 4096).
    assert_eq!(error_of(&s.deliver(c, &create(1, 5, 1, 5000, 16, 1), None)[0]), (8, 0));
    // ARGB8888 is negotiated in `hello`, so it is accepted.
    assert!(is_result(&mut s, c, &create(1, 6, 1, 16, 16, 2)));
}

#[test]
fn surface_limit_is_enforced() {
    let mut s = Server::new();
    let c = active_conn(&mut s);
    for i in 1..=64u64 {
        assert!(is_result(&mut s, c, &ok_create(i, i + 1)), "surface {i}");
    }
    // The 65th surface exceeds MAX_SURFACES.
    assert_eq!(error_of(&s.deliver(c, &ok_create(65, 66), None)[0]), (13, 0), "LIMIT_EXCEEDED");
}

fn import(id: u64, serial: u64, w: u32, h: u32, stride: u32, offset: u64, format: u32) -> Vec<u8> {
    Request::ImportBuffer { width: w, height: h, stride, format, offset }.encode(id, serial)
}

#[test]
fn import_geometry_and_bounds() {
    let mut s = Server::new();
    let c = active_conn(&mut s);
    // Conforming import: end = 64*15 + 0 + 64 = 1024 <= cap.
    let out = s.deliver(c, &import(1, 2, 16, 16, 64, 0, 1), Some(4096));
    assert_eq!(opcode(&out[0]), Opcode::Result, "valid import");
    // Stride not a multiple of 4.
    assert_eq!(error_of(&s.deliver(c, &import(2, 3, 16, 16, 66, 0, 1), Some(4096))[0]), (8, 0));
    // Stride below width*4.
    assert_eq!(error_of(&s.deliver(c, &import(2, 4, 16, 16, 32, 0, 1), Some(4096))[0]), (8, 0));
    // Offset not a multiple of 4.
    assert_eq!(error_of(&s.deliver(c, &import(2, 5, 16, 16, 64, 2, 1), Some(4096))[0]), (8, 0));
    // Backing too small: end 1024 > cap 100.
    assert_eq!(error_of(&s.deliver(c, &import(2, 6, 16, 16, 64, 0, 1), Some(100))[0]), (11, 0));
    // Stride beyond MAX_STRIDE (16384).
    assert_eq!(error_of(&s.deliver(c, &import(2, 7, 16, 16, 20000, 0, 1), Some(4096))[0]), (13, 0));
}

#[test]
fn attach_buffer_type_checks() {
    let mut s = Server::new();
    let c = active_conn(&mut s);
    assert!(is_result(&mut s, c, &ok_create(1, 2))); // surface 1
    assert_eq!(
        opcode(&s.deliver(c, &import(2, 3, 16, 16, 64, 0, 1), Some(4096))[0]),
        Opcode::Result
    ); // buffer 2
    // Attaching the buffer to the surface passes type checks.
    let att = Request::AttachBuffer { buffer: 2, damage: alloc_vec() }.encode(1, 4);
    assert_eq!(opcode(&s.deliver(c, &att, None)[0]), Opcode::Result);
    // Attaching a surface id as if it were a buffer is a type error.
    let bad = Request::AttachBuffer { buffer: 1, damage: alloc_vec() }.encode(1, 5);
    assert_eq!(error_of(&s.deliver(c, &bad, None)[0]), (6, 0), "WRONG_OBJECT_TYPE");
}

fn alloc_vec() -> Vec<gui_protocol_v1::wire::Rect> {
    Vec::new()
}
