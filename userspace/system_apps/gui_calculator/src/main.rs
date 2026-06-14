#![no_std]
#![no_main]

use core::panic::PanicInfo;

const WINDOW_ID: u32 = 2;
const WIDTH: u32 = 330;
const HEIGHT: u32 = 330;
const INPUT_CAP: usize = 18;
const BUTTON_W: i32 = 56;
const BUTTON_H: i32 = 28;
const BUTTON_GAP_X: i32 = 10;
const BUTTON_GAP_Y: i32 = 8;
const GRID_X: i32 = 12;
const GRID_Y: i32 = 82;
const CONTENT_W: u32 = WIDTH - 36;
const CONTENT_H: u32 = HEIGHT - 64;
const COLOR_BG: u32 = 0x15191f;
const COLOR_PANEL: u32 = 0x202832;
const COLOR_DISPLAY: u32 = 0x0b0f14;
const COLOR_BUTTON: u32 = 0x2b3542;
const COLOR_BUTTON_ALT: u32 = 0x384657;
const COLOR_OPERATOR: u32 = 0x256d85;
const COLOR_EQUALS: u32 = 0x2d7d46;
const COLOR_DANGER: u32 = 0x8f3842;

const BUTTONS: [[Button; 4]; 5] = [
    [Button::Clear, Button::Backspace, Button::Op(b'/'), Button::Op(b'*')],
    [Button::Digit(b'7'), Button::Digit(b'8'), Button::Digit(b'9'), Button::Op(b'-')],
    [Button::Digit(b'4'), Button::Digit(b'5'), Button::Digit(b'6'), Button::Op(b'+')],
    [Button::Digit(b'1'), Button::Digit(b'2'), Button::Digit(b'3'), Button::Equals],
    [Button::Digit(b'0'), Button::Clear, Button::Backspace, Button::Equals],
];

#[derive(Clone, Copy)]
enum Button {
    Digit(u8),
    Op(u8),
    Equals,
    Clear,
    Backspace,
}

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    loop {
        unsafe {
            core::arch::asm!("pause", options(nomem, nostack));
        }
    }
}

struct Calculator {
    input: [u8; INPUT_CAP],
    input_len: usize,
    lhs: Option<i64>,
    op: Option<u8>,
    just_evaluated: bool,
    status: [u8; 48],
    status_len: usize,
}

impl Calculator {
    const fn new() -> Self {
        Self {
            input: [0; INPUT_CAP],
            input_len: 1,
            lhs: None,
            op: None,
            just_evaluated: false,
            status: [0; 48],
            status_len: 0,
        }
    }

    fn init(&mut self) {
        self.input[0] = b'0';
        self.set_status(b"Ready");
    }

    fn redraw(&self) {
        libdunit::gui_clear(WINDOW_ID);
        libdunit::gui_draw_rect(WINDOW_ID, 0, 0, CONTENT_W, CONTENT_H, COLOR_BG);
        libdunit::gui_draw_rect(WINDOW_ID, 8, 8, CONTENT_W - 16, 54, COLOR_DISPLAY);
        libdunit::gui_draw_rect(WINDOW_ID, 8, 68, CONTENT_W - 16, 10, COLOR_PANEL);

        let mut display = [b' '; 52];
        let mut display_len = 0usize;
        append_bytes(&mut display, &mut display_len, &self.input[..self.input_len]);
        libdunit::gui_draw_text(WINDOW_ID, 20, 26, line_str(&display, display_len));

        let mut state = [b' '; 52];
        let mut state_len = 0usize;
        if let Some(lhs) = self.lhs {
            append_i64(&mut state, &mut state_len, lhs);
            append_bytes(&mut state, &mut state_len, b" ");
            if let Some(op) = self.op {
                append_bytes(&mut state, &mut state_len, &[display_op(op)]);
            }
        } else {
            append_bytes(&mut state, &mut state_len, &self.status[..self.status_len]);
        }
        libdunit::gui_draw_text(WINDOW_ID, 20, 70, line_str(&state, state_len));

        let mut row = 0usize;
        while row < BUTTONS.len() {
            let mut col = 0usize;
            while col < BUTTONS[row].len() {
                draw_button(row, col, BUTTONS[row][col]);
                col += 1;
            }
            row += 1;
        }
    }

