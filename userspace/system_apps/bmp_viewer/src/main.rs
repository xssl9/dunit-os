#![no_std]
#![no_main]

use core::panic::PanicInfo;

const DEFAULT_PATH: &str = "/assets/logo.bmp";
const MAX_BMP_SIZE: usize = 6 * 1024 * 1024;
const READ_CHUNK_SIZE: usize = 4096;
const PAD_X: u32 = 16;
const PAD_Y: u32 = 4;
const MAX_UPSCALE: usize = 12;

static mut FILE_BUF: [u8; MAX_BMP_SIZE] = [0; MAX_BMP_SIZE];

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    loop {
        unsafe {
            core::arch::asm!("pause", options(nomem, nostack));
        }
    }
}

#[derive(Clone, Copy)]
struct BmpInfo {
    data_offset: usize,
    width: usize,
    height: usize,
    bpp: u16,
    top_down: bool,
    row_stride: usize,
}

#[derive(Clone, Copy)]
struct DrawArea {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

#[derive(Clone, Copy)]
struct RenderPlan {
    x: u32,
    y: u32,
    width: usize,
    height: usize,
    src_step: usize,
    dst_scale: usize,
}

fn read_u16(data: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*data.get(offset)?, *data.get(offset + 1)?]))
}

fn read_u32(data: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes([
        *data.get(offset)?,
        *data.get(offset + 1)?,
        *data.get(offset + 2)?,
        *data.get(offset + 3)?,
    ]))
}

fn read_i32(data: &[u8], offset: usize) -> Option<i32> {
    Some(i32::from_le_bytes([
        *data.get(offset)?,
        *data.get(offset + 1)?,
        *data.get(offset + 2)?,
        *data.get(offset + 3)?,
    ]))
}

fn parse_bmp(data: &[u8]) -> Option<BmpInfo> {
    if data.len() < 54 || data.get(0) != Some(&b'B') || data.get(1) != Some(&b'M') {
        return None;
    }

    let data_offset = read_u32(data, 10)? as usize;
    let dib_size = read_u32(data, 14)?;
    if dib_size < 40 {
        return None;
    }

    let width = read_i32(data, 18)?;
    let height = read_i32(data, 22)?;
    let planes = read_u16(data, 26)?;
    let bpp = read_u16(data, 28)?;
    let compression = read_u32(data, 30)?;
    if width <= 0 || height == 0 || planes != 1 || compression != 0 {
        return None;
    }
    if bpp != 24 && bpp != 32 {
        return None;
    }

    let abs_height = height.unsigned_abs() as usize;
    let width = width as usize;
    let bytes_per_pixel = (bpp / 8) as usize;
    let row_stride = ((width * bytes_per_pixel) + 3) & !3;
    let pixel_bytes = row_stride.checked_mul(abs_height)?;
    if data_offset.checked_add(pixel_bytes)? > data.len() {
        return None;
    }

    Some(BmpInfo {
        data_offset,
        width,
        height: abs_height,
        bpp,
        top_down: height < 0,
        row_stride,
    })
}

fn ceil_div(value: usize, by: usize) -> usize {
    if by == 0 {
        return value;
    }
    (value + by - 1) / by
}

fn render_plan(info: BmpInfo, fb: &libdunit::FbInfo) -> RenderPlan {
    let mut cursor = libdunit::TerminalCursorInfo {
        x: 0,
        y: 0,
        char_width: 8,
        char_height: 16,
    };
    let has_cursor = libdunit::get_terminal_cursor(&mut cursor);
    let x0 = if has_cursor {
        cursor.char_width.saturating_mul(2)
    } else {
        PAD_X
    };
    let y0 = if has_cursor {
        cursor.y.saturating_add(PAD_Y)
    } else {
        fb.height.saturating_sub(info.height as u32) / 2
    };
    let max_w = fb.width.saturating_sub(x0 + PAD_X).max(1) as usize;
    let max_h = fb.height.saturating_sub(y0 + cursor.char_height + PAD_Y).max(1) as usize;
    let src_step = ceil_div(info.width, max_w)
        .max(ceil_div(info.height, max_h))
        .max(1);
    let width = (info.width / src_step).max(1);
    let height = (info.height / src_step).max(1);
    let max_scale = (max_w / width).min(max_h / height).max(1);
    let target_width = (max_w * 2 / 3).max(width);
    let dst_scale = ceil_div(target_width, width)
        .max(1)
        .min(max_scale)
        .min(MAX_UPSCALE);

    RenderPlan {
        x: x0,
        y: y0,
        width,
        height,
        src_step,
        dst_scale,
    }
}

