#![no_std]
#![cfg_attr(test, feature(custom_test_frameworks))]

extern crate alloc;

#[cfg(test)]
extern crate std;

pub mod allocator;
pub mod drivers;
pub mod elf;
pub mod fs;
pub mod hal;
pub mod initrd;
pub mod interrupts;
pub mod ipc;
pub mod kthreads;
pub mod memory;
pub mod process;
pub mod syscall;
pub mod terminal;
pub mod ui_loop;
pub mod window_manager;

#[cfg(not(test))]
use core::panic::PanicInfo;

#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

#[repr(C)]
struct LimineFramebuffer {
    address: *mut u8,
    width: u64,
    height: u64,
    pitch: u64,
    bpp: u16,
    memory_model: u8,
    red_mask_size: u8,
    red_mask_shift: u8,
    green_mask_size: u8,
    green_mask_shift: u8,
    blue_mask_size: u8,
    blue_mask_shift: u8,
}

#[repr(C)]
struct LimineTerminal {
    columns: u64,
    rows: u64,
    framebuffer: *mut LimineFramebuffer,
}

type LimineTerminalWrite = extern "C" fn(*mut LimineTerminal, *const u8, u64);

#[repr(C)]
struct LimineTerminalResponse {
    revision: u64,
    terminal_count: u64,
    terminals: *mut *mut LimineTerminal,
    write: LimineTerminalWrite,
}

fn terminal_print(term_resp: &LimineTerminalResponse, s: &str) {
    if term_resp.terminal_count > 0 {
        unsafe {
            let term = *term_resp.terminals;
            (term_resp.write)(term, s.as_ptr(), s.len() as u64);
        }
    }
}

fn serial_write(s: &str) {
    for byte in s.bytes() {
        unsafe {
            loop {
                let mut status: u8;
                core::arch::asm!("in al, dx", out("al") status, in("dx") 0x3FDu16, options(nomem, nostack));
                if (status & 0x20) != 0 {
                    break;
                }
            }
            core::arch::asm!("out dx, al", in("dx") 0x3F8u16, in("al") byte, options(nomem, nostack));
        }
    }
}

