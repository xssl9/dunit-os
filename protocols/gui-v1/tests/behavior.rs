//! Behavioural conformance for the surface/buffer/focus/input models
//! (M2 item 4). Each test drives the headless [`Server`] over the wire and
//! decodes its replies from raw bytes, re-expressing the README §11 normative
//! scenarios as deterministic sweeps (proptest is unavailable offline). The
//! covered scenarios are §11.1–4, §11.9 and §11.10, plus a seeded byte/event
//! fuzz that asserts the server never panics and never breaks per-connection
//! server-serial monotonicity.

mod common;
use common::*;
use gui_protocol_v1::server::{ConnId, Server};
use gui_protocol_v1::wire::Rect;
use gui_protocol_v1::{Opcode, Request};

// ---- request builders ---------------------------------------------------

fn create(obj: u64, serial: u64, w: u32, h: u32, fmt: u32) -> Vec<u8> {
    Request::CreateSurface { role: 1, width: w, height: h, format: fmt }.encode(obj, serial)
}
fn import(obj: u64, serial: u64, w: u32, h: u32, fmt: u32) -> Vec<u8> {
    // Tightly-packed backing: stride = w*4, offset 0.
    Request::ImportBuffer { width: w, height: h, stride: w * 4, format: fmt, offset: 0 }
        .encode(obj, serial)
}
fn cap_of(w: u32, h: u32) -> u64 {
    (w * 4 * h) as u64
}
fn attach(obj: u64, serial: u64, buffer: u64, damage: Vec<Rect>) -> Vec<u8> {
    Request::AttachBuffer { buffer, damage }.encode(obj, serial)
}
fn commit(obj: u64, serial: u64, configure: u64, frame_callback: u32) -> Vec<u8> {
    Request::Commit { configure, frame_callback }.encode(obj, serial)
}

// ---- wire field accessors (payload begins at byte 32) -------------------

fn obj_of(p: &[u8]) -> u64 {
    u64at(p, 16)
}
fn result_req(p: &[u8]) -> u64 {
    assert_eq!(opcode(p), Opcode::Result, "expected RESULT");
    u64at(p, 32)
}

// A conformant "created + acked" surface `sid`, returning the configure token.
fn create_and_ack(s: &mut Server, c: ConnId, sid: u64, serial: &mut u64, w: u32, h: u32) -> u64 {
    let cs = *serial;
    *serial += 1;
    let out = s.deliver(c, &create(sid, cs, w, h, 1), None);
    assert_eq!(out.len(), 2, "CREATE → RESULT + CONFIGURE");
    assert_eq!(result_req(&out[0]), cs);
    assert_eq!(opcode(&out[1]), Opcode::Configure);
    let token = u64at(&out[1], 24); // header serial of CONFIGURE == configure token
    let acks = *serial;
    *serial += 1;
    let out = s.deliver(c, &Request::AckConfigure { configure: token }.encode(sid, acks), None);
    assert_eq!(result_req(&out[0]), acks);
    token
}

/// §11.1 — full happy path through map, then a composition tick completes the
/// frame callback with FRAME_DONE(PRESENTED).
#[test]
fn scenario_1_map_and_frame_done() {
    let mut s = Server::new();
    let c = active_conn(&mut s);
    let mut serial = 2; // hello consumed serial 1
    let token = create_and_ack(&mut s, c, 1, &mut serial, 32, 32);

    assert_eq!(result_req(&s.deliver(c, &import(2, serial, 32, 32, 1), Some(cap_of(32, 32)))[0]), serial);
    serial += 1;
    assert_eq!(result_req(&s.deliver(c, &attach(1, serial, 2, Vec::new()), None)[0]), serial);
    serial += 1;
    let commit_serial = serial;
    let out = s.deliver(c, &commit(1, commit_serial, token, 1), None);
    assert_eq!(out.len(), 1, "map COMMIT with callback → just RESULT");
    assert_eq!(result_req(&out[0]), commit_serial);

    // Composition tick fires the pending callback for the still-mapped surface.
    let frames = s.composite();
    assert_eq!(frames.len(), 1, "one FRAME_DONE");
    let (fc, fp) = &frames[0];
    assert_eq!(*fc, c);
    assert_eq!(opcode(fp), Opcode::FrameDone);
    assert_eq!(obj_of(fp), 1);
    assert_eq!(u64at(fp, 32), commit_serial, "FRAME_DONE.commit == COMMIT serial");
    assert_eq!(u32at(fp, 48), 0, "PRESENTED");
    // A second tick has nothing pending.
    assert!(s.composite().is_empty());
}

