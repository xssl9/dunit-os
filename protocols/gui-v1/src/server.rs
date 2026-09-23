//! Headless reference server for the connection and object-namespace layers of
//! Dunit GUI protocol v1.0.
//!
//! # Milestone scope (M2 item 2)
//!
//! This models, deterministically and without a framebuffer:
//!
//! * the **connection** state machine — negotiation (`HELLO`/`WELCOME`),
//!   version and feature selection, the `NEW` HELLO timeout, and `DISCONNECT`
//!   (spec §3);
//! * client-request **serial** monotonicity, **opcode** direction and the
//!   reserved DWM policy range (spec §2, §9);
//! * the shared **object namespace** — creation/destruction of surfaces and
//!   buffers, fresh-ID monotonicity, no reuse, existence/type checks and
//!   cross-connection isolation (spec §2, §7), plus the structural validation
//!   of `CREATE_SURFACE`/`IMPORT_BUFFER`/`SET_TITLE`/`SET_APP_ID` payloads and
//!   the buffer memory-bounds check (spec §8).
//!
//! The **behavioural** models — the configure/ack/commit lifecycle, buffer
//! ownership (`AVAILABLE→PENDING→BUSY`), frame callbacks, and focus/input
//! routing, together with the events they emit (`CONFIGURE`, `FRAME_DONE`,
//! `BUFFER_RELEASE`, pointer/key/text) — are deliberately left to M2 item 4.
//! `ACK_CONFIGURE`, `ATTACH_BUFFER` and `COMMIT` are decoded and validated here
//! only up to object existence/type; their acceptance is acknowledged with a
//! `RESULT` and the state-machine effects are added in item 4. Every call site
//! that defers behaviour says so explicitly.
//!
//! Determinism: no wall clock and no host pointer influence the output. Time
//! advances only via [`Server::tick`]; `client_id`s are handed out
//! monotonically. Identical inbound byte streams and injected events therefore
//! yield identical outbound byte streams (spec §11).

use alloc::collections::BTreeMap;
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

