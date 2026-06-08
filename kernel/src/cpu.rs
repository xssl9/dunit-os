/// Enable x87/SSE for Rust code compiled with SSE instructions.
pub unsafe fn init_fpu() {
    let mut cr0: u64;
    core::arch::asm!("mov {}, cr0", out(reg) cr0, options(nomem, nostack));
    cr0 &= !(1 << 3); // TS: no longer "task switched"
    cr0 &= !(1 << 2); // EM: no x87 emulation
    cr0 |= 1 << 1; // MP: monitor coprocessor
    core::arch::asm!("mov cr0, {}", in(reg) cr0, options(nomem, nostack));

    let mut cr4: u64;
    core::arch::asm!("mov {}, cr4", out(reg) cr4, options(nomem, nostack));
    cr4 |= 1 << 9; // OSFXSR: enable FXSAVE/FXRSTOR and SSE
    cr4 |= 1 << 10; // OSXMMEXCPT: enable SIMD FP exceptions
    core::arch::asm!("mov cr4, {}", in(reg) cr4, options(nomem, nostack));

    core::arch::asm!("fninit", options(nostack, preserves_flags));
}