/// §11.2 — committing a second buffer releases the previously committed one,
/// tagged with the serial of the COMMIT that had made it current.
#[test]
fn scenario_2_replace_releases_previous() {
    let mut s = Server::new();
    let c = active_conn(&mut s);
    let mut serial = 2;
    let token = create_and_ack(&mut s, c, 1, &mut serial, 32, 32);

    // Map buffer 2 (no callback).
    s.deliver(c, &import(2, serial, 32, 32, 1), Some(cap_of(32, 32)));
    serial += 1;
    s.deliver(c, &attach(1, serial, 2, Vec::new()), None);
    serial += 1;
    let first_commit = serial;
    assert_eq!(result_req(&s.deliver(c, &commit(1, first_commit, token, 0), None)[0]), first_commit);
    serial += 1;

    // Import + attach + commit buffer 3; buffer 2 is released now.
    s.deliver(c, &import(3, serial, 32, 32, 1), Some(cap_of(32, 32)));
    serial += 1;
    s.deliver(c, &attach(1, serial, 3, Vec::new()), None);
    serial += 1;
    let out = s.deliver(c, &commit(1, serial, token, 0), None);
    assert_eq!(out.len(), 2, "RESULT + BUFFER_RELEASE(old)");
    assert_eq!(result_req(&out[0]), serial);
    assert_eq!(opcode(&out[1]), Opcode::BufferRelease);
    assert_eq!(obj_of(&out[1]), 2, "the previous buffer is released");
    assert_eq!(u64at(&out[1], 32), first_commit, "RELEASE carries its COMMIT serial");
}

/// §11.3 — a CONFIGURE injected between ACK and COMMIT supersedes the acked
/// token: the COMMIT with the stale token is BAD_SERIAL and keeps the pending
/// attach, and after re-ACKing the latest token the COMMIT succeeds.
#[test]
fn scenario_3_reconfigure_invalidates_commit_token() {
    let mut s = Server::new();
    let c = active_conn(&mut s);
    let mut serial = 2;
    let stale = create_and_ack(&mut s, c, 1, &mut serial, 32, 32);

    s.deliver(c, &import(2, serial, 32, 32, 1), Some(cap_of(32, 32)));
    serial += 1;
    s.deliver(c, &attach(1, serial, 2, Vec::new()), None);
    serial += 1;

    // DWM reconfigure (same geometry) mints a new configure token.
    let cfg = s.reconfigure(c, 1, 32, 32, 1, 1);
    assert_eq!(cfg.len(), 1);
    assert_eq!(opcode(&cfg[0].1), Opcode::Configure);
    let latest = u64at(&cfg[0].1, 24);
    assert_ne!(latest, stale);

    // COMMIT against the stale token: BAD_SERIAL, pending attach untouched.
    let out = s.deliver(c, &commit(1, serial, stale, 0), None);
    assert_eq!(error_of(&out[0]), (7, 0), "BAD_SERIAL, recoverable");
    serial += 1;

    // Re-ACK the latest token, then COMMIT succeeds using the retained pending.
    assert_eq!(result_req(&s.deliver(c, &Request::AckConfigure { configure: latest }.encode(1, serial), None)[0]), serial);
    serial += 1;
    let out = s.deliver(c, &commit(1, serial, latest, 0), None);
    assert_eq!(result_req(&out[0]), serial, "COMMIT with the fresh token succeeds");
}

