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

pub fn handle_divide_by_zero(frame: &InterruptFrame) {
    let is_user_mode = (frame.cs & 0x3) == 3;
    
    if is_user_mode {
        terminate_current_process();
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
        terminate_current_process();
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
        terminate_current_process();
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
        terminate_current_process();
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
}

fn terminate_current_process() {
}
