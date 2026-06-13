#![no_std]
#![no_main]

use core::panic::PanicInfo;

const WINDOW_ID: u32 = 1;

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    loop {
        unsafe {
            core::arch::asm!("pause", options(nomem, nostack));
        }
    }
}

fn append_key_line(out: &mut [u8], key: u8) -> &str {
    let prefix = b"key event received: ";
    let mut index = 0usize;
    while index < prefix.len() {
        out[index] = prefix[index];
        index += 1;
    }
    out[index] = key;
    index += 1;
    unsafe { core::str::from_utf8_unchecked(&out[..index]) }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    libdunit::println("gui_terminal_stub: start");
    libdunit::gui_create_window(WINDOW_ID, "Terminal", 420, 260);
    libdunit::gui_draw_text(WINDOW_ID, 16, 24, "Dunit GUI Terminal");
    libdunit::gui_set_status("gui_terminal_stub: waiting for KEY_EVENT");
    libdunit::yield_now();

    loop {
        let mut event = libdunit::GuiMessage::new(0);
        let received = libdunit::gui_recv_event(&mut event);
        if received < 0 {
            libdunit::yield_now();
            continue;
        }

        if event.kind == libdunit::GUI_MSG_KEY_EVENT && event.window_id == WINDOW_ID {
            let key = event.a as u8;
            let mut line = [0u8; 64];
            let text = append_key_line(&mut line, key);
            libdunit::println("gui_terminal_stub: received KEY_EVENT");
            libdunit::gui_draw_text(WINDOW_ID, 16, 48, text);
            libdunit::gui_set_status("gui_terminal_stub: KEY_EVENT handled");
            libdunit::exit(0);
        }
    }
}
