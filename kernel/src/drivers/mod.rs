pub mod block;
pub mod keyboard;
pub mod mouse;
pub mod net;
pub mod pci;
pub mod registry;
pub mod usb;

pub fn init() {
    registry::register("fb0", registry::DeviceClass::Framebuffer, "framebuffer");
    registry::register("kbd", registry::DeviceClass::Input, "ps2-keyboard");
    registry::register("mouse", registry::DeviceClass::Input, "ps2-mouse");
    pci::init();
    net::init();
    usb::init();
    block::init();
    keyboard::init();
    mouse::init();
}