const HELLO_TIMEOUT_NS: u64 = 5_000_000_000;
const MAX_CONNECTIONS: usize = 64;
const MAX_SURFACES: u32 = 64;
const MAX_BUFFERS: u32 = 192;
const MAX_BACKING_BYTES: u64 = 64 * 1024 * 1024;
const MAX_STRIDE: u32 = 16384;
const MAX_TITLE_BYTES: usize = 160;
const MAX_APP_ID_BYTES: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ObjKind {
    Surface,
    Buffer,
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
    /// Highest client request serial accepted so far (0 = none). Strictly
    /// increasing, no repeats, no wrap (spec §2).
    last_client_serial: u64,
    /// Independent server serial sequence, starts at 1 (spec §2).
    next_server_serial: u64,
    features: u64,
    client_id: u64,
    /// Shared surface/buffer namespace of this connection.
    objects: BTreeMap<ObjectId, ObjKind>,
    /// Highest object ID ever successfully created; new IDs must exceed it.
    id_watermark: ObjectId,
    surfaces: u32,
    buffers: u32,
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
            Some(k) if *k != kind => Err(ErrorCode::WrongObjectType),
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

impl Server {
    /// Semantic dispatch (spec §10 stages 6–11). Behavioural effects noted as
    /// M2 item 4 are acknowledged with a `RESULT` after their object/type/value
    /// checks; nothing here fabricates state-machine transitions.
    fn dispatch(
        c: &mut Connection,
        ncid: &mut u64,
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
                // Version intersection: v1.0 supports only minor 0.
                if min_minor > max_minor || min_minor > VERSION_MINOR {
                    return error(obj, ser, ErrorCode::VersionMismatch, true);
                }
                // required must be a subset of offered (spec §3).
                if required_features & !offered_features != 0 {
                    return error(obj, ser, ErrorCode::Malformed, true);
                }
                // Server supports only ARGB8888 as a feature bit.
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
                    | if features & FEATURE_ARGB8888 != 0 {
                        FORMATS_ARGB8888
                    } else {
                        0
                    };
                (
                    one(
                        0,
                        Event::Welcome {
                            request: ser,
                            client_id: c.client_id,
                            features,
                            formats,
                        },
                    ),
                    false,
                )
            }
            Request::Disconnect => (one(0, Event::Result { request: ser }), true),

            Request::CreateSurface {
                role,
                width,
                height,
                format,
            } => {
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
                c.objects.insert(obj, ObjKind::Surface);
                c.id_watermark = obj;
                c.surfaces += 1;
                // Initial CONFIGURE (configure lifecycle) is M2 item 4.
                ok
            }
            Request::DestroySurface => match c.require(obj, ObjKind::Surface) {
                Ok(()) => {
                    c.objects.remove(&obj);
                    c.surfaces -= 1;
                    // Cleanup events (LEAVE/RELEASE/DISCARDED) precede this
                    // RESULT barrier in M2 item 4.
                    ok
                }
                Err(e) => error(obj, ser, e, false),
            },
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
            // Configure-token lifecycle is M2 item 4; validated to object/type.
            Request::AckConfigure { .. } => match c.require(obj, ObjKind::Surface) {
                Ok(()) => ok,
                Err(e) => error(obj, ser, e, false),
            },

            Request::ImportBuffer {
                width,
                height,
                stride,
                format,
                offset,
            } => {
                // `cap` is guaranteed `Some` for IMPORT_BUFFER by the framing
                // stage; its value is the pinned backing size in bytes.
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
                // Geometry (spec §8): stride multiple of 4 and >= width*4,
                // offset multiple of 4.
                let min_stride = width * 4; // width <= 4096 => fits in u32
                if stride % 4 != 0 || offset % 4 != 0 || stride < min_stride {
                    return error(obj, ser, ErrorCode::BadValue, false);
                }
                // Per-object hard maxima (spec §8): stride and backing size.
                if stride > MAX_STRIDE || cap_size > MAX_BACKING_BYTES {
                    return error(obj, ser, ErrorCode::LimitExceeded, false);
                }
                // Checked bounds: end = offset + stride*(height-1) + width*4.
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
                c.objects.insert(obj, ObjKind::Buffer);
                c.id_watermark = obj;
                c.buffers += 1;
                ok
            }
            Request::DestroyBuffer => match c.require(obj, ObjKind::Buffer) {
                Ok(()) => {
                    c.objects.remove(&obj);
                    c.buffers -= 1;
                    // BUSY-state rejection is added with buffer ownership (item 4).
                    ok
                }
                Err(e) => error(obj, ser, e, false),
            },
            // Buffer ownership / pending state is M2 item 4; validated to type.
            Request::AttachBuffer { buffer, damage } => {
                if let Err(e) = c.require(obj, ObjKind::Surface) {
                    return error(obj, ser, e, false);
                }
                if damage.len() as u32 > MAX_DAMAGE_RECTS {
                    return error(obj, ser, ErrorCode::LimitExceeded, false);
                }
                if buffer == 0 {
                    // buffer=0 is explicit unmap; damage must be empty (spec §4).
                    if !damage.is_empty() {
                        return error(obj, ser, ErrorCode::BadValue, false);
                    }
                    return ok;
                }
                match c.require(buffer, ObjKind::Buffer) {
                    Ok(()) => ok,
                    Err(e) => error(obj, ser, e, false),
                }
            }
            // Commit atomicity / frame callbacks are M2 item 4; validated here.
            Request::Commit {
                frame_callback, ..
            } => match c.require(obj, ObjKind::Surface) {
                Ok(()) if frame_callback <= 1 => ok,
                Ok(()) => error(obj, ser, ErrorCode::BadValue, false),
                Err(e) => error(obj, ser, e, false),
            },
        }
    }
}



