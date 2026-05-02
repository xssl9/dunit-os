2# Implementation Plan: Microkernel OS

## Overview

Поэтапная реализация микроядерной операционной системы, начиная с базовой загрузки и HAL, затем ядро на Rust, и заканчивая userspace компонентами. Каждый этап строится на предыдущем и включает тестирование для валидации корректности.

## Tasks

- [x] 1. Project Setup and Toolchain
  - Настроить Rust toolchain с rust-src и llvm-tools
  - Настроить кросс-компилятор для C (x86_64-elf-gcc)
  - Настроить NASM для Assembly
  - Создать структуру проекта (hal/, kernel/, userspace/)
  - Настроить Limine bootloader
  - Создать Makefile для сборки всех компонентов
  - _Requirements: 1.1, 1.2, 1.3_

- [x] 2. HAL Implementation (C + Assembly)
  - [x] 2.1 Implement boot.asm entry point
    - Написать _start в Assembly
    - Проверить Long Mode
    - Настроить начальный стек
    - Вызвать hal_init из C
    - _Requirements: 1.1, 1.4, 1.5_

  - [x] 2.2 Implement GDT initialization (gdt.c/asm)
    - Создать структуры GDT entries
    - Реализовать gdt_set_gate
    - Настроить сегменты для Ring 0 и Ring 3
    - Написать gdt_flush в Assembly
    - _Requirements: 2.4_

  - [x] 2.3 Implement IDT initialization (idt.c/asm)
    - Создать структуры IDT entries
    - Реализовать idt_set_gate
    - Написать макросы ISR_NOERRCODE и ISR_ERRCODE
    - Создать isr_common_stub в Assembly
    - Настроить все 256 interrupt handlers
    - _Requirements: 3.1_

  - [x] 2.4 Implement I/O port operations (ports.c)
    - Реализовать hal_inb/outb
    - Реализовать hal_inw/outw
    - Реализовать hal_inl/outl
    - _Requirements: 3.2, 3.3_

  - [x] 2.5 Implement HAL main initialization (hal.c)
    - Реализовать hal_init
    - Настроить PIC (Programmable Interrupt Controller)
    - Remap IRQs 0-15 to interrupts 32-47
    - Реализовать hal_enable_interrupts/disable_interrupts
    - _Requirements: 3.1, 3.2_

  - [x] 2.6 Write unit tests for HAL components
    - Тест корректности GDT entries
    - Тест корректности IDT entries
    - Тест port I/O операций
    - _Requirements: 2.4, 3.1_

- [x] 3. Checkpoint - HAL Complete
  - Ensure HAL compiles and links correctly, ask the user if questions arise.

- [x] 4. Kernel Core (Rust) - Memory Management
  - [x] 4.1 Create Rust FFI bindings for HAL (kernel/src/hal.rs)
    - Определить extern "C" функции для HAL
    - Создать InterruptFrame структуру
    - Реализовать interrupt_handler для Rust
    - _Requirements: 3.2, 3.3, 3.4_

  - [x] 4.2 Implement Physical Memory Manager (kernel/src/memory/pmm.rs)
    - Создать PhysicalMemoryManager структуру
    - Реализовать bitmap-based allocation
    - Реализовать alloc_frame и free_frame
    - Парсить memory map от Limine
    - _Requirements: 2.1, 2.3_

  - [x] 4.3 Write property test for PMM
    - **Property 22: Allocator Provides Memory**
    - **Validates: Requirements 15.3**
    - _Requirements: 15.3_

  - [x] 4.4 Implement Virtual Memory Manager (kernel/src/memory/vmm.rs)
    - Создать VirtualMemoryManager и PageTable структуры
    - Реализовать map_page и unmap_page
    - Реализовать translate для проверки маппинга
    - Настроить page table flags (Present, Writable, User, NoExecute)
    - _Requirements: 2.2, 2.3, 2.5_

  - [x] 4.5 Write property test for VMM
    - **Property 1: Virtual Memory Mapping Consistency**
    - **Validates: Requirements 2.3**
    - _Requirements: 2.3_

  - [x] 4.6 Implement Global Allocator (kernel/src/allocator.rs)
    - Создать KernelAllocator структуру
    - Реализовать GlobalAlloc trait
    - Реализовать first-fit allocation с free list
    - Установить #[global_allocator]
    - _Requirements: 15.1, 15.2, 15.5_

  - [x] 4.7 Write property test for Global Allocator
    - **Property 22: Allocator Provides Memory**
    - **Validates: Requirements 15.3, 15.4**
    - _Requirements: 15.3, 15.4_