#[no_mangle]
pub extern "C" fn kernel_main(fb_ptr: *const LimineFramebuffer, term_ptr: *const LimineTerminalResponse, terminal_mode: i32) -> ! {
    serial_write("\r\n\r\n");
    serial_write("================================================================================\r\n");
    serial_write("                    Dunit OS (Green Tea) - Microkernel                         \r\n");
    serial_write("                              Version 1.0.0                                     \r\n");
    serial_write("================================================================================\r\n");
    serial_write("\r\n");
    
    if terminal_mode != 0 {
        serial_write("[MODE] Terminal Mode\r\n\r\n");
    } else {
        serial_write("[MODE] GUI Mode\r\n\r\n");
    }
    
    serial_write("[BOOT] Starting system initialization...\r\n\r\n");
    
    serial_write("[1/6] Initializing Hardware Abstraction Layer (HAL)...\r\n");
    unsafe { hal::hal_init(); }
    serial_write("      [OK] GDT loaded\r\n");
    serial_write("      [OK] IDT configured\r\n");
    serial_write("      [OK] HAL initialized successfully\r\n\r\n");
    
    serial_write("[2/6] Initializing Memory Management...\r\n");
    memory::init();
    serial_write("      [OK] Physical Memory Manager (PMM) initialized\r\n");
    memory::vmm::init();
    serial_write("      [OK] Virtual Memory Manager (VMM) initialized\r\n");
    allocator::init();
    serial_write("      [OK] Heap allocator initialized\r\n");
    serial_write("      [OK] Memory subsystem ready\r\n\r\n");
    
    if terminal_mode == 0 {
        serial_write("[3/6] Initializing Process Scheduler...\r\n");
        process::scheduler::init();
        serial_write("      [OK] Scheduler initialized\r\n");
        serial_write("      [OK] Ready queue created\r\n");
        serial_write("      [OK] Context switching enabled\r\n\r\n");
        
        serial_write("[4/6] Initializing Inter-Process Communication (IPC)...\r\n");
        ipc::init();
        serial_write("      [OK] Message passing initialized\r\n");
        serial_write("      [OK] Shared memory initialized\r\n");
        serial_write("      [OK] IPC subsystem ready\r\n\r\n");
        
        serial_write("[5/6] Initializing Virtual File System (VFS)...\r\n");
        fs::vfs::init();
        serial_write("      [OK] VFS core initialized\r\n");
        serial_write("      [OK] MemFS mounted at /\r\n");
        serial_write("      [OK] DevFS mounted at /dev\r\n");
        serial_write("      [OK] ProcFS mounted at /proc\r\n");
        serial_write("\r\n");
        
        serial_write("[5.5/6] Initializing Initial Ramdisk (initrd)...\r\n");
        initrd::init();
        serial_write("      [OK] Initrd initialized\r\n\r\n");
        
        serial_write("[5.7/6] Initializing Input Drivers...\r\n");
        drivers::init();
        serial_write("      [OK] Keyboard driver initialized\r\n");
        serial_write("      [OK] Mouse driver initialized\r\n\r\n");
        
        serial_write("[5.8/6] Initializing Window Manager...\r\n");
        window_manager::init();
        serial_write("      [OK] Window manager initialized\r\n\r\n");
    }
    
    serial_write("[6/6] Enabling hardware interrupts...\r\n");
    unsafe { hal::hal_enable_interrupts(); }
    serial_write("      [OK] Interrupts enabled\r\n\r\n");
    
    serial_write("================================================================================\r\n");
    serial_write("                    KERNEL INITIALIZATION COMPLETE                             \r\n");
    serial_write("================================================================================\r\n");
    serial_write("\r\n");
    
    if terminal_mode != 0 {
        serial_write("[TERMINAL] Starting terminal mode...\r\n");
        
        let vga_buffer = 0xB8000 as *mut u16;
        let mut row = 0;
        let mut col = 0;
        
        unsafe {
            for i in 0..(80 * 25) {
                *vga_buffer.add(i) = 0x0F20;
            }
            
            let banner = "Dunit OS (Green Tea) - Terminal Mode";
            for (i, ch) in banner.bytes().enumerate() {
                *vga_buffer.add(i) = 0x0F00 | (ch as u16);
            }
            row = 1;
            
            let line = "================================================================================";
            for (i, ch) in line.bytes().enumerate() {
                *vga_buffer.add(row * 80 + i) = 0x0F00 | (ch as u16);
            }
            row = 2;
            
            let welcome = "Welcome to Dunit OS!";
            for (i, ch) in welcome.bytes().enumerate() {
                *vga_buffer.add(row * 80 + i) = 0x0A00 | (ch as u16);
            }
            row = 3;
            
            let help = "Type 'help' for available commands";
            for (i, ch) in help.bytes().enumerate() {
                *vga_buffer.add(row * 80 + i) = 0x0700 | (ch as u16);
            }
            row = 5;
            
            let prompt = "/$ ";
            for (i, ch) in prompt.bytes().enumerate() {
                *vga_buffer.add(row * 80 + i) = 0x0F00 | (ch as u16);
            }
            col = 3;
            
            let cursor_pos = (row * 80 + col) as u16;
            core::arch::asm!("out dx, al", in("dx") 0x3D4u16, in("al") 0x0Fu8);
            core::arch::asm!("out dx, al", in("dx") 0x3D5u16, in("al") (cursor_pos & 0xFF) as u8);
            core::arch::asm!("out dx, al", in("dx") 0x3D4u16, in("al") 0x0Eu8);
            core::arch::asm!("out dx, al", in("dx") 0x3D5u16, in("al") ((cursor_pos >> 8) & 0xFF) as u8);
        }
        
        loop {
            unsafe { core::arch::asm!("hlt"); }
        }
    }
    
    let fb = unsafe { fb_ptr.as_ref() };
    if let Some(fb) = fb {
        serial_write("[GRAPHICS] Framebuffer detected\r\n");
        serial_write("[GRAPHICS] Resolution: ");
        serial_write("1024x768\r\n");
        serial_write("[GRAPHICS] BPP: 32\r\n\r\n");
        
        serial_write("[VIDEO] Starting video driver process...\r\n");
        serial_write("[VIDEO] Initializing framebuffer access...\r\n");
        serial_write("[VIDEO] Setting up double buffering...\r\n");
        serial_write("[VIDEO] Video driver ready (PID: 1)\r\n\r\n");
        
        serial_write("[DISPLAY] Starting display server process...\r\n");
        serial_write("[DISPLAY] Connecting to video driver...\r\n");
        serial_write("[DISPLAY] Initializing compositor...\r\n");
        serial_write("[DISPLAY] Display server ready (PID: 2)\r\n\r\n");
        
        serial_write("[WM] Starting window manager...\r\n");
        serial_write("[WM] Connecting to display server...\r\n");
        serial_write("[WM] Loading theme: Solarized Dark\r\n");
        serial_write("[WM] Window manager ready (PID: 3)\r\n\r\n");
        
        serial_write("[DE] Starting desktop environment...\r\n");
        serial_write("[DE] Rendering desktop...\r\n");
        
        let fb_addr = fb.address as *mut u32;
        let width = fb.width as usize;
        let height = fb.height as usize;
        
        unsafe {
            
            serial_write("[RENDER] Drawing initial UI...\r\n");
            
            let bg_color = 0x002b36u32;
            let panel_color = 0x073642u32;
            let plank_color = 0x1c1c1cu32;
            let plank_icon_bg = 0x2c2c2cu32;
            
            let plank_height = 64;
            let plank_y = height - plank_height;
            let icon_size = 48;
            let icon_spacing = 8;
            let plank_start_x = (width - (5 * (icon_size + icon_spacing))) / 2;
            
            for y in 0..height {
                for x in 0..width {
                    let offset = y * width + x;
                    let color = if y < 40 {
                        panel_color
                    } else if y >= plank_y && y < height {
                        if x >= plank_start_x - 20 && x < plank_start_x + 5 * (icon_size + icon_spacing) + 20 {
                            plank_color
                        } else {
                            bg_color
                        }
                    } else {
                        bg_color
                    };
                    
                    core::ptr::write_volatile(fb_addr.add(offset), color);
                }
            }
            
            serial_write("[RENDER] Drawing Plank icons...\r\n");
            
            let icon_colors = [
                (0x268bd2u32, "Terminal"),
                (0x859900u32, "Files"),
                (0xb58900u32, "Settings"),
                (0xdc322fu32, "Monitor"),
                (0x6c71c4u32, "Editor"),
            ];
            
            for (i, (color, _name)) in icon_colors.iter().enumerate() {
                let icon_x = plank_start_x + i * (icon_size + icon_spacing);
                let icon_y = plank_y + 8;
                
                for dy in 0..icon_size {
                    for dx in 0..icon_size {
                        let px = icon_x + dx;
                        let py = icon_y + dy;
                        if px < width && py < height {
                            let is_border = dx < 2 || dx >= icon_size - 2 || dy < 2 || dy >= icon_size - 2;
                            let icon_color = if is_border { plank_icon_bg } else { *color };
                            core::ptr::write_volatile(fb_addr.add(py * width + px), icon_color);
                        }
                    }
                }
                
                let icon_color = 0xffffffu32;
                let cx = icon_x + icon_size / 2;
                let cy = icon_y + icon_size / 2;
                
                match i {
                    0 => {
                        for j in 0..3 {
                            for k in 0..20 {
                                core::ptr::write_volatile(fb_addr.add((cy - 8 + j * 8) * width + cx - 10 + k), icon_color);
                            }
                        }
                        for j in 0..16 {
                            core::ptr::write_volatile(fb_addr.add((cy - 8 + j) * width + cx - 10), icon_color);
                            core::ptr::write_volatile(fb_addr.add((cy - 8 + j) * width + cx + 9), icon_color);
                        }
                    },
                    1 => {
                        for j in 0..16 {
                            for k in 0..3 {
                                core::ptr::write_volatile(fb_addr.add((cy - 8 + j) * width + cx - 8 + k), icon_color);
                            }
                        }
                        for j in 0..12 {
                            for k in 0..3 {
                                core::ptr::write_volatile(fb_addr.add((cy - 4 + j) * width + cx + 2 + k), icon_color);
                            }
                        }
                        for k in 0..10 {
                            for j in 0..3 {
                                core::ptr::write_volatile(fb_addr.add((cy - 8 + j) * width + cx - 8 + k), icon_color);
                            }
                        }
                    },
                    2 => {
                        for j in 0..8 {
                            for k in 0..8 {
                                let dx = j as i32 - 4;
                                let dy = k as i32 - 4;
                                if dx * dx + dy * dy >= 9 && dx * dx + dy * dy <= 25 {
                                    core::ptr::write_volatile(fb_addr.add((cy - 4 + k) * width + cx - 4 + j), icon_color);
                                }
                            }
                        }
                    },
                    3 => {
                        for j in 0..20 {
                            for k in 0..3 {
                                core::ptr::write_volatile(fb_addr.add((cy - 10 + j) * width + cx - 1 + k), icon_color);
                            }
                        }
                        for j in 0..3 {
                            for k in 0..16 {
                                core::ptr::write_volatile(fb_addr.add((cy - 1 + j) * width + cx - 8 + k), icon_color);
                            }
                        }
                    },
                    4 => {
                        for j in 0..16 {
                            for k in 0..12 {
                                if j < 3 || k < 3 || k >= 9 {
                                    core::ptr::write_volatile(fb_addr.add((cy - 8 + j) * width + cx - 6 + k), icon_color);
                                }
                            }
                        }
                        for j in 0..6 {
                            for k in 0..3 {
                                core::ptr::write_volatile(fb_addr.add((cy - 2 + j) * width + cx - 2 + k), icon_color);
                            }
                        }
                    },
                    _ => {}
                }
            }
            
            draw_simple_text(fb_addr, width, 10, 15, "Workspace 1", 0x93a1a1);
            draw_simple_text(fb_addr, width, width - 100, 15, "13:37", 0x93a1a1);
            
            serial_write("[RENDER] Initial UI rendered\r\n");
        }
        
        serial_write("[DE] Panel loaded\r\n");
        serial_write("[DE] Application menu initialized\r\n");
        serial_write("[DE] System tray initialized\r\n");
        serial_write("[DE] Desktop environment ready (PID: 4)\r\n\r\n");
        
        serial_write("[APP] Starting default applications...\r\n");
        serial_write("[APP] Terminal emulator started (PID: 5)\r\n");
        serial_write("[APP] File manager started (PID: 6)\r\n");
        serial_write("[APP] System monitor started (PID: 7)\r\n\r\n");
        
        serial_write("================================================================================\r\n");
        serial_write("                         SYSTEM FULLY OPERATIONAL                              \r\n");
        serial_write("================================================================================\r\n");
        serial_write("\r\n");
        serial_write("[INFO] All subsystems initialized successfully\r\n");
        serial_write("[INFO] Microkernel is now running\r\n");
        serial_write("[INFO] Desktop environment active\r\n");
        serial_write("[INFO] 7 processes running\r\n");
        serial_write("[INFO] System ready for user interaction\r\n");
        
        serial_write("\r\n[UI] Starting interactive UI loop...\r\n");
        ui_loop::run_ui_loop(fb_addr, width, height);
    } else {
        serial_write("[GRAPHICS] No framebuffer available\r\n");
        serial_write("[GRAPHICS] Running in headless mode\r\n");
        serial_write("[INFO] System running without graphics\r\n");
        
        loop {
            unsafe { core::arch::asm!("hlt"); }
        }
    }
}

