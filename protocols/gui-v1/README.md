# Dunit GUI protocol v1.0

Статус: спецификация первого пункта M2, 2026-09-23. Нормативный контракт для
будущей protocol crate и headless reference server; реализация и conformance
suite ещё не выполнены. До их завершения и интеграции M3 это не frozen public
ABI. Изменения контракта должны одновременно обновлять эту спецификацию и
будущие тесты. «Обязан» означает требование, «может» — разрешённый вариант.

## 1. Область действия и связь с репозиторием

v1 определяет соединение клиента с GUI Server, toplevel surfaces, CPU shared
buffers, атомарный commit, configure/ack, frame callbacks, focus и ввод.
Один seat, целочисленный scale, software composition. GPU, subsurfaces, popup,
clipboard, drag-and-drop, IME composition, output enumeration и shell policy
не входят в v1.0. TEXT_INPUT передаёт только готовый текст.

Это отдельный протокол с magic `DGUI`, не совместимый с legacy `GUI1`
(`GuiMessage`, `GUI_SHELL_PID=1`, DRAW_TEXT/DRAW_RECT в
`userspace/libdunit/src/lib.rs` и обработчик `kernel/src/ui_loop.rs`). Автоматического
fallback или угадывания формата нет. Старый desktop продолжает работать до M3.
Модели `userspace/display_server/src/lib.rs` — источник идей, не wire ABI.

Транспорт абстрактный: надёжный FIFO в каждом направлении, сохранение границ
сообщений, аутентифицированный peer, уведомление disconnect и атомарная доставка
сообщения с capabilities. Частичная доставка наружу запрещена. Один пакет —
32–256 байт; фрагментации на уровне GUI нет. Это укладывается в текущий
`kernel/src/ipc/mod.rs::MAX_MESSAGE_SIZE`, но per-PID byte IPC сам по себе
не реализует соединение и атомарную передачу capabilities.

GUI Server получает endpoint через service manager/discovery (`dunit.gui.v1` —
логическое имя), клиент — разрешённое соединение, без magic PID. PID и число
в payload не служат доказательством прав. Конкретный syscall/service-discovery
binding остаётся работой M3 и не меняет wire layout.

### Требования к kernel binding M3

- Shared-memory capability закрепляет неизменяемый размер и те же физические
  страницы, поддерживает независимые ссылки клиента и сервера и read-only
  mapping сервера. Сервер получает только READ|MAP, клиент сохраняет запись.
- IMPORT_BUFFER переносит одну ограниченную ссылку атомарно с сообщением;
  ошибка доставки оставляет её отправителю, успешная доставка — получателю.
  Закрытие клиентской ссылки не отзывает уже принятые сервером страницы.
- Ни physical address, ни user pointer, ни голый shared VM ID не передаются
  по wire. Успех отправки публикует записи pixels (release), приём обеспечивает
  acquire; BUFFER_RELEASE даёт обратную границу безопасной записи.
- Сейчас `kernel/src/handle.rs::Memory(Vec<u8>)`, `duplicate` и
  `process/mod.rs::handle_map` копируют данные, limit равен 1 MiB; это **не**
  реализация нужного shared buffer. VM syscalls 36–38 имеют отдельные bearer IDs.
  Их нельзя объявлять готовым безопасным binding протокола. Raw display/input
  требуется ограничить сервисом; DISPLAY_MASTER сам по себе этого не доказывает.

## 2. Кодирование, номера и порядок

Все целые little-endian; `uN/iN` имеют ровно N бит. Поля идут подряд без implicit
padding, native structs/usize запрещены. Все reserved поля равны нулю.
Строка `str` = `length:u16` + ровно length байт UTF-8 без NUL и без padding.
Invalid UTF-8/NUL — BAD_VALUE. Rect = `x:i32,y:i32,w:u32,h:u32`.

| Offset | Поле | Значение |
|---|---|---|
| 0 | magic:u32 | 0x49554744 (байты `44 47 55 49`, DGUI) |
| 4 | major:u16, minor:u16 | 1, 0 (также для HELLO) |
| 8 | opcode:u16, flags:u16 | flags=0 |
| 12 | size:u32 | полная длина, включая header |
| 16 | object:u64 | 0 = connection; иначе ID surface/buffer |
| 24 | serial:u64 | request ID или event sequence |