- [x] 5. Checkpoint - Memory Management Complete
  - Ensure all memory tests pass, ask the user if questions arise.

- [x] 6. Kernel Core (Rust) - Process Management
  - [x] 6.1 Implement Process structures (kernel/src/process/mod.rs)
    - Создать Process структуру с PID, state, context
    - Создать CpuContext для сохранения регистров
    - Создать ProcessState enum (Ready, Running, Blocked, Dead)
    - _Requirements: 4.1, 4.2, 4.5_

  - [x] 6.2 Implement Scheduler (kernel/src/process/scheduler.rs)
    - Создать Scheduler структуру с очередью процессов
    - Реализовать Round-robin алгоритм
    - Реализовать schedule для выбора следующего процесса
    - Реализовать switch_context в Assembly
    - Реализовать add_process и remove_process
    - _Requirements: 4.1, 4.2_

  - [x] 6.3 Write property test for Scheduler
    - **Property 2: Timer Interrupt Triggers Scheduling**
    - **Property 5: Context Switch Preservation**
    - **Validates: Requirements 3.2, 4.5**
    - _Requirements: 3.2, 4.5_

  - [x] 6.4 Implement interrupt handlers (kernel/src/interrupts.rs)
    - Реализовать handle_timer для планировщика
    - Реализовать handle_keyboard для ввода
    - Реализовать handle_divide_by_zero и другие exceptions
    - Реализовать handle_unknown_interrupt
    - _Requirements: 3.2, 3.3, 3.4, 3.5_

  - [x] 6.5 Write property tests for interrupt handling
    - **Property 3: Keyboard Interrupt Processing**
    - **Validates: Requirements 3.3**
    - _Requirements: 3.3_

  - [x] 6.6 Implement process isolation and fault handling
    - Обработка page faults в userspace
    - Терминация процесса при критических ошибках
    - Сохранение стабильности ядра при крашах процессов
    - _Requirements: 4.3, 4.4_

  - [x] 6.7 Write property test for process isolation
    - **Property 4: Process Isolation on Crash**
    - **Validates: Requirements 4.4**
    - _Requirements: 4.4_

- [x] 7. Checkpoint - Process Management Complete
  - Ensure all process tests pass, ask the user if questions arise.

- [x] 8. System Calls
  - [x] 8.1 Implement syscall handler (kernel/src/syscall/mod.rs)
    - Создать Syscall enum с вариантами
    - Реализовать syscall_handler функцию
    - Настроить syscall instruction в Assembly
    - Реализовать переключение Ring 3 → Ring 0
    - _Requirements: 5.1, 5.5_

  - [x] 8.2 Implement syscall parameter validation
    - Проверка null pointers
    - Проверка out-of-bounds значений
    - Проверка валидности file descriptors
    - Возврат error codes при невалидных параметрах
    - _Requirements: 5.2, 5.4_

  - [x] 8.3 Write property test for syscall validation
    - **Property 6: Syscall Parameter Validation**
    - **Validates: Requirements 5.2, 5.4**
    - _Requirements: 5.2, 5.4_

  - [x] 8.3 Implement core syscalls
    - Exit, Fork, Exec
    - Read, Write, Open, Close
    - Mmap для управления памятью
    - _Requirements: 5.3_

