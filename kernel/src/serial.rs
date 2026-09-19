//! COM1 (0x3F8) serial output — the single source of truth for kernel serial
//! logging. Every subsystem routes through `serial_write` here instead of
//! carrying its own copy of the port I/O loop.

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