Client request serial начинается с 1 и строго возрастает, без повторов и wrap.
Server serial — независимая последовательность с теми же правилами для **всех**
ответов и событий. Корреляция ответов — payload `request:u64`, не header serial.
Configure token — header serial события CONFIGURE. Commit token — request serial
COMMIT. Счётчики исчерпаны: закрыть соединение, переподключиться, wrap запрещён.
Несколько потоков клиента сериализуют отправку общей очередью.

Каждый корректно оформленный request получает ровно один RESULT, WELCOME или
ERROR. Ответы идут в порядке requests; события могут вклиниваться. RESULT
означает принятие, не показ кадра. Причинное событие идёт после ответа на запрос
(CREATE → RESULT → CONFIGURE; COMMIT → RESULT → callbacks). Исключение —
DESTROY_SURFACE: cleanup events предшествуют его RESULT-барьеру (раздел 6).
При fatal error или
disconnect доставка оставшихся ответов не гарантируется; повторить request
с тем же serial нельзя. Неизвестные opcode, flags, enum и bits отвергаются,
trailing bytes запрещены. Проверка size предшествует выделению памяти.

Object ID — выбранный клиентом ненулевой u64, общий namespace surfaces/buffers
внутри соединения. Каждый запрос создания использует ID больше любого ранее
**успешно** созданного ID; ID не переиспользуются до нового соединения. После
удаления старый ID всегда STALE_OBJECT. Чужой ID не адресует чужие объекты.
client_id:u64 выдаёт сервер монотонно с 1, без wrap, уникальный в течение его
запуска; это не capability. Неверный object=0/ненулевой object для connection
request — BAD_VALUE. Отсутствующий ID для операций над объектом — STALE_OBJECT.

## 3. Negotiation и connection state machine

```text
NEW -- HELLO / WELCOME --> ACTIVE -- DISCONNECT / RESULT --> CLOSED
NEW или ACTIVE -- fatal error / EOF / peer death --> CLOSED
```

В NEW разрешён только HELLO, в ACTIVE повторный HELLO — BAD_STATE. В NEW любой
другой request — fatal BAD_STATE. HELLO предлагает диапазон minor, сервер
выбирает максимальную общую версию; v1.0 поддерживает только [0,0]. Major в
header обязан быть 1. После WELCOME header содержит выбранную версию.
`required_features` обязан быть подмножеством `offered_features`.
В v1.0 feature bit 0 = ARGB8888; XRGB8888 обязателен без feature bit. Сервер
возвращает поддерживаемое подмножество offered; неизвестные offered bits
игнорируются **только в HELLO**, неподдержанный required bit — UNSUPPORTED.
formats: bit 0 XRGB8888, bit 1 ARGB8888, совпадает с negotiated features.

Сервер закрывает NEW после 5 s monotonic без полного HELLO. Клиент при ожидании
WELCOME дольше 5 s закрывает endpoint. В ACTIVE отсутствует idle timeout.
DISCONNECT завершает все объекты; после его RESULT новых событий нет.
Перезапуск сервера инвалидирует всё соединение: клиент повторяет HELLO,
создаёт объекты и перерисовывает; старые tokens и IDs не восстанавливаются.

## 4. Полный набор requests v1.0

`object` указан в третьем столбце. `—` означает пустой payload. Только
IMPORT_BUFFER допускает ровно одну attached capability, остальные — ноль.
Размер payload точно выводится из таблицы (переменные хвосты — str/Rect array).

