//! Headless reference server for Dunit GUI protocol v1.0.
//!
//! # Milestone scope
//!
//! This models, deterministically and without a framebuffer:
//!
//! * the **connection** state machine — negotiation (`HELLO`/`WELCOME`),
//!   version/feature selection, the `NEW` HELLO timeout and `DISCONNECT`
//!   (spec §3), client-request **serial** monotonicity, **opcode** direction
//!   and the reserved DWM policy range (spec §2, §9);
//! * the shared **object namespace** — surfaces/buffers, fresh-ID monotonicity,
//!   no reuse, existence/type checks, cross-connection isolation and the
//!   structural/bounds validation of every request (spec §2, §7, §8);
//! * (M2 item 4) the **behavioural** models: the surface
//!   `UNMAPPED↔MAPPED→DESTROYED` lifecycle with the configure/ack/commit token
//!   protocol, buffer ownership (`AVAILABLE→PENDING→BUSY` + `BUFFER_RELEASE`),
//!   frame callbacks completed at a composition tick, and the single-seat
//!   focus/input router (pointer & keyboard enter/leave, motion/button/axis,
//!   key/text, implicit grab). Policy decisions (focus assignment, reconfigure)
//!   enter the model as verified external events (spec §9), never from a host
//!   pointer or wall clock.
//!
//! One deferral remains, flagged at its call site: duplicate-import detection
//! (spec §7) needs a kernel memory-object identity that the abstract transport
//! shim here does not carry, so it is left to the M3 kernel binding.
//!
//! Determinism: time advances only via [`Server::tick`]; `client_id`s and
//! serials are handed out monotonically, and every emitted event is a pure
//! function of the injected request/event stream (spec §11).

#![allow(clippy::too_many_arguments)]

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use crate::wire::{
    ErrorCode, Event, Header, Opcode, Request, DIM_MAX, DIM_MIN, FEATURE_ARGB8888,
    FORMATS_ARGB8888, FORMATS_XRGB8888, FORMAT_ARGB8888, FORMAT_XRGB8888, MAX_DAMAGE_RECTS,
    POLICY_OPCODE_HI, POLICY_OPCODE_LO, ROLE_TOPLEVEL, VERSION_MAJOR, VERSION_MINOR,
};

/// Transport-level connection identifier assigned by [`Server::connect`].
pub type ConnId = u32;
/// Client-chosen object identifier (surfaces and buffers share one namespace).
pub type ObjectId = u64;
/// A global input target: the connection owning a surface, and its object id.
pub type Target = (ConnId, ObjectId);

const HELLO_TIMEOUT_NS: u64 = 5_000_000_000;
const MAX_CONNECTIONS: usize = 64;
const MAX_SURFACES: u32 = 64;
const MAX_BUFFERS: u32 = 192;
/// Per-object backing maximum (spec §8).
const MAX_BACKING_BYTES: u64 = 64 * 1024 * 1024;
/// Per-connection pinned backing budget (spec §8).
const MAX_BACKING_BYTES_CONN: u64 = 256 * 1024 * 1024;
const MAX_STRIDE: u32 = 16384;
const MAX_TITLE_BYTES: usize = 160;
const MAX_APP_ID_BYTES: usize = 128;
const MAX_TEXT_BYTES: usize = 160;
/// Simultaneously-held key usages before input overflow (spec §8).
const MAX_HELD_KEYS: usize = 256;

/// FRAME_DONE status values (spec §5).
const FRAME_PRESENTED: u32 = 0;
const FRAME_DISCARDED: u32 = 1;
/// CONFIGURE state bit 0 = activated (spec §5).
const STATE_ACTIVATED: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ObjKind {
    Surface,
    Buffer,
}

/// Buffer ownership state (spec §7): `AVAILABLE → PENDING → BUSY`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BufState {
    Available,
    Pending,
    Busy,
}

/// A CPU shared buffer imported by a client (spec §7).
#[derive(Debug, Clone, Copy)]
struct Buffer {
    width: u32,
    height: u32,
    format: u32,
    /// Pinned backing size in bytes, returned to the budget on destroy.
    backing: u64,
    state: BufState,
    /// COMMIT serial that made this buffer BUSY (reported on RELEASE); else 0.
    commit_token: u64,
}

/// A managed toplevel surface with its configure/commit lifecycle (spec §6).
#[derive(Debug, Clone, Copy)]
struct Surface {
    /// Immutable pixel format chosen at creation.
    format: u32,
    /// Latest configured logical content size / scale / state.
    width: u32,
    height: u32,
    scale: u32,
    state: u32,
    /// Header serial of the latest CONFIGURE (the current configure token).
    latest_token: u64,
    /// Token of the last accepted ACK_CONFIGURE (0 = none).
    acked_token: u64,
    /// A pending ATTACH is reserved until the next COMMIT/DESTROY.
    pending_attach: bool,
    /// Buffer reserved by the pending ATTACH; 0 encodes an unmap attach.
    pending_buffer: ObjectId,
    /// Currently committed buffer (0 = none / unmapped).
    committed_buffer: ObjectId,
    /// COMMIT serial of the committed content (reported on its RELEASE).
    committed_token: u64,
    mapped: bool,
    /// A frame callback awaiting the next composition tick.
    frame_pending: bool,
    /// COMMIT serial to report in the pending FRAME_DONE.
    frame_token: u64,
}

