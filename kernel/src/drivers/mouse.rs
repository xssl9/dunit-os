static mut MOUSE_X: i32 = 512;
static mut MOUSE_Y: i32 = 384;
static mut MOUSE_BUTTONS: u8 = 0;
static mut MOUSE_MAX_X: i32 = 1023;
static mut MOUSE_MAX_Y: i32 = 767;
static mut PACKET: [u8; 3] = [0; 3];
static mut PACKET_INDEX: usize = 0;

pub fn init() {
    unsafe {
        wait_write();
        outb(0x64, 0xA8);

        wait_write();
        outb(0x64, 0x20);
        wait_read();
        let mut status = inb(0x60);
        status |= 0x02;

        wait_write();
        outb(0x64, 0x60);
        wait_write();
        outb(0x60, status);

        mouse_write(0xF6);
        let _ = mouse_read_ack();
        mouse_write(0xF4);
        let _ = mouse_read_ack();

        drain_output();
        PACKET_INDEX = 0;
    }
}

pub fn set_bounds(width: usize, height: usize) {
    unsafe {
        MOUSE_MAX_X = width.saturating_sub(1) as i32;
        MOUSE_MAX_Y = height.saturating_sub(1) as i32;
        MOUSE_X = MOUSE_X.max(0).min(MOUSE_MAX_X);
        MOUSE_Y = MOUSE_Y.max(0).min(MOUSE_MAX_Y);
    }
}

pub fn set_position(x: i32, y: i32) {
    unsafe {
        MOUSE_X = x.max(0).min(MOUSE_MAX_X);
        MOUSE_Y = y.max(0).min(MOUSE_MAX_Y);
    }
}

pub fn update() {
    unsafe {
        while (inb(0x64) & 0x01) != 0 {
            let status = inb(0x64);
            let byte = inb(0x60);
            if (status & 0x20) == 0 {
                continue;
            }

            if PACKET_INDEX == 0 && (byte & 0x08) == 0 {
                continue;
            }

            PACKET[PACKET_INDEX] = byte;
            PACKET_INDEX += 1;

            if PACKET_INDEX == 3 {
                MOUSE_BUTTONS = PACKET[0] & 0x07;
                let dx = PACKET[1] as i8 as i32;
                let dy = -(PACKET[2] as i8 as i32);

                MOUSE_X = (MOUSE_X + dx).max(0).min(MOUSE_MAX_X);
                MOUSE_Y = (MOUSE_Y + dy).max(0).min(MOUSE_MAX_Y);
                PACKET_INDEX = 0;
            }
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

unsafe fn wait_read() {
    for _ in 0..100000 {
        if (inb(0x64) & 0x01) != 0 {
            break;
        }
        core::arch::asm!("pause");
    }
}

unsafe fn wait_write() {
    for _ in 0..100000 {
        if (inb(0x64) & 0x02) == 0 {
            break;
        }
        core::arch::asm!("pause");
    }
}

unsafe fn mouse_write(value: u8) {
    wait_write();
    outb(0x64, 0xD4);
    wait_write();
    outb(0x60, value);
}

unsafe fn mouse_read_ack() -> u8 {
    wait_read();
    inb(0x60)
}

unsafe fn drain_output() {
    for _ in 0..32 {
        if (inb(0x64) & 0x01) == 0 {
            break;
        }
        let _ = inb(0x60);
    }
}
