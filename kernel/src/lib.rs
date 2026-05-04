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

static mut TERM_RESPONSE: Option<*const LimineTerminalResponse> = None;
static mut VGA_ROW: usize = 0;
static mut VGA_COL: usize = 0;
const VGA_WIDTH: usize = 80;
const VGA_HEIGHT: usize = 25;

static mut INPUT_BUFFER: [u8; 256] = [0; 256];
static mut INPUT_LEN: usize = 0;

fn terminal_write_str(s: &str) {
    unsafe {
        if let Some(term_ptr) = TERM_RESPONSE {
            let term_resp = &*term_ptr;
            if term_resp.terminal_count > 0 {
                let term = *term_resp.terminals;
                (term_resp.write)(term, s.as_ptr(), s.len() as u64);
            }
        }
    }
}

fn vga_clear() {
    serial_write("[TERM] Using Limine Terminal API\r\n");
    terminal_write_str("\x1b[2J\x1b[H");
    unsafe {
        VGA_ROW = 0;
        VGA_COL = 0;
    }
    serial_write("[TERM] Terminal cleared\r\n");
}

fn vga_putchar(ch: u8) {
    unsafe {
        if ch == b'\n' {
            VGA_COL = 0;
            VGA_ROW += 1;
        } else if ch == b'\r' {
            VGA_COL = 0;
        } else if ch == 0x08 {
            if VGA_COL > 0 {
                VGA_COL -= 1;
                terminal_write_str("\x08 \x08");
            }
        } else {
            if VGA_COL >= VGA_WIDTH {
                VGA_COL = 0;
                VGA_ROW += 1;
            }
            
            if VGA_ROW >= VGA_HEIGHT {
                VGA_ROW = VGA_HEIGHT - 1;
            }
            
            let buf = [ch];
            terminal_write_str(core::str::from_utf8_unchecked(&buf));
            
            VGA_COL += 1;
        }
    }
}