/// §11.4 — destroying a surface that holds focus, a pending attach, a committed
/// buffer and a live frame callback emits, in order: the input LEAVEs, the
/// buffer releases (pending first, commit=0; then committed, with its serial),
/// the DISCARDED frame callback, then the RESULT barrier. Freeing the now-idle
/// buffers returns the namespace to baseline.
#[test]
fn scenario_4_destroy_surface_teardown_order() {
    let mut s = Server::new();
    let c = active_conn(&mut s);
    let mut serial = 2;
    let token = create_and_ack(&mut s, c, 1, &mut serial, 32, 32);

    s.deliver(c, &import(2, serial, 32, 32, 1), Some(cap_of(32, 32)));
    serial += 1;
    s.deliver(c, &attach(1, serial, 2, Vec::new()), None);
    serial += 1;
    let committed = serial;
    s.deliver(c, &commit(1, committed, token, 1), None); // maps, frame pending
    serial += 1;

    // Give the surface both input foci, then leave a second attach pending.
    assert_eq!(opcode(&s.set_pointer_focus(Some((c, 1)), 4, 5)[0].1), Opcode::PointerEnter);
    assert_eq!(opcode(&s.set_keyboard_focus(Some((c, 1)), 0)[0].1), Opcode::KeyEnter);
    s.deliver(c, &import(3, serial, 32, 32, 1), Some(cap_of(32, 32)));
    serial += 1;
    s.deliver(c, &attach(1, serial, 3, Vec::new()), None);
    serial += 1;

    let out = s.deliver(c, &Request::DestroySurface.encode(1, serial), None);
    assert_eq!(out.len(), 6, "leaves + 2 releases + discarded + result");
    assert_eq!(opcode(&out[0]), Opcode::PointerLeave);
    assert_eq!(opcode(&out[1]), Opcode::KeyLeave);
    assert_eq!((opcode(&out[2]), obj_of(&out[2]), u64at(&out[2], 32)), (Opcode::BufferRelease, 3, 0));
    assert_eq!((opcode(&out[3]), obj_of(&out[3]), u64at(&out[3], 32)), (Opcode::BufferRelease, 2, committed));
    assert_eq!((opcode(&out[4]), u32at(&out[4], 48)), (Opcode::FrameDone, 1), "DISCARDED");
    assert_eq!(result_req(&out[5]), serial);
    serial += 1;

    // Both buffers are Available again; destroying them clears the namespace.
    assert_eq!(result_req(&s.deliver(c, &Request::DestroyBuffer.encode(2, serial), None)[0]), serial);
    serial += 1;
    assert_eq!(result_req(&s.deliver(c, &Request::DestroyBuffer.encode(3, serial), None)[0]), serial);
    assert_eq!(s.connection(c).unwrap().object_count(), 0, "back to baseline");
}

/// §11.9 — a committed (BUSY) buffer can be neither re-attached nor destroyed,
/// and a peer disconnect frees its objects and focus without delivering
/// anything.
#[test]
fn scenario_9_busy_buffer_and_disconnect() {
    let mut s = Server::new();
    let c = active_conn(&mut s);
    let mut serial = 2;
    let token = create_and_ack(&mut s, c, 1, &mut serial, 32, 32);

    s.deliver(c, &import(2, serial, 32, 32, 1), Some(cap_of(32, 32)));
    serial += 1;
    s.deliver(c, &attach(1, serial, 2, Vec::new()), None);
    serial += 1;
    s.deliver(c, &commit(1, serial, token, 0), None); // buffer 2 is now BUSY
    serial += 1;

    // Re-attaching the BUSY buffer is rejected; a frame callback never frees it.
    assert_eq!(error_of(&s.deliver(c, &attach(1, serial, 2, Vec::new()), None)[0]), (12, 0), "BUSY");
    serial += 1;
    // Destroying a BUSY buffer is BUSY, not a deferred free.
    assert_eq!(error_of(&s.deliver(c, &Request::DestroyBuffer.encode(2, serial), None)[0]), (12, 0));

    // Focus, then a transport-level disconnect: no packets, no dangling focus.
    s.set_pointer_focus(Some((c, 1)), 0, 0);
    s.disconnect_peer(c);
    assert!(s.connection(c).is_none(), "connection and its objects gone");
    assert!(s.pointer_motion(9, 9).is_empty(), "focus released without delivery");
}

