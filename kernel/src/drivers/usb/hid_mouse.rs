pub fn handle_boot_mouse_packet(packet: &[u8]) {
    if packet.len() < 3 {
        return;
    }

    let buttons = packet[0] & 0x07;
    let dx = packet[1] as i8 as i32;
    let dy = -(packet[2] as i8 as i32);
    let wheel = if packet.len() >= 4 {
        packet[3] as i8 as i32
    } else {
        0
    };

    crate::input::push_mouse_relative(dx, dy, buttons, wheel);
}