    fn handle_pointer(&mut self, x: i32, y: i32) {
        let Some(button) = button_at(x, y) else {
            return;
        };
        self.press(button);
        self.redraw();
    }

    fn handle_key(&mut self, key: u8) {
        match key {
            b'0'..=b'9' => self.press(Button::Digit(key)),
            b'+' | b'-' | b'*' | b'/' => self.press(Button::Op(key)),
            b'\n' | b'=' => self.press(Button::Equals),
            b'c' | b'C' => self.press(Button::Clear),
            8 => self.press(Button::Backspace),
            _ => return,
        }
        self.redraw();
    }

    fn press(&mut self, button: Button) {
        match button {
            Button::Digit(digit) => self.push_digit(digit),
            Button::Op(op) => self.set_operator(op),
            Button::Equals => self.evaluate(),
            Button::Clear => self.clear(),
            Button::Backspace => self.backspace(),
        }
    }

    fn push_digit(&mut self, digit: u8) {
        if self.just_evaluated {
            self.input_len = 0;
            self.just_evaluated = false;
        }
        if self.input_len == 1 && self.input[0] == b'0' {
            self.input[0] = digit;
            self.set_status(b"Ready");
            return;
        }
        if self.input_len < self.input.len() {
            self.input[self.input_len] = digit;
            self.input_len += 1;
        }
        self.set_status(b"Ready");
    }

    fn set_operator(&mut self, op: u8) {
        if self.op.is_some() {
            self.evaluate();
        }
        self.lhs = Some(self.current_value());
        self.op = Some(op);
        self.input_len = 1;
        self.input[0] = b'0';
        self.just_evaluated = false;
        self.set_status(b"Operator set");
    }

    fn evaluate(&mut self) {
        let Some(lhs) = self.lhs else {
            self.set_status(b"No operation");
            return;
        };
        let Some(op) = self.op else {
            self.set_status(b"No operation");
            return;
        };
        let rhs = self.current_value();
        let result = match op {
            b'+' => Some(lhs + rhs),
            b'-' => Some(lhs - rhs),
            b'*' => Some(lhs * rhs),
            b'/' if rhs != 0 => Some(lhs / rhs),
            b'/' => None,
            _ => None,
        };
        match result {
            Some(value) => {
                self.set_input_i64(value);
                self.lhs = None;
                self.op = None;
                self.just_evaluated = true;
                self.set_status(b"Done");
            }
            None => {
                self.set_input_bytes(b"0");
                self.lhs = None;
                self.op = None;
                self.just_evaluated = true;
                self.set_status(b"Error: divide by zero");
            }
        }
    }

    fn clear(&mut self) {
        self.input_len = 1;
        self.input[0] = b'0';
        self.lhs = None;
        self.op = None;
        self.just_evaluated = false;
        self.set_status(b"Ready");
    }

    fn backspace(&mut self) {
        if self.just_evaluated {
            self.clear();
            return;
        }
        if self.input_len > 1 {
            self.input_len -= 1;
        } else {
            self.input[0] = b'0';
            self.input_len = 1;
        }
        self.set_status(b"Ready");
    }

    fn current_value(&self) -> i64 {
        let mut value = 0i64;
        let mut index = 0usize;
        let mut sign = 1i64;
        if self.input_len > 0 && self.input[0] == b'-' {
            sign = -1;
            index = 1;
        }
        while index < self.input_len {
            let byte = self.input[index];
            if byte.is_ascii_digit() {
                value = value * 10 + (byte - b'0') as i64;
            }
            index += 1;
        }
        value * sign
    }

    fn set_input_i64(&mut self, value: i64) {
        let mut out = [0u8; INPUT_CAP];
        let mut len = 0usize;
        append_i64(&mut out, &mut len, value);
        self.set_input_bytes(&out[..len]);
    }

    fn set_input_bytes(&mut self, value: &[u8]) {
        self.input_len = value.len().min(self.input.len()).max(1);
        self.input[..self.input_len].copy_from_slice(&value[..self.input_len]);
    }

