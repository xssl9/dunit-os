pub mod pmm;
pub mod vmm;

pub use pmm::MemRegion;

pub(crate) use crate::serial::serial_write;

pub fn init() {
    if !pmm::init() {
        return;
    }

    vmm::init();
    crate::allocator::init();
    vmm::run_address_space_smoke();
}
