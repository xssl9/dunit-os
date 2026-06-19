use crate::serial_write;

pub mod hid_mouse;
pub mod xhci;

pub fn init() {
    serial_write("[USB] initializing xHCI host controllers\r\n");
    xhci::init();
    serial_write("[USB] HID mouse report parser ready; device enumeration is next\r\n");
}
