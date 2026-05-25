pub mod pmm;
pub mod vmm;

pub use pmm::MemRegion;

pub(crate) fn serial_write(s: &str) {
    for byte in s.bytes() {
        unsafe {
            loop {
                let mut status: u8;
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
}

pub fn init() {
    if !pmm::init() {
        return;
    }

    vmm::init();
    crate::allocator::init();
}