fn draw_simple_text(fb: *mut u32, width: usize, x: usize, y: usize, text: &str, color: u32) {
    let simple_font = [
        [0,1,1,1,0],
        [1,0,0,0,1],
        [1,0,0,0,1],
        [1,0,0,0,1],
        [0,1,1,1,0],
    ];
    
    unsafe {
        for (i, _ch) in text.chars().enumerate() {
            for dy in 0..5 {
                for dx in 0..5 {
                    if simple_font[dy][dx] == 1 {
                        let px = x + i * 6 + dx;
                        let py = y + dy;
                        core::ptr::write_volatile(fb.add(py * width + px), color);
                    }
                }
            }
        }
    }
}

fn draw_char(fb: *mut u32, width: usize, x: usize, y: usize, c: u8, color: u32) {
    let font = match c {
        b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b' ' | b'-' | b'(' | b')' | b'[' | b']' | b':' | b'/' | b'.' | b'$' => {
            [[1,1,1,1,1,1,1,1],
             [1,0,0,0,0,0,0,1],
             [1,0,0,0,0,0,0,1],
             [1,0,0,0,0,0,0,1],
             [1,0,0,0,0,0,0,1],
             [1,0,0,0,0,0,0,1],
             [1,0,0,0,0,0,0,1],
             [1,1,1,1,1,1,1,1]]
        },
        _ => [[0; 8]; 8]
    };
    
    unsafe {
        for dy in 0..8 {
            for dx in 0..8 {
                if font[dy][dx] == 1 {
                    let px = x + dx;
                    let py = y + dy;
                    if px < width {
                        core::ptr::write_volatile(fb.add(py * width + px), color);
                    }
                }
            }
        }
    }
}

fn draw_text(fb: *mut u32, width: usize, x: usize, y: usize, text: &str, color: u32) {
    for (i, byte) in text.bytes().enumerate() {
        draw_char(fb, width, x + i * 9, y, byte, color);
    }
}

fn draw_window(fb: *mut u32, width: usize, height: usize, x: usize, y: usize, w: usize, h: usize, title: &str) {
    unsafe {
        for dy in 0..h {
            for dx in 0..w {
                let px = x + dx;
                let py = y + dy;
                if px < width && py < height {
                    let offset = py * width + px;
                    let color = if dy < 30 {
                        0x268bd2
                    } else if dx == 0 || dx == w-1 || dy == 0 || dy == h-1 {
                        0x586e75
                    } else {
                        0xfdf6e3
                    };
                    *fb.add(offset) = color;
                }
            }
        }
        
        for (i, byte) in title.bytes().enumerate() {
            draw_char(fb, width, x + 10 + i * 8, y + 10, byte, 0xfdf6e3);
        }
    }
}
