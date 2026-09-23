//! Wire codec and framing validation for Dunit GUI protocol v1.0.
//!
//! All integers are little-endian and fixed width; fields are packed with no
//! implicit padding and every reserved field is zero (spec §2). Decoding is
//! total: malformed input yields a [`WireError`] and never panics, so an
//! invalid client cannot destabilise the server (spec §10).

use alloc::string::String;
use alloc::vec::Vec;

/// `DGUI` as a little-endian `u32` (bytes `44 47 55 49`).
pub const MAGIC: u32 = 0x4955_4744;
/// Fixed header length in bytes.
pub const HEADER_LEN: usize = 32;
/// Smallest legal packet (header only), spec §8.
pub const MIN_PACKET: usize = 32;
/// Largest legal packet, spec §8.
pub const MAX_PACKET: usize = 256;

pub const VERSION_MAJOR: u16 = 1;
pub const VERSION_MINOR: u16 = 0;

/// Feature bit 0 = ARGB8888 (spec §3). XRGB8888 needs no feature bit.
pub const FEATURE_ARGB8888: u64 = 1 << 0;

/// Pixel formats (spec §8).
pub const FORMAT_XRGB8888: u32 = 1;
pub const FORMAT_ARGB8888: u32 = 2;
/// `formats` bitmask reported in WELCOME (spec §3).
pub const FORMATS_XRGB8888: u32 = 1 << 0;
pub const FORMATS_ARGB8888: u32 = 1 << 1;

/// Only surface role in v1.0 (spec §4).
pub const ROLE_TOPLEVEL: u32 = 1;

/// Reserved DWM policy opcode range (spec §9): rejected with ACCESS_DENIED.
pub const POLICY_OPCODE_LO: u16 = 0x1000;
pub const POLICY_OPCODE_HI: u16 = 0x1fff;

/// Dimension bounds shared by logical and pixel sizes (spec §8).
pub const DIM_MIN: u32 = 1;
pub const DIM_MAX: u32 = 4096;
/// Maximum damage rectangles per attach (spec §8).
pub const MAX_DAMAGE_RECTS: u32 = 8;

/// Protocol error codes (spec §10). Positive `u32`, distinct from kernel errno.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum ErrorCode {
    Malformed = 1,
    VersionMismatch = 2,
    BadOpcode = 3,
    BadState = 4,
    StaleObject = 5,
    WrongObjectType = 6,
    BadSerial = 7,
    BadValue = 8,
    Unsupported = 9,
    AccessDenied = 10,
    OutOfBounds = 11,
    Busy = 12,
    LimitExceeded = 13,
    NoMemory = 14,
    SlowClient = 15,
    Internal = 16,
}

impl ErrorCode {
    /// Numeric wire value.
    pub const fn code(self) -> u32 {
        self as u32
    }

    /// Whether this error is *unconditionally* fatal (spec §10 table). Codes
    /// whose fatality depends on context (BAD_STATE in NEW, BAD_SERIAL on a bad
    /// header serial, UNSUPPORTED during HELLO, input-overflow LIMIT_EXCEEDED)
    /// are marked non-fatal here and promoted to fatal by the server at the
    /// site that has the context.
    pub const fn always_fatal(self) -> bool {
        matches!(
            self,
            ErrorCode::Malformed
                | ErrorCode::VersionMismatch
                | ErrorCode::BadOpcode
                | ErrorCode::SlowClient
                | ErrorCode::Internal
        )
    }
}

/// Every v1.0 opcode, both directions (spec §4, §5). Request opcodes are in the
/// low range; replies/events have bit 0x8000 set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum Opcode {
    // Requests (client -> server).
    Hello = 0x0001,
    Disconnect = 0x0002,
    CreateSurface = 0x0010,
    DestroySurface = 0x0011,
    SetTitle = 0x0012,
    SetAppId = 0x0013,
    AckConfigure = 0x0014,
    ImportBuffer = 0x0020,
    DestroyBuffer = 0x0021,
    AttachBuffer = 0x0022,
    Commit = 0x0023,
    // Replies / events (server -> client).
    Welcome = 0x8001,
    Result = 0x8002,
    Error = 0x8003,
    Configure = 0x8010,
    RequestClose = 0x8011,
    BufferRelease = 0x8020,
    FrameDone = 0x8021,
    PointerEnter = 0x8030,
    PointerLeave = 0x8031,
    PointerMotion = 0x8032,
    PointerButton = 0x8033,
    PointerAxis = 0x8034,
    KeyEnter = 0x8040,
    KeyLeave = 0x8041,
    Key = 0x8042,
    TextInput = 0x8043,
}

