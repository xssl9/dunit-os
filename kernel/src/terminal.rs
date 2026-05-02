use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

pub struct Terminal {
    pub buffer: Vec<String>,
    pub input: String,
    pub cursor_pos: usize,
    pub current_dir: String,
}

impl Terminal {
    pub fn new() -> Self {
        let mut term = Self {
            buffer: Vec::new(),
            input: String::new(),
            cursor_pos: 0,
            current_dir: String::from("/"),
        };
        term.buffer.push(String::from("Dunit OS (Green Tea) Terminal v1.0"));
        term.buffer.push(String::from("Type 'help' for available commands"));
        term.buffer.push(String::new());
        term
    }

    pub fn add_char(&mut self, c: char) {
        self.input.push(c);
        self.cursor_pos += 1;
    }

    pub fn backspace(&mut self) {
        if self.cursor_pos > 0 {
            self.input.pop();
            self.cursor_pos -= 1;
        }
    }

    pub fn execute(&mut self) {
        let cmd = self.input.trim();
        self.buffer.push(format!("{}$ {}", self.current_dir, cmd));
        
        if !cmd.is_empty() {
            let parts: Vec<&str> = cmd.split_whitespace().collect();
            let output = match parts.get(0) {
                Some(&"help") => String::from("Commands: help, clear, echo, pwd, ls, cd"),
                Some(&"clear") => {
                    self.buffer.clear();
                    String::new()
                },
                Some(&"pwd") => self.current_dir.clone(),
                Some(&"ls") => String::from("conf  dev  home  proc  tmp"),
                Some(&"cd") => {
                    if let Some(path) = parts.get(1) {
                        if *path == "/" {
                            self.current_dir = String::from("/");
                        } else if *path == ".." {
                            if self.current_dir != "/" {
                                self.current_dir = String::from("/");
                            }
                        } else {
                            self.current_dir = format!("/{}", path);
                        }
                        String::new()
                    } else {
                        String::new()
                    }
                },
                Some(&"echo") => {
                    parts[1..].join(" ")
                },
                Some(cmd) => format!("Command not found: {}", cmd),
                None => String::new(),
            };
            
            if !output.is_empty() {
                self.buffer.push(output);
            }
        }
        
        self.input.clear();
        self.cursor_pos = 0;
        
        if self.buffer.len() > 20 {
            self.buffer.remove(0);
        }
    }

    pub fn get_visible_lines(&self) -> &[String] {
        let start = if self.buffer.len() > 15 {
            self.buffer.len() - 15
        } else {
            0
        };
        &self.buffer[start..]
    }

    pub fn get_prompt(&self) -> String {
        format!("{}$ {}", self.current_dir, self.input)
    }
}

static mut TERMINAL_INSTANCE: Option<Terminal> = None;

pub fn init() {
}

pub fn get_terminal() -> Option<&'static mut Terminal> {
    unsafe {
        if TERMINAL_INSTANCE.is_none() {
            TERMINAL_INSTANCE = Some(Terminal::new());
        }
        TERMINAL_INSTANCE.as_mut()
    }
}
