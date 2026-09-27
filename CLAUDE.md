# Dunit OS — правила разработки для Claude

Ты работаешь над Dunit OS.

Главный принцип:
**человек определяет архитектуру и направление, ты реализуешь, проверяешь и исправляешь результат.**

Не жди от пользователя ручных действий, промежуточных подтверждений или запуска команд.

---

# 1. ЕДИНСТВЕННАЯ ТОЧКА ЗАПУСКА И ТЕСТИРОВАНИЯ

Использовать только:

    tools/qemu_test.py

как единственную точку запуска и тестирования системы.

Не запускай QEMU вручную.

Не создавай альтернативные скрипты запуска QEMU для обхода `tools/qemu_test.py`.

Не проси пользователя запускать команды вручную.

---

# 2. ПОЛНОСТЬЮ АВТОМАТИЧЕСКОЕ ТЕСТИРОВАНИЕ

Тестирование должно выполняться полностью автоматически без участия пользователя.

Предполагай, что пользователь находится не за компьютером и не может:

- запускать команды вручную;
- закрывать QEMU;
- предоставлять логи;
- нажимать клавиши вручную;
- подтверждать промежуточные шаги;
- выполнять дополнительные команды по твоей просьбе.

Если для проверки необходимо взаимодействие с системой, используй возможности `tools/qemu_test.py` и существующего автоматического тестового окружения.

Не останавливайся на сообщении:

    "Теперь пользователь должен запустить..."

Ты должен выполнить проверку самостоятельно.

---

# 3. ОБЯЗАТЕЛЬНЫЙ ЦИКЛ РАЗРАБОТКИ

После изменения кода:

    изменить код
        ↓
    tools/qemu_test.py
        ↓
    QEMU
        ↓
    serial log
        ↓
    анализ результата
        ↓
    если ошибка → исправить
        ↓
    tools/qemu_test.py
        ↓
    повторить

Не считай задачу выполненной только потому, что код компилируется.

Успешная сборка ≠ успешная работа системы.

---

# 4. ПОСЛЕ КАЖДОЙ ПРОВЕРКИ

После завершения проверки необходимо:

1. Проанализировать serial log.
2. Убедиться, что система действительно загрузилась.
3. Проверить ожидаемые признаки успешной работы.
4. Показать релевантный tail лога.
5. Кратко описать результат тестирования.
6. Если обнаружена проблема — исправить её и повторить тестирование.

Не утверждай, что система работает, если это не подтверждено тестом.

---

# 5. ЕСЛИ ОБНАРУЖЕНА ПРОБЛЕМА

Если тест обнаружил проблему:

1. Проанализируй serial log.
2. Найди вероятную первопричину.
3. Исправь код.
4. Снова используй только:

       tools/qemu_test.py

5. Повторяй цикл до тех пор, пока:
   - проверка не станет успешной;
   - либо не возникнет явная блокирующая ошибка, которую невозможно устранить в рамках текущей задачи.

Не останавливайся после первого найденного сбоя.

Не передавай пользователю очевидную ремонтную работу, которую можешь выполнить самостоятельно.

---

# 6. НИКАКОГО НОВОГО HARDCODE DESKTOP

Dunit Desktop должен быть configuration-driven.

Не добавляй desktop policy непосредственно в Rust-код, если она концептуально является пользовательской настройкой или desktop configuration.

Не хардкодь в коде:

- список приложений;
- application names;
- application IDs;
- executable paths;
- application icon paths;
- launcher entries;
- dock entries;
- wallpaper;
- theme;
- цвета desktop;
- цвета окон;
- panel configuration;
- dock configuration;
- workspace configuration;
- пользовательские размеры;
- window decoration configuration;
- border radius;
- panel/dock dimensions;
- animation parameters;
- desktop layout;
- пользовательские shortcuts;
- application metadata;
- desktop assets;
- другие значения, которые должны изменяться через конфигурацию.

Плохой пример:

    match app {
        "gui_files" => ...,
        "gui_terminal" => ...,
        "gui_calc" => ...,
    }

Не заменяй такой hardcode на другой hardcode в виде:

    HashMap::from([...])

Если данные являются configuration data, они должны приходить из configuration/application metadata.

---

# 7. ЕДИНСТВЕННЫЙ SOURCE OF TRUTH

Не создавай ситуацию, когда значение одновременно существует:

    в TOML
    и в Rust
    и в runtime fallback
    и в отдельной таблице

Конфигурация должна иметь один авторитетный источник.

Плохо:

    config.theme.accent

но renderer всё ещё использует:

    const DEFAULT_ACCENT: ...

как фактический источник значения.

Плохо:

    config.dock.entries

но launcher продолжает использовать отдельный hardcoded список.

Плохо:

    config.application.icon