impl Opcode {
    pub const fn raw(self) -> u16 {
        self as u16
    }

    /// Recognise a known opcode. Unknown opcodes are rejected as BAD_OPCODE by
    /// the caller; the reserved policy range is handled separately as
    /// ACCESS_DENIED (spec §9, §10).
    pub fn from_u16(v: u16) -> Option<Opcode> {
        use Opcode::*;
        Some(match v {
            0x0001 => Hello,
            0x0002 => Disconnect,
            0x0010 => CreateSurface,
            0x0011 => DestroySurface,
            0x0012 => SetTitle,
            0x0013 => SetAppId,
            0x0014 => AckConfigure,
            0x0020 => ImportBuffer,
            0x0021 => DestroyBuffer,
            0x0022 => AttachBuffer,
            0x0023 => Commit,
            0x8001 => Welcome,
            0x8002 => Result,
            0x8003 => Error,
            0x8010 => Configure,
            0x8011 => RequestClose,
            0x8020 => BufferRelease,
            0x8021 => FrameDone,
            0x8030 => PointerEnter,
            0x8031 => PointerLeave,
            0x8032 => PointerMotion,
            0x8033 => PointerButton,
            0x8034 => PointerAxis,
            0x8040 => KeyEnter,
            0x8041 => KeyLeave,
            0x8042 => Key,
            0x8043 => TextInput,
            _ => return None,
        })
    }

    /// True for client -> server opcodes (no 0x8000 bit).
    pub const fn is_request(self) -> bool {
        (self as u16) & 0x8000 == 0
    }
}

/// A rectangle as encoded on the wire: `x:i32, y:i32, w:u32, h:u32` (spec §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

/// A decode failure carrying the protocol error it maps to. Decoding is total:
/// this is returned instead of panicking on any malformed input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WireError {
    pub code: ErrorCode,
    pub reason: &'static str,
}

impl WireError {
    const fn new(code: ErrorCode, reason: &'static str) -> WireError {
        WireError { code, reason }
    }
    const fn malformed(reason: &'static str) -> WireError {
        WireError::new(ErrorCode::Malformed, reason)
    }
    const fn bad_value(reason: &'static str) -> WireError {
        WireError::new(ErrorCode::BadValue, reason)
    }
}

#[inline]
fn rd_u16(b: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([b[off], b[off + 1]])
}
#[inline]
fn rd_u32(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])
}
#[inline]
fn rd_u64(b: &[u8], off: usize) -> u64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&b[off..off + 8]);
    u64::from_le_bytes(a)
}

/// Parsed fixed header (spec §2). `opcode_raw` is kept unparsed so framing can
/// be validated before opcode direction/policy checks (spec §10 ordering).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub major: u16,
    pub minor: u16,
    pub opcode_raw: u16,
    pub size: u32,
    pub object: u64,
    pub serial: u64,
}

impl Header {
    /// Decode and framing-validate a whole packet: magic, `size` matches the
    /// buffer length exactly (no trailing bytes, no truncation), packet length
    /// within `[MIN_PACKET, MAX_PACKET]`, and `flags == 0`. This is the first
    /// validation stage (spec §10) and runs before any allocation.
    pub fn decode(packet: &[u8]) -> Result<Header, WireError> {
        if packet.len() < HEADER_LEN {
            return Err(WireError::malformed("short header"));
        }
        if packet.len() < MIN_PACKET || packet.len() > MAX_PACKET {
            return Err(WireError::malformed("packet length out of range"));
        }
        if rd_u32(packet, 0) != MAGIC {
            return Err(WireError::malformed("bad magic"));
        }
        let flags = rd_u16(packet, 10);
        if flags != 0 {
            return Err(WireError::malformed("nonzero flags"));
        }
        let size = rd_u32(packet, 12);
        if size as usize != packet.len() {
            return Err(WireError::malformed("size mismatch / trailing bytes"));
        }
        Ok(Header {
            major: rd_u16(packet, 4),
            minor: rd_u16(packet, 6),
            opcode_raw: rd_u16(packet, 8),
            size,
            object: rd_u64(packet, 16),
            serial: rd_u64(packet, 24),
        })
    }
}