| Opcode | Request | object | Payload (по порядку) |
|---|---|---|---|
| 0x0001 | HELLO | 0 | min_minor:u16, max_minor:u16, reserved:u32, offered_features:u64, required_features:u64 |
| 0x0002 | DISCONNECT | 0 | — |
| 0x0010 | CREATE_SURFACE | новый surface ID | role:u32, width:u32, height:u32, format:u32 |
| 0x0011 | DESTROY_SURFACE | surface | — |
| 0x0012 | SET_TITLE | surface | title:str |
| 0x0013 | SET_APP_ID | surface | app_id:str |
| 0x0014 | ACK_CONFIGURE | surface | configure:u64 |
| 0x0020 | IMPORT_BUFFER | новый buffer ID | width:u32, height:u32, stride:u32, format:u32, offset:u64 |
| 0x0021 | DESTROY_BUFFER | buffer | — |
| 0x0022 | ATTACH_BUFFER | surface | buffer:u64, damage_count:u32, reserved:u32, damage:Rect[damage_count] |
| 0x0023 | COMMIT | surface | configure:u64, frame_callback:u32, reserved:u32 |

role=1 TOPLEVEL, другие роли UNSUPPORTED. CREATE width/height — желаемый logical
content size; format неизменяем для lifetime surface. SET_TITLE — до 160 байт,
пустой допустим. SET_APP_ID — 1–128 ASCII байт из `[A-Za-z0-9._-]`, начально пуст;
это метаданные, не авторизация. Метаданные применяются на успешном RESULT.
frame_callback = 0 или 1. IMMUTABLE object type не меняется.
ATTACH buffer=0 — explicit unmap, damage_count тогда 0.

## 5. Полный набор replies/events v1.0

| Opcode | Reply/event | object | Payload |
|---|---|---|---|
| 0x8001 | WELCOME | 0 | request:u64, client_id:u64, features:u64, formats:u32, reserved:u32 |
| 0x8002 | RESULT | как в request | request:u64 |
| 0x8003 | ERROR | как в request, либо 0 при повреждённом header | request:u64, code:u32, fatal:u32 |
| 0x8010 | CONFIGURE | surface | width:u32, height:u32, scale:u32, state:u32 |
| 0x8011 | REQUEST_CLOSE | surface | — |
| 0x8020 | BUFFER_RELEASE | buffer | commit:u64 |
| 0x8021 | FRAME_DONE | surface | commit:u64, time_ns:u64, status:u32, reserved:u32 |
| 0x8030 | POINTER_ENTER | surface | x:i32, y:i32 |
| 0x8031 | POINTER_LEAVE | surface | — |
| 0x8032 | POINTER_MOTION | surface | time_ns:u64, x:i32, y:i32 |
| 0x8033 | POINTER_BUTTON | surface | time_ns:u64, button:u32, pressed:u32 |
| 0x8034 | POINTER_AXIS | surface | time_ns:u64, dx:i32, dy:i32 |
| 0x8040 | KEY_ENTER | surface | modifiers:u32, reserved:u32 |
| 0x8041 | KEY_LEAVE | surface | — |
| 0x8042 | KEY | surface | time_ns:u64, usage:u32, pressed:u32, modifiers:u32, repeat:u32 |
| 0x8043 | TEXT_INPUT | surface | time_ns:u64, text:str |

CONFIGURE state: bit 0 activated, bit 1 maximized, bit 2 fullscreen; остальные 0.
FRAME_DONE status: 0 PRESENTED (включён в композицию, не обещание физического
scanout), 1 DISCARDED (например, unmapped/occluded), time — monotonic ns от старта
сервера, не wall clock. TEXT_INPUT — 1–160 байт UTF-8, границы code points целые.
ERROR fatal = 0 или 1; request=0, если header нельзя прочитать безопасно.
WELCOME заменяет RESULT для HELLO. На ошибочный запрос только ERROR.

## 6. Surface/configure/commit state machines

Surface: `ABSENT → UNMAPPED → MAPPED ↔ UNMAPPED → DESTROYED`.
CREATE создаёт UNMAPPED и после RESULT посылает initial CONFIGURE. Видимость
разрешена только после первого успешного COMMIT с buffer. DESTROY допустим
из любого живого состояния, дальнейшие операции дают STALE_OBJECT.

