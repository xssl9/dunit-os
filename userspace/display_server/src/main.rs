#![no_std]
#![no_main]

extern crate alloc;

use display_server::DisplayServer;

#[no_mangle]
pub extern "C" fn main() -> i32 {
    let mut server = DisplayServer::new();
    
    0
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