/// Bounded cursor over a request payload. Every read is length-checked and the
/// payload must be consumed exactly ([`Cursor::finish`]), so both truncated and
/// trailing-byte payloads are rejected as MALFORMED (spec §2, §10).
struct Cursor<'a> {
    b: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(b: &'a [u8]) -> Cursor<'a> {
        Cursor { b, pos: 0 }
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8], WireError> {
        let end = self
            .pos
            .checked_add(n)
            .ok_or(WireError::malformed("payload overflow"))?;
        if end > self.b.len() {
            return Err(WireError::malformed("payload truncated"));
        }
        let s = &self.b[self.pos..end];
        self.pos = end;
        Ok(s)
    }
    fn u16(&mut self) -> Result<u16, WireError> {
        Ok(rd_u16(self.take(2)?, 0))
    }
    fn u32(&mut self) -> Result<u32, WireError> {
        Ok(rd_u32(self.take(4)?, 0))
    }
    fn u64(&mut self) -> Result<u64, WireError> {
        Ok(rd_u64(self.take(8)?, 0))
    }
    fn i32(&mut self) -> Result<i32, WireError> {
        Ok(self.u32()? as i32)
    }
    /// Reserved field: present on the wire, must be zero (spec §2).
    fn reserved_u32(&mut self) -> Result<(), WireError> {
        if self.u32()? != 0 {
            return Err(WireError::malformed("nonzero reserved"));
        }
        Ok(())
    }
    /// `str` = `length:u16` + exactly `length` UTF-8 bytes, no NUL, no padding.
    fn take_str(&mut self) -> Result<String, WireError> {
        let len = self.u16()? as usize;
        let bytes = self.take(len)?;
        if bytes.contains(&0) {
            return Err(WireError::bad_value("NUL in string"));
        }
        let s = core::str::from_utf8(bytes).map_err(|_| WireError::bad_value("invalid UTF-8"))?;
        Ok(String::from(s))
    }
    fn finish(self) -> Result<(), WireError> {
        if self.pos != self.b.len() {
            Err(WireError::malformed("trailing payload bytes"))
        } else {
            Ok(())
        }
    }
}

/// A decoded, structurally-valid client request (spec §4). Structural decode
/// checks framing, exact payload length, reserved-zero and string well-formed;
/// semantic checks (enum values, geometry, quotas, object state) belong to
/// later validation stages performed by [`crate::server`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    Hello {
        min_minor: u16,
        max_minor: u16,
        offered_features: u64,
        required_features: u64,
    },
    Disconnect,
    CreateSurface {
        role: u32,
        width: u32,
        height: u32,
        format: u32,
    },
    DestroySurface,
    SetTitle(String),
    SetAppId(String),
    AckConfigure {
        configure: u64,
    },
    ImportBuffer {
        width: u32,
        height: u32,
        stride: u32,
        format: u32,
        offset: u64,
    },
    DestroyBuffer,
    AttachBuffer {
        buffer: u64,
        damage: Vec<Rect>,
    },
    Commit {
        configure: u64,
        frame_callback: u32,
    },
}

impl Request {
    /// Structurally decode a request payload for a known request `opcode`.
    /// Length, reserved-zero, string validity and `damage_count`/array
    /// agreement are enforced here; enum/geometry/quota semantics are not.
    pub fn decode(opcode: Opcode, payload: &[u8]) -> Result<Request, WireError> {
        if !opcode.is_request() {
            return Err(WireError::new(ErrorCode::BadOpcode, "not a request opcode"));
        }
        let mut c = Cursor::new(payload);
        let req = match opcode {
            Opcode::Hello => {
                let min_minor = c.u16()?;
                let max_minor = c.u16()?;
                c.reserved_u32()?;
                let offered_features = c.u64()?;
                let required_features = c.u64()?;
                Request::Hello {
                    min_minor,
                    max_minor,
                    offered_features,
                    required_features,
                }
            }
            Opcode::Disconnect => Request::Disconnect,
            Opcode::CreateSurface => Request::CreateSurface {
                role: c.u32()?,
                width: c.u32()?,
                height: c.u32()?,
                format: c.u32()?,
            },
            Opcode::DestroySurface => Request::DestroySurface,
            Opcode::SetTitle => Request::SetTitle(c.take_str()?),
            Opcode::SetAppId => Request::SetAppId(c.take_str()?),
            Opcode::AckConfigure => Request::AckConfigure { configure: c.u64()? },
            Opcode::ImportBuffer => Request::ImportBuffer {
                width: c.u32()?,
                height: c.u32()?,
                stride: c.u32()?,
                format: c.u32()?,
                offset: c.u64()?,
            },
            Opcode::DestroyBuffer => Request::DestroyBuffer,
            Opcode::AttachBuffer => {
                let buffer = c.u64()?;
                let damage_count = c.u32()?;
                c.reserved_u32()?;
                let mut damage = Vec::new();
                for _ in 0..damage_count {
                    damage.push(Rect {
                        x: c.i32()?,
                        y: c.i32()?,
                        w: c.u32()?,
                        h: c.u32()?,
                    });
                }
                Request::AttachBuffer { buffer, damage }
            }
            Opcode::Commit => {
                let configure = c.u64()?;
                let frame_callback = c.u32()?;
                c.reserved_u32()?;
                Request::Commit {
                    configure,
                    frame_callback,
                }
            }
            _ => return Err(WireError::new(ErrorCode::BadOpcode, "not a request opcode")),
        };
        c.finish()?;
        Ok(req)
    }