Сервер хранит latest CONFIGURE и последний ACK; новый CONFIGURE замещает старый.
ACK допустим только для latest token этой surface и только один раз; старый,
чужой или повторный token — BAD_SERIAL. Получение ACK не меняет committed scene.
В каждый момент может быть только один latest CONFIGURE, необработанные
старые tokens не накапливаются. Для COMMIT требуется ACK latest и равный ему
payload configure; token можно применять в нескольких commits до нового
CONFIGURE. Если configure обогнал commit — BAD_SERIAL, pending сохраняется,
клиент ACK нового token и отправляет новый request. Сервер не ждёт ACK для
обслуживания других клиентов; до нового commit отображается старое содержимое.

ATTACH резервирует AVAILABLE buffer, geometry/damage сохраняются в pending.
Повторный ATTACH — BAD_STATE, пока pending не сброшен успешным COMMIT или
DESTROY_SURFACE. Сначала ACK configure, затем ATTACH/COMMIT — рекомендуемый
порядок, но ATTACH до ACK допустим. COMMIT без pending — BAD_STATE; implicit
reuse текущего buffer нет. Для unmap также нужен ACK latest.

COMMIT атомарно проверяет token, размеры, format, quotas и доступность callback
slot, затем заменяет committed state. Ошибка не меняет pending/committed и не
переводит buffer в BUSY. Success освобождает pending slot, связывает buffer с
commit token; буфер immutable для клиента с принятия ATTACH до RELEASE.
Новый buffer должен иметь `width=configure.width*scale`,
`height=configure.height*scale`, совпадающий surface format. Проверять checked
multiply. Logical size и scale относятся к content без server decorations.
Initial CONFIGURE выбирает size в пределах лимитов; сервер может скорректировать
запрошенный размер. scale = 1, 2, 3 или 4.

Damage — координаты pixels нового buffer, полуоткрытые прямоугольники,
неотрицательные x/y, ненулевые w/h, все границы внутри buffer. До 8 Rect;
пересечения разрешены, damage_count=0 означает полный buffer. При первом map,
изменении размера/scale и повторном map сервер сам считает damage полным.
Damage — подсказка оптимизации: весь buffer обязан содержать валидный кадр.
Смена метаданных не требует COMMIT.

На успешный COMMIT с frame_callback=1 приходит ровно один FRAME_DONE, кроме
disconnect. До него второй callback request на той же surface — BUSY;
commit с callback=0 разрешён. DISCARDED позволяет продолжать анимацию даже
у скрытой surface. Сервер завершает callback при ближайшем composition tick,
не ждёт видимости. Это pacing, не разрешение переписать buffer: ждать RELEASE.

REQUEST_CLOSE — предложение приложениям завершиться, не уничтожение сервером.
Оно может быть проигнорировано; повторные close gestures допустимы. DESTROY
отписывает input, снимает pending/committed, завершает callback DISCARDED и
освобождает все buffers. RESULT на DESTROY_SURFACE идёт **после** этих событий
и является барьером: далее событий с этим surface ID нет.

## 7. Buffer ownership и object lifetimes

```text
IMPORT → AVAILABLE -- ATTACH --> PENDING -- COMMIT --> BUSY
                   PENDING -- destroy surface --> AVAILABLE + RELEASE(commit=0)
                   BUSY -- last server reader done --> AVAILABLE + RELEASE(commit)
AVAILABLE -- DESTROY_BUFFER --> DESTROYED
```

IMPORT проверяет memory capability/type/READ|MAP и закрепляет mapping, metadata
копируется и остаётся неизменной. Любая ошибка import закрывает полученную
сервером capability и не создаёт объект; клиентская исходная ссылка остаётся.
Capability не может быть указана числом в payload. Повторный import того же
kernel memory object в одном соединении — BAD_VALUE: binding обязан предоставлять
сравнение identity. Один memory object — один buffer, без overlapping aliases.
Между соединениями shared pixels не дают доступа к чужим GUI objects.

Buffer принадлежит ровно одному connection и одновременно максимум одной
surface/pending/commit. ATTACH busy/pending buffer — BUSY. DESTROY_BUFFER в этих
состояниях — BUSY (не deferred destroy). До AVAILABLE клиент не пишет pixels
через **любую** ссылку; сервер не должен полагаться на честность клиента для
безопасности памяти: offset/size проверяются по сохранённой metadata, права
на resize/revoke отсутствуют. Нарушение immutable pixels может испортить только
содержимое этого buffer, не повредить память или зависнуть в compositor.

