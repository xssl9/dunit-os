#![no_std]
#![no_main]

use core::panic::PanicInfo;

const WINDOW_ID: u32 = 1;
const WIDTH: u32 = 820;
const HEIGHT: u32 = 520;
const INPUT_CAP: usize = 96;
const PROMPT_Y: i32 = 0;
const HISTORY_CAP: usize = 16;

// Control bytes the kernel forwards for arrow keys (see ui_loop.rs).
const KEY_UP: u8 = 0x11;
const KEY_DOWN: u8 = 0x12;
const KEY_LEFT: u8 = 0x13;
const KEY_RIGHT: u8 = 0x14;

const COMMANDS: [&str; 28] = [
    "help", "dufetch", "ls", "pwd", "cd", "mkdir", "touch", "cat", "echo", "rm", "tree", "clear",
    "exec", "ps", "top", "uname", "date", "whoami", "uptime", "free", "exit", "poweroff",
    "shutdown", "devs", "blk", "blkread", "lspci", "usb",
];

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
    history: [[u8; INPUT_CAP]; HISTORY_CAP],
    history_lens: [usize; HISTORY_CAP],
    history_count: usize,
    // -1 = editing a fresh line; otherwise an index into `history`.
    browse: isize,
}

impl TerminalInput {
    const fn new() -> Self {
        Self {
            input: [0; INPUT_CAP],
            input_len: 0,
            history: [[0; INPUT_CAP]; HISTORY_CAP],
            history_lens: [0; HISTORY_CAP],
            history_count: 0,
            browse: -1,
        }
    }

    fn redraw_prompt(&self) {
        let mut line = [b' '; 112];
        let mut len = 0usize;
        append_bytes(&mut line, &mut len, b"root@dunit:# ");
        append_bytes(&mut line, &mut len, &self.input[..self.input_len]);
        libdunit::gui_draw_text(WINDOW_ID, 0, PROMPT_Y, line_str(&line, len));
    }

    fn set_input(&mut self, bytes: &[u8]) {
        let len = bytes.len().min(self.input.len());
        self.input[..len].copy_from_slice(&bytes[..len]);
        self.input_len = len;
    }

    fn push_history(&mut self) {
        if self.input_len == 0 {
            return;
        }
        if self.history_count == HISTORY_CAP {
            for i in 1..HISTORY_CAP {
                self.history[i - 1] = self.history[i];
                self.history_lens[i - 1] = self.history_lens[i];
            }
            self.history_count -= 1;
        }
        let idx = self.history_count;
        self.history[idx][..self.input_len].copy_from_slice(&self.input[..self.input_len]);
        self.history_lens[idx] = self.input_len;
        self.history_count += 1;
    }

    fn load_history(&mut self) {
        if self.browse < 0 {
            self.input_len = 0;
            self.redraw_prompt();
            return;
        }
        let idx = self.browse as usize;
        let len = self.history_lens[idx];
        let mut tmp = [0u8; INPUT_CAP];
        tmp[..len].copy_from_slice(&self.history[idx][..len]);
        self.set_input(&tmp[..len]);
        self.redraw_prompt();
    }

    fn history_prev(&mut self) {
        if self.history_count == 0 {
            return;
        }
        if self.browse < 0 {
            self.browse = (self.history_count - 1) as isize;
        } else if self.browse > 0 {
            self.browse -= 1;
        }
        self.load_history();
    }

    fn history_next(&mut self) {
        if self.browse < 0 {
            return;
        }
        if (self.browse as usize) + 1 < self.history_count {
            self.browse += 1;
        } else {
            self.browse = -1;
        }
        self.load_history();
    }

    fn autocomplete(&mut self) {
        // Only complete the command word (no spaces typed yet).
        let prefix = line_str(&self.input, self.input_len);
        if prefix.is_empty() || prefix.contains(' ') {
            return;
        }

        let mut first_match: Option<&str> = None;
        let mut common_len = 0usize;
        let mut match_count = 0usize;

        for cmd in COMMANDS.iter() {
            if !cmd.starts_with(prefix) {
                continue;
            }
            match_count += 1;
            match first_match {
                None => {
                    first_match = Some(cmd);
                    common_len = cmd.len();
                }
                Some(prev) => {
                    common_len = common_prefix_len(prev, cmd).min(common_len);
                }
            }
        }

        let Some(first) = first_match else {
            return;
        };

        let completion = if match_count == 1 {
            first
        } else {
            &first[..common_len]
        };

        if completion.len() > self.input_len {
            self.set_input(completion.as_bytes());
            self.redraw_prompt();
        }
    }

    fn handle_key(&mut self, key: u8) -> bool {
        match key {
            b'\n' => self.submit_line(),
            b'\t' => {
                self.autocomplete();
                false
            }
            KEY_UP => {
                self.history_prev();
                false
            }
            KEY_DOWN => {
                self.history_next();
                false
            }
            KEY_LEFT | KEY_RIGHT => false,
            8 => {
                if self.input_len > 0 {
                    self.input_len -= 1;
                    self.browse = -1;
                    self.redraw_prompt();
                }
                false
            }
            byte if byte >= 0x20 && byte < 0x7f => {
                if self.input_len < self.input.len() {
                    self.input[self.input_len] = byte;
                    self.input_len += 1;
                    self.browse = -1;
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
        self.push_history();
        self.input_len = 0;
        self.browse = -1;
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

fn common_prefix_len(a: &str, b: &str) -> usize {
    let mut count = 0usize;
    for (x, y) in a.bytes().zip(b.bytes()) {
        if x != y {
            break;
        }
        count += 1;
    }
    count
}

fn line_str(buf: &[u8], len: usize) -> &str {
    core::str::from_utf8(&buf[..len.min(buf.len())]).unwrap_or("<invalid utf8>")
}
