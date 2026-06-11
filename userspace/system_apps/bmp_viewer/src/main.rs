#![no_std]
#![no_main]

use core::panic::PanicInfo;

const DEFAULT_PATH: &str = "/assets/logo.bmp";
const MAX_BMP_SIZE: usize = 6 * 1024 * 1024;
const READ_CHUNK_SIZE: usize = 4096;
const SCALE: u32 = 1;

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

fn draw_bmp(data: &[u8], info: BmpInfo, fb: &libdunit::FbInfo) {
    let image_w = info.width as u32 * SCALE;
    let image_h = info.height as u32 * SCALE;
    let x0 = fb.width.saturating_sub(image_w) / 2;
    let y0 = fb.height.saturating_sub(image_h) / 2;
    let bytes_per_pixel = (info.bpp / 8) as usize;

    libdunit::draw_rect(
        x0.saturating_sub(18),
        y0.saturating_sub(18),
        image_w + 36,
        image_h + 36,
        0x0008_1014,
    );

    for y in 0..info.height {
        let file_y = if info.top_down { y } else { info.height - 1 - y };
        for x in 0..info.width {
            let offset = info.data_offset + file_y * info.row_stride + x * bytes_per_pixel;
            let b = data[offset] as u32;
            let g = data[offset + 1] as u32;
            let r = data[offset + 2] as u32;
            let color = (r << 16) | (g << 8) | b;
            libdunit::draw_rect(
                x0 + x as u32 * SCALE,
                y0 + y as u32 * SCALE,
                SCALE,
                SCALE,
                color,
            );
        }
    }
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
    let path = if argc > 1 {
        unsafe { libdunit::argv_get(argc, argv, 1) }.unwrap_or(DEFAULT_PATH)
    } else {
        DEFAULT_PATH
    };

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

    draw_bmp(data, info, &fb);
    libdunit::println("bmp_viewer: rendered BMP");
    libdunit::exit(0);
}