BUFFER_RELEASE отправляется ровно один раз на успешный ATTACH, когда ссылка
pending/committed больше не используется. COMMIT error не порождает RELEASE.
У текущего committed buffer RELEASE может ждать замены, unmap или destroy;
сервер также может release раньше, если сохранил собственную копию изображения.
RELEASE(commit=0) относится к отменённому pending, иначе к успешному COMMIT.
FRAME_DONE и RELEASE не имеют обязательного взаимного порядка. Клиент держит
2–3 buffers и никогда не блокирует event dispatch ожиданием RELEASE.

DESTROY/EOF/crash удаляет только objects этого connection, focus и привязки
policy. Сервер сначала исключает surfaces из scene/input, завершает readers,
затем закрывает mappings/capabilities. После disconnect события не обязательны,
но освобождение ресурсов обязательно. Клиент при потере сервера закрывает
endpoint, mappings и свои handles; новые buffers создаёт для нового connection.
Нельзя считать timeout RELEASE доказательством того, что старый сервер перестал
читать: lifetime kernel references сохраняется до реального закрытия.

## 8. Pixel formats, bounds и budgets

format=1 XRGB8888: байты B,G,R,X, X игнорируется (opaque).
format=2 ARGB8888: байты B,G,R,A, цветовые каналы premultiplied alpha в sRGB
byte space; compositor использует source-over с округлением `(a*b+127)/255`.
Для недоверенных pixels R/G/B clamp до A; XRGB alpha=255. ARGB требует feature
bit 0. stride и offset кратны 4, stride >= width*4. Mapping read-only/non-exec.
Сервер checked arithmetic вычисляет `end=offset+stride*(height-1)+width*4`;
end <= capability.size. Padding строк не является пикселями, его не читают.

| Ресурс | Жёсткий максимум v1.0 |
|---|---|
| Packet / inbound и outbound очередь на connection | 256 байт / по 128 пакетов (32 KiB) |
| Соединения на сервер | 64 (лишние закрываются до HELLO) |
| Surfaces / buffers на connection | 64 / 192 |
| width, height (logical и pixels) | 1–4096; также pixels=logical*scale <=4096 |
| stride / один backing memory object | 16384 байта / 64 MiB |
| Закреплённые memory bytes connection / server | 256 MiB / 512 MiB |
| Damage / pending attach / pending callback | 8 Rect / 1 на surface / 1 на surface |
| Title / app ID / text event | 160 / 128 / 160 байт |
| Одновременные нажатые key usages / pointer buttons | 256 / 5 |

Backings учитываются целиком, включая offset/padding; global accounting
консервативно считает каждый import, даже если страницы разделены connections.
Внутренние pixel copies также списываются в memory budgets. Успешная операция
резервирует resources до изменения scene; освобождение возвращает budget.
Лимиты фиксированы, меньшее фактическое наличие памяти даёт NO_MEMORY;
превышение budget — LIMIT_EXCEEDED. Сервер не обязан заранее выделять максимум.

При заполнении inbound очереди transport возвращает WOULD_BLOCK, не принимает
пакет и не теряет уже принятые requests.

Backpressure: send WOULD_BLOCK оставляет пакет и capabilities у отправителя,
serial не считается отправленным. Клиент ждёт writable и параллельно читает
события. Сервер обрабатывает максимум 32 requests одного connection за проход,
обслуживает остальные round-robin. Переполнение outbound disconnect-ит только
медленного клиента (fatal SLOW_CLIENT, best effort ERROR); важные события не
теряются молча. Input overflow (key/button count) закрывает connection с
LIMIT_EXCEEDED fatal=1 и сбрасывает focus, не растит структуры без ограничений.

## 9. Focus, input и граница DWM

