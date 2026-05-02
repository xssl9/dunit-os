pub mod keyboard;
pub mod mouse;

pub fn init() {
    keyboard::init();
    mouse::init();
}