    /// Opcode this request encodes to.
    pub fn opcode(&self) -> Opcode {
        match self {
            Request::Hello { .. } => Opcode::Hello,
            Request::Disconnect => Opcode::Disconnect,
            Request::CreateSurface { .. } => Opcode::CreateSurface,
            Request::DestroySurface => Opcode::DestroySurface,
            Request::SetTitle(_) => Opcode::SetTitle,
            Request::SetAppId(_) => Opcode::SetAppId,
            Request::AckConfigure { .. } => Opcode::AckConfigure,
            Request::ImportBuffer { .. } => Opcode::ImportBuffer,
            Request::DestroyBuffer => Opcode::DestroyBuffer,
            Request::AttachBuffer { .. } => Opcode::AttachBuffer,
            Request::Commit { .. } => Opcode::Commit,
        }
    }

    /// Encode into a full wire packet with the given header `object` and client
    /// `serial`. Provided so clients (and the test suite) can build conformant
    /// requests; the result always satisfies [`Header::decode`] for legal sizes.
    pub fn encode(&self, object: u64, serial: u64) -> Vec<u8> {
        let mut w = Writer::header(self.opcode(), object, serial);
        match self {
            Request::Hello {
                min_minor,
                max_minor,
                offered_features,
                required_features,
            } => {
                w.u16(*min_minor)
                    .u16(*max_minor)
                    .reserved_u32()
                    .u64(*offered_features)
                    .u64(*required_features);
            }
            Request::Disconnect | Request::DestroySurface | Request::DestroyBuffer => {}
            Request::CreateSurface {
                role,
                width,
                height,
                format,
            } => {
                w.u32(*role).u32(*width).u32(*height).u32(*format);
            }
            Request::SetTitle(s) | Request::SetAppId(s) => {
                w.str(s);
            }
            Request::AckConfigure { configure } => {
                w.u64(*configure);
            }
            Request::ImportBuffer {
                width,
                height,
                stride,
                format,
                offset,
            } => {
                w.u32(*width)
                    .u32(*height)
                    .u32(*stride)
                    .u32(*format)
                    .u64(*offset);
            }
            Request::AttachBuffer { buffer, damage } => {
                w.u64(*buffer).u32(damage.len() as u32).reserved_u32();
                for r in damage {
                    w.i32(r.x).i32(r.y).u32(r.w).u32(r.h);
                }
            }
            Request::Commit {
                configure,
                frame_callback,
            } => {
                w.u64(*configure).u32(*frame_callback).reserved_u32();
            }
        }
        w.finish()
    }
}


/// Little-endian packet writer used to encode replies and events.
struct Writer {
    buf: Vec<u8>,
}