Сервер обеспечивает максимум один keyboard focus и один pointer focus seat.
События идут только владельцу живой mapped surface. Hit-testing использует
committed scene, clipping и z-order; прозрачность pixels не меняет hit region.
Coordinates POINTER_ENTER/MOTION — signed 24.8 fixed point в logical content
coordinates; AXIS dx/dy — signed 24.8 logical pixels, положительные вправо/вниз.
button=1 left, 2 right, 3 middle, 4 back, 5 forward; pressed=0/1.

Pointer focus: LEAVE(old) → ENTER(new) → MOTION/BUTTON/AXIS(new). При нажатии
есть implicit grab surface до отпускания всех кнопок; координаты тогда могут
быть вне content. Destroy/unmap/disconnect отменяет grab. POINTER_LEAVE очищает
все pressed buttons локально; синтетические releases не требуются.
Keyboard focus: KEY_LEAVE(old) → KEY_ENTER(new) → KEY/TEXT_INPUT(new).
KEY_LEAVE очищает held keys/modifiers/repeat. KEY_ENTER передаёт текущие
modifiers, физически удерживаемые keys не пересылаются как новые нажатия;
их release без delivered press подавляется, до нового press.

KEY usage — USB HID keyboard/keypad page 0x07 usage в младших 16 битах,
верхние 16 равны нулю; это нормализованный код, не PS/2 scancode.
modifiers: bit 0 Shift, 1 Ctrl, 2 Alt, 3 Super, 4 CapsLock, 5 NumLock;
left/right объединены, значение отражает состояние после KEY. repeat=0/1;
repeat=1 допустим только при pressed=1 для уже нажатой клавиши. Сервер генерирует
repeat, клиент не дублирует. TEXT_INPUT следует после вызвавшего его KEY,
только keyboard-focused клиенту; modifiers сами по себе текст не создают.
При смене focus pending text/repeat старой surface отбрасываются.

Destroy/unmap обязан отправить LEAVE до удаления input target. Для COMMIT-unmap
сначала RESULT, затем LEAVE и callback/release; scene/input меняются атомарно
до обработки следующего input. DESTROY использует barrier из раздела 6.
При недоступном peer LEAVE заменяется локальной очисткой сервером.

DWM — отдельный аутентифицированный policy endpoint с отдельной capability,
не feature bit из HELLO и не доверие к SET_APP_ID. На обычном endpoint нет
операций установки z-order, global position, focus или чтения чужих buffers;
зарезервированный диапазон 0x1000–0x1fff возвращает ACCESS_DENIED.
Конкретный shell protocol будет специфицирован отдельно для M3/M4. Его policy
решения входят в headless model как проверенные внешние события. При crash DWM
GUI Server сохраняет surfaces, снимает его policy привязки, использует fallback:
новые toplevel поверх старых, click-to-focus, закрытие по server close gesture.
Доступ к raw display/input обычному GUI client не выдаётся.

## 10. Ошибки и порядок валидации

Protocol errors — положительные u32 из таблицы, не kernel errno.
Recoverable ERROR сохраняет connection и всё состояние ошибочной операции,
кроме потребления её request serial и закрытия переданных capabilities.
Transport failure до доставки serial не потребляет. Повтор после ERROR имеет
новый serial. Исключение cleanup: fatal всегда уничтожает connection.

| Code | Имя | Типичный случай | fatal |
|---|---|---|---|
| 1 | MALFORMED | magic/header/length/count mismatch, reserved/flags, неверное число capabilities | 1 |
| 2 | VERSION_MISMATCH | major/minor или нет пересечения | 1 |
| 3 | BAD_OPCODE | неизвестный opcode или неверное направление | 1 |
| 4 | BAD_STATE | операция не разрешена в текущем состоянии | 0 (NEW: 1) |
| 5 | STALE_OBJECT | отсутствующий/удалённый object ID | 0 |
| 6 | WRONG_OBJECT_TYPE | buffer вместо surface | 0 |
| 7 | BAD_SERIAL | configure token устарел/не ACK | 0; header serial неверен: 1 |
| 8 | BAD_VALUE | enum, geometry, UTF-8, duplicate ID/backing | 0 |
| 9 | UNSUPPORTED | role/format/required feature | 0; HELLO: 1 |
| 10 | ACCESS_DENIED | rights/type capability, policy opcode | 0 |
| 11 | OUT_OF_BOUNDS | overflow или выход pixel range/Rect за пределы | 0 |
| 12 | BUSY | buffer занят, callback pending | 0 |
| 13 | LIMIT_EXCEEDED | resource quota | 0 (input overflow: 1) |
| 14 | NO_MEMORY | allocation/mapping failure | 0 |
| 15 | SLOW_CLIENT | outbound переполнен | 1 |
| 16 | INTERNAL | отказ backend/инварианта сервера | 1 |

