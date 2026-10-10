#![no_std]
#![no_main]

//! `fsck_dunit` — пользовательская CLI-утилита проверки целостности DunitFS (M5 item 5).
//!
//! Архитектура: сам чекер — это механизм ядра (`dunitfs::fsck` + `sys_fsck`,
//! syscall 73): DunitFS целиком живёт в ядре и у userspace нет сырого
//! block-доступа. Эта программа — тонкий инструмент поверх механизма: она
//! вызывает `libdunit::fsck`, печатает человекочитаемый recovery report и
//! выходит с кодом вердикта. Так соблюдается граница kernel=mechanism /
//! userspace=tool из CLAUDE.md.
//!
//! Код выхода: 0 = clean, 1 = degraded, 2 = unrecoverable,
//! 3 = нет Dunit-раздела или ошибка syscall.
//!
//! Каждая строка отчёта печатается ОДНИМ вызовом `libdunit::print`,
//! оканчивающимся на `\n` (ядро дописывает `\r\n` к write_stdout без хвостового
//! перевода строки, поэтому фрагментировать строку нельзя).

use core::panic::PanicInfo;

extern crate alloc;

use alloc::string::String;

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    loop {
        core::hint::spin_loop();
    }
}

fn push_u32(s: &mut String, mut v: u32) {
    if v == 0 {
        s.push('0');
        return;
    }
    let mut buf = [0u8; 10];
    let mut i = buf.len();
    while v > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    s.push_str(unsafe { core::str::from_utf8_unchecked(&buf[i..]) });
}

fn push_u64(s: &mut String, mut v: u64) {
    if v == 0 {
        s.push('0');
        return;
    }
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    while v > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    s.push_str(unsafe { core::str::from_utf8_unchecked(&buf[i..]) });
}

fn push_i64(s: &mut String, v: i64) {
    if v < 0 {
        s.push('-');
    }
    push_u64(s, v.unsigned_abs());
}

fn verdict_str(code: u32) -> &'static str {
    match code {
        0 => "clean",
        1 => "degraded",
        2 => "unrecoverable",
        _ => "unknown",
    }
}

fn yes_no(v: u32) -> &'static str {
    if v != 0 {
        "yes"
    } else {
        "no"
    }
}

fn line_kv(label: &str, body: &str) {
    let mut line = String::from("[fsck.dunit] ");
    line.push_str(label);
    line.push_str(": ");
    line.push_str(body);
    line.push('\n');
    libdunit::print(&line);
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut report = libdunit::FsckReport::default();
    let rc = libdunit::fsck(&mut report);
    if rc < 0 {
        // Нет Dunit-раздела (ENOENT) или ошибка копирования отчёта.
        let mut line = String::from("[fsck.dunit] error: no Dunit partition or syscall failure (rc=");
        push_i64(&mut line, rc as i64);
        line.push_str(")\n");
        libdunit::print(&line);
        libdunit::exit(3);
    }

    // Шапка вердикта.
    {
        let mut line = String::from("[fsck.dunit] verdict=");
        line.push_str(verdict_str(report.verdict));
        line.push_str(" generation=");
        push_u64(&mut line, report.generation);
        line.push('\n');
        libdunit::print(&line);
    }

    // Суперблоки.
    {
        let mut body = String::from("primary=");
        body.push_str(yes_no(report.primary_ok));
        body.push_str(" backup=");
        body.push_str(yes_no(report.backup_ok));
        body.push_str(" slot_crc=");
        body.push_str(yes_no(report.slot_crc_ok));
        line_kv("superblocks", &body);
    }

    // Статистика по узлам.
    {
        let mut body = String::new();
        push_u32(&mut body, report.nodes_ok);
        body.push('/');
        push_u32(&mut body, report.nodes_total);
        body.push_str(" ok");
        line_kv("nodes", &body);
    }
    {
        let mut body = String::new();
        push_u32(&mut body, report.nodes_corrupt);
        body.push_str(" corrupt, ");
        push_u32(&mut body, report.duplicate_paths);
        body.push_str(" duplicate path(s), ");
        push_u32(&mut body, report.extent_errors);
        body.push_str(" extent error(s), ");
        push_u32(&mut body, report.bitmap_errors);
        body.push_str(" bitmap error(s)");
        line_kv("issues", &body);
    }

    // Диагностика (одна строка на вердикт).
    match report.verdict {
        0 => line_kv("result", "filesystem is consistent"),
        1 => line_kv(
            "result",
            "filesystem mountable read-only; corrupt nodes will be dropped",
        ),
        _ => line_kv("result", "filesystem unrecoverable; no valid metadata slot"),
    }

    libdunit::exit(report.verdict as i32)
}



