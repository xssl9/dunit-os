//! Deterministic replay and wire-encoding invariants (spec §2, §11): identical
//! input yields byte-identical output, server serials are monotonic, and the
//! header is fixed 32-byte little-endian with no native padding.

mod common;
use common::*;
use gui_protocol_v1::server::Server;
use gui_protocol_v1::wire::{HEADER_LEN, MAGIC};
use gui_protocol_v1::{Opcode, Request};

/// A fixed request sequence exercising negotiation, creation and import.
fn scenario(s: &mut Server) -> Vec<Vec<u8>> {
    let c = s.connect().unwrap();
    let mut out = Vec::new();
    out.extend(s.deliver(c, &hello(1), None));
    out.extend(s.deliver(
        c,
        &Request::CreateSurface { role: 1, width: 32, height: 32, format: 1 }.encode(1, 2),
        None,
    ));
    out.extend(s.deliver(
        c,
        &Request::ImportBuffer { width: 8, height: 8, stride: 32, format: 1, offset: 0 }.encode(2, 3),
        Some(4096),
    ));
    out.extend(s.deliver(c, &Request::Commit { configure: 0, frame_callback: 0 }.encode(1, 4), None));
    out
}

#[test]
fn replay_is_byte_identical() {
    let mut a = Server::new();
    let mut b = Server::new();
    assert_eq!(scenario(&mut a), scenario(&mut b), "no hidden nondeterminism");
}

#[test]
fn server_serials_are_monotonic_from_one() {
    let mut s = Server::new();
    let out = scenario(&mut s);
    // The first client's stream gets server serials 1,2,3,4 in order.
    for (i, p) in out.iter().enumerate() {
        assert_eq!(u64at(p, 24), (i + 1) as u64, "outbound serial #{i}");
    }
}

#[test]
fn header_is_fixed_little_endian() {
    let p = hello(1);
    assert_eq!(HEADER_LEN, 32);
    // magic little-endian at bytes 0..4.
    assert_eq!(&p[0..4], &MAGIC.to_le_bytes());
    // major=1, minor=0 little-endian.
    assert_eq!(u16at(&p, 4), 1);
    assert_eq!(u16at(&p, 6), 0);
    // opcode HELLO=0x0001.
    assert_eq!(u16at(&p, 8), 0x0001);
    // flags zero.
    assert_eq!(u16at(&p, 10), 0);
    // size field equals actual length, no trailing padding.
    assert_eq!(u32at(&p, 12) as usize, p.len());
}

#[test]
fn every_reply_has_matching_size_field() {
    let mut s = Server::new();
    for p in scenario(&mut s) {
        assert_eq!(u32at(&p, 12) as usize, p.len(), "size field matches length");
        assert!(p.len() >= HEADER_LEN, "at least a header");
        assert_eq!(u16at(&p, 10), 0, "reply flags are zero");
        // All replies are events (high bit set).
        assert!(u16at(&p, 8) & 0x8000 != 0, "reply opcode is an event");
    }
}

#[test]
fn welcome_is_first_reply() {
    let mut s = Server::new();
    let out = scenario(&mut s);
    assert_eq!(opcode(&out[0]), Opcode::Welcome);
}