Validation priority: framing/attachments → version → header serial → opcode
(direction, policy range) → connection state → object existence/type или fresh
ID → payload enums/strings → configure token и object state → checked bounds →
quotas/allocation → atomic mutation. При нескольких ошибках вернуть первую
по этому порядку; поля одного этапа проверять в wire order. Неизвестный enum
BAD_VALUE, известный, но отключённый format или неподдержанная role — UNSUPPORTED.
Malformed не должен приводить к panic даже до HELLO; если ERROR отправить нельзя,
достаточно закрыть endpoint. Ошибки чужого клиента не раскрывают его ID/data.

## 11. Нормативные сценарии для следующих пунктов M2

Это требования к будущим headless/property tests, **не отчёт о пройденных тестах**.
Replay фиксирует поток requests, transport failures, clock ticks, input и policy
решения; при одинаковом вводе server output/scene/resource counters совпадают.
Wall clock, случайный ID и host pointer не могут влиять на replay.

1. HELLO → WELCOME; CREATE(1) → RESULT → CONFIGURE(c); ACK(c) → RESULT;
   IMPORT(2) → RESULT; ATTACH(1,2) → RESULT; COMMIT(1,c,callback=1) → RESULT;
   composition tick → FRAME_DONE. Buffer 2 ещё может быть BUSY.
2. IMPORT(3), ATTACH(1,3), COMMIT; после завершения reader buffer 2 получает
   RELEASE с serial его COMMIT. Double/triple buffering без premature write.
3. Новый CONFIGURE между ACK и COMMIT → BAD_SERIAL без потери pending;
   ACK latest, повтор COMMIT с новым serial успешен, старый frame виден до него.
4. DESTROY surface с pending/callback/committed → LEAVE, releases, DISCARDED,
   затем RESULT; DESTROY свободных buffers возвращает counters к baseline.
5. short header, oversized packet, length/count mismatch, trailing bytes,
   лишняя capability и invalid reserved → disconnect только виновного peer.
6. Stale/reused/cross-connection IDs, неверный тип объекта, повтор request serial,
   чужой configure token, повтор ACK и COMMIT без pending проверяются отдельно.
7. Проверить end == memory.size, end на байт больше, offset/u64 overflow,
   width*4/stride/scale overflow, нулевой размер, damage на границе, 9 Rect.
   Валидно оформленные 9 Rect дают LIMIT_EXCEEDED; count, не совпадающий с
   длиной массива, даёт MALFORMED ещё до проверки quota.
8. LIMIT_EXCEEDED/NO_MEMORY/IMPORT error/WOULD_BLOCK не оставляют partial objects,
   mappings или capabilities; retry после ERROR использует новый serial.
9. Frame callback не разрешает reuse BUSY buffer; DESTROY_BUFFER BUSY отклонён;
   отмена pending даёт RELEASE(commit=0), disconnect освобождает без delivery.
10. Focus A→B, grab за пределами, destroy focused surface, modifiers/repeat,
    UTF-8 и подавление release удержанных keys не дают input чужому client.
11. Queue full/slow consumer/crash app/crash DWM/crash server: память bounded,
    peers продолжают работать; restart требует новых объектов и negotiation.
12. Обойти каждый hard limit на min/max/max+1; подтвердить fixed endianness,
    отсутствие native padding, causal ordering и destroy barrier.

Все будущие автоматические suites подключаются к единственной точке запуска
`tools/qemu_test.py`. До появления decoder/model обычный QEMU smoke доказывает
только сохранение текущей загрузки/runtime, а не conformance нового протокола.
