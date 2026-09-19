//! COM1 (0x3F8) serial output — the single source of truth for kernel serial
//! logging. Every subsystem routes through `serial_write` here instead of
//! carrying its own copy of the port I/O loop.

use core::sync::atomic::{AtomicBool, Ordering};

/// When false (default), high-frequency debug traces on hot paths
/// (scheduling, process state transitions) are suppressed. Unconditional
/// diagnostics still go out via `serial_write`.
static DEBUG_TRACE: AtomicBool = AtomicBool::new(false);

/// Enable/disable high-frequency debug tracing at runtime.
pub fn set_debug_trace(enabled: bool) {
    DEBUG_TRACE.store(enabled, Ordering::Relaxed);
}

/// Whether high-frequency debug tracing is currently enabled.
#[inline]
pub fn debug_trace_enabled() -> bool {
    DEBUG_TRACE.load(Ordering::Relaxed)
}

/// Write a string only when debug tracing is enabled.
#[inline]
pub fn trace(s: &str) {
    if debug_trace_enabled() {
        serial_write(s);
    }
}

/// Write one byte to COM1, spinning until the transmitter holding register is
/// empty.
#[inline]
pub fn serial_write_byte(byte: u8) {
    unsafe {
        loop {
            let status: u8;
            core::arch::asm!(
                "in al, dx",
                out("al") status,
                in("dx") 0x3FDu16,
                options(nomem, nostack)
            );
            if (status & 0x20) != 0 {
                break;
            }
        }
        core::arch::asm!(
            "out dx, al",
            in("dx") 0x3F8u16,
            in("al") byte,
            options(nomem, nostack)
        );
    }
}

/// Write a UTF-8 string to COM1 byte-for-byte.
pub fn serial_write(s: &str) {
    for byte in s.bytes() {
        serial_write_byte(byte);
    }
}

/// Write `value` as `digits` lowercase hex digits (zero-padded, no `0x`).
pub fn write_hex(mut value: u64, digits: usize) {
    let mut buf = [0u8; 16];
    let width = digits.min(buf.len());
    let mut index = width;
    while index > 0 {
        index -= 1;
        let nibble = (value & 0xF) as u8;
        buf[index] = if nibble < 10 {
            b'0' + nibble
        } else {
            b'a' + nibble - 10
        };
        value >>= 4;
    }
    if let Ok(text) = core::str::from_utf8(&buf[..width]) {
        serial_write(text);
    }
}

/// Write an 8-bit value as two hex digits.
#[inline]
pub fn write_hex8(value: u8) {
    write_hex(value as u64, 2);
}

/// Write a 16-bit value as four hex digits.
#[inline]
pub fn write_hex16(value: u16) {
    write_hex(value as u64, 4);
}

/// Write an unsigned value in decimal.
pub fn write_dec(mut value: u64) {
    if value == 0 {
        serial_write("0");
        return;
    }
    let mut buf = [0u8; 20];
    let mut index = buf.len();
    while value > 0 {
        index -= 1;
        buf[index] = b'0' + (value % 10) as u8;
        value /= 10;
    }
    if let Ok(text) = core::str::from_utf8(&buf[index..]) {
        serial_write(text);
    }
}

/// Write a MAC address from the e1000 RAL/RAH register pair as `aa:bb:...`.
pub fn write_mac(ral: u32, rah: u32) {
    let bytes = [
        (ral & 0xFF) as u8,
        ((ral >> 8) & 0xFF) as u8,
        ((ral >> 16) & 0xFF) as u8,
        ((ral >> 24) & 0xFF) as u8,
        (rah & 0xFF) as u8,
        ((rah >> 8) & 0xFF) as u8,
    ];
    for (index, byte) in bytes.iter().enumerate() {
        if index != 0 {
            serial_write(":");
        }
        write_hex8(*byte);
    }
}
