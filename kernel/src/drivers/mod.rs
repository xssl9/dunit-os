pub mod ahci;
pub mod block;
pub mod display;
pub mod keyboard;
pub mod mouse;
pub mod net;
pub mod pci;
pub mod registry;
pub mod usb;
pub mod virtio_blk;

pub fn init() {
    registry::register("fb0", registry::DeviceClass::Framebuffer, "framebuffer");
    registry::register("kbd", registry::DeviceClass::Input, "ps2-keyboard");
    registry::register("mouse", registry::DeviceClass::Input, "ps2-mouse");
    pci::init();
    display::init();
    net::init();
    usb::init();
    block::init();
    ahci::init();
    virtio_blk::init();
    // DunitFS v2 boot self-test (feature-gated): запускается на отдельном
    // экземпляре ФС и полностью отбрасывается ДО auto_mount, чтобы не было двух
    // писателей на одном разделе.
    #[cfg(feature = "boot-smoke-tests")]
    crate::fs::dunitfs::smoke_self_test();
    // Transactional installer self-test (feature-gated): проверяет install →
    // rollback на ПУСТОМ scratch-диске в Live/ISO-окружении (где доступны ESP/BIOS
    // payload'ы), ДО auto_mount. Оставляет на scratch установленную систему, но не
    // монтирует — следующий auto_mount поднимет её на /persist.
    #[cfg(feature = "boot-smoke-tests")]
    crate::storage::installer::installer_self_test();
    if let Some(vfs) = crate::fs::vfs::get_vfs() {
        crate::fs::dunitfs::auto_mount(vfs);
    }
    keyboard::init();
    mouse::init();
}