impl Surface {
    fn new(format: u32, width: u32, height: u32, token: u64) -> Surface {
        Surface {
            format,
            width,
            height,
            scale: 1,
            state: STATE_ACTIVATED,
            latest_token: token,
            acked_token: 0,
            pending_attach: false,
            pending_buffer: 0,
            committed_buffer: 0,
            committed_token: 0,
            mapped: false,
            frame_pending: false,
            frame_token: 0,
        }
    }
    /// Whether `token` names the latest CONFIGURE and it has been acked (spec §6).
    fn configure_ready(&self, token: u64) -> bool {
        token == self.latest_token && self.acked_token == self.latest_token
    }
}

#[derive(Debug, Clone, Copy)]
enum Object {
    Surface(Surface),
    Buffer(Buffer),
}

impl Object {
    fn kind(&self) -> ObjKind {
        match self {
            Object::Surface(_) => ObjKind::Surface,
            Object::Buffer(_) => ObjKind::Buffer,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// Awaiting `HELLO`.
    New,
    /// `HELLO`/`WELCOME` complete.
    Active,
    /// Terminated; further inbound packets are ignored.
    Closed,
}

/// Per-connection state.
pub struct Connection {
    phase: Phase,
    created_ns: u64,
    /// Highest client request serial accepted (0 = none); strictly increasing.
    last_client_serial: u64,
    /// Independent server serial sequence for every reply and event (spec §2).
    next_server_serial: u64,
    features: u64,
    client_id: u64,
    /// Shared surface/buffer namespace of this connection.
    objects: BTreeMap<ObjectId, Object>,
    /// Highest object ID ever created; new IDs must exceed it.
    id_watermark: ObjectId,
    surfaces: u32,
    buffers: u32,
    /// Pinned backing bytes across live buffers (spec §8 per-connection budget).
    backing_bytes: u64,
}

impl Connection {
    fn new(now_ns: u64) -> Connection {
        Connection {
            phase: Phase::New,
            created_ns: now_ns,
            last_client_serial: 0,
            next_server_serial: 1,
            features: 0,
            client_id: 0,
            objects: BTreeMap::new(),
            id_watermark: 0,
            surfaces: 0,
            buffers: 0,
            backing_bytes: 0,
        }
    }

    fn argb_enabled(&self) -> bool {
        self.features & FEATURE_ARGB8888 != 0
    }

    /// Whether the connection is finished and should be dropped by the caller.
    pub fn is_closed(&self) -> bool {
        self.phase == Phase::Closed
    }

    /// Number of live objects — used by tests to check namespace bookkeeping.
    pub fn object_count(&self) -> usize {
        self.objects.len()
    }

    /// Object existence + type check (spec §10 stage 6).
    fn require(&self, id: ObjectId, kind: ObjKind) -> Result<(), ErrorCode> {
        match self.objects.get(&id) {
            None => Err(ErrorCode::StaleObject),
            Some(o) if o.kind() != kind => Err(ErrorCode::WrongObjectType),
            Some(_) => Ok(()),
        }
    }

    /// Fresh-ID rule for a creation request (spec §2): nonzero and strictly
    /// greater than any previously created ID (which also rules out reuse).
    fn check_fresh(&self, id: ObjectId) -> Result<(), ErrorCode> {
        if id == 0 || id <= self.id_watermark {
            Err(ErrorCode::BadValue)
        } else {
            Ok(())
        }
    }

    fn surface_of(&self, id: ObjectId) -> Result<Surface, ErrorCode> {
        match self.objects.get(&id) {
            None => Err(ErrorCode::StaleObject),
            Some(Object::Surface(s)) => Ok(*s),
            Some(_) => Err(ErrorCode::WrongObjectType),
        }
    }
    fn buffer_of(&self, id: ObjectId) -> Result<Buffer, ErrorCode> {
        match self.objects.get(&id) {
            None => Err(ErrorCode::StaleObject),
            Some(Object::Buffer(b)) => Ok(*b),
            Some(_) => Err(ErrorCode::WrongObjectType),
        }
    }
    fn put_surface(&mut self, id: ObjectId, s: Surface) {
        self.objects.insert(id, Object::Surface(s));
    }
    fn put_buffer(&mut self, id: ObjectId, b: Buffer) {
        self.objects.insert(id, Object::Buffer(b));
    }
}

fn check_format(fmt: u32, argb_enabled: bool) -> Result<(), ErrorCode> {
    match fmt {
        FORMAT_XRGB8888 => Ok(()),
        FORMAT_ARGB8888 if argb_enabled => Ok(()),
        FORMAT_ARGB8888 => Err(ErrorCode::Unsupported),
        _ => Err(ErrorCode::BadValue),
    }
}

fn app_id_ok(bytes: &[u8]) -> bool {
    (1..=MAX_APP_ID_BYTES).contains(&bytes.len())
        && bytes
            .iter()
            .all(|&b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

fn dim_ok(v: u32) -> bool {
    v >= DIM_MIN && v <= DIM_MAX
}

/// The single input seat shared by the whole server (spec §9): at most one
/// pointer focus and one keyboard focus, an implicit pointer grab while any
/// button is held, and the held key/button sets used for overflow and
/// release-suppression bookkeeping.
#[derive(Default)]
struct Seat {
    pointer_focus: Option<Target>,
    keyboard_focus: Option<Target>,
    grab: Option<Target>,
    buttons: Vec<u32>,
    held_keys: Vec<u32>,
    modifiers: u32,
}

impl Seat {
    /// Drop all focus/grab bindings held by a vanished connection, with no
    /// LEAVE (the peer is gone — spec §9 local cleanup).
    fn drop_conn(&mut self, conn: ConnId) {
        if matches!(self.pointer_focus, Some((c, _)) if c == conn) {
            self.pointer_focus = None;
            self.buttons.clear();
        }
        if matches!(self.grab, Some((c, _)) if c == conn) {
            self.grab = None;
            self.buttons.clear();
        }
        if matches!(self.keyboard_focus, Some((c, _)) if c == conn) {
            self.keyboard_focus = None;
            self.held_keys.clear();
            self.modifiers = 0;
        }
    }

    /// Drop focus/grab for one surface, emitting the LEAVE(s) that must precede
    /// its removal from the scene (spec §6, §9). Events target the surface id.
    fn drop_surface(&mut self, tgt: Target, out: &mut Vec<(ObjectId, Event)>) {
        if self.pointer_focus == Some(tgt) {
            out.push((tgt.1, Event::PointerLeave));
            self.pointer_focus = None;
            self.buttons.clear();
        }
        if self.grab == Some(tgt) {
            self.grab = None;
            self.buttons.clear();
        }
        if self.keyboard_focus == Some(tgt) {
            out.push((tgt.1, Event::KeyLeave));
            self.keyboard_focus = None;
            self.held_keys.clear();
            self.modifiers = 0;
        }
    }
}

impl Server {
    /// Semantic dispatch (spec §10 stages 5–11). Emits the reply plus any causal
    /// events for the requesting connection; cross-connection input/frame events
    /// are produced by the dedicated injection methods instead.
    fn dispatch(
        c: &mut Connection,
        ncid: &mut u64,
        seat: &mut Seat,
        conn_id: ConnId,
        now_ns: u64,
        header: &Header,
        req: Request,
        cap: Option<u64>,
    ) -> Emit {
        let obj = header.object;
        let ser = header.serial;
        let ok = (one(obj, Event::Result { request: ser }), false);

        match req {
            Request::Hello {
                min_minor,
                max_minor,
                offered_features,
                required_features,
            } => {
                if min_minor > max_minor || min_minor > VERSION_MINOR {
                    return error(obj, ser, ErrorCode::VersionMismatch, true);
                }
                if required_features & !offered_features != 0 {
                    return error(obj, ser, ErrorCode::Malformed, true);
                }
                let supported = FEATURE_ARGB8888;
                if required_features & !supported != 0 {
                    return error(obj, ser, ErrorCode::Unsupported, true);
                }
                let features = offered_features & supported;
                c.features = features;
                c.phase = Phase::Active;
                c.client_id = *ncid;
                *ncid += 1;
                let formats = FORMATS_XRGB8888
                    | if features & FEATURE_ARGB8888 != 0 { FORMATS_ARGB8888 } else { 0 };
                (
                    one(0, Event::Welcome { request: ser, client_id: c.client_id, features, formats }),
                    false,
                )
            }
            Request::Disconnect => (one(0, Event::Result { request: ser }), true),

            Request::CreateSurface { role, width, height, format } => {
                if let Err(e) = c.check_fresh(obj) {
                    return error(obj, ser, e, false);
                }
                if role != ROLE_TOPLEVEL {
                    return error(obj, ser, ErrorCode::Unsupported, false);
                }
                if let Err(e) = check_format(format, c.argb_enabled()) {
                    return error(obj, ser, e, false);
                }
                if !(dim_ok(width) && dim_ok(height)) {
                    return error(obj, ser, ErrorCode::BadValue, false);
                }
                if c.surfaces >= MAX_SURFACES {
                    return error(obj, ser, ErrorCode::LimitExceeded, false);
                }
                // CREATE → RESULT → initial CONFIGURE (spec §6). The CONFIGURE is
                // the second emit, so it will take server serial `base + 1`; that
                // header serial is the surface's initial configure token.
                let token = c.next_server_serial + 1;
                c.put_surface(obj, Surface::new(format, width, height, token));
                c.id_watermark = obj;
                c.surfaces += 1;
                let mut ev = one(obj, Event::Result { request: ser });
                ev.push((obj, Event::Configure { width, height, scale: 1, state: STATE_ACTIVATED }));
                (ev, false)
            }
            Request::DestroySurface => {
                let s = match c.surface_of(obj) {
                    Ok(s) => s,
                    Err(e) => return error(obj, ser, e, false),
                };
                // Cleanup events precede the RESULT barrier (spec §6): LEAVE,
                // buffer releases, DISCARDED callback, then RESULT.
                let mut ev: Vec<(ObjectId, Event)> = Vec::new();
                seat.drop_surface((conn_id, obj), &mut ev);
                if s.pending_attach && s.pending_buffer != 0 {
                    if let Ok(mut b) = c.buffer_of(s.pending_buffer) {
                        b.state = BufState::Available;
                        b.commit_token = 0;
                        c.put_buffer(s.pending_buffer, b);
                        ev.push((s.pending_buffer, Event::BufferRelease { commit: 0 }));
                    }
                }
                if s.committed_buffer != 0 {
                    if let Ok(mut b) = c.buffer_of(s.committed_buffer) {
                        b.state = BufState::Available;
                        b.commit_token = 0;
                        c.put_buffer(s.committed_buffer, b);
                        ev.push((s.committed_buffer, Event::BufferRelease { commit: s.committed_token }));
                    }
                }
                if s.frame_pending {
                    ev.push((obj, Event::FrameDone { commit: s.frame_token, time_ns: now_ns, status: FRAME_DISCARDED }));
                }
                c.objects.remove(&obj);
                c.surfaces -= 1;
                ev.push((obj, Event::Result { request: ser }));
                (ev, false)
            }

            Request::SetTitle(s) => match c.require(obj, ObjKind::Surface) {
                Ok(()) if s.len() <= MAX_TITLE_BYTES => ok,
                Ok(()) => error(obj, ser, ErrorCode::BadValue, false),
                Err(e) => error(obj, ser, e, false),
            },
            Request::SetAppId(s) => match c.require(obj, ObjKind::Surface) {
                Ok(()) if app_id_ok(s.as_bytes()) => ok,
                Ok(()) => error(obj, ser, ErrorCode::BadValue, false),
                Err(e) => error(obj, ser, e, false),
            },
            Request::AckConfigure { configure } => {
                let mut s = match c.surface_of(obj) {
                    Ok(s) => s,
                    Err(e) => return error(obj, ser, e, false),
                };
                // ACK is valid only for the latest token, and only once; a stale,
                // foreign or repeated token is BAD_SERIAL (spec §6).
                if configure != s.latest_token || s.acked_token == s.latest_token {
                    return error(obj, ser, ErrorCode::BadSerial, false);
                }
                s.acked_token = configure;
                c.put_surface(obj, s);
                ok
            }
            Request::ImportBuffer { width, height, stride, format, offset } => {
                // `cap` is guaranteed `Some` for IMPORT_BUFFER by the framing
                // stage; its value is the pinned backing size in bytes. Duplicate
                // memory-object detection (spec §7) is deferred to the M3 kernel
                // binding: the transport shim carries no memory-object identity.
                let cap_size = cap.unwrap_or(0);
                if let Err(e) = c.check_fresh(obj) {
                    return error(obj, ser, e, false);
                }
                if let Err(e) = check_format(format, c.argb_enabled()) {
                    return error(obj, ser, e, false);
                }
                if !(dim_ok(width) && dim_ok(height)) {
                    return error(obj, ser, ErrorCode::BadValue, false);
                }
                let min_stride = width * 4; // width <= 4096 => fits in u32
                if stride % 4 != 0 || offset % 4 != 0 || stride < min_stride {
                    return error(obj, ser, ErrorCode::BadValue, false);
                }
                if stride > MAX_STRIDE || cap_size > MAX_BACKING_BYTES {
                    return error(obj, ser, ErrorCode::LimitExceeded, false);
                }
                let end = (stride as u64)
                    .checked_mul((height - 1) as u64)
                    .and_then(|v| v.checked_add(offset))
                    .and_then(|v| v.checked_add((width * 4) as u64));
                match end {
                    Some(end) if end <= cap_size => {}
                    _ => return error(obj, ser, ErrorCode::OutOfBounds, false),
                }
                if c.buffers >= MAX_BUFFERS {
                    return error(obj, ser, ErrorCode::LimitExceeded, false);
                }
                if c.backing_bytes + cap_size > MAX_BACKING_BYTES_CONN {
                    return error(obj, ser, ErrorCode::LimitExceeded, false);
                }
                c.put_buffer(obj, Buffer { width, height, format, backing: cap_size, state: BufState::Available, commit_token: 0 });
                c.id_watermark = obj;
                c.buffers += 1;
                c.backing_bytes += cap_size;
                ok
            }

            Request::DestroyBuffer => {
                let b = match c.buffer_of(obj) {
                    Ok(b) => b,
                    Err(e) => return error(obj, ser, e, false),
                };
                // A pending/committed buffer is BUSY, not a deferred destroy (§7).
                if b.state != BufState::Available {
                    return error(obj, ser, ErrorCode::Busy, false);
                }
                c.objects.remove(&obj);
                c.buffers -= 1;
                c.backing_bytes -= b.backing;
                ok
            }
            Request::AttachBuffer { buffer, damage } => {
                let mut s = match c.surface_of(obj) {
                    Ok(s) => s,
                    Err(e) => return error(obj, ser, e, false),
                };
                if buffer == 0 {
                    // Explicit unmap attach: damage must be empty (spec §4).
                    if s.pending_attach {
                        return error(obj, ser, ErrorCode::BadState, false);
                    }
                    if !damage.is_empty() {
                        return error(obj, ser, ErrorCode::BadValue, false);
                    }
                    s.pending_attach = true;
                    s.pending_buffer = 0;
                    c.put_surface(obj, s);
                    return ok;
                }
                // Object existence/type is checked before object state (§10 order).
                let mut b = match c.buffer_of(buffer) {
                    Ok(b) => b,
                    Err(e) => return error(obj, ser, e, false),
                };
                if s.pending_attach {
                    return error(obj, ser, ErrorCode::BadState, false);
                }
                if b.state != BufState::Available {
                    return error(obj, ser, ErrorCode::Busy, false);
                }
                // Checked bounds: half-open rects fully inside the buffer (§6, §8).
                for r in &damage {
                    let out_of_bounds = r.w == 0
                        || r.h == 0
                        || r.x < 0
                        || r.y < 0
                        || r.x as i64 + r.w as i64 > b.width as i64
                        || r.y as i64 + r.h as i64 > b.height as i64;
                    if out_of_bounds {
                        return error(obj, ser, ErrorCode::OutOfBounds, false);
                    }
                }
                if damage.len() as u32 > MAX_DAMAGE_RECTS {
                    return error(obj, ser, ErrorCode::LimitExceeded, false);
                }
                b.state = BufState::Pending;
                c.put_buffer(buffer, b);
                s.pending_attach = true;
                s.pending_buffer = buffer;
                c.put_surface(obj, s);
                ok
            }

            Request::Commit { configure, frame_callback } => {
                if frame_callback > 1 {
                    return error(obj, ser, ErrorCode::BadValue, false);
                }
                let mut s = match c.surface_of(obj) {
                    Ok(s) => s,
                    Err(e) => return error(obj, ser, e, false),
                };
                // COMMIT needs a pending attach and an acked latest configure;
                // on any error pending/committed and buffer states are untouched
                // (spec §6, no partial mutation).
                if !s.pending_attach {
                    return error(obj, ser, ErrorCode::BadState, false);
                }
                if !s.configure_ready(configure) {
                    return error(obj, ser, ErrorCode::BadSerial, false);
                }
                if frame_callback == 1 && s.frame_pending {
                    return error(obj, ser, ErrorCode::Busy, false);
                }

                if s.pending_buffer == 0 {
                    // Unmap commit: RESULT first, then LEAVE + release + DISCARDED
                    // callback (spec §6, §9).
                    let old = s.committed_buffer;
                    let old_token = s.committed_token;
                    s.committed_buffer = 0;
                    s.committed_token = 0;
                    s.mapped = false;
                    s.pending_attach = false;
                    s.pending_buffer = 0;
                    let discard = if s.frame_pending {
                        s.frame_pending = false;
                        Some(s.frame_token)
                    } else if frame_callback == 1 {
                        Some(ser)
                    } else {
                        None
                    };
                    c.put_surface(obj, s);
                    let mut ev = one(obj, Event::Result { request: ser });
                    seat.drop_surface((conn_id, obj), &mut ev);
                    if old != 0 {
                        if let Ok(mut b) = c.buffer_of(old) {
                            b.state = BufState::Available;
                            b.commit_token = 0;
                            c.put_buffer(old, b);
                            ev.push((old, Event::BufferRelease { commit: old_token }));
                        }
                    }
                    if let Some(t) = discard {
                        ev.push((obj, Event::FrameDone { commit: t, time_ns: now_ns, status: FRAME_DISCARDED }));
                    }
                    return (ev, false);
                }

                // Map commit: the new buffer must be `configure.{w,h} * scale`
                // in the surface format, via checked multiply (spec §6, §8).
                let b = match c.buffer_of(s.pending_buffer) {
                    Ok(b) => b,
                    Err(e) => return error(obj, ser, e, false),
                };
                let px = s.width.checked_mul(s.scale).zip(s.height.checked_mul(s.scale));
                let (pw, ph) = match px {
                    Some((w, h)) => (w, h),
                    None => return error(obj, ser, ErrorCode::OutOfBounds, false),
                };
                if b.width != pw || b.height != ph || b.format != s.format {
                    return error(obj, ser, ErrorCode::BadValue, false);
                }
                // Atomic mutation: the pending buffer becomes BUSY, replacing the
                // previous committed buffer, which is released now (spec §7).
                let old = s.committed_buffer;
                let old_token = s.committed_token;
                let pending_buffer = s.pending_buffer;
                let mut nb = b;
                nb.state = BufState::Busy;
                nb.commit_token = ser;
                c.put_buffer(pending_buffer, nb);
                s.committed_buffer = pending_buffer;
                s.committed_token = ser;
                s.mapped = true;
                s.pending_attach = false;
                s.pending_buffer = 0;
                if frame_callback == 1 {
                    s.frame_pending = true;
                    s.frame_token = ser;
                }
                c.put_surface(obj, s);
                let mut ev = one(obj, Event::Result { request: ser });
                if old != 0 && old != pending_buffer {
                    if let Ok(mut ob) = c.buffer_of(old) {
                        ob.state = BufState::Available;
                        ob.commit_token = 0;
                        c.put_buffer(old, ob);
                        ev.push((old, Event::BufferRelease { commit: old_token }));
                    }
                }
                (ev, false)
            }
        }
    }
}

/// A batch of `(header object, event)` pairs to emit on the requesting
/// connection, plus whether the connection must be torn down afterwards.
type Emit = (Vec<(ObjectId, Event)>, bool);

fn one(object: ObjectId, ev: Event) -> Vec<(ObjectId, Event)> {
    let mut v = Vec::with_capacity(1);
    v.push((object, ev));
    v
}

fn error(object: ObjectId, request: u64, code: ErrorCode, fatal: bool) -> Emit {
    (
        one(object, Event::Error { request, code: code.code(), fatal: fatal as u32 }),
        fatal,
    )
}

/// Deterministic headless GUI server.
pub struct Server {
    now_ns: u64,
    next_conn: ConnId,
    next_client_id: u64,
    conns: BTreeMap<ConnId, Connection>,
    seat: Seat,
}

impl Default for Server {
    fn default() -> Self {
        Self::new()
    }
}

impl Server {
    pub fn new() -> Server {
        Server {
            now_ns: 0,
            next_conn: 1,
            next_client_id: 1,
            conns: BTreeMap::new(),
            seat: Seat::default(),
        }
    }

    /// Current injected monotonic time.
    pub fn now_ns(&self) -> u64 {
        self.now_ns
    }

    /// Accept a transport connection. Returns `None` once the server-wide
    /// connection limit is reached (spec §8).
    pub fn connect(&mut self) -> Option<ConnId> {
        if self.conns.len() >= MAX_CONNECTIONS {
            return None;
        }
        let id = self.next_conn;
        self.next_conn += 1;
        self.conns.insert(id, Connection::new(self.now_ns));
        Some(id)
    }

    /// Transport-level peer death: drop the connection and all its objects, and
    /// release any focus/grab it held (spec §7, §9). Idempotent.
    pub fn disconnect_peer(&mut self, conn: ConnId) {
        self.conns.remove(&conn);
        self.seat.drop_conn(conn);
    }

    /// Read-only view of a connection, for assertions in tests.
    pub fn connection(&self, conn: ConnId) -> Option<&Connection> {
        self.conns.get(&conn)
    }

    /// Advance the injected clock to `now_ns` (monotonic) and close every
    /// connection still in `NEW` past the HELLO deadline (spec §4). Returns the
    /// ids closed this tick; their focus/grab bindings are released. No packet
    /// is emitted — a timed-out peer that never said HELLO has no serial stream
    /// to trust, matching the connection-layer model.
    pub fn tick(&mut self, now_ns: u64) -> Vec<ConnId> {
        debug_assert!(now_ns >= self.now_ns, "clock must be monotonic");
        if now_ns > self.now_ns {
            self.now_ns = now_ns;
        }
        let mut closed = Vec::new();
        for (&id, c) in self.conns.iter_mut() {
            if c.phase == Phase::New && self.now_ns.saturating_sub(c.created_ns) >= HELLO_TIMEOUT_NS {
                c.phase = Phase::Closed;
                closed.push(id);
            }
        }
        for id in &closed {
            self.conns.remove(id);
            self.seat.drop_conn(*id);
        }
        closed
    }

    /// Feed one client packet (with its optional single attachment `cap`, the
    /// pinned backing size for IMPORT_BUFFER) on `conn` and return the reply/
    /// event packets for that connection. Unknown/closed connections yield
    /// nothing. Cross-connection input and frame events are produced by the
    /// dedicated injection methods, not here.
    pub fn deliver(&mut self, conn: ConnId, packet: &[u8], cap: Option<u64>) -> Vec<Vec<u8>> {
        // Disjoint field borrows so the pipeline can touch the connection, the
        // client-id allocator and the seat at once.
        let ncid = &mut self.next_client_id;
        let seat = &mut self.seat;
        let now = self.now_ns;
        let c = match self.conns.get_mut(&conn) {
            Some(c) if c.phase != Phase::Closed => c,
            _ => return Vec::new(),
        };
        let (emit, close) = Self::pipeline(c, ncid, seat, conn, now, packet, cap);
        let mut out = Vec::with_capacity(emit.len());
        for (object, ev) in emit {
            let ser = c.next_server_serial;
            c.next_server_serial += 1;
            out.push(ev.encode(object, ser));
        }
        if close {
            c.phase = Phase::Closed;
            self.seat.drop_conn(conn);
        }
        out
    }

    /// Validation pipeline (spec §10 stages 1–5), then structural decode, then
    /// semantic [`Server::dispatch`]. `seat`, `conn_id` and `now_ns` are threaded
    /// through for the behavioural arms (focus cleanup, frame timestamps).
    fn pipeline(
        c: &mut Connection,
        ncid: &mut u64,
        seat: &mut Seat,
        conn_id: ConnId,
        now_ns: u64,
        packet: &[u8],
        cap: Option<u64>,
    ) -> Emit {
        // Stage 1a: framing.
        let header = match Header::decode(packet) {
            Ok(h) => h,
            // Header unreadable: ERROR with object=0, request=0 (spec §5).
            Err(e) => return error(0, 0, e.code, true),
        };
        // Stage 1b: attachments. Exactly one capability is legal only on
        // IMPORT_BUFFER; any other count is MALFORMED (spec §4, §10).
        let is_import = header.opcode_raw == Opcode::ImportBuffer.raw();
        if is_import != cap.is_some() {
            return error(header.object, header.serial, ErrorCode::Malformed, true);
        }
        // Stage 2: version. Major must be exactly 1 on every packet (spec §3).
        if header.major != VERSION_MAJOR {
            return error(header.object, header.serial, ErrorCode::VersionMismatch, true);
        }
        // Stage 3: header serial — strictly increasing, nonzero, no repeat/wrap.
        // A bad header serial is fatal (spec §10). Valid serials are consumed
        // even if a later recoverable error occurs (spec §2, §10).
        if header.serial == 0 || header.serial <= c.last_client_serial {
            return error(header.object, header.serial, ErrorCode::BadSerial, true);
        }
        c.last_client_serial = header.serial;
        // Stage 4: opcode direction and reserved policy range.
        let opcode = match Opcode::from_u16(header.opcode_raw) {
            Some(op) if op.is_request() => op,
            Some(_) => return error(header.object, header.serial, ErrorCode::BadOpcode, true),
            None => {
                if (POLICY_OPCODE_LO..=POLICY_OPCODE_HI).contains(&header.opcode_raw) {
                    return error(header.object, header.serial, ErrorCode::AccessDenied, false);
                }
                return error(header.object, header.serial, ErrorCode::BadOpcode, true);
            }
        };
        // Stage 5: connection state.
        match c.phase {
            Phase::New if opcode != Opcode::Hello => {
                return error(header.object, header.serial, ErrorCode::BadState, true);
            }
            Phase::Active if opcode == Opcode::Hello => {
                return error(header.object, header.serial, ErrorCode::BadState, false);
            }
            _ => {}
        }
        // Structural payload decode. Truncation/trailing/reserved are MALFORMED
        // (fatal framing errors); bad UTF-8/NUL in a string is a recoverable
        // BAD_VALUE (spec §2, §10).
        let req = match Request::decode(opcode, &packet[crate::wire::HEADER_LEN..]) {
            Ok(r) => r,
            Err(e) => {
                let fatal = e.code == ErrorCode::Malformed;
                return error(header.object, header.serial, e.code, fatal);
            }
        };
        Self::dispatch(c, ncid, seat, conn_id, now_ns, &header, req, cap)
    }

    // ---- external-event injection API (spec §9) -------------------------
    // Focus assignment, reconfigure, composition ticks and raw input all enter
    // the model here as *verified external events* from the DWM/kernel, never
    // from a host pointer or wall clock. Each returns the per-connection packets
    // to deliver, addressed by owning connection.

    /// Encode `ev` on `conn`'s server-serial stream, skipping unknown/closed
    /// connections. Keeps serial monotonicity per connection (spec §2).
    fn emit_to(&mut self, conn: ConnId, obj: ObjectId, ev: Event, out: &mut Vec<(ConnId, Vec<u8>)>) {
        if let Some(c) = self.conns.get_mut(&conn) {
            if c.phase == Phase::Closed {
                return;
            }
            let ser = c.next_server_serial;
            c.next_server_serial += 1;
            out.push((conn, ev.encode(obj, ser)));
        }
    }

    /// Whether `tgt` names a live MAPPED surface on an active connection — the
    /// precondition for routing any input to it (spec §9).
    fn is_mapped(&self, tgt: Target) -> bool {
        match self.conns.get(&tgt.0) {
            Some(c) if c.phase == Phase::Active => matches!(c.surface_of(tgt.1), Ok(s) if s.mapped),
            _ => false,
        }
    }

    /// Held-input overflow (spec §8): emit a fatal LIMIT_EXCEEDED and tear the
    /// connection down, releasing its focus/grab.
    fn input_overflow(&mut self, conn: ConnId) -> Vec<(ConnId, Vec<u8>)> {
        let mut out = Vec::new();
        self.emit_to(
            conn,
            0,
            Event::Error { request: 0, code: ErrorCode::LimitExceeded.code(), fatal: 1 },
            &mut out,
        );
        if let Some(c) = self.conns.get_mut(&conn) {
            c.phase = Phase::Closed;
        }
        self.seat.drop_conn(conn);
        out
    }

    /// A composition tick: complete every pending frame callback (spec §6).
    /// A still-mapped surface reports PRESENTED, an unmapped one DISCARDED.
    pub fn composite(&mut self) -> Vec<(ConnId, Vec<u8>)> {
        let now = self.now_ns;
        let mut done: Vec<(ConnId, ObjectId, u64, u32)> = Vec::new();
        for (&conn, c) in self.conns.iter() {
            if c.phase != Phase::Active {
                continue;
            }
            for (&id, o) in c.objects.iter() {
                if let Object::Surface(s) = o {
                    if s.frame_pending {
                        let status = if s.mapped { FRAME_PRESENTED } else { FRAME_DISCARDED };
                        done.push((conn, id, s.frame_token, status));
                    }
                }
            }
        }
        let mut out = Vec::new();
        for (conn, id, token, status) in done {
            if let Some(c) = self.conns.get_mut(&conn) {
                if let Ok(mut s) = c.surface_of(id) {
                    s.frame_pending = false;
                    c.put_surface(id, s);
                }
            }
            self.emit_to(conn, id, Event::FrameDone { commit: token, time_ns: now, status }, &mut out);
        }
        out
    }

    /// External reconfigure (DWM policy, spec §9): push a fresh CONFIGURE whose
    /// header serial becomes the surface's new latest token, superseding any
    /// prior ack so the client must ACK again before its next COMMIT (spec §6).
    /// No-op for an unknown/foreign surface or out-of-range geometry.
    pub fn reconfigure(&mut self, conn: ConnId, surface: ObjectId, width: u32, height: u32, scale: u32, state: u32) -> Vec<(ConnId, Vec<u8>)> {
        let mut out = Vec::new();
        if !(dim_ok(width) && dim_ok(height)) || scale == 0 {
            return out;
        }
        let token = match self.conns.get(&conn) {
            Some(c) if c.phase == Phase::Active && c.surface_of(surface).is_ok() => c.next_server_serial,
            _ => return out,
        };
        if let Some(c) = self.conns.get_mut(&conn) {
            if let Ok(mut s) = c.surface_of(surface) {
                s.width = width;
                s.height = height;
                s.scale = scale;
                s.state = state;
                s.latest_token = token;
                c.put_surface(surface, s);
            }
        }
        self.emit_to(conn, surface, Event::Configure { width, height, scale, state }, &mut out);
        out
    }

    /// DWM pointer-focus assignment (spec §9). No-op while an implicit grab is
    /// active (the grab pins focus until all buttons release). A change emits
    /// PointerLeave on the old surface then PointerEnter on the new; a target
    /// that is not a live mapped surface clears focus.
    pub fn set_pointer_focus(&mut self, target: Option<Target>, x: i32, y: i32) -> Vec<(ConnId, Vec<u8>)> {
        let mut out = Vec::new();
        if self.seat.grab.is_some() {
            return out;
        }
        let target = target.filter(|&t| self.is_mapped(t));
        if self.seat.pointer_focus == target {
            return out;
        }
        if let Some(old) = self.seat.pointer_focus {
            self.emit_to(old.0, old.1, Event::PointerLeave, &mut out);
        }
        self.seat.pointer_focus = target;
        self.seat.buttons.clear();
        if let Some(new) = target {
            self.emit_to(new.0, new.1, Event::PointerEnter { x, y }, &mut out);
        }
        out
    }

    /// Pointer motion routed to the grab holder, else the pointer focus (§9).
    pub fn pointer_motion(&mut self, x: i32, y: i32) -> Vec<(ConnId, Vec<u8>)> {
        let mut out = Vec::new();
        let now = self.now_ns;
        if let Some(t) = self.seat.grab.or(self.seat.pointer_focus) {
            self.emit_to(t.0, t.1, Event::PointerMotion { time_ns: now, x, y }, &mut out);
        }
        out
    }

    /// Pointer button. A press starts the implicit grab (if none) and is held
    /// until every button releases; the event routes to the grab/focus target.
    pub fn pointer_button(&mut self, button: u32, pressed: bool) -> Vec<(ConnId, Vec<u8>)> {
        let mut out = Vec::new();
        let now = self.now_ns;
        let Some(t) = self.seat.grab.or(self.seat.pointer_focus) else {
            return out;
        };
        if pressed {
            if !self.seat.buttons.contains(&button) {
                self.seat.buttons.push(button);
            }
            if self.seat.grab.is_none() {
                self.seat.grab = Some(t);
            }
        } else {
            self.seat.buttons.retain(|&b| b != button);
        }
        self.emit_to(t.0, t.1, Event::PointerButton { time_ns: now, button, pressed: pressed as u32 }, &mut out);
        if !pressed && self.seat.buttons.is_empty() {
            self.seat.grab = None;
        }
        out
    }

    /// Pointer axis (scroll) routed to the grab holder, else the pointer focus.
    pub fn pointer_axis(&mut self, dx: i32, dy: i32) -> Vec<(ConnId, Vec<u8>)> {
        let mut out = Vec::new();
        let now = self.now_ns;
        if let Some(t) = self.seat.grab.or(self.seat.pointer_focus) {
            self.emit_to(t.0, t.1, Event::PointerAxis { time_ns: now, dx, dy }, &mut out);
        }
        out
    }

    /// DWM keyboard-focus assignment (spec §9): KeyLeave on the old surface then
    /// KeyEnter on the new, resetting the held-key set. A non-mapped target
    /// clears focus.
    pub fn set_keyboard_focus(&mut self, target: Option<Target>, modifiers: u32) -> Vec<(ConnId, Vec<u8>)> {
        let mut out = Vec::new();
        let target = target.filter(|&t| self.is_mapped(t));
        if self.seat.keyboard_focus == target {
            return out;
        }
        if let Some(old) = self.seat.keyboard_focus {
            self.emit_to(old.0, old.1, Event::KeyLeave, &mut out);
        }
        self.seat.keyboard_focus = target;
        self.seat.held_keys.clear();
        self.seat.modifiers = modifiers;
        if let Some(new) = target {
            self.emit_to(new.0, new.1, Event::KeyEnter { modifiers }, &mut out);
        }
        out
    }

    /// A key event routed to the keyboard focus (spec §9). A release of an
    /// unheld key is suppressed; a repeat is delivered only for a currently-held
    /// key; a fresh press grows the held set, whose overflow is fatal (spec §8).
    pub fn key(&mut self, usage: u32, pressed: bool, modifiers: u32, repeat: bool) -> Vec<(ConnId, Vec<u8>)> {
        let mut out = Vec::new();
        let now = self.now_ns;
        self.seat.modifiers = modifiers;
        let Some(t) = self.seat.keyboard_focus else {
            return out;
        };
        let held = self.seat.held_keys.contains(&usage);
        if pressed {
            if repeat {
                if !held {
                    return out;
                }
            } else if !held {
                if self.seat.held_keys.len() >= MAX_HELD_KEYS {
                    return self.input_overflow(t.0);
                }
                self.seat.held_keys.push(usage);
            }
        } else {
            if !held {
                return out;
            }
            self.seat.held_keys.retain(|&k| k != usage);
        }
        let repeat_flag = if pressed && repeat { 1 } else { 0 };
        self.emit_to(
            t.0,
            t.1,
            Event::Key { time_ns: now, usage, pressed: pressed as u32, modifiers, repeat: repeat_flag },
            &mut out,
        );
        out
    }

    /// Composed text routed to the keyboard focus (spec §5): 1..=160 UTF-8
    /// bytes; an empty or over-long string is dropped.
    pub fn text_input(&mut self, text: &str) -> Vec<(ConnId, Vec<u8>)> {
        let mut out = Vec::new();
        let now = self.now_ns;
        let Some(t) = self.seat.keyboard_focus else {
            return out;
        };
        if text.is_empty() || text.len() > MAX_TEXT_BYTES {
            return out;
        }
        self.emit_to(t.0, t.1, Event::TextInput { time_ns: now, text: String::from(text) }, &mut out);
        out
    }
}
