#![cfg(test)]

use proptest::prelude::*;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct InterruptFrame {
    r15: u64,
    r14: u64,
    r13: u64,
    r12: u64,
    r11: u64,
    r10: u64,
    r9: u64,
    r8: u64,
    rbp: u64,
    rdi: u64,
    rsi: u64,
    rdx: u64,
    rcx: u64,
    rbx: u64,
    rax: u64,
    int_no: u64,
    err_code: u64,
    rip: u64,
    cs: u64,
    rflags: u64,
    rsp: u64,
    ss: u64,
}

struct KeyboardBuffer {
    buffer: Vec<u8>,
}

impl KeyboardBuffer {
    fn new() -> Self {
        Self { buffer: Vec::new() }
    }

    fn push(&mut self, scancode: u8) {
        self.buffer.push(scancode);
    }

    fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }
}

static mut KEYBOARD_BUFFER: Option<KeyboardBuffer> = None;

fn init_keyboard_buffer() {
    unsafe {
        KEYBOARD_BUFFER = Some(KeyboardBuffer::new());
    }
}

fn handle_keyboard_interrupt(frame: &InterruptFrame) {
    let scancode = (frame.rax & 0xFF) as u8;
    unsafe {
        if let Some(ref mut buffer) = KEYBOARD_BUFFER {
            buffer.push(scancode);
        }
    }
}

fn keyboard_buffer_has_data() -> bool {
    unsafe {
        if let Some(ref buffer) = KEYBOARD_BUFFER {
            !buffer.is_empty()
        } else {
            false
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn prop_keyboard_interrupt_processing(scancode in any::<u8>()) {
        init_keyboard_buffer();

        let frame = InterruptFrame {
            r15: 0,
            r14: 0,
            r13: 0,
            r12: 0,
            r11: 0,
            r10: 0,
            r9: 0,
            r8: 0,
            rbp: 0,
            rdi: 0,
            rsi: 0,
            rdx: 0,
            rcx: 0,
            rbx: 0,
            rax: scancode as u64,
            int_no: 33,
            err_code: 0,
            rip: 0x1000,
            cs: 0x08,
            rflags: 0x202,
            rsp: 0x7fff_ffff_f000,
            ss: 0x10,
        };

        handle_keyboard_interrupt(&frame);

        assert!(keyboard_buffer_has_data(), "Keyboard interrupt should process input and make it available");
    }
}