impl Writer {
    fn header(opcode: Opcode, object: u64, serial: u64) -> Writer {
        let mut buf = Vec::with_capacity(HEADER_LEN);
        buf.extend_from_slice(&MAGIC.to_le_bytes());
        buf.extend_from_slice(&VERSION_MAJOR.to_le_bytes());
        buf.extend_from_slice(&VERSION_MINOR.to_le_bytes());
        buf.extend_from_slice(&opcode.raw().to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes()); // flags
        buf.extend_from_slice(&0u32.to_le_bytes()); // size patched in finish()
        buf.extend_from_slice(&object.to_le_bytes());
        buf.extend_from_slice(&serial.to_le_bytes());
        Writer { buf }
    }
    fn u16(&mut self, v: u16) -> &mut Self {
        self.buf.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn u32(&mut self, v: u32) -> &mut Self {
        self.buf.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn i32(&mut self, v: i32) -> &mut Self {
        self.buf.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn u64(&mut self, v: u64) -> &mut Self {
        self.buf.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn reserved_u32(&mut self) -> &mut Self {
        self.u32(0)
    }
    fn str(&mut self, s: &str) -> &mut Self {
        self.buf
            .extend_from_slice(&(s.len() as u16).to_le_bytes());
        self.buf.extend_from_slice(s.as_bytes());
        self
    }
    fn finish(mut self) -> Vec<u8> {
        let size = self.buf.len() as u32;
        self.buf[12..16].copy_from_slice(&size.to_le_bytes());
        self.buf
    }
}

/// A reply or event the server sends to the client (spec §5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Welcome {
        request: u64,
        client_id: u64,
        features: u64,
        formats: u32,
    },
    Result {
        request: u64,
    },
    Error {
        request: u64,
        code: u32,
        fatal: u32,
    },
    Configure {
        width: u32,
        height: u32,
        scale: u32,
        state: u32,
    },
    RequestClose,
    BufferRelease {
        commit: u64,
    },
    FrameDone {
        commit: u64,
        time_ns: u64,
        status: u32,
    },
    PointerEnter {
        x: i32,
        y: i32,
    },
    PointerLeave,
    PointerMotion {
        time_ns: u64,
        x: i32,
        y: i32,
    },
    PointerButton {
        time_ns: u64,
        button: u32,
        pressed: u32,
    },
    PointerAxis {
        time_ns: u64,
        dx: i32,
        dy: i32,
    },
    KeyEnter {
        modifiers: u32,
    },
    KeyLeave,
    Key {
        time_ns: u64,
        usage: u32,
        pressed: u32,
        modifiers: u32,
        repeat: u32,
    },
    TextInput {
        time_ns: u64,
        text: String,
    },
}

impl Event {
    /// Opcode this event encodes to.
    pub fn opcode(&self) -> Opcode {
        match self {
            Event::Welcome { .. } => Opcode::Welcome,
            Event::Result { .. } => Opcode::Result,
            Event::Error { .. } => Opcode::Error,
            Event::Configure { .. } => Opcode::Configure,
            Event::RequestClose => Opcode::RequestClose,
            Event::BufferRelease { .. } => Opcode::BufferRelease,
            Event::FrameDone { .. } => Opcode::FrameDone,
            Event::PointerEnter { .. } => Opcode::PointerEnter,
            Event::PointerLeave => Opcode::PointerLeave,
            Event::PointerMotion { .. } => Opcode::PointerMotion,
            Event::PointerButton { .. } => Opcode::PointerButton,
            Event::PointerAxis { .. } => Opcode::PointerAxis,
            Event::KeyEnter { .. } => Opcode::KeyEnter,
            Event::KeyLeave => Opcode::KeyLeave,
            Event::Key { .. } => Opcode::Key,
            Event::TextInput { .. } => Opcode::TextInput,
        }
    }

    /// Encode into a full wire packet with the given header `object` and server
    /// `serial`. The result always satisfies [`Header::decode`].
    pub fn encode(&self, object: u64, serial: u64) -> Vec<u8> {
        let mut w = Writer::header(self.opcode(), object, serial);
        match self {
            Event::Welcome {
                request,
                client_id,
                features,
                formats,
            } => {
                w.u64(*request)
                    .u64(*client_id)
                    .u64(*features)
                    .u32(*formats)
                    .reserved_u32();
            }
            Event::Result { request } => {
                w.u64(*request);
            }
            Event::Error {
                request,
                code,
                fatal,
            } => {
                w.u64(*request).u32(*code).u32(*fatal);
            }
            Event::Configure {
                width,
                height,
                scale,
                state,
            } => {
                w.u32(*width).u32(*height).u32(*scale).u32(*state);
            }
            Event::RequestClose | Event::PointerLeave | Event::KeyLeave => {}
            Event::BufferRelease { commit } => {
                w.u64(*commit);
            }
            Event::FrameDone {
                commit,
                time_ns,
                status,
            } => {
                w.u64(*commit).u64(*time_ns).u32(*status).reserved_u32();
            }
            Event::PointerEnter { x, y } => {
                w.i32(*x).i32(*y);
            }
            Event::PointerMotion { time_ns, x, y } => {
                w.u64(*time_ns).i32(*x).i32(*y);
            }
            Event::PointerButton {
                time_ns,
                button,
                pressed,
            } => {
                w.u64(*time_ns).u32(*button).u32(*pressed);
            }
            Event::PointerAxis { time_ns, dx, dy } => {
                w.u64(*time_ns).i32(*dx).i32(*dy);
            }
            Event::KeyEnter { modifiers } => {
                w.u32(*modifiers).reserved_u32();
            }
            Event::Key {
                time_ns,
                usage,
                pressed,
                modifiers,
                repeat,
            } => {
                w.u64(*time_ns)
                    .u32(*usage)
                    .u32(*pressed)
                    .u32(*modifiers)
                    .u32(*repeat);
            }
            Event::TextInput { time_ns, text } => {
                w.u64(*time_ns).str(text);
            }
        }
        w.finish()
    }
}