- [x] 9. IPC Subsystem
  - [x] 9.1 Implement IPC Manager (kernel/src/ipc/mod.rs)
    - Создать IpcManager структуру
    - Создать SharedMemoryRegion структуру
    - Создать Message структуру и MessageType enum
    - _Requirements: 6.1, 6.3_

  - [x] 9.2 Implement shared memory
    - Реализовать create_shared_memory
    - Реализовать attach_shared_memory
    - Маппинг одних физических страниц в разные процессы
    - _Requirements: 6.2_

  - [x] 9.3 Write property test for shared memory
    - **Property 7: Shared Memory Visibility**
    - **Validates: Requirements 6.2**
    - _Requirements: 6.2_

  - [x] 9.4 Implement message passing
    - Реализовать send_message
    - Реализовать receive_message
    - Создать очереди сообщений для процессов
    - Реализовать MessageType варианты (MouseEvent, KeyboardEvent, RenderFrame, etc.)
    - _Requirements: 6.4, 6.5_

  - [x] 9.5 Write property test for message delivery
    - **Property 8: IPC Message Delivery**
    - **Validates: Requirements 6.5**
    - _Requirements: 6.5_

- [x] 10. Checkpoint - IPC Complete
  - Ensure all IPC tests pass, ask the user if questions arise.

- [x] 11. ELF Loader
  - [x] 11.1 Implement ELF parser (kernel/src/elf/mod.rs)
    - Парсинг ELF header
    - Проверка magic number (0x7F 'E' 'L' 'F')
    - Проверка архитектуры (x86_64)
    - Парсинг program headers
    - _Requirements: 14.1, 14.2_

  - [x] 11.2 Implement ELF loader
    - Загрузка LOAD сегментов в память
    - Создание page table mappings для процесса
    - Установка прав доступа (R/W/X)
    - Извлечение entry point
    - Создание стека процесса
    - _Requirements: 14.3, 14.4_

  - [x] 11.3 Write property tests for ELF loading
    - **Property 20: ELF Header Parsing**
    - **Property 21: Corrupted ELF Rejection**
    - **Validates: Requirements 14.2, 14.3, 14.4, 14.5**
    - _Requirements: 14.2, 14.3, 14.4, 14.5_

- [x] 12. Virtual File System
  - [x] 12.1 Implement VFS core (kernel/src/fs/vfs.rs)
    - Создать VirtualFileSystem структуру
    - Создать FileSystem trait
    - Реализовать mount, open, read, write, close
    - Создать OpenFile структуру для file descriptors
    - _Requirements: 10.1, 10.3_

  - [x] 12.2 Implement basic filesystems
    - DevFS для /dev (framebuffer, keyboard, mouse)
    - ProcFS для /proc (process information)
    - Простая in-memory FS для /bin
    - _Requirements: 10.1, 10.4_

  - [x] 12.3 Write property tests for VFS
    - **Property 15: File Open Returns Descriptor**
    - **Property 16: File Not Found Error**
    - **Validates: Requirements 10.2, 10.5**
    - _Requirements: 10.2, 10.5_

- [x] 13. Checkpoint - Kernel Core Complete
  - Ensure all kernel tests pass, ask the user if questions arise.

- [x] 14. Video Driver (Userspace)
  - [x] 14.1 Implement Video Driver (userspace/video_driver/src/main.rs)
    - Создать VideoDriver структуру
    - Получить framebuffer info от ядра
    - Реализовать double buffering
    - Реализовать swap_buffers
    - Реализовать blit и clear
    - _Requirements: 7.1, 7.2, 7.4, 7.5_

  - [x] 14.2 Implement IPC protocol with Display Server
    - Обработка RenderFrame сообщений
    - Копирование из shared memory в framebuffer
    - _Requirements: 7.3_

  - [x] 14.3 Write property test for render completion
    - **Property 9: Framebuffer Render Completion**
    - **Validates: Requirements 7.3**
    - _Requirements: 7.3_

