#[repr(C)]
pub struct InterruptFrame {
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub r11: u64,
    pub r10: u64,
    pub r9: u64,
    pub r8: u64,
    pub rbp: u64,
    pub rdi: u64,
    pub rsi: u64,
    pub rdx: u64,
    pub rcx: u64,
    pub rbx: u64,
    pub rax: u64,
    pub int_no: u64,
    pub err_code: u64,
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}

pub fn handle_timer(_frame: &InterruptFrame) {
    unsafe {
        core::arch::asm!("out dx, al", in("dx") 0x20u16, in("al") 0x20u8, options(nomem, nostack));
    }
}

pub fn handle_keyboard(_frame: &InterruptFrame) {
    unsafe {
        let status: u8;
        core::arch::asm!("in al, dx", out("al") status, in("dx") 0x64u16, options(nomem, nostack));
        
        if (status & 0x01) != 0 && (status & 0x20) == 0 {
            let scancode: u8;
            core::arch::asm!("in al, dx", out("al") scancode, in("dx") 0x60u16, options(nomem, nostack));
            crate::drivers::keyboard::push_scancode(scancode);
        }
        
        core::arch::asm!("out dx, al", in("dx") 0x20u16, in("al") 0x20u8, options(nomem, nostack));
    }
}

pub fn handle_mouse(_frame: &InterruptFrame) {
    unsafe {
        for _ in 0..64 {
            let status: u8;
            core::arch::asm!("in al, dx", out("al") status, in("dx") 0x64u16, options(nomem, nostack));

            if (status & 0x01) == 0 {
                break;
            }

            let byte: u8;
            core::arch::asm!("in al, dx", out("al") byte, in("dx") 0x60u16, options(nomem, nostack));
            if (status & 0x20) != 0 {
                crate::drivers::mouse::push_packet_byte(byte);
            } else {
                crate::drivers::keyboard::push_scancode(byte);
            }
        }

        core::arch::asm!("out dx, al", in("dx") 0xA0u16, in("al") 0x20u8, options(nomem, nostack));
        core::arch::asm!("out dx, al", in("dx") 0x20u16, in("al") 0x20u8, options(nomem, nostack));
    }
}

pub fn handle_divide_by_zero(frame: &InterruptFrame) {
    let is_user_mode = (frame.cs & 0x3) == 3;
    
    if is_user_mode {
        terminate_current_process("divide-by-zero", frame, 0, crate::process::ProcessFault::DivideByZero);
    } else {
        panic!("Kernel divide by zero at RIP: {:#x}", frame.rip);
    }
}

pub fn handle_debug(_frame: &InterruptFrame) {
}

pub fn handle_breakpoint(_frame: &InterruptFrame) {
}

pub fn handle_overflow(_frame: &InterruptFrame) {
}

pub fn handle_bound_range(_frame: &InterruptFrame) {
}

pub fn handle_invalid_opcode(frame: &InterruptFrame) {
    let is_user_mode = (frame.cs & 0x3) == 3;
    
    if is_user_mode {
        terminate_current_process("invalid-opcode", frame, 0, crate::process::ProcessFault::InvalidOpcode);
    } else {
        panic!("Kernel invalid opcode at RIP: {:#x}", frame.rip);
    }
}

pub fn handle_device_not_available(_frame: &InterruptFrame) {
    // Clear CR0.TS so SSE/MMX instructions work after #NM.
    unsafe {
        let mut cr0: u64;
        core::arch::asm!("mov {}, cr0", out(reg) cr0);
        cr0 &= !(1 << 3);
        core::arch::asm!("mov cr0, {}", in(reg) cr0);
        core::arch::asm!("fninit", options(nostack, preserves_flags));
    }
}

pub fn handle_double_fault(frame: &InterruptFrame) {
    panic!("Double fault at RIP: {:#x}, error code: {:#x}", frame.rip, frame.err_code);
}

pub fn handle_invalid_tss(frame: &InterruptFrame) {
    panic!("Invalid TSS at RIP: {:#x}, error code: {:#x}", frame.rip, frame.err_code);
}

pub fn handle_segment_not_present(frame: &InterruptFrame) {
    panic!("Segment not present at RIP: {:#x}, error code: {:#x}", frame.rip, frame.err_code);
}

pub fn handle_stack_segment_fault(frame: &InterruptFrame) {
    panic!("Stack segment fault at RIP: {:#x}, error code: {:#x}", frame.rip, frame.err_code);
}