но gui_server продолжает использовать `match app`.

Если конфигурация существует, runtime должен действительно использовать её.

---

# 8. ЧТО МОЖЕТ ОСТАВАТЬСЯ HARDCODED

Не нужно механически выносить абсолютно каждую константу в конфигурацию.

В коде могут оставаться настоящие технические инварианты:

- ABI;
- структура системных вызовов;
- protocol layout;
- memory safety limits;
- внутренние алгоритмические константы;
- аппаратные ограничения;
- значения, необходимые исключительно для безопасности;
- compile-time ограничения;
- внутренние implementation details.

Правило:

> Если пользовательская/desktop policy может логично измениться без изменения архитектуры системы, она должна быть configuration-driven.

Не превращай configuration system в бессмысленный набор из сотен технических констант.

---

# 9. НИКАКОГО LEGACY KERNEL GUI

Dunit использует userspace GUI architecture.

Целевая архитектура:

    kernel
        ↓
    display/input/memory/IPC mechanisms
        ↓
    gui_server.elf
        ↓
    DWM / compositor
        ↓
    libdunit
        ↓
    independent GUI ELF applications

Kernel предоставляет механизмы.

Userspace предоставляет desktop policy.

Не возвращай desktop/window-management policy обратно в kernel.

Не создавай новый kernel GUI вместо удалённого legacy GUI.

Не восстанавливай старую архитектуру под другими именами.

---

# 10. GUI SERVER — USERSPACE

`gui_server` является userspace ELF-программой.

Desktop functionality должна находиться в userspace.

В kernel не должны возвращаться:

- window manager;
- compositor;
- desktop;
- panel;
- dock;
- launcher;
- wallpaper;
- workspace UI;
- window decorations;
- application-specific GUI logic;
- theme logic;
- desktop configuration;
- application launch policy.

Kernel может предоставлять необходимые механизмы:

- display access;
- input;
- memory;
- shared buffers;
- IPC;
- synchronization;
- handles/capabilities;
- процессы/потоки;
- соответствующие syscalls;
- hardware drivers.

Не путай mechanism с policy.

---

# 11. НЕ ВОЗВРАЩАТЬ APPLICATION-SPECIFIC KNOWLEDGE В KERNEL

Kernel не должен знать о существовании конкретных desktop applications.

Не добавляй в kernel знания о:

- `gui_terminal`;
- `gui_files`;
- `gui_calc`;
- `gui_stat`;
- `gui_client`;
- конкретных application paths;
- application icons;
- application names.

Application-specific metadata относится к userspace/configuration layer.

---

# 12. GUI APPLICATIONS — НАСТОЯЩИЕ ELF ПРОЦЕССЫ

GUI applications должны оставаться независимыми userspace ELF binaries.

Не объединяй приложения обратно в `gui_server`.

Каждое приложение должно по возможности оставаться:

    application ELF
        ↓
    libdunit
        ↓
    GUI protocol / IPC / shared buffers
        ↓
    gui_server

Не превращай desktop в один огромный процесс только ради удобства.

Сохраняй:

- отдельные address spaces;
- отдельные ELF binaries;
- crash isolation;
- независимость GUI clients;
- userspace application boundary.

---

# 13. НЕ ВСТРАИВАЙ APPLICATIONS В KERNEL БЕЗ НЕОБХОДИМОСТИ

Не добавляй новые `include_bytes!` для userspace applications или desktop assets в kernel.

Не возвращай архитектуру:

    kernel image
        ├── application
        ├── application
        ├── icon
        └── wallpaper

если текущая архитектура и roadmap предусматривают загрузку userspace компонентов с filesystem.

Embedded assets могут существовать только там, где это действительно необходимо текущему этапу архитектуры.

Не используй embedding как удобный способ обойти нормальную userspace/filesystem архитектуру.

---

# 14. НЕ СОЗДАВАЙ ВРЕМЕННЫЕ КОСТЫЛИ БЕЗ НЕОБХОДИМОСТИ

Перед добавлением workaround спроси себя:

    Можно ли решить это архитектурно?

Если можно — предпочти архитектурное решение.

Особенно избегай:

- специальных `if` для конкретного приложения;
- специальных `match` для конкретного приложения;
- дублирования configuration data;
- hardcoded fallback, который фактически является главным поведением;
- временных compatibility layers без причины;
- копирования одной и той же логики в несколько подсистем;
- TODO вместо необходимой реализации;
- отключения тестов ради прохождения тестов;
- скрытия ошибок.

Если временный workaround действительно необходим:

1. Сделай его минимальным.
2. Документируй причину.
3. Убедись, что он не становится новым source of truth.
4. Если возможно — добавь TODO с конкретным условием удаления.

Не маскируй технический долг под архитектуру.

---

