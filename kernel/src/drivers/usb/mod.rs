use crate::serial_write;

pub mod hid_mouse;

pub fn init() {
    serial_write("[USB] HID mouse foundation ready; xHCI enumeration not enabled yet\r\n");
}
