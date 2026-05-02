static mut MOUSE_X: i32 = 512;
static mut MOUSE_Y: i32 = 384;
static mut MOUSE_BUTTONS: u8 = 0;

pub fn init() {
    unsafe {
        outb(0x64, 0xA8);
        outb(0x64, 0x20);
        let mut status = inb(0x60);
        status |= 0x02;
        outb(0x64, 0x60);
        outb(0x60, status);
        outb(0x64, 0xD4);
        outb(0x60, 0xF4);
    }
}

pub fn update() {
    unsafe {
        let status = inb(0x64);
        if (status & 0x01) != 0 && (status & 0x20) != 0 {
            let mut packet = [0u8; 3];
            for i in 0..3 {
                packet[i] = inb(0x60);
            }
            
            MOUSE_BUTTONS = packet[0];
            let dx = packet[1] as i8 as i32;
            let dy = -(packet[2] as i8 as i32);
            
            MOUSE_X = (MOUSE_X + dx).max(0).min(1023);
            MOUSE_Y = (MOUSE_Y + dy).max(0).min(767);
        }
    }
}

pub fn get_position() -> (i32, i32) {
    unsafe { (MOUSE_X, MOUSE_Y) }
}

pub fn get_buttons() -> u8 {
    unsafe { MOUSE_BUTTONS }
}

unsafe fn inb(port: u16) -> u8 {
    let result: u8;
    core::arch::asm!("in al, dx", out("al") result, in("dx") port, options(nomem, nostack));
    result
}

unsafe fn outb(port: u16, value: u8) {
    core::arch::asm!("out dx, al", in("dx") port, in("al") value, options(nomem, nostack));
}