# 15. НЕ ЛОМАЙ АРХИТЕКТУРУ РАДИ БЫСТРОГО ФИКСА

Если текущая архитектура требует изменений, сначала пойми существующие границы.

Не делай:

    "быстрее всего просто добавить это в kernel"

если это относится к userspace policy.

Не делай:

    "быстрее всего захардкодить приложение"

если это configuration/application metadata.

Не делай:

    "быстрее всего продублировать состояние"

если уже существует источник истины.

Сначала ищи правильную точку интеграции.

---

# 16. ПЕРЕД БОЛЬШИМИ ИЗМЕНЕНИЯМИ ИССЛЕДУЙ КОДОВУЮ БАЗУ

Перед крупной задачей:

- прочитай соответствующий roadmap;
- изучи существующую архитектуру;
- найди связанные модули;
- найди всех callers;
- найди зависимости;
- проверь существующие tests;
- проверь userspace/kernel boundary;
- проверь configuration system.

Не предполагай, что один очевидный файл содержит всю реализацию.

Например:

    удаление ui_loop.rs

не означает:

    удалить только ui_loop.rs.

Сначала нужно найти:

- imports;
- initialization;
- callers;
- syscalls;
- state;
- assets;
- build references;
- tests;
- documentation;
- userspace dependencies.

---

# 17. ПРИ УДАЛЕНИИ LEGACY КОДА — УДАЛЯЙ ЕГО ПОЛНОСТЬЮ

Если задача заключается в удалении legacy subsystem:

1. Найди все зависимости.
2. Перенеси необходимую функциональность в правильный новый слой.
3. Проверь новую реализацию.
4. Удали старую реализацию.
5. Удали dead code.
6. Удали старые imports.
7. Удали старые module declarations.
8. Удали obsolete tests.
9. Проверь repository-wide references.
10. Запусти `tools/qemu_test.py`.

Не оставляй старую реализацию "на всякий случай".

Если legacy code больше не является частью архитектуры, он должен быть удалён.

---

# 18. НЕ ДЕЛАЙ FAKE REFACTOR

Не считай задачу выполненной, если:

- старый код просто переименован;
- hardcode просто перемещён в другой Rust-файл;
- конфиг существует, но не используется;
- старый GUI не используется, но продолжает компилироваться;
- новый abstraction layer просто скрывает старый hardcode;
- application-specific logic осталась внутри gui_server;
- kernel всё ещё фактически управляет desktop policy.

Проверяй фактическую архитектуру, а не названия файлов.

---

# 19. CONFIGURATION ДОЛЖНА РАБОТАТЬ В РЕАЛЬНОСТИ

Если задача касается configuration-driven поведения, обязательно проверяй:

    изменить config
        ↓
    reload/apply
        ↓
    реальное изменение поведения

Не достаточно проверить, что TOML успешно парсится.

Например, если меняется:

- wallpaper;
- theme;
- accent;
- window radius;
- dock;
- launcher;
- application icon;
- application metadata;

изменение должно реально отражаться в системе.

---

# 20. CONFIGURATION ERRORS

Configuration должна обрабатываться безопасно.

При наличии configuration reload:

    read
      ↓
    parse
      ↓
    validate
      ↓
    apply

Если configuration malformed:

- не падай;
- не применяй частично повреждённую configuration;
- сохрани рабочее состояние;
- используй last-known-good configuration, если это предусмотрено архитектурой;
- выдай понятную диагностическую информацию.

Не превращай повреждённый пользовательский config в crash desktop.

---

# 21. СОХРАНЯЙ СУЩЕСТВУЮЩУЮ ФУНКЦИОНАЛЬНОСТЬ

Архитектурный рефакторинг не означает право случайно удалить рабочую функциональность.

После изменений проверяй существующее поведение.

Особенно для GUI:

- boot;
- gui_server;
- input;
- display;
- multiple GUI clients;
- focus;
- raise;
- drag;
- close;
- terminal;
- file manager;
- configuration;
- wallpaper;
- theme;
- dock;
- panel;
- workspaces.

Если функциональность должна быть удалена по архитектурным причинам — это должно быть осознанным результатом задачи, а не случайной регрессией.

---

# 22. НЕ ОСТАНАВЛИВАЙСЯ НА COMPILE SUCCESS

Следующие утверждения НЕ являются доказательством успеха:

    "cargo build прошёл"

    "код компилируется"

    "ошибок Rust нет"

    "файл успешно удалён"

Успех определяется поведением работающей системы.

Всегда используй:

    tools/qemu_test.py

и анализируй serial log.

---

# 23. ПЕРЕД ЗАВЕРШЕНИЕМ БОЛЬШОЙ ЗАДАЧИ — FINAL AUDIT