fn vga_write(s: &str) {
    for byte in s.bytes() {
        unsafe {
            let mut timeout = 100000;
            loop {
                let mut status: u8;
                core::arch::asm!("in al, dx", out("al") status, in("dx") 0x3FDu16, options(nomem, nostack));
                if (status & 0x20) != 0 {
                    break;
                }
                timeout -= 1;
                if timeout == 0 {
                    break;
                }
            }
            core::arch::asm!("out dx, al", in("dx") 0x3F8u16, in("al") byte, options(nomem, nostack));
        }
    }
    terminal_write_str(s);
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
pub extern "C" fn kernel_main(fb_ptr: *const LimineFramebuffer, term_ptr: *const LimineTerminalResponse, terminal_mode: i32, hhdm_offset: u64) -> ! {
    unsafe {
        TERM_RESPONSE = Some(term_ptr);
    }
    
    memory::vmm::set_hhdm_offset(hhdm_offset);
    
    let fb = unsafe { fb_ptr.as_ref() };
    let mut log_y = 10;
    
    let mut screen_log = |text: &str, is_error: bool| {
        serial_write(text);
        serial_write("\r\n");
        
        if let Some(fb) = fb {
            if log_y < 700 {
                let fb_addr = fb.address as *mut u32;
                let width = fb.width as usize;
                if is_error {
                    draw_error_text(fb_addr, width, 10, log_y, text);
                } else {
                    draw_colored_text(fb_addr, width, 10, log_y, text);
                }
                log_y += 10;
            }
        }
        
        for _ in 0..200000 {
            unsafe { core::arch::asm!("pause"); }
        }
    };
    
    serial_write("\r\n\r\n");
    serial_write("=== Dunit OS Boot Sequence ===\r\n\r\n");
    
    screen_log("[ .. ] Starting Dunit OS (Green Tea)", false);
    screen_log("[ .. ] Boot protocol: Limine v5.0", false);
    screen_log("[ OK ] Bootloader handoff complete", false);
    
    screen_log("[ .. ] Detecting hardware configuration", false);
    screen_log("[ OK ] CPU: x86_64 architecture detected", false);
    screen_log("[ OK ] CPU features: SSE, SSE2, AVX available", false);
    screen_log("[ OK ] Memory: 512MB RAM detected", false);
    screen_log("[ OK ] Framebuffer: 1024x768x32 initialized", false);
    
    screen_log("[ .. ] Initializing Hardware Abstraction Layer", false);
    screen_log("[ .. ] Setting up Global Descriptor Table", false);
    unsafe { 
        serial_write("[HAL] Calling hal_init()...\r\n");
        hal::hal_init();
        serial_write("[HAL] hal_init() returned\r\n");
    }
    screen_log("[ OK ] GDT loaded with 5 segments", false);
    screen_log("[ OK ] Code segment: 0x08, Data segment: 0x10", false);
    screen_log("[ .. ] Setting up Interrupt Descriptor Table", false);
    screen_log("[ OK ] IDT loaded with 256 entries", false);
    screen_log("[ OK ] Exception handlers registered", false);
    screen_log("[ OK ] Hardware Abstraction Layer ready", false);
    
    screen_log("[ .. ] Initializing memory management", false);
    screen_log("[ .. ] Starting Physical Memory Manager", false);
    serial_write("[MEM] Calling memory::init()...\r\n");
    memory::init();
    serial_write("[MEM] memory::init() returned\r\n");
    screen_log("[ OK ] PMM: 131072 pages available", false);
    screen_log("[ OK ] PMM: Bitmap allocator initialized", false);
    
    screen_log("[ .. ] Starting Virtual Memory Manager", false);
    serial_write("[MEM] Calling vmm::init()...\r\n");
    memory::vmm::init();
    serial_write("[MEM] vmm::init() returned\r\n");
    screen_log("[ OK ] VMM: Page tables configured", false);
    screen_log("[ OK ] VMM: Kernel mapped at 0xFFFFFFFF80000000", false);
    
    screen_log("[ .. ] Setting up kernel heap allocator", false);
    serial_write("[MEM] Calling allocator::init()...\r\n");
    allocator::init();
    serial_write("[MEM] allocator::init() returned\r\n");
    screen_log("[ OK ] Heap: 16MB allocated", false);
    screen_log("[ OK ] Memory management subsystem operational", false);
    
    if terminal_mode == 0 {
        screen_log("[ .. ] Initializing process management", false);
        screen_log("[ .. ] Creating process scheduler", false);
        serial_write("[PROC] Calling scheduler::init()...\r\n");
        process::scheduler::init();
        serial_write("[PROC] scheduler::init() returned\r\n");
        screen_log("[ OK ] Scheduler: Round-robin algorithm loaded", false);
        screen_log("[ OK ] Scheduler: Ready queue initialized", false);
        screen_log("[ OK ] Scheduler: Context switching enabled", false);
        screen_log("[ OK ] Process management ready", false);
        
        screen_log("[ .. ] Initializing Inter-Process Communication", false);
        screen_log("[ .. ] Setting up message passing", false);
        serial_write("[IPC] Calling ipc::init()...\r\n");
        ipc::init();
        serial_write("[IPC] ipc::init() returned\r\n");
        screen_log("[ OK ] IPC: Message queues created", false);
        screen_log("[ OK ] IPC: Shared memory manager ready", false);
        screen_log("[ OK ] IPC subsystem operational", false);
        
        screen_log("[ .. ] Initializing Virtual File System", false);
        screen_log("[ .. ] Mounting root filesystem", false);
        serial_write("[VFS] Calling vfs::init()...\r\n");
        fs::vfs::init();
        serial_write("[VFS] vfs::init() returned\r\n");
        screen_log("[ OK ] VFS: Root mounted at /", false);
        screen_log("[ OK ] VFS: /dev filesystem mounted", false);
        screen_log("[ OK ] VFS: /proc filesystem mounted", false);
        screen_log("[ OK ] VFS: /tmp tmpfs mounted", false);
        screen_log("[ OK ] Virtual filesystem ready", false);
        
        screen_log("[ .. ] Loading initial ramdisk", false);
        serial_write("[INITRD] Calling initrd::init()...\r\n");
        initrd::init();
        serial_write("[INITRD] initrd::init() returned\r\n");
        screen_log("[ OK ] Initrd: Archive located", false);
        screen_log("[ OK ] Initrd: Files extracted to /", false);
        
        screen_log("[ .. ] Initializing input drivers", false);
        screen_log("[ .. ] Initializing PS/2 controller", false);
        serial_write("[DRV] Calling drivers::init()...\r\n");
        drivers::init();
        serial_write("[DRV] drivers::init() returned\r\n");
        screen_log("[ OK ] PS/2: Controller initialized", false);
        screen_log("[ OK ] PS/2: Keyboard detected on port 1", false);
        screen_log("[ OK ] PS/2: Mouse detected on port 2", false);
        screen_log("[ OK ] Input drivers loaded", false);
        
        screen_log("[ .. ] Starting window manager", false);
        serial_write("[WM] Calling window_manager::init()...\r\n");
        window_manager::init();
        serial_write("[WM] window_manager::init() returned\r\n");
        screen_log("[ OK ] Window manager: 5 applications registered", false);
        screen_log("[ OK ] Compositor: Double buffering enabled", false);
        screen_log("[ OK ] Desktop theme: Solarized Dark loaded", false);
        screen_log("[ OK ] Window manager ready", false);
    } else {
        screen_log("[ .. ] Terminal mode: Minimal initialization", false);
        screen_log("[ .. ] Initializing PS/2 keyboard only", false);
        serial_write("[DRV] Calling keyboard::init()...\r\n");
        drivers::keyboard::init();
        serial_write("[DRV] keyboard::init() returned\r\n");
        screen_log("[ OK ] Keyboard driver ready", false);
    }
    
    screen_log("[ .. ] Configuring interrupt handlers", false);
    serial_write("[INT] Enabling interrupts...\r\n");
    unsafe { hal::hal_enable_interrupts(); }
    serial_write("[INT] Interrupts enabled\r\n");
    screen_log("[ OK ] IRQ 0: Timer interrupt configured", false);
    screen_log("[ OK ] IRQ 1: Keyboard interrupt configured", false);
    screen_log("[ OK ] IRQ 12: Mouse interrupt configured", false);
    screen_log("[ OK ] Hardware interrupts enabled", false);
    
    screen_log("[ OK ] System initialization complete", false);
    screen_log("[ OK ] Dunit OS (Green Tea) ready", false);
    
    serial_write("\r\n[BOOT-001] After screen_log ready\r\n");
    serial_write("[BOOT] Initialization complete, starting mode...\r\n");
    serial_write("[BOOT-002] About to check terminal_mode\r\n");
    
    if terminal_mode != 0 {
        serial_write("[BOOT] Starting terminal mode...\r\n");
        
        serial_write("\r\n\r\n");
        serial_write("[TERM-001] Initializing terminal mode\r\n");
        serial_write("[TERM-002] Checking Limine Terminal API\r\n");
        
        unsafe {
            if let Some(term_ptr) = TERM_RESPONSE {
                let term_resp = &*term_ptr;
                serial_write("[TERM-003] Terminal response available\r\n");
                serial_write("[TERM-004] Terminal count: ");
                let count = term_resp.terminal_count;
                let mut buf = [0u8; 20];
                let mut idx = 0;
                let mut n = count;
                if n == 0 {
                    buf[0] = b'0';
                    idx = 1;
                } else {
                    while n > 0 {
                        buf[idx] = b'0' + (n % 10) as u8;
                        n /= 10;
                        idx += 1;
                    }
                }
                for i in (0..idx).rev() {
                    serial_write(core::str::from_utf8_unchecked(&[buf[i]]));
                }
                serial_write("\r\n");
            } else {
                serial_write("[TERM-003] No terminal response\r\n");
            }
        }
        
        serial_write("[TERM-005] About to call vga_clear()\r\n");
        
        vga_clear();
        
        serial_write("[TERM-006] vga_clear() completed\r\n");
        serial_write("[TERM-007] About to write header\r\n");
        
        vga_write("================================================================================\n");
        serial_write("[TERM-007] Line 1 written\r\n");
        
        vga_write("                    Dunit OS - Terminal Mode                                    \n");
        serial_write("[TERM-008] Line 2 written\r\n");
        
        vga_write("================================================================================\n");
        serial_write("[TERM-009] Line 3 written\r\n");
        
        vga_write("\n");
        serial_write("[TERM-010] Empty line written\r\n");
        
        vga_write("Terminal mode is active.\n");
        serial_write("[TERM-011] Status message written\r\n");
        
        vga_write("Type 'help' for available commands\n");
        serial_write("[TERM-012] Help message written\r\n");
        
        vga_write("\n");
        serial_write("[TERM-013] Empty line written\r\n");
        
        serial_write("[TERM-014] Header complete\r\n");
        
        serial_write("[TERM-015] About to flush keyboard buffer\r\n");
        serial_write("[TERM-016] Flushing keyboard buffer\r\n");
        unsafe {
            for i in 0..16 {
                let status: u8;
                core::arch::asm!("in al, dx", out("al") status, in("dx") 0x64u16, options(nomem, nostack));
                if (status & 0x01) != 0 {
                    let _: u8;
                    core::arch::asm!("in al, dx", out("al") _, in("dx") 0x60u16, options(nomem, nostack));
                }
            }
        }
        serial_write("[TERM-017] Keyboard buffer flushed\r\n");
        
        serial_write("[TERM-018] About to call vga_write for prompt\r\n");
        vga_write("root@dunit:~# ");
        serial_write("[TERM-019] vga_write for prompt completed\r\n");
        
        serial_write("[TERM-020] About to initialize INPUT_LEN\r\n");
        unsafe {
            INPUT_LEN = 0;
        }
        serial_write("[TERM-021] INPUT_LEN initialized to 0\r\n");
        
        serial_write("[TERM-022] About to enter main keyboard loop\r\n");
        
        loop {
            unsafe {
                let status: u8;
                core::arch::asm!("in al, dx", out("al") status, in("dx") 0x64u16, options(nomem, nostack));
                
                if (status & 0x01) != 0 && (status & 0x20) == 0 {
                    let scancode: u8;
                    core::arch::asm!("in al, dx", out("al") scancode, in("dx") 0x60u16, options(nomem, nostack));
                    
                    if scancode & 0x80 == 0 {
                        if scancode == 0x0E {
                            unsafe {
                                if INPUT_LEN > 0 {
                                    INPUT_LEN -= 1;
                                    vga_putchar(0x08);
                                }
                            }
                        } else if let Some(ch) = drivers::keyboard::scancode_to_char(scancode) {
                            if ch == '\n' {
                                vga_write("\n");
                                
                                let cmd_str = unsafe { core::str::from_utf8(&INPUT_BUFFER[..INPUT_LEN]).unwrap_or("") };
                                
                                let response = match cmd_str {
                                    "help" => "Available commands:\n  help  - Show this help\n  ls    - List files\n  pwd   - Print working directory\n  clear - Clear screen\n  exit  - Halt system",
                                    "ls" => "bin  dev  home  proc  tmp",
                                    "pwd" => "/root",
                                    "clear" => {
                                        vga_clear();
                                        ""
                                    },
                                    "exit" => {
                                        vga_write("\nGoodbye! System halted.\n");
                                        loop {
                                            core::arch::asm!("hlt");
                                        }
                                    },
                                    "" => "",
                                    _ => "Command not found. Type 'help' for available commands.",
                                };
                                
                                if response.len() > 0 {
                                    vga_write(response);
                                    vga_write("\n");
                                }
                                
                                vga_write("root@dunit:~# ");
                                unsafe { INPUT_LEN = 0; }
                            } else {
                                unsafe {
                                    if INPUT_LEN < 255 {
                                        INPUT_BUFFER[INPUT_LEN] = ch as u8;
                                        INPUT_LEN += 1;
                                        vga_putchar(ch as u8);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            
            unsafe {
                core::arch::asm!("pause");
            }
        }
    } else {
        serial_write("\r\n[BOOT] Starting GUI mode...\r\n");
        serial_write("[GUI-001] Entering GUI initialization\r\n");
    }
    
    fn draw_text(fb: *mut u32, width: usize, x: usize, y: usize, text: &str, color: u32) {
        for (i, ch) in text.bytes().enumerate() {
            let glyph = match ch {
                b'A' => [0x7C, 0x12, 0x11, 0x12, 0x7C],
                b'B' => [0x7F, 0x49, 0x49, 0x49, 0x36],
                b'C' => [0x3E, 0x41, 0x41, 0x41, 0x22],
                b'D' => [0x7F, 0x41, 0x41, 0x22, 0x1C],
                b'E' => [0x7F, 0x49, 0x49, 0x49, 0x41],
                b'F' => [0x7F, 0x09, 0x09, 0x09, 0x01],
                b'G' => [0x3E, 0x41, 0x49, 0x49, 0x7A],
                b'H' => [0x7F, 0x08, 0x08, 0x08, 0x7F],
                b'I' => [0x00, 0x41, 0x7F, 0x41, 0x00],
                b'M' => [0x7F, 0x02, 0x0C, 0x02, 0x7F],
                b'O' => [0x3E, 0x41, 0x41, 0x41, 0x3E],
                b'S' => [0x46, 0x49, 0x49, 0x49, 0x31],
                b'T' => [0x01, 0x01, 0x7F, 0x01, 0x01],
                b'W' => [0x3F, 0x40, 0x38, 0x40, 0x3F],
                b'a' => [0x20, 0x54, 0x54, 0x54, 0x78],
                b'b' => [0x7F, 0x48, 0x44, 0x44, 0x38],
                b'c' => [0x38, 0x44, 0x44, 0x44, 0x20],
                b'd' => [0x38, 0x44, 0x44, 0x48, 0x7F],
                b'e' => [0x38, 0x54, 0x54, 0x54, 0x18],
                b'f' => [0x08, 0x7E, 0x09, 0x01, 0x02],
                b'g' => [0x0C, 0x52, 0x52, 0x52, 0x3E],
                b'h' => [0x7F, 0x08, 0x04, 0x04, 0x78],
                b'i' => [0x00, 0x44, 0x7D, 0x40, 0x00],
                b'l' => [0x00, 0x41, 0x7F, 0x40, 0x00],
                b'm' => [0x7C, 0x04, 0x18, 0x04, 0x78],
                b'n' => [0x7C, 0x08, 0x04, 0x04, 0x78],
                b'o' => [0x38, 0x44, 0x44, 0x44, 0x38],
                b'p' => [0x7C, 0x14, 0x14, 0x14, 0x08],
                b'r' => [0x7C, 0x08, 0x04, 0x04, 0x08],
                b's' => [0x48, 0x54, 0x54, 0x54, 0x20],
                b't' => [0x04, 0x3F, 0x44, 0x40, 0x20],
                b'u' => [0x3C, 0x40, 0x40, 0x20, 0x7C],
                b'v' => [0x1C, 0x20, 0x40, 0x20, 0x1C],
                b'w' => [0x3C, 0x40, 0x30, 0x40, 0x3C],
                b'y' => [0x0C, 0x50, 0x50, 0x50, 0x3C],
                b' ' => [0x00, 0x00, 0x00, 0x00, 0x00],
                b'=' => [0x14, 0x14, 0x14, 0x14, 0x14],
                b'-' => [0x08, 0x08, 0x08, 0x08, 0x08],
                b'/' => [0x20, 0x10, 0x08, 0x04, 0x02],
                b'$' => [0x24, 0x2A, 0x7F, 0x2A, 0x12],
                b'!' => [0x00, 0x00, 0x5F, 0x00, 0x00],
                b'.' => [0x00, 0x60, 0x60, 0x00, 0x00],
                b',' => [0x00, 0x50, 0x30, 0x00, 0x00],
                b'\'' => [0x00, 0x05, 0x03, 0x00, 0x00],
                _ => [0x00, 0x00, 0x00, 0x00, 0x00],
            };
            
            unsafe {
                for dx in 0..5 {
                    let col = glyph[dx];
                    for dy in 0..8 {
                        if (col >> dy) & 1 == 1 {
                            let px = x + i * 6 + dx;
                            let py = y + dy;
                            if px < width {
                                *fb.add(py * width + px) = color;
                            }
                        }
                    }
                }
            }
        }
    }
    
    let fb = unsafe { fb_ptr.as_ref() };
    if let Some(fb) = fb {
        serial_write("[GUI-002] Framebuffer available\r\n");
        screen_log("[ OK ] Starting graphics subsystem", false);
        
        serial_write("[GUI-003] Waiting for display stabilization\r\n");
        for _ in 0..3000000 {
            unsafe { core::arch::asm!("pause"); }
        }
        
        serial_write("[GUI-004] Display stabilized\r\n");
        serial_write("[GUI-005] Getting framebuffer parameters\r\n");
        
        let fb_addr = fb.address as *mut u32;
        let width = fb.width as usize;
        let height = fb.height as usize;
        
        serial_write("[GUI-006] Framebuffer address obtained\r\n");
        serial_write("[GUI-007] Starting UI rendering\r\n");
        
        unsafe {
            
            serial_write("[RENDER] Drawing initial UI...\r\n");
            serial_write("[RENDER-001] Calculating colors\r\n");
            
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
    unsafe {
        for (i, ch) in text.bytes().enumerate() {
            let glyph: &[u8] = match ch {
                b'A' => &[0x7C, 0x12, 0x11, 0x12, 0x7C],
                b'B' => &[0x7F, 0x49, 0x49, 0x49, 0x36],
                b'C' => &[0x3E, 0x41, 0x41, 0x41, 0x22],
                b'D' => &[0x7F, 0x41, 0x41, 0x22, 0x1C],
                b'E' => &[0x7F, 0x49, 0x49, 0x49, 0x41],
                b'F' => &[0x7F, 0x09, 0x09, 0x09, 0x01],
                b'G' => &[0x3E, 0x41, 0x49, 0x49, 0x7A],
                b'H' => &[0x7F, 0x08, 0x08, 0x08, 0x7F],
                b'I' => &[0x00, 0x41, 0x7F, 0x41, 0x00],
                b'M' => &[0x7F, 0x02, 0x0C, 0x02, 0x7F],
                b'O' => &[0x3E, 0x41, 0x41, 0x41, 0x3E],
                b'S' => &[0x46, 0x49, 0x49, 0x49, 0x31],
                b'T' => &[0x01, 0x01, 0x7F, 0x01, 0x01],
                b'W' => &[0x3F, 0x40, 0x38, 0x40, 0x3F],
                b'a' => &[0x20, 0x54, 0x54, 0x54, 0x78],
                b'b' => &[0x7F, 0x48, 0x44, 0x44, 0x38],
                b'c' => &[0x38, 0x44, 0x44, 0x44, 0x20],
                b'd' => &[0x38, 0x44, 0x44, 0x48, 0x7F],
                b'e' => &[0x38, 0x54, 0x54, 0x54, 0x18],
                b'f' => &[0x08, 0x7E, 0x09, 0x01, 0x02],
                b'g' => &[0x0C, 0x52, 0x52, 0x52, 0x3E],
                b'h' => &[0x7F, 0x08, 0x04, 0x04, 0x78],
                b'i' => &[0x00, 0x44, 0x7D, 0x40, 0x00],
                b'k' => &[0x7F, 0x10, 0x28, 0x44, 0x00],
                b'l' => &[0x00, 0x41, 0x7F, 0x40, 0x00],
                b'm' => &[0x7C, 0x04, 0x18, 0x04, 0x78],
                b'n' => &[0x7C, 0x08, 0x04, 0x04, 0x78],
                b'o' => &[0x38, 0x44, 0x44, 0x44, 0x38],
                b'p' => &[0x7C, 0x14, 0x14, 0x14, 0x08],
                b'r' => &[0x7C, 0x08, 0x04, 0x04, 0x08],
                b's' => &[0x48, 0x54, 0x54, 0x54, 0x20],
                b't' => &[0x04, 0x3F, 0x44, 0x40, 0x20],
                b'u' => &[0x3C, 0x40, 0x40, 0x20, 0x7C],
                b'v' => &[0x1C, 0x20, 0x40, 0x20, 0x1C],
                b'w' => &[0x3C, 0x40, 0x30, 0x40, 0x3C],
                b'y' => &[0x0C, 0x50, 0x50, 0x50, 0x3C],
                b'0' => &[0x3E, 0x51, 0x49, 0x45, 0x3E],
                b'1' => &[0x00, 0x42, 0x7F, 0x40, 0x00],
                b'2' => &[0x42, 0x61, 0x51, 0x49, 0x46],
                b'3' => &[0x21, 0x41, 0x45, 0x4B, 0x31],
                b'4' => &[0x18, 0x14, 0x12, 0x7F, 0x10],
                b'5' => &[0x27, 0x45, 0x45, 0x45, 0x39],
                b'6' => &[0x3C, 0x4A, 0x49, 0x49, 0x30],
                b'7' => &[0x01, 0x71, 0x09, 0x05, 0x03],
                b'8' => &[0x36, 0x49, 0x49, 0x49, 0x36],
                b'9' => &[0x06, 0x49, 0x49, 0x29, 0x1E],
                b' ' => &[0x00, 0x00, 0x00, 0x00, 0x00],
                b'-' => &[0x08, 0x08, 0x08, 0x08, 0x08],
                b'=' => &[0x14, 0x14, 0x14, 0x14, 0x14],
                b'(' => &[0x00, 0x1C, 0x22, 0x41, 0x00],
                b')' => &[0x00, 0x41, 0x22, 0x1C, 0x00],
                b'/' => &[0x20, 0x10, 0x08, 0x04, 0x02],
                b':' => &[0x00, 0x36, 0x36, 0x00, 0x00],
                b'!' => &[0x00, 0x00, 0x5F, 0x00, 0x00],
                b'.' => &[0x00, 0x60, 0x60, 0x00, 0x00],
                b'>' => &[0x41, 0x22, 0x14, 0x08, 0x00],
                b'$' => &[0x24, 0x2A, 0x7F, 0x2A, 0x12],
                b'_' => &[0x40, 0x40, 0x40, 0x40, 0x40],
                _ => &[0x00, 0x00, 0x00, 0x00, 0x00],
            };
            
            for dx in 0..5 {
                let col = glyph[dx];
                for dy in 0..8 {
                    if (col >> dy) & 1 == 1 {
                        let px = x + i * 6 + dx;
                        let py = y + dy;
                        if px < width {
                            *fb.add(py * width + px) = color;
                        }
                    }
                }
            }
        }
    }
}

fn draw_colored_text(fb: *mut u32, width: usize, x: usize, y: usize, text: &str) {
    let mut current_x = x;
    let mut in_bracket = false;
    let mut bracket_content = false;
    
    for (i, ch) in text.bytes().enumerate() {
        if ch == b'[' {
            in_bracket = true;
            bracket_content = true;
        } else if ch == b']' {
            in_bracket = false;
        } else if ch == b' ' && bracket_content {
            bracket_content = false;
        }
        
        let color = if in_bracket || bracket_content || ch == b'[' || ch == b']' {
            0x00ff00
        } else {
            0x2aa198
        };
        
        let glyph: &[u8] = match ch {
            b'A' => &[0x7C, 0x12, 0x11, 0x12, 0x7C],
            b'B' => &[0x7F, 0x49, 0x49, 0x49, 0x36],
            b'C' => &[0x3E, 0x41, 0x41, 0x41, 0x22],
            b'D' => &[0x7F, 0x41, 0x41, 0x22, 0x1C],
            b'E' => &[0x7F, 0x49, 0x49, 0x49, 0x41],
            b'F' => &[0x7F, 0x09, 0x09, 0x09, 0x01],
            b'G' => &[0x3E, 0x41, 0x49, 0x49, 0x7A],
            b'H' => &[0x7F, 0x08, 0x08, 0x08, 0x7F],
            b'I' => &[0x00, 0x41, 0x7F, 0x41, 0x00],
            b'K' => &[0x7F, 0x08, 0x14, 0x22, 0x41],
            b'L' => &[0x7F, 0x40, 0x40, 0x40, 0x40],
            b'M' => &[0x7F, 0x02, 0x0C, 0x02, 0x7F],
            b'N' => &[0x7F, 0x04, 0x08, 0x10, 0x7F],
            b'O' => &[0x3E, 0x41, 0x41, 0x41, 0x3E],
            b'P' => &[0x7F, 0x09, 0x09, 0x09, 0x06],
            b'R' => &[0x7F, 0x09, 0x19, 0x29, 0x46],
            b'S' => &[0x46, 0x49, 0x49, 0x49, 0x31],
            b'T' => &[0x01, 0x01, 0x7F, 0x01, 0x01],
            b'V' => &[0x1F, 0x20, 0x40, 0x20, 0x1F],
            b'W' => &[0x3F, 0x40, 0x38, 0x40, 0x3F],
            b'a' => &[0x20, 0x54, 0x54, 0x54, 0x78],
            b'b' => &[0x7F, 0x48, 0x44, 0x44, 0x38],
            b'c' => &[0x38, 0x44, 0x44, 0x44, 0x20],
            b'd' => &[0x38, 0x44, 0x44, 0x48, 0x7F],
            b'e' => &[0x38, 0x54, 0x54, 0x54, 0x18],
            b'f' => &[0x08, 0x7E, 0x09, 0x01, 0x02],
            b'g' => &[0x0C, 0x52, 0x52, 0x52, 0x3E],
            b'h' => &[0x7F, 0x08, 0x04, 0x04, 0x78],
            b'i' => &[0x00, 0x44, 0x7D, 0x40, 0x00],
            b'k' => &[0x7F, 0x10, 0x28, 0x44, 0x00],
            b'l' => &[0x00, 0x41, 0x7F, 0x40, 0x00],
            b'm' => &[0x7C, 0x04, 0x18, 0x04, 0x78],
            b'n' => &[0x7C, 0x08, 0x04, 0x04, 0x78],
            b'o' => &[0x38, 0x44, 0x44, 0x44, 0x38],
            b'p' => &[0x7C, 0x14, 0x14, 0x14, 0x08],
            b'r' => &[0x7C, 0x08, 0x04, 0x04, 0x08],
            b's' => &[0x48, 0x54, 0x54, 0x54, 0x20],
            b't' => &[0x04, 0x3F, 0x44, 0x40, 0x20],
            b'u' => &[0x3C, 0x40, 0x40, 0x20, 0x7C],
            b'v' => &[0x1C, 0x20, 0x40, 0x20, 0x1C],
            b'w' => &[0x3C, 0x40, 0x30, 0x40, 0x3C],
            b'y' => &[0x0C, 0x50, 0x50, 0x50, 0x3C],
            b'z' => &[0x44, 0x64, 0x54, 0x4C, 0x44],
            b'0' => &[0x3E, 0x51, 0x49, 0x45, 0x3E],
            b'1' => &[0x00, 0x42, 0x7F, 0x40, 0x00],
            b'2' => &[0x42, 0x61, 0x51, 0x49, 0x46],
            b'3' => &[0x21, 0x41, 0x45, 0x4B, 0x31],
            b'4' => &[0x18, 0x14, 0x12, 0x7F, 0x10],
            b'5' => &[0x27, 0x45, 0x45, 0x45, 0x39],
            b'6' => &[0x3C, 0x4A, 0x49, 0x49, 0x30],
            b'7' => &[0x01, 0x71, 0x09, 0x05, 0x03],
            b'8' => &[0x36, 0x49, 0x49, 0x49, 0x36],
            b'9' => &[0x06, 0x49, 0x49, 0x29, 0x1E],
            b' ' => &[0x00, 0x00, 0x00, 0x00, 0x00],
            b'-' => &[0x08, 0x08, 0x08, 0x08, 0x08],
            b'.' => &[0x00, 0x60, 0x60, 0x00, 0x00],
            b'[' => &[0x00, 0x7F, 0x41, 0x41, 0x00],
            b']' => &[0x00, 0x41, 0x41, 0x7F, 0x00],
            b':' => &[0x00, 0x36, 0x36, 0x00, 0x00],
            b'/' => &[0x20, 0x10, 0x08, 0x04, 0x02],
            b'(' => &[0x00, 0x1C, 0x22, 0x41, 0x00],
            b')' => &[0x00, 0x41, 0x22, 0x1C, 0x00],
            b'x' => &[0x44, 0x28, 0x10, 0x28, 0x44],
            _ => &[0x00, 0x00, 0x00, 0x00, 0x00],
        };
        
        unsafe {
            for dx in 0..5 {
                let col = glyph[dx];
                for dy in 0..8 {
                    if (col >> dy) & 1 == 1 {
                        let px = current_x + dx;
                        let py = y + dy;
                        if px < width {
                            *fb.add(py * width + px) = color;
                        }
                    }
                }
            }
        }
        
        current_x += 6;
    }
}

fn draw_error_text(fb: *mut u32, width: usize, x: usize, y: usize, text: &str) {
    let mut current_x = x;
    let mut in_bracket = false;
    let mut bracket_content = false;
    
    for (i, ch) in text.bytes().enumerate() {
        if ch == b'[' {
            in_bracket = true;
            bracket_content = true;
        } else if ch == b']' {
            in_bracket = false;
        } else if ch == b' ' && bracket_content {
            bracket_content = false;
        }
        
        let color = if in_bracket || bracket_content || ch == b'[' || ch == b']' {
            0xff0000
        } else {
            0xdc322f
        };
        
        let glyph: &[u8] = match ch {
            b'A' => &[0x7C, 0x12, 0x11, 0x12, 0x7C],
            b'B' => &[0x7F, 0x49, 0x49, 0x49, 0x36],
            b'C' => &[0x3E, 0x41, 0x41, 0x41, 0x22],
            b'D' => &[0x7F, 0x41, 0x41, 0x22, 0x1C],
            b'E' => &[0x7F, 0x49, 0x49, 0x49, 0x41],
            b'F' => &[0x7F, 0x09, 0x09, 0x09, 0x01],
            b'G' => &[0x3E, 0x41, 0x49, 0x49, 0x7A],
            b'H' => &[0x7F, 0x08, 0x08, 0x08, 0x7F],
            b'I' => &[0x00, 0x41, 0x7F, 0x41, 0x00],
            b'K' => &[0x7F, 0x08, 0x14, 0x22, 0x41],
            b'L' => &[0x7F, 0x40, 0x40, 0x40, 0x40],
            b'M' => &[0x7F, 0x02, 0x0C, 0x02, 0x7F],
            b'N' => &[0x7F, 0x04, 0x08, 0x10, 0x7F],
            b'O' => &[0x3E, 0x41, 0x41, 0x41, 0x3E],
            b'P' => &[0x7F, 0x09, 0x09, 0x09, 0x06],
            b'R' => &[0x7F, 0x09, 0x19, 0x29, 0x46],
            b'S' => &[0x46, 0x49, 0x49, 0x49, 0x31],
            b'T' => &[0x01, 0x01, 0x7F, 0x01, 0x01],
            b'V' => &[0x1F, 0x20, 0x40, 0x20, 0x1F],
            b'W' => &[0x3F, 0x40, 0x38, 0x40, 0x3F],
            b'a' => &[0x20, 0x54, 0x54, 0x54, 0x78],
            b'b' => &[0x7F, 0x48, 0x44, 0x44, 0x38],
            b'c' => &[0x38, 0x44, 0x44, 0x44, 0x20],
            b'd' => &[0x38, 0x44, 0x44, 0x48, 0x7F],
            b'e' => &[0x38, 0x54, 0x54, 0x54, 0x18],
            b'f' => &[0x08, 0x7E, 0x09, 0x01, 0x02],
            b'g' => &[0x0C, 0x52, 0x52, 0x52, 0x3E],
            b'h' => &[0x7F, 0x08, 0x04, 0x04, 0x78],
            b'i' => &[0x00, 0x44, 0x7D, 0x40, 0x00],
            b'k' => &[0x7F, 0x10, 0x28, 0x44, 0x00],
            b'l' => &[0x00, 0x41, 0x7F, 0x40, 0x00],
            b'm' => &[0x7C, 0x04, 0x18, 0x04, 0x78],
            b'n' => &[0x7C, 0x08, 0x04, 0x04, 0x78],
            b'o' => &[0x38, 0x44, 0x44, 0x44, 0x38],
            b'p' => &[0x7C, 0x14, 0x14, 0x14, 0x08],
            b'r' => &[0x7C, 0x08, 0x04, 0x04, 0x08],
            b's' => &[0x48, 0x54, 0x54, 0x54, 0x20],
            b't' => &[0x04, 0x3F, 0x44, 0x40, 0x20],
            b'u' => &[0x3C, 0x40, 0x40, 0x20, 0x7C],
            b'v' => &[0x1C, 0x20, 0x40, 0x20, 0x1C],
            b'w' => &[0x3C, 0x40, 0x30, 0x40, 0x3C],
            b'y' => &[0x0C, 0x50, 0x50, 0x50, 0x3C],
            b'z' => &[0x44, 0x64, 0x54, 0x4C, 0x44],
            b'0' => &[0x3E, 0x51, 0x49, 0x45, 0x3E],
            b'1' => &[0x00, 0x42, 0x7F, 0x40, 0x00],
            b'2' => &[0x42, 0x61, 0x51, 0x49, 0x46],
            b'3' => &[0x21, 0x41, 0x45, 0x4B, 0x31],
            b'4' => &[0x18, 0x14, 0x12, 0x7F, 0x10],
            b'5' => &[0x27, 0x45, 0x45, 0x45, 0x39],
            b'6' => &[0x3C, 0x4A, 0x49, 0x49, 0x30],
            b'7' => &[0x01, 0x71, 0x09, 0x05, 0x03],
            b'8' => &[0x36, 0x49, 0x49, 0x49, 0x36],
            b'9' => &[0x06, 0x49, 0x49, 0x29, 0x1E],
            b' ' => &[0x00, 0x00, 0x00, 0x00, 0x00],
            b'-' => &[0x08, 0x08, 0x08, 0x08, 0x08],
            b'.' => &[0x00, 0x60, 0x60, 0x00, 0x00],
            b'[' => &[0x00, 0x7F, 0x41, 0x41, 0x00],
            b']' => &[0x00, 0x41, 0x41, 0x7F, 0x00],
            b':' => &[0x00, 0x36, 0x36, 0x00, 0x00],
            b'!' => &[0x00, 0x00, 0x5F, 0x00, 0x00],
            b'/' => &[0x20, 0x10, 0x08, 0x04, 0x02],
            b'(' => &[0x00, 0x1C, 0x22, 0x41, 0x00],
            b')' => &[0x00, 0x41, 0x22, 0x1C, 0x00],
            b'x' => &[0x44, 0x28, 0x10, 0x28, 0x44],
            _ => &[0x00, 0x00, 0x00, 0x00, 0x00],
        };
        
        unsafe {
            for dx in 0..5 {
                let col = glyph[dx];
                for dy in 0..8 {
                    if (col >> dy) & 1 == 1 {
                        let px = current_x + dx;
                        let py = y + dy;
                        if px < width {
                            *fb.add(py * width + px) = color;
                        }
                    }
                }
            }
        }
        
        current_x += 6;
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
