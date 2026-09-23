//! Shared helpers for the host protocol tests. These build conformant request
//! packets and decode the server's replies straight from the wire bytes, so the
//! tests exercise the real codec end to end.
#![allow(dead_code)] // each test binary uses a different subset of these.

use gui_protocol_v1::server::{ConnId, Server};
use gui_protocol_v1::wire::FEATURE_ARGB8888;
use gui_protocol_v1::{Opcode, Request};

/// Read a little-endian field from a packet at `off`.
pub fn u16at(p: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([p[off], p[off + 1]])
}
pub fn u32at(p: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([p[off], p[off + 1], p[off + 2], p[off + 3]])
}
pub fn u64at(p: &[u8], off: usize) -> u64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&p[off..off + 8]);
    u64::from_le_bytes(a)
}

/// Opcode of an outbound packet.
pub fn opcode(p: &[u8]) -> Opcode {
    Opcode::from_u16(u16at(p, 8)).expect("known opcode")
}

/// `(code, fatal)` of an ERROR packet.
pub fn error_of(p: &[u8]) -> (u32, u32) {
    assert_eq!(opcode(p), Opcode::Error, "expected ERROR packet");
    (u32at(p, 40), u32at(p, 44))
}

/// A HELLO offering ARGB8888 and requiring nothing.
pub fn hello(serial: u64) -> Vec<u8> {
    Request::Hello {
        min_minor: 0,
        max_minor: 0,
        offered_features: FEATURE_ARGB8888,
        required_features: 0,
    }
    .encode(0, serial)
}

/// Connect and complete negotiation; returns the active connection id.
pub fn active_conn(s: &mut Server) -> ConnId {
    let c = s.connect().expect("connect");
    let out = s.deliver(c, &hello(1), None);
    assert_eq!(out.len(), 1);
    assert_eq!(opcode(&out[0]), Opcode::Welcome);
    c
}