/// Deterministic headless GUI server.
pub struct Server {
    now_ns: u64,
    next_conn: ConnId,
    next_client_id: u64,
    conns: BTreeMap<ConnId, Connection>,
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
        }
    }

    /// Current injected monotonic time.
    pub fn now_ns(&self) -> u64 {
        self.now_ns
    }

    /// Accept a transport connection. Returns `None` once the server-wide
    /// connection limit is reached; the transport is expected to close excess
    /// peers before `HELLO` (spec §8).
    pub fn connect(&mut self) -> Option<ConnId> {
        if self.conns.len() >= MAX_CONNECTIONS {
            return None;
        }
        let id = self.next_conn;
        self.next_conn += 1;
        self.conns.insert(id, Connection::new(self.now_ns));
        Some(id)
    }

    /// Transport-level peer death: drop the connection and all its objects
    /// (spec §7). Idempotent.
    pub fn disconnect_peer(&mut self, conn: ConnId) {
        self.conns.remove(&conn);
    }

    /// Read-only view of a connection, for assertions in tests.
    pub fn connection(&self, conn: ConnId) -> Option<&Connection> {
        self.conns.get(&conn)
    }

    /// Advance the monotonic clock to `now_ns` (must not move backwards) and
    /// close any `NEW` connection that has not completed `HELLO` within the
    /// timeout (spec §3). Returns the connections that were closed.
    pub fn tick(&mut self, now_ns: u64) -> Vec<ConnId> {
        debug_assert!(now_ns >= self.now_ns, "clock must be monotonic");
        if now_ns > self.now_ns {
            self.now_ns = now_ns;
        }
        let mut closed = Vec::new();
        for (&id, c) in self.conns.iter_mut() {
            if c.phase == Phase::New && self.now_ns.saturating_sub(c.created_ns) >= HELLO_TIMEOUT_NS
            {
                c.phase = Phase::Closed;
                closed.push(id);
            }
        }
        for id in &closed {
            self.conns.remove(id);
        }
        closed
    }
}

/// A batch of `(header object, event)` pairs to emit, plus whether the
/// connection must be torn down after emitting them.
type Emit = (Vec<(ObjectId, Event)>, bool);

fn one(object: ObjectId, ev: Event) -> Vec<(ObjectId, Event)> {
    let mut v = Vec::with_capacity(1);
    v.push((object, ev));
    v
}

fn error(object: ObjectId, request: u64, code: ErrorCode, fatal: bool) -> Emit {
    (
        one(
            object,
            Event::Error {
                request,
                code: code.code(),
                fatal: fatal as u32,
            },
        ),
        fatal,
    )
}

impl Server {
    /// Feed one inbound packet on `conn`, with `cap` = the memory capability
    /// attached to it (exactly one is legal only on `IMPORT_BUFFER`; its value
    /// is the pinned backing size in bytes). Returns the encoded outbound
    /// packets, in order. Unknown or already-closed connections yield nothing,
    /// since a real transport would not deliver to them.
    pub fn deliver(&mut self, conn: ConnId, packet: &[u8], cap: Option<u64>) -> Vec<Vec<u8>> {
        let next_client_id = &mut self.next_client_id;
        let c = match self.conns.get_mut(&conn) {
            Some(c) if c.phase != Phase::Closed => c,
            _ => return Vec::new(),
        };
        let (emits, close) = Self::pipeline(c, next_client_id, packet, cap);
        let mut out = Vec::with_capacity(emits.len());
        for (obj, ev) in emits {
            let serial = c.next_server_serial;
            c.next_server_serial += 1;
            out.push(ev.encode(obj, serial));
        }
        if close {
            c.phase = Phase::Closed;
        }
        out
    }

    /// The validation pipeline in the exact priority order of spec §10.
    fn pipeline(c: &mut Connection, ncid: &mut u64, packet: &[u8], cap: Option<u64>) -> Emit {
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
        // BAD_VALUE. The fine-grained tie-break between a malformed string and
        // a stale target object is refined with the string models in M2 item 4.
        let req = match Request::decode(opcode, &packet[crate::wire::HEADER_LEN..]) {
            Ok(r) => r,
            Err(e) => {
                let fatal = e.code == ErrorCode::Malformed;
                return error(header.object, header.serial, e.code, fatal);
            }
        };
        Self::dispatch(c, ncid, &header, req, cap)
    }
}



