# Dunit Userspace ABI v1

> Версионированный снимок фактического syscall/exec ABI ядра Green Tea на
> момент milestone M0. Всё в этом документе сверено с исходниками, а не с
> планами: расхождение между кодом и этой таблицей считается багом документа.
>
> Источники истины:
> - таблица номеров и диспетчер — `kernel/src/syscall/mod.rs`
> - calling convention и entry-трамплины — `hal/src/syscall.asm`
> - process-entry / ELF contract — `kernel/src/elf/mod.rs`
> - userspace-обёртки — `userspace/libdunit/src/lib.rs`

Это **не Linux ABI**. Совпадение отдельных номеров или флагов с Linux
случайно. Green Tea не принимает Linux syscall numbers и не экспортирует Linux
kernel structs. Документ фиксирует то, что есть сейчас, чтобы M6 (`Dunit musl
fork`) строился поверх стабильного контракта, а не поверх текущего кода
`libdunit`.

## Статус ABI

- **Версия:** v1 (соответствует enum `Syscall` в `kernel/src/syscall/mod.rs`).
- **Стабильность:** нестабилен. До объявления v1-freeze любой номер, сигнатура
  или структура может измениться. `fork`/`exec`-образ пока возвращают `ENOSYS`
  и зарезервированы, но не реализованы.
- **Профиль:** один активный userspace-процесс за раз (single-active-process
  syscall stack policy, см. `syscall.asm`). Кооперативное переключение через
  `yield`/`wait`.

## Calling convention

Инструкция входа — `syscall` (не `int 0x80`). Настраивается в `syscall_init`
(MSR `EFER.SCE`, `STAR`, `LSTAR`=`syscall_entry`, `SFMASK` маскирует IF).

| Регистр | Роль |
|---|---|
| `rax` | номер syscall (вход) / результат (выход) |
| `rdi` | arg0 |
| `rsi` | arg1 |
| `rdx` | arg2 |
| `r10` | arg3 |
| `r8`  | arg4 |
| `r9`  | arg5 |
| `rcx`, `r11` | затираются инструкцией `syscall` (RIP/RFLAGS), не аргументы |

`rax` возвращает `isize`: `>= 0` — успех (значение зависит от вызова), `< 0` —
`-errno`. Возврат в userspace выполняется через `IRETQ` (не `SYSRET`), потому
что текущий GDT кладёт user code перед user data, что несовместимо с правилами
выбора селекторов у `SYSRET`.

Три возвращаемых значения из handler'а — служебные, userspace их не видит как
результат: `USER_CONTEXT_RETURN_MAGIC` (`0x0051595343414C4C`) означает, что
процесс завершился/переключился и ядро не должно возвращать управление в
прежний контекст.

## Таблица syscalls

Номера — из enum `Syscall`. Реализованные помечены ✅; заглушки, честно
возвращающие `ENOSYS`, помечены ⛔.