    fn set_status(&mut self, value: &[u8]) {
        self.status_len = value.len().min(self.status.len());
        self.status[..self.status_len].copy_from_slice(&value[..self.status_len]);
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    libdunit::println("gui_calculator: start");
    libdunit::gui_create_window(WINDOW_ID, "Calculator", WIDTH, HEIGHT);
    libdunit::gui_set_status("gui_calculator: running");
    libdunit::gui_set_title(WINDOW_ID, "Calculator");

    let mut calculator = Calculator::new();
    calculator.init();
    calculator.redraw();

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
            libdunit::GUI_MSG_POINTER_EVENT => {
                libdunit::println("gui_calculator: received POINTER_EVENT");
                calculator.handle_pointer(event.a, event.b);
            }
            libdunit::GUI_MSG_KEY_EVENT => {
                libdunit::println("gui_calculator: received KEY_EVENT");
                calculator.handle_key(event.a as u8);
            }
            libdunit::GUI_MSG_CLOSE_EVENT => {
                libdunit::println("gui_calculator: received CLOSE_EVENT");
                send_exit();
                libdunit::exit(0);
            }
            _ => {}
        }
    }
}

fn send_exit() {
    libdunit::gui_set_status("gui_calculator: exiting");
    let mut message = libdunit::GuiMessage::new(libdunit::GUI_MSG_EXIT);
    message.window_id = WINDOW_ID;
    libdunit::gui_send(&message);
}

fn button_at(x: i32, y: i32) -> Option<Button> {
    if x < GRID_X || y < GRID_Y {
        return None;
    }
    let rel_x = x - GRID_X;
    let rel_y = y - GRID_Y;
    let cell_w = BUTTON_W + BUTTON_GAP_X;
    let cell_h = BUTTON_H + BUTTON_GAP_Y;
    let col = rel_x / cell_w;
    let row = rel_y / cell_h;
    if row < 0 || col < 0 || row >= 5 || col >= 4 {
        return None;
    }
    if rel_x % cell_w >= BUTTON_W || rel_y % cell_h >= BUTTON_H {
        return None;
    }
    Some(BUTTONS[row as usize][col as usize])
}

fn draw_button(row: usize, col: usize, button: Button) {
    let x = GRID_X + col as i32 * (BUTTON_W + BUTTON_GAP_X);
    let y = GRID_Y + row as i32 * (BUTTON_H + BUTTON_GAP_Y);
    let color = match button {
        Button::Digit(_) => COLOR_BUTTON,
        Button::Op(_) => COLOR_OPERATOR,
        Button::Equals => COLOR_EQUALS,
        Button::Clear => COLOR_DANGER,
        Button::Backspace => COLOR_BUTTON_ALT,
    };
    libdunit::gui_draw_rect(WINDOW_ID, x, y, BUTTON_W as u32, BUTTON_H as u32, color);
    let label = button_label(button);
    let label_x = x + ((BUTTON_W - (label.len() as i32 * 6)) / 2).max(2);
    let label_y = y + 10;
    libdunit::gui_draw_text(WINDOW_ID, label_x, label_y, label);
}

fn button_label(button: Button) -> &'static str {
    match button {
        Button::Digit(b'0') => "0",
        Button::Digit(b'1') => "1",
        Button::Digit(b'2') => "2",
        Button::Digit(b'3') => "3",
        Button::Digit(b'4') => "4",
        Button::Digit(b'5') => "5",
        Button::Digit(b'6') => "6",
        Button::Digit(b'7') => "7",
        Button::Digit(b'8') => "8",
        Button::Digit(b'9') => "9",
        Button::Op(b'+') => "+",
        Button::Op(b'-') => "-",
        Button::Op(b'*') => "x",
        Button::Op(b'/') => "/",
        Button::Equals => "=",
        Button::Clear => "C",
        Button::Backspace => "DEL",
        _ => "?",
    }
}

fn display_op(op: u8) -> u8 {
    if op == b'*' {
        b'x'
    } else {
        op
    }
}

fn append_i64(out: &mut [u8], len: &mut usize, value: i64) {
    if value < 0 {
        append_bytes(out, len, b"-");
        append_u64(out, len, value.wrapping_neg() as u64);
    } else {
        append_u64(out, len, value as u64);
    }
}

fn append_u64(out: &mut [u8], len: &mut usize, mut value: u64) {
    let mut digits = [0u8; 20];
    let mut count = 0usize;
    if value == 0 {
        digits[0] = b'0';
        count = 1;
    } else {
        while value > 0 {
            digits[count] = b'0' + (value % 10) as u8;
            count += 1;
            value /= 10;
        }
    }
    while count > 0 {
        count -= 1;
        if *len >= out.len() {
            return;
        }
        out[*len] = digits[count];
        *len += 1;
    }
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
