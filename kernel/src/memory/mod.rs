pub mod pmm;
pub mod vmm;

pub use pmm::MemRegion;

pub(crate) use crate::serial::serial_write;

#[must_use]
pub fn init() -> bool {
    if !pmm::init() {
        serial_write("[MEMORY] FATAL: physical memory manager initialization failed\r\n");
        return false;
    }

    vmm::init();
    crate::allocator::init();
    vmm::run_address_space_smoke();
    true
}