| № | Имя | Аргументы (rdi, rsi, rdx, r10, r8) | Возврат | Статус |
|---|---|---|---|---|
| 0 | `exit` | code: i32 | не возвращается | ✅ |
| 1 | `fork` | — | `ENOSYS` | ⛔ зарезервирован |
| 2 | `exec` | path: *const u8, path_len | `ENOSYS` | ⛔ зарезервирован (образный exec; для запуска см. `spawn`) |
| 3 | `read` | fd: u32, buf: *mut u8, count | прочитано байт / `-errno` | ✅ |
| 4 | `write` | fd: u32, buf: *const u8, count | записано байт / `-errno` | ✅ |
| 5 | `open` | path: *const u8, path_len, flags: u32 | fd (>=3) / `-errno` | ✅ |
| 6 | `close` | fd: u32 | 0 / `-errno` | ✅ |
| 7 | `mmap` | addr, length, prot: u32, flags: u32 | адрес / `-errno` | ✅ только anonymous private |
| 8 | `send_message` | target_pid: u32, msg: *const u8, len | len / `-errno` | ✅ байтовый IPC |
| 9 | `receive_message` | msg: *mut u8, len | прочитано / `-errno` | ✅ неблокирующий (`EAGAIN`, если пусто) |
| 10 | `get_framebuffer` | info: *mut FbInfo | 0 / `-errno` | ✅ (raw, без capability) |
| 11 | `draw_pixel` | x: u32, y: u32, color: u32 | 0 | ✅ (raw) |
| 12 | `draw_rect` | x, y, w, h, color: u32 | 0 | ✅ (raw) |
| 13 | `get_key` | — | scancode / -1 если нет | ✅ |
| 14 | `get_mouse_pos` | x: *mut u32, y: *mut u32 | 0 / `-errno` | ✅ |
| 15 | `spawn_process` | path: *const u8, path_len | pid / `-errno` | ✅ готовит Ready-ребёнка |
| 16 | `wait_process` | pid: u32, status: *mut WaitStatus | pid / `-errno` | ✅ (`EAGAIN` на ещё не запущенного) |
| 17 | `get_pid` | — | pid | ✅ |
| 18 | `kill_process` | pid: u32 | 0/magic / `-errno` | ✅ (SIGKILL-подобно, code -9) |
| 19 | `sleep` | ms: u64 | 0 | ✅ Blocked до PIT deadline |
| 20 | `debug_log` | code: u64 | 0 | ✅ пишет в serial |
| 21 | `smoke_done` | code: i32 | magic | ⚙️ только под `boot-smoke-tests` |
| 22 | `get_cwd` | buf: *mut u8, len | длина / `-errno` | ✅ |
| 23 | `chdir` | path: *const u8, path_len | 0 / `-errno` | ✅ |
| 24 | `yield` | — | magic / `-errno` | ✅ кооперативная передача |
| 25 | `get_terminal_cursor` | info: *mut TerminalCursorInfo | 0 / `-errno` | ✅ |
| 26 | `get_system_stats` | info: *mut SystemStats | 0 / `-errno` | ✅ |
| 27 | `readdir` | path, path_len, ents: *mut UserDirEntry, max | кол-во / `-errno` | ✅ |
| 28 | `stat` | path, path_len, stat: *mut UserFileStat | 0 / `-errno` | ✅ |
| 29 | `thread_create` | entry: fn(usize), stack_top: *mut u8, arg: usize | tid / `-errno` | ✅ user stack supplied by caller |
| 30 | `thread_join` | tid: u64, status: *mut WaitStatus | tid / `-errno` | ✅ nonblocking (`EAGAIN` while active) |
| 31 | `thread_exit` | code: i32 | не возвращается | ✅ secondary thread only; main thread exits process |
| 32 | `get_tid` | — | tid | ✅ main TID равен PID |
| 33 | `wait_event` | timeout_ms: u64 (`0` = без deadline) | 0 / `-errno` | ✅ ждёт непустую IPC очередь процесса или deadline; после wake нужно повторить `receive_message` |

Дополнительные потоки имеют собственные GPR, kernel stack и FXSAVE state, но
используют адресное пространство, cwd и fd table процесса. `thread_create`
проверяет executable entry и writable user stack; `stack_top` выровнен по 16
байт, место `stack_top-8` занято return slot. Entry должен вызвать
`thread_exit`; обычный `ret` приводит к fault только этого потока.
`thread_join` удаляет завершённую запись TID и возвращает её `WaitStatus`.
User stack остаётся собственностью вызывающей стороны и должен сохраняться до
`join`; `detach`, TLS и blocking join ещё не определены. Вызов `kill_process`
на собственный PID из дополнительного потока пока возвращает `EINVAL`.

## Коды ошибок (errno)

Значения совпадают с Linux по числам ради удобства будущего musl-адаптера, но
это внутренний контракт Dunit. Полный список — `kernel/src/syscall/mod.rs`.

| errno | Значение | Смысл |
|---|---|---|
| `EINTR` | -4 | прервано |
| `EIO` | -5 | ошибка ввода-вывода |
| `EBADF` | -9 | неверный дескриптор |
| `ECHILD` | -10 | не дочерний процесс |
| `EAGAIN` | -11 | повторить позже (нет сообщения / ребёнок ещё не запущен) |
| `ENOMEM` | -12 | нет памяти |
| `EACCES` | -13 | доступ запрещён (напр. чтение из write-only fd) |
| `EFAULT` | -14 | неверный user-указатель |
| `EEXIST` | -17 | уже существует |
| `ENOTDIR` | -20 | не каталог |
| `EISDIR` | -21 | это каталог |
| `ENFILE` | -23 | таблица fd переполнена |
| `ENAMETOOLONG` | -36 | путь/буфер слишком длинный |
| `ENOSYS` | -38 | не реализовано |
| `EMSGSIZE` | -90 | сообщение вне диапазона размера |
| `EOPNOTSUPP` | -95 | не поддерживается |
| `ENOBUFS` | -105 | очередь сообщений заполнена |
| `ENOENT` | -2 | не найдено |
| `EINVAL` | -22 | неверный аргумент |

## Дескрипторы

- Зарезервированы: `0` = stdin, `1` = stdout, `2` = stderr
  (`kernel/src/process/mod.rs`, `FdTarget::{Stdin,Stdout,Stderr}`).
- Первый выдаваемый VFS-fd — `3` (`FIRST_PROCESS_FD`), максимум `1024`
  (`MAX_PROCESS_FD`).
- stdin сейчас: терминальный foreground-ввод построчно или EOF; иначе чтение
  из fd 0 возвращает EOF. stdout/stderr зеркалятся в терминал и serial.
- Наследование fd при `spawn` пока не специфицировано как контракт — ребёнок
  получает свежую таблицу.