/// Map surface `sid` backed by buffer `bid` on connection `c` (no callback).
fn map(s: &mut Server, c: ConnId, sid: u64, bid: u64, serial: &mut u64, w: u32, h: u32) {
    let token = create_and_ack(s, c, sid, serial, w, h);
    s.deliver(c, &import(bid, *serial, w, h, 1), Some(cap_of(w, h)));
    *serial += 1;
    s.deliver(c, &attach(sid, *serial, bid, Vec::new()), None);
    *serial += 1;
    s.deliver(c, &commit(sid, *serial, token, 0), None);
    *serial += 1;
}

/// §11.10 — the single seat routes input to exactly one focused client:
/// focus hand-off emits paired LEAVE/ENTER, an implicit grab pins routing to the
/// grabbed surface (even for out-of-bounds motion), key repeat/release honour
/// the held-key set, and destroying the focused surface leaves cleanly. No event
/// ever reaches the other client.
#[test]
fn scenario_10_single_seat_focus_and_input() {
    let mut s = Server::new();
    let a = active_conn(&mut s);
    let mut sa = 2;
    map(&mut s, a, 1, 2, &mut sa, 32, 32);
    let b = active_conn(&mut s);
    let mut sb = 2;
    map(&mut s, b, 1, 2, &mut sb, 32, 32);

    // Focus A, then hand off to B: LEAVE(A) then ENTER(B).
    let out = s.set_pointer_focus(Some((a, 1)), 1, 2);
    assert_eq!((out[0].0, opcode(&out[0].1)), (a, Opcode::PointerEnter));
    let out = s.set_pointer_focus(Some((b, 1)), 3, 4);
    assert_eq!((out[0].0, opcode(&out[0].1)), (a, Opcode::PointerLeave));
    assert_eq!((out[1].0, opcode(&out[1].1)), (b, Opcode::PointerEnter));

    // Implicit grab on B: focus changes are ignored and motion stays on B.
    assert_eq!(s.pointer_button(0, true)[0].0, b);
    assert!(s.set_pointer_focus(Some((a, 1)), 0, 0).is_empty(), "grab pins focus");
    let out = s.pointer_motion(100_000, -100_000);
    assert_eq!((out[0].0, opcode(&out[0].1)), (b, Opcode::PointerMotion), "grab keeps out-of-bounds motion on B");
    assert_eq!(s.pointer_button(0, false)[0].0, b); // release ends grab

    // Now focus A for keyboard input.
    s.set_pointer_focus(Some((a, 1)), 0, 0);
    assert_eq!((s.set_keyboard_focus(Some((a, 1)), 1)[0]).0, a);
    // Fresh press, then a held repeat, then release; a second release is dropped.
    let out = s.key(30, true, 1, false);
    assert_eq!((out[0].0, u32at(&out[0].1, 44), u32at(&out[0].1, 52)), (a, 1, 0), "press, repeat=0");
    assert_eq!(u32at(&s.key(30, true, 1, true)[0].1, 52), 1, "held repeat=1");
    assert_eq!(u32at(&s.key(30, false, 1, false)[0].1, 44), 0, "release pressed=0");
    assert!(s.key(30, false, 1, false).is_empty(), "release of unheld key suppressed");

    // UTF-8 text goes only to A.
    let out = s.text_input("héllo");
    assert_eq!((out[0].0, opcode(&out[0].1)), (a, Opcode::TextInput));

    // Destroying the focused surface emits its LEAVEs, releases its buffer, then
    // RESULT — and nothing lands on B.
    let out = s.deliver(a, &Request::DestroySurface.encode(1, sa), None);
    assert!(out.iter().all(|p| opcode(p) != Opcode::Error));
    assert_eq!(opcode(&out[0]), Opcode::PointerLeave);
    assert_eq!(opcode(&out[1]), Opcode::KeyLeave);
    assert!(s.pointer_motion(1, 1).is_empty());
    assert!(s.key(31, true, 0, false).is_empty());
}

// ---- seeded fuzz --------------------------------------------------------