- [x] 15. Display Server and Window Manager (Userspace)
  - [x] 15.1 Implement Display Server core (userspace/display_server/src/main.rs)
    - Создать DisplayServer структуру
    - Создать Window структуру
    - Реализовать create_window и destroy_window
    - Управление HashMap окон
    - _Requirements: 8.1_

  - [x] 15.2 Write property test for window creation
    - **Property 12: Window Resource Allocation**
    - **Validates: Requirements 8.4**
    - _Requirements: 8.4_

  - [x] 15.3 Implement window management
    - Реализовать handle_mouse_event для драга окон
    - Реализовать window focus management
    - Обновление позиций окон при драге
    - _Requirements: 8.2, 8.3_

  - [x] 15.4 Write property tests for window management
    - **Property 10: Window Position Update on Drag**
    - **Property 11: Window Focus Management**
    - **Validates: Requirements 8.2, 8.3**
    - _Requirements: 8.2, 8.3_

  - [x] 15.5 Integrate egui
    - Добавить egui dependency
    - Реализовать egui::RawInput trait
    - Реализовать input event forwarding
    - Настроить egui context
    - _Requirements: 9.1, 9.2_

  - [x] 15.6 Write property test for input forwarding
    - **Property 14: Input Event Forwarding to egui**
    - **Validates: Requirements 9.3, 9.4**
    - _Requirements: 9.3, 9.4_

  - [x] 15.7 Implement compositing
    - Реализовать render_frame
    - Композитинг всех видимых окон
    - Отрисовка рамок окон через egui
    - Отрисовка курсора мыши
    - Отправка RenderFrame в Video Driver
    - _Requirements: 8.5, 9.5_

  - [x] 15.8 Write property test for compositing
    - **Property 13: Window Compositing Completeness**
    - **Validates: Requirements 8.5**
    - _Requirements: 8.5_

- [x] 16. Checkpoint - Display System Complete
  - [x] 16.1 Configure bootloader (Limine)
    - Проверить limine.conf конфигурацию
    - Настроить kernel entry point
    - Настроить framebuffer mode
    - Проверить memory map передачу
    - _Requirements: 1.1, 1.2_

  - [x] 16.2 Build complete system image
    - Собрать HAL (C + Assembly)
    - Собрать Kernel (Rust)
    - Собрать Video Driver
    - Собрать Display Server
    - Создать ISO образ с Limine
    - _Requirements: 1.1, 1.2, 1.3_

  - [x] 16.3 Test boot sequence in QEMU
    - Запустить QEMU с созданным ISO
    - Проверить успешную загрузку Limine
    - Проверить переход в Long Mode
    - Проверить инициализацию HAL
    - Проверить запуск Rust kernel
    - _Requirements: 1.4, 1.5, 2.4_

  - [x] 16.4 Test memory management in QEMU
    - Проверить работу PMM
    - Проверить работу VMM
    - Проверить работу Global Allocator
    - Мониторинг memory leaks
    - _Requirements: 2.1, 2.2, 2.3, 15.1, 15.2_

  - [x] 16.5 Test interrupt handling in QEMU
    - Проверить timer interrupts
    - Проверить keyboard interrupts
    - Проверить exception handling
    - _Requirements: 3.1, 3.2, 3.3, 3.4_

  - [x] 16.6 Test process management in QEMU
    - Проверить создание процессов
    - Проверить context switching
    - Проверить scheduler работу
    - Проверить process isolation
    - _Requirements: 4.1, 4.2, 4.3, 4.4, 4.5_

  - [x] 16.7 Test IPC in QEMU
    - Проверить shared memory между процессами
    - Проверить message passing
    - Проверить IPC между Video Driver и Display Server
    - _Requirements: 6.1, 6.2, 6.3, 6.4, 6.5_

  - [x] 16.8 Test display system in QEMU
    - Проверить framebuffer инициализацию
    - Проверить Video Driver rendering
    - Проверить Display Server compositing
    - Проверить window management
    - Проверить mouse/keyboard input
    - _Requirements: 7.1, 7.2, 7.3, 8.1, 8.2, 8.3, 9.1, 9.2, 9.3_

  - [x] 16.9 Debug and fix bootloader issues
    - Исправить проблемы с загрузкой если есть
    - Исправить проблемы с memory map
    - Исправить проблемы с framebuffer setup
    - Настроить QEMU параметры для оптимальной работы
    - _Requirements: 1.1, 1.2, 1.4_

  - [x] 16.10 Verify system stability
    - Длительный тест работы системы
    - Проверка отсутствия kernel panics
    - Проверка отсутствия memory corruption
    - Проверка корректной работы всех компонентов
    - _Requirements: 4.4, 15.5_

