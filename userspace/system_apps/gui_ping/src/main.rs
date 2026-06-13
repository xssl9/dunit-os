#![no_std]
#![no_main]

use core::panic::PanicInfo;

const GUI_SHELL_PID: u32 = 1;
const MESSAGE: &[u8] = b"gui_ping: hello from userspace";

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    loop {
        unsafe {
            core::arch::asm!("pause", options(nomem, nostack));
        }
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    libdunit::println("gui_ping: hello from userspace");
    if libdunit::ipc_send(GUI_SHELL_PID, MESSAGE) != MESSAGE.len() as isize {
        libdunit::println("gui_ping: ipc send failed");
        libdunit::exit(1);
    }
    libdunit::exit(0);
}
