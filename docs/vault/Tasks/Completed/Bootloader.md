# ✅ Bootloader — Limine

**Статус:** ✅ Выполнено  
**Из плана:** [[../../ROADMAP|ROADMAP]]  
**Требования:** [[../../Origin/REQUIREMENTS|REQ-1]]

---

## Что сделано

- [x] Limine конфиг (`limine.conf`)
- [x] GUI режим — запуск с framebuffer
- [x] Terminal режим — `limine_terminal.conf`
- [x] Передача framebuffer pointer в kernel
- [x] Передача memory map в kernel
- [x] ISO образ через `xorriso` + `limine bios-install`
- [x] UEFI поддержка (`BOOTX64.EFI`)

## Запуск

```bash
make run          # GUI
make run-terminal # Terminal (nographic)
```

## Скриншоты

> Место для скринов

## Заметки

_Место для заметок_
