pub mod keyboard;
pub mod mouse;
pub mod pci;
pub mod usb;

pub fn init() {
    pci::init();
    usb::init();
    keyboard::init();
    mouse::init();
}