- [ ] 17. Network Driver (Userspace)
  - [ ] 17.1 Integrate smoltcp
    - Добавить smoltcp dependency
    - Настроить network interface
    - Реализовать packet transmission
    - _Requirements: 12.1, 12.5_

  - [ ] 17.2 Implement e1000 driver
    - Реализовать e1000 network card driver для QEMU
    - Обработка packet transmission через hardware
    - _Requirements: 12.2, 12.4_

  - [ ] 17.3 Write property test for packet transmission
    - **Property 18: Network Packet Transmission**
    - **Validates: Requirements 12.3**
    - _Requirements: 12.3_

- [ ] 18. C Library Support (relibc)
  - [ ] 18.1 Integrate relibc from Redox
    - Клонировать relibc repository
    - Настроить сборку для нашей ОС
    - Компилировать как shared library
    - _Requirements: 11.1, 11.5_

  - [ ] 18.2 Implement syscall bridge
    - Реализовать malloc через kernel syscalls
    - Реализовать free
    - Реализовать file operations (open, read, write, close)
    - _Requirements: 11.2, 11.3, 11.4_

  - [ ] 18.3 Write property test for malloc/free
    - **Property 17: Malloc/Free Round Trip**
    - **Validates: Requirements 11.2**
    - _Requirements: 11.2_

- [ ] 19. Browser Support (NetSurf)
  - [ ] 19.1 Port NetSurf
    - Клонировать NetSurf repository
    - Настроить сборку с relibc
    - Создать backend для egui вместо X11
    - _Requirements: 13.1, 13.2, 13.4_

  - [ ] 19.2 Implement rendering integration
    - Реализовать предоставление drawing surface от Display Server
    - Интеграция NetSurf rendering с egui
    - _Requirements: 13.3_

  - [ ] 19.3 Write property test for render surface provision
    - **Property 19: Render Surface Provision**
    - **Validates: Requirements 13.3**
    - _Requirements: 13.3_

  - [ ] 19.4 Implement network integration
    - Подключение NetSurf к Network Driver
    - Обработка HTTP requests
    - _Requirements: 13.5_

- [ ] 20. Final Integration and Testing
  - [ ] 20.1 Integration testing
    - Запуск полной системы в QEMU
    - Тестирование всех компонентов вместе
    - Проверка стабильности при длительной работе

  - [ ] 20.2 Performance optimization
    - Профилирование критических путей
    - Оптимизация IPC latency
    - Оптимизация композитинга

  - [ ] 20.3 Documentation
    - Документация API ядра
    - Документация IPC протоколов
    - Руководство по сборке и запуску

- [ ] 21. Final Checkpoint
  - Ensure all tests pass, system boots and runs stably, ask the user if questions arise.

## Notes

- Все задачи являются обязательными для полного покрытия тестами
- Каждая задача ссылается на конкретные требования для трассируемости
- Checkpoints обеспечивают инкрементальную валидацию
- Property tests валидируют универсальные свойства корректности
- Unit tests валидируют конкретные примеры и граничные случаи
- Реализация идет снизу вверх: HAL → Kernel → Drivers → Applications
