#![no_std]
#![no_main]

use core::panic::PanicInfo;

const WINDOW_ID: u32 = 3;
const WIDTH: u32 = 500;
const HEIGHT: u32 = 350;
const CONTENT_W: u32 = WIDTH - 36;
const CONTENT_H: u32 = HEIGHT - 64;

const BG: u32 = 0x121820;
const CARD: u32 = 0x202a35;
const CARD_ALT: u32 = 0x253241;
const ACCENT: u32 = 0x2f8fbd;
const GREEN: u32 = 0x3ca65c;
const YELLOW: u32 = 0xcaa84a;
const RED: u32 = 0xa64c58;
const MUTED: u32 = 0x566271;

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    loop {
        unsafe {
            core::arch::asm!("pause", options(nomem, nostack));
        }
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    libdunit::println("gui_stats: start");
    libdunit::gui_create_window(WINDOW_ID, "Stats", WIDTH, HEIGHT);
    libdunit::gui_set_title(WINDOW_ID, "Stats");
    libdunit::gui_set_status("gui_stats: running");

    redraw();
    let mut idle_ticks = 0u32;

    loop {
        let mut event = libdunit::GuiMessage::new(0);
        let received = libdunit::gui_recv_event(&mut event);
        if received >= 0 && event.window_id == WINDOW_ID {
            match event.kind {
                libdunit::GUI_MSG_CLOSE_EVENT => {
                    libdunit::println("gui_stats: received CLOSE_EVENT");
                    send_exit();
                    libdunit::exit(0);
                }
                _ => {}
            }
        }

        idle_ticks = idle_ticks.wrapping_add(1);
        if idle_ticks >= 180 {
            redraw();
            idle_ticks = 0;
        }
        libdunit::yield_now();
    }
}

fn send_exit() {
    libdunit::gui_set_status("gui_stats: exiting");
    let mut message = libdunit::GuiMessage::new(libdunit::GUI_MSG_EXIT);
    message.window_id = WINDOW_ID;
    libdunit::gui_send(&message);
}

fn redraw() {
    let mut stats = libdunit::SystemStats::default();
    let ok = libdunit::get_system_stats(&mut stats) == 0;

    libdunit::gui_clear(WINDOW_ID);
    libdunit::gui_draw_rect(WINDOW_ID, 0, 0, CONTENT_W, CONTENT_H, BG);
    libdunit::gui_draw_rect(WINDOW_ID, 0, 0, CONTENT_W, 34, 0x182431);
    libdunit::gui_draw_rect(WINDOW_ID, 0, 33, CONTENT_W, 2, ACCENT);
    libdunit::gui_draw_text(WINDOW_ID, 14, 12, "System Stats");

    if !ok {
        draw_card(14, 52, 436, 52, RED, "Stats API", "unavailable");
        libdunit::gui_set_status("gui_stats: stats syscall unavailable");
        return;
    }

    draw_processes(&stats);
    draw_memory(&stats);
    draw_ipc(&stats);
    draw_fs(&stats);
    draw_network(&stats);
    libdunit::gui_set_status("gui_stats: live real counters");
}

fn draw_processes(stats: &libdunit::SystemStats) {
    let mut a = [0u8; 80];
    let mut b = [0u8; 80];
    let mut alen = 0usize;
    let mut blen = 0usize;
    append_label_u64(&mut a, &mut alen, b"total", stats.process_total);
    append_bytes(&mut a, &mut alen, b"  run ");
    append_u64(&mut a, &mut alen, stats.process_running);
    append_bytes(&mut a, &mut alen, b"  ready ");
    append_u64(&mut a, &mut alen, stats.process_ready);
    append_label_u64(&mut b, &mut blen, b"dead", stats.process_dead);
    append_bytes(&mut b, &mut blen, b"  reaped ");
    append_u64(&mut b, &mut blen, stats.process_reaped);
    append_bytes(&mut b, &mut blen, b"  blocked ");
    append_u64(&mut b, &mut blen, stats.process_blocked);
    draw_card_two(14, 52, 214, 76, ACCENT, "Processes", line(&a, alen), line(&b, blen));
}

fn draw_memory(stats: &libdunit::SystemStats) {
    let mut a = [0u8; 80];
    let mut b = [0u8; 80];
    let mut alen = 0usize;
    let mut blen = 0usize;
    append_bytes(&mut a, &mut alen, b"PMM used ");
    append_bytes_size(&mut a, &mut alen, stats.pmm_used_bytes);
    append_bytes(&mut a, &mut alen, b" / ");
    append_bytes_size(&mut a, &mut alen, stats.pmm_total_bytes);
    append_bytes(&mut b, &mut blen, b"Heap used ");
    append_bytes_size(&mut b, &mut blen, stats.heap_used_bytes);
    append_bytes(&mut b, &mut blen, b"  free blocks ");
    append_u64(&mut b, &mut blen, stats.heap_free_blocks);
    draw_card_two(236, 52, 214, 76, GREEN, "Memory", line(&a, alen), line(&b, blen));
}