/// Small deterministic xorshift PRNG (no external crates offline).
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// Every emitted packet must be well-framed and carry a strictly increasing
/// server serial within its connection (spec §2, §11 determinism).
fn track(pairs: &[(ConnId, Vec<u8>)], last: &mut std::collections::BTreeMap<ConnId, u64>) {
    for (cid, p) in pairs {
        assert!(p.len() >= 32, "framed packet");
        let ser = u64at(p, 24);
        let e = last.entry(*cid).or_insert(0);
        assert!(ser > *e, "server serial strictly increasing per connection");
        *e = ser;
    }
}

#[test]
fn fuzz_no_panic_and_serials_monotonic() {
    for seed in 1..=48u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let mut s = Server::new();
        let mut conns: Vec<ConnId> = Vec::new();
        let mut cser: Vec<u64> = Vec::new();
        let mut last = std::collections::BTreeMap::new();
        let mut now = 0u64;

        for _ in 0..400 {
            let pick = |rng: &mut Rng, conns: &[ConnId]| -> Option<usize> {
                if conns.is_empty() { None } else { Some(rng.below(conns.len() as u64) as usize) }
            };
            match rng.below(13) {
                0 => {
                    if let Some(c) = s.connect() {
                        conns.push(c);
                        cser.push(1);
                    }
                }
                op @ 1..=6 => {
                    let Some(i) = pick(&mut rng, &conns) else { continue };
                    let (c, ser) = (conns[i], cser[i]);
                    cser[i] += 1;
                    let oid = 1 + rng.below(4);
                    let (packet, cap) = match op {
                        1 => (hello(ser), None),
                        2 => (create(oid, ser, 1 + rng.below(64) as u32, 1 + rng.below(64) as u32, 1 + rng.below(3) as u32), None),
                        3 => {
                            let (w, h) = (1 + rng.below(32) as u32, 1 + rng.below(32) as u32);
                            (import(oid, ser, w, h, 1), Some(cap_of(w, h)))
                        }
                        4 => (attach(oid, ser, rng.below(5), Vec::new()), None),
                        5 => (commit(oid, ser, rng.below(6), (rng.below(2)) as u32), None),
                        _ => {
                            if rng.below(2) == 0 {
                                (Request::DestroySurface.encode(oid, ser), None)
                            } else {
                                (Request::DestroyBuffer.encode(oid, ser), None)
                            }
                        }
                    };
                    let out = s.deliver(c, &packet, cap);
                    let pairs: Vec<(ConnId, Vec<u8>)> = out.into_iter().map(|p| (c, p)).collect();
                    track(&pairs, &mut last);
                }
                7 => {
                    let tgt = pick(&mut rng, &conns).map(|i| (conns[i], 1 + rng.below(4)));
                    track(&s.set_pointer_focus(tgt, rng.next() as i32, rng.next() as i32), &mut last);
                }
                8 => {
                    let out = match rng.below(3) {
                        0 => s.pointer_motion(rng.next() as i32, rng.next() as i32),
                        1 => s.pointer_button(rng.below(6) as u32, rng.below(2) == 0),
                        _ => s.pointer_axis(rng.next() as i32, rng.next() as i32),
                    };
                    track(&out, &mut last);
                }
                9 => {
                    let tgt = pick(&mut rng, &conns).map(|i| (conns[i], 1 + rng.below(4)));
                    track(&s.set_keyboard_focus(tgt, rng.next() as u32), &mut last);
                }
                10 => {
                    let out = s.key(rng.below(300) as u32, rng.below(2) == 0, rng.next() as u32, rng.below(2) == 0);
                    track(&out, &mut last);
                }
                11 => {
                    if rng.below(2) == 0 {
                        track(&s.text_input("αβγ"), &mut last);
                    } else if let Some(i) = pick(&mut rng, &conns) {
                        track(&s.reconfigure(conns[i], 1 + rng.below(4), 1 + rng.below(64) as u32, 1 + rng.below(64) as u32, 1, rng.next() as u32), &mut last);
                    }
                }
                _ => {
                    if rng.below(2) == 0 {
                        now += rng.below(3_000_000_000);
                        s.tick(now);
                    } else {
                        track(&s.composite(), &mut last);
                    }
                }
            }
        }
    }
}









