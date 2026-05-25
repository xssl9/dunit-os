/// Enable x87/SSE for Rust code compiled with SSE instructions.
pub unsafe fn init_fpu() {
    let mut cr0: u64;
    core::arch::asm!("mov {}, cr0", out(reg) cr0, options(nomem, nostack));
    cr0 &= !(1 << 3); // TS — no longer "task switched"
    cr0 &= !(1 << 2); // EM — no x87 emulation
    core::arch::asm!("mov cr0, {}", in(reg) cr0, options(nomem, nostack));
    core::arch::asm!("fninit", options(nostack, preserves_flags));
}