fn draw_bmp(data: &[u8], info: BmpInfo, fb: &libdunit::FbInfo) -> DrawArea {
    let plan = render_plan(info, fb);
    let image_w = (plan.width * plan.dst_scale) as u32;
    let image_h = (plan.height * plan.dst_scale) as u32;
    let bytes_per_pixel = (info.bpp / 8) as usize;
    let area = DrawArea {
        x: plan.x.saturating_sub(2),
        y: plan.y.saturating_sub(2),
        width: image_w + 4,
        height: image_h + 4,
    };

    libdunit::draw_rect(
        area.x,
        area.y,
        area.width,
        area.height,
        0x0008_1014,
    );

    for y in 0..plan.height {
        let src_y = y * plan.src_step;
        let file_y = if info.top_down {
            src_y
        } else {
            info.height - 1 - src_y
        };
        for x in 0..plan.width {
            let src_x = x * plan.src_step;
            let offset = info.data_offset + file_y * info.row_stride + src_x * bytes_per_pixel;
            let b = data[offset] as u32;
            let g = data[offset + 1] as u32;
            let r = data[offset + 2] as u32;
            let color = (r << 16) | (g << 8) | b;
            let dst_x = plan.x + (x * plan.dst_scale) as u32;
            let dst_y = plan.y + (y * plan.dst_scale) as u32;
            if plan.dst_scale == 1 {
                libdunit::draw_pixel(dst_x, dst_y, color);
            } else {
                libdunit::draw_rect(
                    dst_x,
                    dst_y,
                    plan.dst_scale as u32,
                    plan.dst_scale as u32,
                    color,
                );
            }
        }
    }

    area
}

fn drain_stale_keys() {
    let mut quiet_ticks = 0usize;
    while quiet_ticks < 8 {
        if libdunit::get_key().is_some() {
            quiet_ticks = 0;
        } else {
            quiet_ticks += 1;
            libdunit::sleep_ms(10);
        }
    }
}

fn wait_for_key() {
    drain_stale_keys();
    loop {
        if let Some(scancode) = libdunit::get_key() {
            if (scancode & 0x80) == 0 {
                break;
            }
        }
        libdunit::sleep_ms(20);
    }
}

fn is_no_wait_arg(arg: &str) -> bool {
    arg.as_bytes() == b"--no-wait"
}

fn parse_args(argc: usize, argv: libdunit::RawArgv) -> (&'static str, bool) {
    let mut path = DEFAULT_PATH;
    let mut wait = true;
    let mut index = 1usize;
    while index < argc {
        if let Some(arg) = unsafe { libdunit::argv_get(argc, argv, index) } {
            if is_no_wait_arg(arg) {
                wait = false;
            } else {
                path = arg;
            }
        }
        index += 1;
    }
    (path, wait)
}

fn read_file(path: &str) -> Result<&'static [u8], i32> {
    let fd = libdunit::open(path, libdunit::OPEN_READ);
    if fd < 0 {
        return Err(1);
    }

    let mut total = 0usize;
    loop {
        if total >= MAX_BMP_SIZE {
            libdunit::close(fd as usize);
            return Err(2);
        }
        let end = (total + READ_CHUNK_SIZE).min(MAX_BMP_SIZE);
        let buf = unsafe { &mut FILE_BUF[total..end] };
        let read = libdunit::read(fd as usize, buf);
        if read < 0 {
            libdunit::close(fd as usize);
            return Err(3);
        }
        if read == 0 {
            break;
        }
        total += read as usize;
    }

    if libdunit::close(fd as usize) != 0 {
        return Err(4);
    }

    Ok(unsafe { &FILE_BUF[..total] })
}

#[no_mangle]
pub extern "C" fn _start(
    argc: usize,
    argv: libdunit::RawArgv,
    _envp: libdunit::RawEnvp,
) -> ! {
    let (path, wait) = parse_args(argc, argv);

    let data = match read_file(path) {
        Ok(data) => data,
        Err(code) => {
            libdunit::println("bmp_viewer: read failed");
            libdunit::exit(code);
        }
    };

    let info = match parse_bmp(data) {
        Some(info) => info,
        None => {
            libdunit::println("bmp_viewer: unsupported BMP");
            libdunit::exit(5);
        }
    };

    let mut fb = libdunit::FbInfo {
        addr: 0,
        width: 0,
        height: 0,
        pitch: 0,
    };
    if !libdunit::get_framebuffer(&mut fb) {
        libdunit::println("bmp_viewer: framebuffer unavailable");
        libdunit::exit(6);
    }

    let area = draw_bmp(data, info, &fb);
    if wait {
        libdunit::println("bmp_viewer: rendered BMP, press any key to close");
        wait_for_key();
        libdunit::draw_rect(area.x, area.y, area.width, area.height, 0x0000_0000);
    } else {
        libdunit::println("bmp_viewer: rendered BMP");
    }
    libdunit::exit(0);
}