fn draw_ipc(stats: &libdunit::SystemStats) {
    let mut a = [0u8; 80];
    let mut b = [0u8; 80];
    let mut alen = 0usize;
    let mut blen = 0usize;
    append_label_u64(&mut a, &mut alen, b"queues", stats.ipc_queue_count);
    append_bytes(&mut a, &mut alen, b"  messages ");
    append_u64(&mut a, &mut alen, stats.ipc_queued_messages);
    append_label_u64(&mut b, &mut blen, b"shared", stats.ipc_shared_regions);
    append_bytes(&mut b, &mut blen, b"  max queue ");
    append_u64(&mut b, &mut blen, stats.ipc_max_queue_messages);
    draw_card_two(14, 142, 214, 76, YELLOW, "IPC", line(&a, alen), line(&b, blen));
}

fn draw_fs(stats: &libdunit::SystemStats) {
    let mut a = [0u8; 80];
    let mut b = [0u8; 80];
    let mut alen = 0usize;
    let mut blen = 0usize;
    append_label_u64(&mut a, &mut alen, b"files", stats.fs_files);
    append_bytes(&mut a, &mut alen, b"  dirs ");
    append_u64(&mut a, &mut alen, stats.fs_directories);
    append_bytes(&mut b, &mut blen, b"bytes ");
    append_bytes_size(&mut b, &mut blen, stats.fs_bytes);
    append_bytes(&mut b, &mut blen, b"  open ");
    append_u64(&mut b, &mut blen, stats.fs_open_handles);
    draw_card_two(236, 142, 214, 76, ACCENT, "MemFS", line(&a, alen), line(&b, blen));
}

fn draw_network(stats: &libdunit::SystemStats) {
    let mut a = [0u8; 80];
    let mut b = [0u8; 80];
    let mut len = 0usize;
    let mut blen = 0usize;
    append_label_u64(&mut a, &mut len, b"nics", stats.net_total_nics);
    append_bytes(&mut a, &mut len, b"  supported ");
    append_u64(&mut a, &mut len, stats.net_supported_nics);
    append_label_u64(&mut b, &mut blen, b"mmio", stats.net_mmio_ready_nics);
    append_bytes(&mut b, &mut blen, b"  mac ");
    append_u64(&mut b, &mut blen, stats.net_mac_ready_nics);
    draw_card_two(14, 232, 436, 60, MUTED, "Network", line(&a, len), line(&b, blen));
}

fn draw_card(x: i32, y: i32, w: u32, h: u32, accent: u32, title: &str, body: &str) {
    libdunit::gui_draw_rect(WINDOW_ID, x, y, w, h, CARD);
    libdunit::gui_draw_rect(WINDOW_ID, x, y, 4, h, accent);
    libdunit::gui_draw_text(WINDOW_ID, x + 14, y + 10, title);
    libdunit::gui_draw_text(WINDOW_ID, x + 14, y + 30, body);
}

fn draw_card_two(x: i32, y: i32, w: u32, h: u32, accent: u32, title: &str, line_a: &str, line_b: &str) {
    libdunit::gui_draw_rect(WINDOW_ID, x, y, w, h, CARD_ALT);
    libdunit::gui_draw_rect(WINDOW_ID, x, y, 4, h, accent);
    libdunit::gui_draw_text(WINDOW_ID, x + 14, y + 10, title);
    libdunit::gui_draw_text(WINDOW_ID, x + 14, y + 32, line_a);
    libdunit::gui_draw_text(WINDOW_ID, x + 14, y + 50, line_b);
}

fn append_label_u64(out: &mut [u8], len: &mut usize, label: &[u8], value: u64) {
    append_bytes(out, len, label);
    append_bytes(out, len, b" ");
    append_u64(out, len, value);
}

fn append_bytes_size(out: &mut [u8], len: &mut usize, bytes: u64) {
    if bytes >= 1024 * 1024 {
        append_u64(out, len, bytes / (1024 * 1024));
        append_bytes(out, len, b"MiB");
    } else if bytes >= 1024 {
        append_u64(out, len, bytes / 1024);
        append_bytes(out, len, b"KiB");
    } else {
        append_u64(out, len, bytes);
        append_bytes(out, len, b"B");
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
            value /= 10;
            count += 1;
        }
    }
    while count > 0 {
        count -= 1;
        append_bytes(out, len, &digits[count..count + 1]);
    }
}

fn append_bytes(out: &mut [u8], len: &mut usize, value: &[u8]) {
    let mut index = 0usize;
    while index < value.len() && *len < out.len() {
        out[*len] = value[index];
        *len += 1;
        index += 1;
    }
}

fn line(buf: &[u8], len: usize) -> &str {
    core::str::from_utf8(&buf[..len]).unwrap_or("<invalid>")
}