После выполнения большой архитектурной задачи сделай дополнительный audit.

Проверь:

- не осталось ли старого hardcode;
- не осталось ли dead code;
- не осталось ли старых references;
- не появился ли новый workaround;
- не существует ли два source of truth;
- не вернулась ли desktop policy в kernel;
- не нарушена ли userspace/kernel boundary;
- не осталось ли старых application mappings;
- не осталось ли embedded desktop state;
- не сломалась ли существующая функциональность.

Для задач по удалению legacy subsystem сделай repository-wide поиск старых symbols/modules.

Для задач по устранению hardcode сделай repository-wide поиск hardcoded values и application-specific mappings.

---

# 24. ROADMAP — ИСТОЧНИК АРХИТЕКТУРНОГО НАПРАВЛЕНИЯ

DUNIT_OS_TECHNICAL_ROADMAP.md является архитектурным ориентиром проекта.

Не рассматривай roadmap только как список фич.

Учитывай:

- архитектурные зависимости;
- acceptance criteria;
- ограничения;
- intended boundaries;
- planned migrations;
- технический долг;
- причины отложенных решений.

Если текущая реализация противоречит roadmap, не маскируй противоречие новым workaround.

Определи правильную архитектурную точку и исправь её.

---

# 25. ПРИОРИТЕТЫ ПРИ ПРИНЯТИИ РЕШЕНИЙ

Если есть несколько вариантов реализации, предпочитай вариант в следующем порядке:

1. Архитектурно корректный.
2. Соответствующий roadmap.
3. Сохраняющий разделение kernel/userspace.
4. Configuration-driven, если это policy.
5. Тестируемый через `tools/qemu_test.py`.
6. Минимальный по техническому долгу.
7. Простой для дальнейшего развития.

Не выбирай решение только потому, что оно требует меньше строк кода прямо сейчас.

---

# 26. НЕ ПЕРЕУСЛОЖНЯЙ

Архитектурная чистота не означает необходимость создавать абстракции ради абстракций.

Не создавай:

- giant generic frameworks;
- ненужные layers;
- configuration fields для технических implementation constants;
- новые subsystem'ы без необходимости;
- сложные dependency chains ради простой задачи.

Используй существующие механизмы проекта, если они подходят.

Цель:

    simple architecture
    + clear boundaries
    + explicit data flow
    + testable behavior

---

# 27. РЕАЛЬНЫЙ РЕЗУЛЬТАТ ВАЖНЕЕ ВИДИМОСТИ РАБОТЫ

Не пытайся просто создать впечатление прогресса.

Например:

    "configuration file added"

не означает:

    "desktop is configuration-driven".

    "legacy file deleted"

не означает:

    "legacy architecture is removed".

    "test passed once"

не означает:

    "system is verified".

Всегда проверяй реальный end-to-end результат.

---

# 28. ФИНАЛЬНЫЙ ОТЧЁТ

После завершения задачи кратко сообщи:

1. Что было изменено.
2. Какие архитектурные изменения сделаны.
3. Какой hardcode был удалён, если задача касалась hardcode.
4. Какой legacy code был удалён, если задача касалась legacy.
5. Что намеренно осталось и почему.
6. Какие тесты были выполнены.
7. Результат `tools/qemu_test.py`.
8. Релевантный tail serial log.
9. Остался ли технический долг, относящийся к задаче.

Не скрывай оставшиеся проблемы.

Не говори "всё готово", если есть непроверенные части.

---

# 29. ГЛАВНЫЙ ПРИНЦИП DUNIT

Dunit строится как система, которую можно понимать, изменять и проверять.

Поэтому:

    kernel → mechanisms
    userspace → policy
    config → user-facing state
    applications → independent ELF processes
    libdunit → native application interface
    gui_server → desktop/compositor
    tests → objective verification

И:

    НЕ хардкодь policy,
    НЕ возвращай legacy architecture,
    НЕ создавай второй source of truth,
    НЕ оставляй технический долг без причины,
    НЕ считай compile success доказательством работы.

Если ты можешь решить проблему архитектурно — решай её архитектурно.

Если существующий код противоречит целевой архитектуре — исправляй код, а не подгоняй архитектуру под старый код.

---

# 30. ОСНОВНОЙ LOOP

Всегда работай по циклу:

    UNDERSTAND
        ↓
    PLAN
        ↓
    IMPLEMENT
        ↓
    TEST WITH tools/qemu_test.py
        ↓
    ANALYZE SERIAL LOG
        ↓
    FIX
        ↓
    TEST AGAIN
        ↓
    AUDIT
        ↓
    REPORT

Не проси пользователя вмешиваться в этот цикл.

Пользователь должен определять направление и принимать архитектурные решения.

Твоя задача — самостоятельно довести реализацию до проверенного состояния.