## Флаги open (`flags: u32`)

Битовые флаги (`kernel/src/fs/vfs.rs`, `OpenFlags`):

| Бит | Флаг |
|---|---|
| `1 << 0` | READ |
| `1 << 1` | WRITE |
| `1 << 2` | CREATE |
| `1 << 3` | TRUNC |
| `1 << 4` | APPEND |

Прочие биты → `EINVAL`. Чтение из write-only или запись в read-only → `EACCES`.

## Флаги mmap (`prot`, `flags`)

Из `userspace/libdunit/src/lib.rs` / `sys_mmap`:

- `prot`: `PROT_READ = 1<<0`, `PROT_WRITE = 1<<1`, `PROT_EXEC` учитывается как
  W^X-намерение.
- `flags`: `MAP_PRIVATE = 1<<1`, `MAP_ANONYMOUS = 1<<5`. Поддержан только
  анонимный private mapping; `munmap`/`mprotect`/file/shared mappings — нет.

## Структуры (repr(C), little-endian, x86_64)

Публикуются как versioned wire structs; musl-адаптер (M6) обязан переводить их
в публичные типы, а не пробрасывать напрямую.

```
FbInfo             { addr: u64, width: u32, height: u32, pitch: u32 }
TerminalCursorInfo { x: u32, y: u32, char_width: u32, char_height: u32 }
UserDirEntry       { name: [u8;64], name_len: usize, file_type: u32 }
UserFileStat       { file_type: u32, size: usize }
WaitStatus         { kind: i32, code: i32 }
```

`file_type`: `1` = обычный файл, `2` = каталог, `3` = устройство
(`user_file_type` в `syscall/mod.rs`).

`WaitStatus.kind`:

| kind | Значение |
|---|---|
| 0 | нормальный exit; `code` = код возврата |
| 1 | fault: page fault (`code` обычно `-14`/EFAULT) |
| 2 | fault: general protection |
| 3 | fault: invalid opcode |
| 4 | fault: divide-by-zero |
| 5 | fault: unknown |
| -1 | `WAIT_KIND_EMPTY` — статуса нет |
| -2 | `WAIT_KIND_SPAWN_PREPARED` — ребёнок ещё не запускался |

Ограничения: путь ≤ 256 байт (`MAX_USER_PATH`); один user-copy ≤ 64 KiB
(`MAX_USER_COPY`); readdir ≤ 64 записей за вызов (`MAX_USER_DIRENTS`);
IPC-сообщение ≤ 256 байт (`MAX_MESSAGE_SIZE`).

## Process-entry ABI (exec ABI v1)

Реализация — `prepare_initial_stack` в `kernel/src/elf/mod.rs` и трамплин
`run_user_process`/`run_user_context` в `hal/src/syscall.asm`.

На входе в процесс:

- `%rsp` указывает на блок ниже и выровнен `8 mod 16` (контракт входа функции
  SysV, который ожидает Rust `_start` после `call`).
- `%rdi = argc`, `%rsi = argv`, `%rdx = envp` — для no-libc Rust `_start`.
- Содержимое стека:

```text
rsp -> argc: u64
       argv[0]: *const u8
       ...
       argv[argc-1]: *const u8
       NULL
       envp[0]: *const u8
       ...
       NULL
       padding, затем NUL-терминированные строки argv/env
```

`envp` намеренно минимален. До интеграции musl (M6.3) планируется переход к
полноценному initial stack с auxiliary vector; сейчас его нет.

Верхушка user-стека — `initial_user_stack()` = `USER_STACK_TOP & !0xF`.
Максимум аргументов — `MAX_EXEC_ARGS`.

## ELF contract

Парсер/загрузчик — `kernel/src/elf/mod.rs`. Принимается:

- magic `\x7fELF`, class `ELFCLASS64` (2), data `ELFDATA2LSB` (1),
  machine `EM_X86_64` (0x3E), type `ET_EXEC` (2).
- Загружаются только сегменты `PT_LOAD`. Флаги `PF_R/PF_W/PF_X` переносятся в
  page flags с политикой W^X.
- `PT_TLS`, `PT_GNU_RELRO`, `PT_INTERP`, `ET_DYN`/PIE, динамический линковщик —
  **не поддерживаются**. Все бинарники static, non-PIE, с фиксированным load
  bias.

## Известные несоответствия «идеальному» ABI (долг для M6.3)

- Нет versioned ABI-манифеста, из которого генерировались бы Rust- и
  C-определения; сейчас номера продублированы вручную в `syscall/mod.rs` и
  `libdunit`.
- Нет capability/handle-модели: raw framebuffer/input доступны любому процессу.
- Нет `poll`/событий, `dup`/`pipe`, `seek`, часов, thread/TLS-примитивов.
- `receive_message` неблокирующий; блокирующего wait нет.
- Auxiliary vector, page size, random seed при входе процесса отсутствуют.
