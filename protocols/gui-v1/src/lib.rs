//! Dunit GUI protocol v1.0 — headless reference implementation.
//!
//! This crate implements the wire contract specified in
//! `protocols/gui-v1/README.md`. It is the deliverable for the M2 roadmap item
//! *"Реализовать host/headless protocol tests без framebuffer"*: a pure,
//! deterministic model of the protocol that runs on the host (no framebuffer,
//! no real IPC, no wall clock) and is exercised by the test suite in `tests/`.
//!
//! Scope of this milestone slice:
//!
//! * [`wire`] — complete codec and framing validation for every v1.0 request,
//!   reply and event, plus the constant/enum tables from the spec.
//! * [`server`] — a headless reference [`server::Server`] covering the
//!   *connection* layer end to end (negotiation state machine, version/feature
//!   negotiation, HELLO timeout via an injected monotonic clock, DISCONNECT,
//!   client-request serial monotonicity, opcode direction and the reserved DWM
//!   policy range) and the shared object-namespace rules (fresh-ID
//!   monotonicity, existence/type tracking, stale/reused/cross-connection ID
//!   rejection). Surface visibility, buffer ownership, configure/commit and
//!   focus/input *behaviour* are deliberately deferred to M2 item 4 and are
//!   documented as such at their call sites; nothing here silently returns a
//!   wrong result for them.
//! * [`wm`] — a pure window-management + software-composition model ported from
//!   the `userspace/display_server` prototype (its `egui` binary lifecycle is
//!   dropped): a window stack, focus policy, top-most hit-testing, drag
//!   interaction and a CPU compositor painting into an ARGB `Vec<u32>`. Kept
//!   headless and deterministic; it is the substrate the focus/input models of
//!   M2 item 4 build on.
//!
//! Determinism: the server never reads a wall clock or a host pointer. Time
//! advances only through [`server::Server::tick`], and identifiers are handed
//! out monotonically, so identical input byte streams and injected events
//! produce identical output byte streams and resource counters (spec §11).

#![no_std]

extern crate alloc;

pub mod server;
pub mod wire;
pub mod wm;

pub use wire::{
    ErrorCode, Event, Header, Opcode, Rect, Request, WireError, HEADER_LEN, MAGIC, MAX_PACKET,
    MIN_PACKET, VERSION_MAJOR, VERSION_MINOR,
};
