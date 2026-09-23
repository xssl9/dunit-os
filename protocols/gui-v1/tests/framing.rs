//! Framing and malformed-input rejection (spec §2, §10, scenario §11.5).
//! An invalid client must never destabilise the server and, on a fatal framing
//! error, the connection is torn down.

mod common;
use common::*;
use gui_protocol_v1::server::Server;
use gui_protocol_v1::wire::FEATURE_ARGB8888;
use gui_protocol_v1::Request;

const MALFORMED: u32 = 1;

/// Every fatal framing error yields exactly one ERROR and closes the peer,
/// while other connections keep working.
fn assert_fatal_malformed(packet: &[u8], cap: Option<u64>) {
    let mut s = Server::new();
    let victim = s.connect().unwrap();
    let bystander = active_conn(&mut s);

    let out = s.deliver(victim, packet, cap);
    assert_eq!(out.len(), 1, "one ERROR expected");
    assert_eq!(error_of(&out[0]), (MALFORMED, 1), "MALFORMED, fatal");

    // Victim is closed: further packets are dropped.
    assert!(s.deliver(victim, &hello(2), None).is_empty());
    // Bystander is unaffected: a create still validates normally.
    let create = Request::CreateSurface {
        role: 1,
        width: 100,
        height: 100,
        format: 1,
    }
    .encode(1, 2);
    let out = s.deliver(bystander, &create, None);
    assert_eq!(opcode(&out[0]), gui_protocol_v1::Opcode::Result);
}

#[test]
fn short_header() {
    assert_fatal_malformed(&[0u8; 10], None);
}

#[test]
fn oversized_packet() {
    // 300 > 256: rejected on length before anything else is trusted.
    assert_fatal_malformed(&vec![0u8; 300], None);
}

#[test]
fn bad_magic() {
    let mut p = hello(1);
    p[0] ^= 0xff;
    assert_fatal_malformed(&p, None);
}

#[test]
fn nonzero_flags() {
    let mut p = hello(1);
    p[10] = 1; // flags
    assert_fatal_malformed(&p, None);
}

#[test]
fn size_mismatch_trailing_bytes() {
    let mut p = hello(1);
    p.push(0); // extra byte: size field no longer matches length
    assert_fatal_malformed(&p, None);
}

#[test]
fn nonzero_reserved_in_payload() {
    let mut p = hello(1);
    p[36] = 0x01; // HELLO reserved:u32 lives at payload offset 4 == byte 36
    assert_fatal_malformed(&p, None);
}

#[test]
fn unexpected_capability() {
    // A capability is legal only on IMPORT_BUFFER.
    assert_fatal_malformed(&hello(1), Some(4096));
}

#[test]
fn import_without_capability() {
    let import = Request::ImportBuffer {
        width: 16,
        height: 16,
        stride: 64,
        format: 1,
        offset: 0,
    }
    .encode(1, 1);
    // Missing the required capability is a framing error too.
    let mut s = Server::new();
    let c = active_conn(&mut s);
    let out = s.deliver(c, &import, None);
    assert_eq!(error_of(&out[0]), (MALFORMED, 1));
}

#[test]
fn bad_utf8_title_is_recoverable() {
    // BAD_VALUE (not MALFORMED) and the connection survives (spec §10).
    let mut s = Server::new();
    let c = active_conn(&mut s);
    let create = Request::CreateSurface {
        role: 1,
        width: 10,
        height: 10,
        format: 1,
    }
    .encode(1, 2);
    assert_eq!(opcode(&s.deliver(c, &create, None)[0]), gui_protocol_v1::Opcode::Result);

    // Hand-build a SET_TITLE whose string bytes are invalid UTF-8.
    let mut p = Request::SetTitle(String::new()).encode(1, 3);
    // payload: length:u16 then bytes. Set length=1 and append a lone 0xff.
    let len_off = 32;
    p[len_off] = 1;
    p[len_off + 1] = 0;
    p.push(0xff);
    // fix size field
    let size = p.len() as u32;
    p[12..16].copy_from_slice(&size.to_le_bytes());

    let out = s.deliver(c, &p, None);
    assert_eq!(error_of(&out[0]), (8, 0), "BAD_VALUE, recoverable");
    // Connection still alive.
    assert!(!s.deliver(c, &hello(99), None).is_empty());
    let _ = FEATURE_ARGB8888;
}