pub fn handle_general_protection_fault(frame: &InterruptFrame) {
    let is_user_mode = (frame.cs & 0x3) == 3;
    
    if is_user_mode {
        terminate_current_process("general-protection", frame, 0, crate::process::ProcessFault::GeneralProtection);
    } else {
        panic!("Kernel general protection fault at RIP: {:#x}, error code: {:#x}", frame.rip, frame.err_code);
    }
}

pub fn handle_page_fault(frame: &InterruptFrame) {
    let cr2: u64;
    unsafe {
        core::arch::asm!("mov {}, cr2", out(reg) cr2);
    }
    
    let is_user_mode = (frame.cs & 0x3) == 3;
    
    if is_user_mode {
        terminate_current_process("page-fault", frame, cr2, crate::process::ProcessFault::PageFault);
    } else {
        panic!("Kernel page fault at RIP: {:#x}, address: {:#x}, error code: {:#x}", frame.rip, cr2, frame.err_code);
    }
}

pub fn handle_x87_floating_point(_frame: &InterruptFrame) {
}

pub fn handle_alignment_check(frame: &InterruptFrame) {
    panic!("Alignment check at RIP: {:#x}, error code: {:#x}", frame.rip, frame.err_code);
}

pub fn handle_machine_check(frame: &InterruptFrame) {
    panic!("Machine check at RIP: {:#x}", frame.rip);
}

pub fn handle_simd_floating_point(_frame: &InterruptFrame) {
}

pub fn handle_virtualization(_frame: &InterruptFrame) {
}

pub fn handle_security_exception(frame: &InterruptFrame) {
    panic!("Security exception at RIP: {:#x}, error code: {:#x}", frame.rip, frame.err_code);
}

pub fn handle_unknown_interrupt(frame: &InterruptFrame) {
    if frame.int_no >= 40 && frame.int_no < 48 {
        unsafe {
            core::arch::asm!("out dx, al", in("dx") 0xA0u16, in("al") 0x20u8, options(nomem, nostack));
            core::arch::asm!("out dx, al", in("dx") 0x20u16, in("al") 0x20u8, options(nomem, nostack));
        }
    } else if frame.int_no >= 32 && frame.int_no < 40 {
        unsafe {
            core::arch::asm!("out dx, al", in("dx") 0x20u16, in("al") 0x20u8, options(nomem, nostack));
        }
    }
}

fn terminate_current_process(
    reason: &str,
    frame: &InterruptFrame,
    fault_addr: u64,
    fault: crate::process::ProcessFault,
) {
    let pid = crate::process::current_process()
        .map(|process| process.pid.0)
        .unwrap_or(0);

    crate::memory::serial_write("[USER-FAULT] pid=");
    serial_write_u64(pid);
    crate::memory::serial_write(" reason=");
    crate::memory::serial_write(reason);
    crate::memory::serial_write(" rip=");
    serial_write_hex(frame.rip);
    crate::memory::serial_write(" rsp=");
    serial_write_hex(frame.rsp);
    if fault_addr != 0 {
        crate::memory::serial_write(" addr=");
        serial_write_hex(fault_addr);
    }
    crate::memory::serial_write(" err=");
    serial_write_hex(frame.err_code);
    crate::memory::serial_write("\r\n");

    let _ = crate::process::request_current_user_fault(fault);
}

fn serial_write_u64(mut value: u64) {
    let mut buf = [0u8; 20];
    let mut index = buf.len();

    if value == 0 {
        crate::memory::serial_write("0");
        return;
    }

    while value > 0 {
        index -= 1;
        buf[index] = b'0' + (value % 10) as u8;
        value /= 10;
    }

    for byte in &buf[index..] {
        let ch = [*byte];
        let s = unsafe { core::str::from_utf8_unchecked(&ch) };
        crate::memory::serial_write(s);
    }
}

fn serial_write_hex(value: u64) {
    crate::memory::serial_write("0x");
    let mut started = false;
    for shift in (0..64).step_by(4).rev() {
        let nibble = ((value >> shift) & 0xF) as u8;
        if nibble != 0 || started || shift == 0 {
            started = true;
            let byte = if nibble < 10 { b'0' + nibble } else { b'a' + (nibble - 10) };
            let ch = [byte];
            let s = unsafe { core::str::from_utf8_unchecked(&ch) };
            crate::memory::serial_write(s);
        }
    }
}
