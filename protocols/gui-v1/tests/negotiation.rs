//! Connection negotiation, serial, opcode-direction, policy range and timeout
//! (spec §2, §3, §9, §10).

mod common;
use common::*;
use gui_protocol_v1::server::Server;
use gui_protocol_v1::wire::{FEATURE_ARGB8888, FORMATS_ARGB8888, FORMATS_XRGB8888, MAGIC};
use gui_protocol_v1::{Opcode, Request};

/// Craft a header-only packet with an arbitrary raw opcode (for direction /
/// policy / unknown-opcode tests).
fn raw(opcode: u16, serial: u64) -> Vec<u8> {
    let mut p = vec![0u8; 32];
    p[0..4].copy_from_slice(&MAGIC.to_le_bytes());
    p[4..6].copy_from_slice(&1u16.to_le_bytes()); // major
    p[8..10].copy_from_slice(&opcode.to_le_bytes());
    p[12..16].copy_from_slice(&32u32.to_le_bytes()); // size
    p[24..32].copy_from_slice(&serial.to_le_bytes());
    p
}

#[test]
fn welcome_fields() {
    let mut s = Server::new();
    let c = s.connect().unwrap();
    let out = s.deliver(c, &hello(1), None);
    let p = &out[0];
    assert_eq!(opcode(p), Opcode::Welcome);
    assert_eq!(u64at(p, 32), 1, "request echoes HELLO serial");
    assert_eq!(u64at(p, 40), 1, "client_id starts at 1");
    assert_eq!(u64at(p, 48), FEATURE_ARGB8888, "negotiated features");
    assert_eq!(u32at(p, 56), FORMATS_XRGB8888 | FORMATS_ARGB8888, "formats");
}

#[test]
fn client_id_is_monotonic() {
    let mut s = Server::new();
    let a = s.connect().unwrap();
    let b = s.connect().unwrap();
    assert_eq!(u64at(&s.deliver(a, &hello(1), None)[0], 40), 1);
    assert_eq!(u64at(&s.deliver(b, &hello(1), None)[0], 40), 2);
}

#[test]
fn version_no_intersection() {
    let mut s = Server::new();
    let c = s.connect().unwrap();
    let hi = Request::Hello {
        min_minor: 1,
        max_minor: 2,
        offered_features: 0,
        required_features: 0,
    }
    .encode(0, 1);
    assert_eq!(error_of(&s.deliver(c, &hi, None)[0]), (2, 1), "VERSION_MISMATCH fatal");
}

#[test]
fn required_not_subset_of_offered() {
    let mut s = Server::new();
    let c = s.connect().unwrap();
    let h = Request::Hello {
        min_minor: 0,
        max_minor: 0,
        offered_features: 0,
        required_features: FEATURE_ARGB8888,
    }
    .encode(0, 1);
    assert_eq!(error_of(&s.deliver(c, &h, None)[0]), (1, 1), "MALFORMED fatal");
}

#[test]
fn required_unsupported_feature() {
    let mut s = Server::new();
    let c = s.connect().unwrap();
    let bit5 = 1u64 << 5;
    let h = Request::Hello {
        min_minor: 0,
        max_minor: 0,
        offered_features: bit5,
        required_features: bit5,
    }
    .encode(0, 1);
    assert_eq!(error_of(&s.deliver(c, &h, None)[0]), (9, 1), "UNSUPPORTED fatal");
}

#[test]
fn unknown_offered_features_ignored() {
    let mut s = Server::new();
    let c = s.connect().unwrap();
    let h = Request::Hello {
        min_minor: 0,
        max_minor: 0,
        offered_features: FEATURE_ARGB8888 | (1 << 7),
        required_features: 0,
    }
    .encode(0, 1);
    let out = s.deliver(c, &h, None);
    assert_eq!(u64at(&out[0], 48), FEATURE_ARGB8888, "only supported bits survive");
}

#[test]
fn serial_must_be_nonzero_and_increasing() {
    let mut s = Server::new();
    let c = s.connect().unwrap();
    assert_eq!(error_of(&s.deliver(c, &hello(0), None)[0]), (7, 1), "serial 0 fatal");

    let mut s = Server::new();
    let c = active_conn(&mut s); // consumes serial 1
    let create = Request::CreateSurface { role: 1, width: 4, height: 4, format: 1 }.encode(1, 1);
    assert_eq!(error_of(&s.deliver(c, &create, None)[0]), (7, 1), "repeat serial fatal");
}

#[test]
fn wrong_state_in_new_is_fatal() {
    let mut s = Server::new();
    let c = s.connect().unwrap();
    let create = Request::CreateSurface { role: 1, width: 4, height: 4, format: 1 }.encode(1, 1);
    assert_eq!(error_of(&s.deliver(c, &create, None)[0]), (4, 1), "BAD_STATE fatal in NEW");
}

#[test]
fn repeat_hello_in_active_is_recoverable() {
    let mut s = Server::new();
    let c = active_conn(&mut s);
    let out = s.deliver(c, &hello(2), None);
    assert_eq!(error_of(&out[0]), (4, 0), "BAD_STATE recoverable");
    // Still usable.
    let create = Request::CreateSurface { role: 1, width: 4, height: 4, format: 1 }.encode(1, 3);
    assert_eq!(opcode(&s.deliver(c, &create, None)[0]), Opcode::Result);
}

#[test]
fn disconnect_closes() {
    let mut s = Server::new();
    let c = active_conn(&mut s);
    let out = s.deliver(c, &Request::Disconnect.encode(0, 2), None);
    assert_eq!(opcode(&out[0]), Opcode::Result);
    assert!(s.deliver(c, &hello(3), None).is_empty(), "closed after DISCONNECT");
}

#[test]
fn opcode_direction_and_unknown() {
    let mut s = Server::new();
    let c = active_conn(&mut s);
    // An event opcode (0x8002 RESULT) sent by a client: wrong direction.
    assert_eq!(error_of(&s.deliver(c, &raw(0x8002, 2), None)[0]), (3, 1), "BAD_OPCODE");
    let mut s = Server::new();
    let c = active_conn(&mut s);
    // Unknown opcode outside the policy range.
    assert_eq!(error_of(&s.deliver(c, &raw(0x0099, 2), None)[0]), (3, 1), "BAD_OPCODE");
}

#[test]
fn reserved_policy_range_is_access_denied() {
    let mut s = Server::new();
    let c = active_conn(&mut s);
    let out = s.deliver(c, &raw(0x1500, 2), None);
    assert_eq!(error_of(&out[0]), (10, 0), "ACCESS_DENIED recoverable");
    // Connection survives a policy-range attempt.
    let create = Request::CreateSurface { role: 1, width: 4, height: 4, format: 1 }.encode(1, 3);
    assert_eq!(opcode(&s.deliver(c, &create, None)[0]), Opcode::Result);
}

#[test]
fn hello_timeout_closes_new_connection() {
    let mut s = Server::new();
    let c = s.connect().unwrap();
    let closed = s.tick(6_000_000_000); // > 5s
    assert!(closed.contains(&c));
    assert!(s.deliver(c, &hello(1), None).is_empty(), "timed-out connection is gone");
}
