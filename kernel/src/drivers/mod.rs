pub mod ata;
pub mod keyboard;
pub mod mouse;

pub fn init() {
    keyboard::init();
    mouse::init();
    ata::init();
}
