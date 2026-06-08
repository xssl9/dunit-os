pub use crate::interrupts::InterruptFrame;

extern "C" {
    pub fn hal_init();
    pub fn hal_init_gdt();
    pub fn hal_init_idt();
    pub fn syscall_init();
    pub fn hal_enable_interrupts();
    pub fn hal_disable_interrupts();
    pub fn hal_set_vga_text_mode();
    pub fn hal_outb(port: u16, value: u8);
    pub fn hal_inb(port: u16) -> u8;
    pub fn hal_outw(port: u16, value: u16);
    pub fn hal_inw(port: u16) -> u16;
    pub fn hal_outl(port: u16, value: u32);
    pub fn hal_inl(port: u16) -> u32;
    pub fn run_user_syscall_smoke(entry: u64, stack_top: u64);
}

#[no_mangle]
pub extern "C" fn interrupt_handler(frame: *const InterruptFrame) {
    let frame = unsafe { &*frame };
    
    use crate::interrupts::*;
    
    match frame.int_no {
        0 => handle_divide_by_zero(frame),
        1 => handle_debug(frame),
        2 => handle_unknown_interrupt(frame),
        3 => handle_breakpoint(frame),
        4 => handle_overflow(frame),
        5 => handle_bound_range(frame),
        6 => handle_invalid_opcode(frame),
        7 => handle_device_not_available(frame),
        8 => handle_double_fault(frame),
        10 => handle_invalid_tss(frame),
        11 => handle_segment_not_present(frame),
        12 => handle_stack_segment_fault(frame),
        13 => handle_general_protection_fault(frame),
        14 => handle_page_fault(frame),
        16 => handle_x87_floating_point(frame),
        17 => handle_alignment_check(frame),
        18 => handle_machine_check(frame),
        19 => handle_simd_floating_point(frame),
        20 => handle_virtualization(frame),
        30 => handle_security_exception(frame),
        32 => handle_timer(frame),
        33 => handle_keyboard(frame),
        _ => handle_unknown_interrupt(frame),
    }
}
