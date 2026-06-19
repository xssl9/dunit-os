#![no_std]
#![no_main]

use core::panic::PanicInfo;

const WINDOW_ID: u32 = 1;
const WIDTH: u32 = 560;
const HEIGHT: u32 = 320;
const INPUT_CAP: usize = 96;
const PROMPT_Y: i32 = 0;

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    loop {
        unsafe {
            core::arch::asm!("pause", options(nomem, nostack));
        }
    }
}

struct TerminalInput {
    input: [u8; INPUT_CAP],
    input_len: usize,
}

impl TerminalInput {
    const fn new() -> Self {
        Self {
            input: [0; INPUT_CAP],
            input_len: 0,
        }
    }

    fn redraw_prompt(&self) {
        let mut line = [b' '; 112];
        let mut len = 0usize;
        append_bytes(&mut line, &mut len, b"root@dunit:# ");
        append_bytes(&mut line, &mut len, &self.input[..self.input_len]);
        libdunit::gui_draw_text(WINDOW_ID, 0, PROMPT_Y, line_str(&line, line.len()));
    }

    fn handle_key(&mut self, key: u8) -> bool {
        match key {
            b'\n' => self.submit_line(),
            8 => {
                if self.input_len > 0 {
                    self.input_len -= 1;
                    self.redraw_prompt();
                }
                false
            }
            byte if byte >= 0x20 && byte < 0x7f => {
                if self.input_len < self.input.len() {
                    self.input[self.input_len] = byte;
                    self.input_len += 1;
                    self.redraw_prompt();
                }
                false
            }
            _ => false,
        }
    }

    fn submit_line(&mut self) -> bool {
        let mut command = libdunit::GuiMessage::new(libdunit::GUI_MSG_COMMAND);
        command.window_id = WINDOW_ID;
        command.set_data(&self.input[..self.input_len]);
        libdunit::gui_send(&command);

        let input = line_str(&self.input, self.input_len);
        let should_exit = input == "exit";
        self.input_len = 0;
        self.redraw_prompt();
        if should_exit {
            send_exit();
        }
        should_exit
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    libdunit::println("gui_terminal_stub: start");
    libdunit::gui_create_window(WINDOW_ID, "Terminal", WIDTH, HEIGHT);
    libdunit::gui_set_status("gui_terminal_stub: running");

    let mut terminal = TerminalInput::new();
    terminal.redraw_prompt();

    loop {
        let mut event = libdunit::GuiMessage::new(0);
        let received = libdunit::gui_recv_event(&mut event);
        if received < 0 {
            libdunit::yield_now();
            continue;
        }
        if event.window_id != WINDOW_ID {
            continue;
        }

        match event.kind {
            libdunit::GUI_MSG_KEY_EVENT => {
                if terminal.handle_key(event.a as u8) {
                    libdunit::exit(0);
                }
            }
            libdunit::GUI_MSG_CLOSE_EVENT => {
                libdunit::println("gui_terminal_stub: received CLOSE_EVENT");
                send_exit();
                libdunit::exit(0);
            }
            _ => {}
        }
    }
}

fn send_exit() {
    libdunit::gui_set_status("gui_terminal_stub: exiting");
    let mut message = libdunit::GuiMessage::new(libdunit::GUI_MSG_EXIT);
    message.window_id = WINDOW_ID;
    libdunit::gui_send(&message);
}

fn append_bytes(out: &mut [u8], len: &mut usize, bytes: &[u8]) {
    for byte in bytes {
        if *len >= out.len() {
            return;
        }
        out[*len] = *byte;
        *len += 1;
    }
}

fn line_str(buf: &[u8], len: usize) -> &str {
    core::str::from_utf8(&buf[..len.min(buf.len())]).unwrap_or("<invalid utf8>")
}
