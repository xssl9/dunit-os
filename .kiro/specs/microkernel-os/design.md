# Design Document: Microkernel OS

## Overview

Микроядерная операционная система для x86_64 архитектуры с многоязычной реализацией:
- **HAL (Hardware Abstraction Layer)**: Assembly + C - низкоуровневая инициализация CPU, GDT, IDT
- **Kernel**: Rust - управление памятью, планировщик, IPC, системные вызовы
- **Userspace**: Rust + C - драйверы, системные сервисы, приложения

Система следует принципам микроядерной архитектуры, где драйверы и системные сервисы работают как отдельные процессы в userspace, взаимодействуя через IPC.

Ключевые особенности:
- Трехслойная архитектура: HAL (C/Asm) → Kernel (Rust) → Userspace (Rust/C)
- Минимальное ядро с базовыми функциями (память, планировщик, IPC)
- Изоляция компонентов через Ring 3 userspace
- Display Server и Window Manager как отдельные процессы
- Поддержка GUI через egui
- Возможность запуска C-приложений через relibc
- FFI граница между C/Assembly HAL и Rust Kernel

## Architecture

### System Layers

```
┌─────────────────────────────────────────────────┐
│         Applications (NetSurf, Terminal)        │
│         Rust/C (Ring 3 Userspace)               │
├─────────────────────────────────────────────────┤
│    Display Server + WM (egui)    │   relibc    │
│         Rust (Ring 3)            │   C lib     │
├──────────────────────┬───────────┴─────────────┤
│  Video Driver        │  Network Driver         │
│  Rust (Ring 3)       │  Rust (Ring 3)          │
├──────────────────────┴─────────────────────────┤
│              Kernel (Ring 0) - RUST             │
│  - Scheduler (Rust)                             │
│  - IPC (Rust)                                   │
│  - Memory Manager (Rust: PMM + VMM)             │
│  - Syscall Handler (Rust)                       │
│  - Global Allocator (Rust)                      │
├─────────────────────────────────────────────────┤
│         HAL (C + Assembly) - Ring 0             │
│  - boot.asm: Entry point, Long Mode setup       │
│  - gdt.c/asm: GDT initialization                │
│  - idt.c/asm: IDT setup, interrupt stubs        │
│  - ports.c: I/O port operations                 │
│  - cpu.c: CPU feature detection                 │
└─────────────────────────────────────────────────┘
         ↑
    [Limine Bootloader]
```

**Языковые границы:**
- **Assembly → C**: Вызовы функций через C calling convention
- **C → Rust**: FFI через `extern "C"` блоки
- **Rust → C**: Экспорт функций через `#[no_mangle] extern "C"`

### Boot Sequence

1. **Limine Bootloader** загружает ядро в память, передает управление на `_start`
2. **boot.asm** (Assembly):
   - Проверяет, что CPU в Long Mode (64-bit)
   - Настраивает начальный стек
   - Вызывает `hal_init()` из C
3. **HAL (C)** инициализирует:
   - GDT (Global Descriptor Table) для сегментации
   - IDT (Interrupt Descriptor Table) с заглушками обработчиков
   - Базовые прерывания (таймер, клавиатура)
   - Вызывает `kernel_main()` из Rust
4. **Kernel (Rust)** настраивает:
   - Global Allocator для heap
   - PMM (Physical Memory Manager) на основе memory map от Limine
   - VMM (Virtual Memory Manager) с page tables
   - Регистрирует Rust-обработчики прерываний через HAL
   - Запускает планировщик
5. **Kernel** создает первый userspace процесс (init)
6. **Init** запускает Video Driver, Network Driver
7. **Init** запускает Display Server + WM
8. **Display Server** готов принимать приложения

## Components and Interfaces

### 1. Hardware Abstraction Layer (HAL)

**Язык:** Assembly + C

**Файловая структура:**
```
hal/
├── boot.asm          # Entry point, stack setup
├── gdt.c / gdt.asm   # GDT initialization
├── idt.c / idt.asm   # IDT setup, interrupt stubs
├── interrupts.asm    # Low-level interrupt handlers
├── ports.c           # I/O port operations (inb/outb)
├── cpu.c             # CPU feature detection
└── hal.h             # Header for Rust FFI
```

**Ответственность:**
- Инициализация CPU в Long Mode (если еще не сделано bootloader'ом)
- Настройка GDT (Global Descriptor Table) с сегментами для Ring 0 и Ring 3
- Настройка IDT (Interrupt Descriptor Table) с обработчиками прерываний
- Низкоуровневые операции с портами ввода-вывода
- Переключение контекста на уровне Assembly
- Вход/выход из прерываний

**Assembly компоненты (boot.asm):**
```nasm
section .text
bits 64

global _start
extern hal_init
extern kernel_main

_start:
    ; Проверка Long Mode
    mov rax, cr0
    test rax, 0x80000000
    jz .no_long_mode
    
    ; Настройка стека
    mov rsp, stack_top
    
    ; Очистка регистров
    xor rbp, rbp
    xor rax, rax
    xor rbx, rbx
    xor rcx, rcx
    xor rdx, rdx
    
    ; Вызов HAL init (C)
    call hal_init
    
    ; Вызов Rust kernel
    call kernel_main
    
    ; Если kernel_main вернулся - halt
    cli
    hlt
    jmp $

.no_long_mode:
    ; Ошибка: не в Long Mode
    hlt
    jmp $

section .bss
align 16
stack_bottom:
    resb 16384  ; 16KB stack
stack_top:
```

**Assembly компоненты (interrupts.asm):**
```nasm
; Макрос для создания interrupt stub без error code
%macro ISR_NOERRCODE 1
global isr%1
isr%1:
    push 0              ; Dummy error code
    push %1             ; Interrupt number
    jmp isr_common_stub
%endmacro

; Макрос для interrupt stub с error code
%macro ISR_ERRCODE 1
global isr%1
isr%1:
    push %1             ; Interrupt number
    jmp isr_common_stub
%endmacro

; Создаем stubs для всех 256 прерываний
ISR_NOERRCODE 0   ; Divide by zero
ISR_NOERRCODE 1   ; Debug
; ... (остальные)
ISR_ERRCODE 8     ; Double fault (has error code)
; ... и т.д.

extern interrupt_handler  ; Rust function

isr_common_stub:
    ; Сохранить все регистры
    push rax
    push rbx
    push rcx
    push rdx
    push rsi
    push rdi
    push rbp
    push r8
    push r9
    push r10
    push r11
    push r12
    push r13
    push r14
    push r15
    
    ; Вызвать Rust обработчик
    mov rdi, rsp        ; Передать указатель на сохраненные регистры
    call interrupt_handler
    
    ; Восстановить регистры
    pop r15
    pop r14
    pop r13
    pop r12
    pop r11
    pop r10
    pop r9
    pop r8
    pop rbp
    pop rdi
    pop rsi
    pop rdx
    pop rcx
    pop rbx
    pop rax
    
    ; Убрать error code и interrupt number
    add rsp, 16
    
    ; Вернуться из прерывания
    iretq
```

**C компоненты (gdt.c):**
```c
#include <stdint.h>

typedef struct {
    uint16_t limit_low;
    uint16_t base_low;
    uint8_t base_middle;
    uint8_t access;
    uint8_t granularity;
    uint8_t base_high;
} __attribute__((packed)) gdt_entry_t;

typedef struct {
    uint16_t limit;
    uint64_t base;
} __attribute__((packed)) gdt_ptr_t;

gdt_entry_t gdt[5];
gdt_ptr_t gdt_ptr;

void gdt_set_gate(int num, uint64_t base, uint64_t limit, uint8_t access, uint8_t gran) {
    gdt[num].base_low = (base & 0xFFFF);
    gdt[num].base_middle = (base >> 16) & 0xFF;
    gdt[num].base_high = (base >> 24) & 0xFF;
    gdt[num].limit_low = (limit & 0xFFFF);
    gdt[num].granularity = (limit >> 16) & 0x0F;
    gdt[num].granularity |= gran & 0xF0;
    gdt[num].access = access;
}

void hal_init_gdt(void) {
    gdt_ptr.limit = (sizeof(gdt_entry_t) * 5) - 1;
    gdt_ptr.base = (uint64_t)&gdt;
    
    gdt_set_gate(0, 0, 0, 0, 0);                // Null segment
    gdt_set_gate(1, 0, 0xFFFFFFFF, 0x9A, 0xA0); // Kernel code (64-bit)
    gdt_set_gate(2, 0, 0xFFFFFFFF, 0x92, 0xA0); // Kernel data
    gdt_set_gate(3, 0, 0xFFFFFFFF, 0xFA, 0xA0); // User code (64-bit)
    gdt_set_gate(4, 0, 0xFFFFFFFF, 0xF2, 0xA0); // User data
    
    gdt_flush((uint64_t)&gdt_ptr);  // Assembly function
}
```

**C компоненты (idt.c):**
```c
#include <stdint.h>

typedef struct {
    uint16_t offset_low;
    uint16_t selector;
    uint8_t ist;
    uint8_t type_attr;
    uint16_t offset_middle;
    uint32_t offset_high;
    uint32_t zero;
} __attribute__((packed)) idt_entry_t;

typedef struct {
    uint16_t limit;
    uint64_t base;
} __attribute__((packed)) idt_ptr_t;

idt_entry_t idt[256];
idt_ptr_t idt_ptr;

void idt_set_gate(int num, uint64_t handler, uint16_t selector, uint8_t flags) {
    idt[num].offset_low = handler & 0xFFFF;
    idt[num].offset_middle = (handler >> 16) & 0xFFFF;
    idt[num].offset_high = (handler >> 32) & 0xFFFFFFFF;
    idt[num].selector = selector;
    idt[num].ist = 0;
    idt[num].type_attr = flags;
    idt[num].zero = 0;
}

extern void isr0(void);
extern void isr1(void);
// ... declarations for all ISRs

void hal_init_idt(void) {
    idt_ptr.limit = (sizeof(idt_entry_t) * 256) - 1;
    idt_ptr.base = (uint64_t)&idt;
    
    // Установить все ISR handlers
    idt_set_gate(0, (uint64_t)isr0, 0x08, 0x8E);
    idt_set_gate(1, (uint64_t)isr1, 0x08, 0x8E);
    // ... для всех 256 прерываний
    
    idt_flush((uint64_t)&idt_ptr);  // Assembly function
}
```

**C компоненты (ports.c):**
```c
#include <stdint.h>

uint8_t hal_inb(uint16_t port) {
    uint8_t ret;
    asm volatile("inb %1, %0" : "=a"(ret) : "Nd"(port));
    return ret;
}

void hal_outb(uint16_t port, uint8_t value) {
    asm volatile("outb %0, %1" : : "a"(value), "Nd"(port));
}

uint16_t hal_inw(uint16_t port) {
    uint16_t ret;
    asm volatile("inw %1, %0" : "=a"(ret) : "Nd"(port));
    return ret;
}

void hal_outw(uint16_t port, uint16_t value) {
    asm volatile("outw %0, %1" : : "a"(value), "Nd"(port));
}

uint32_t hal_inl(uint16_t port) {
    uint32_t ret;
    asm volatile("inl %1, %0" : "=a"(ret) : "Nd"(port));
    return ret;
}

void hal_outl(uint16_t port, uint32_t value) {
    asm volatile("outl %0, %1" : : "a"(value), "Nd"(port));
}
```

**C компоненты (hal.c - main init):**
```c
#include <stdint.h>

extern void hal_init_gdt(void);
extern void hal_init_idt(void);

void hal_enable_interrupts(void) {
    asm volatile("sti");
}

void hal_disable_interrupts(void) {
    asm volatile("cli");
}

void hal_init(void) {
    // Инициализация GDT
    hal_init_gdt();
    
    // Инициализация IDT
    hal_init_idt();
    
    // Настройка PIC (Programmable Interrupt Controller)
    // Remap IRQs 0-15 to interrupts 32-47
    hal_outb(0x20, 0x11);
    hal_outb(0xA0, 0x11);
    hal_outb(0x21, 0x20);
    hal_outb(0xA1, 0x28);
    hal_outb(0x21, 0x04);
    hal_outb(0xA1, 0x02);
    hal_outb(0x21, 0x01);
    hal_outb(0xA1, 0x01);
    hal_outb(0x21, 0x0);
    hal_outb(0xA1, 0x0);
    
    // Включить прерывания
    hal_enable_interrupts();
}
```

**Интерфейс для Rust (hal.h):**
```c
#ifndef HAL_H
#define HAL_H

#include <stdint.h>

// Initialization
void hal_init(void);
void hal_init_gdt(void);
void hal_init_idt(void);

// Interrupts
void hal_enable_interrupts(void);
void hal_disable_interrupts(void);

// I/O Ports
uint8_t hal_inb(uint16_t port);
void hal_outb(uint16_t port, uint8_t value);
uint16_t hal_inw(uint16_t port);
void hal_outw(uint16_t port, uint16_t value);
uint32_t hal_inl(uint16_t port);
void hal_outl(uint16_t port, uint32_t value);

#endif
```

**Rust FFI интерфейс:**
```rust
// В kernel/src/hal.rs
#[repr(C)]
pub struct InterruptFrame {
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub r11: u64,
    pub r10: u64,
    pub r9: u64,
    pub r8: u64,
    pub rbp: u64,
    pub rdi: u64,
    pub rsi: u64,
    pub rdx: u64,
    pub rcx: u64,
    pub rbx: u64,
    pub rax: u64,
    pub int_no: u64,
    pub err_code: u64,
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}

extern "C" {
    pub fn hal_init();
    pub fn hal_init_gdt();
    pub fn hal_init_idt();
    pub fn hal_enable_interrupts();
    pub fn hal_disable_interrupts();
    pub fn hal_outb(port: u16, value: u8);
    pub fn hal_inb(port: u16) -> u8;
    pub fn hal_outw(port: u16, value: u16);
    pub fn hal_inw(port: u16) -> u16;
    pub fn hal_outl(port: u16, value: u32);
    pub fn hal_inl(port: u16) -> u32;
}

// Rust обработчик прерываний, вызываемый из Assembly
#[no_mangle]
pub extern "C" fn interrupt_handler(frame: *const InterruptFrame) {
    let frame = unsafe { &*frame };
    
    match frame.int_no {
        0 => handle_divide_by_zero(frame),
        1 => handle_debug(frame),
        32 => handle_timer(frame),
        33 => handle_keyboard(frame),
        _ => handle_unknown_interrupt(frame),
    }
}
```

### 2. Physical Memory Manager (PMM)

**Язык:** Rust

**Ответственность:**
- Управление физическими страницами памяти (4KB)
- Аллокация и деаллокация физических фреймов
- Отслеживание свободной памяти

**Структура данных:**
```rust
pub struct PhysicalMemoryManager {
    bitmap: &'static mut [u8],
    total_frames: usize,
    free_frames: usize,
}

impl PhysicalMemoryManager {
    pub fn alloc_frame() -> Option<PhysicalAddress>;
    pub fn free_frame(addr: PhysicalAddress);
    pub fn available_memory() -> usize;
}
```

**Алгоритм:** Bitmap-based allocation
- Каждый бит представляет один фрейм (4KB)
- 0 = свободен, 1 = занят
- Поиск первого свободного бита для аллокации

### 3. Virtual Memory Manager (VMM)

**Язык:** Rust

**Ответственность:**
- Управление виртуальной памятью процессов
- Маппинг виртуальных адресов на физические
- Управление таблицами страниц (Page Tables)

**Структура данных:**
```rust
pub struct VirtualMemoryManager {
    page_table: &'static mut PageTable,
}

impl VirtualMemoryManager {
    pub fn map_page(virt: VirtualAddress, phys: PhysicalAddress, flags: PageFlags);
    pub fn unmap_page(virt: VirtualAddress);
    pub fn translate(virt: VirtualAddress) -> Option<PhysicalAddress>;
}

pub struct PageTable {
    entries: [PageTableEntry; 512],
}
```

**Флаги страниц:**
- Present: страница присутствует в памяти
- Writable: разрешена запись
- User: доступна из Ring 3
- WriteThrough: сквозная запись в кэш
- NoExecute: запрет выполнения кода

### 4. Scheduler

**Язык:** Rust

**Ответственность:**
- Планирование выполнения процессов
- Переключение контекста
- Управление очередью готовых процессов

**Структура данных:**
```rust
pub struct Scheduler {
    processes: Vec<Process>,
    current_pid: ProcessId,
}

pub struct Process {
    pid: ProcessId,
    state: ProcessState,
    context: CpuContext,
    page_table: PageTable,
}

pub struct CpuContext {
    rax, rbx, rcx, rdx: u64,
    rsi, rdi, rbp, rsp: u64,
    r8, r9, r10, r11: u64,
    r12, r13, r14, r15: u64,
    rip: u64,
    rflags: u64,
}

impl Scheduler {
    pub fn schedule() -> &'static Process;
    pub fn switch_context(from: &mut Process, to: &Process);
    pub fn add_process(process: Process);
    pub fn remove_process(pid: ProcessId);
}
```

**Алгоритм:** Round-robin
- Каждый процесс получает фиксированный квант времени (10ms)
- При истечении кванта происходит переключение на следующий процесс
- Процессы в состоянии Blocked пропускаются

### 5. System Call Handler

**Язык:** Rust + Assembly

**Ответственность:**
- Обработка системных вызовов из userspace
- Валидация параметров
- Диспетчеризация вызовов к соответствующим обработчикам

**Интерфейс:**
```rust
pub enum Syscall {
    Exit(i32),
    Fork,
    Exec(path: *const u8),
    Read(fd: u32, buf: *mut u8, count: usize),
    Write(fd: u32, buf: *const u8, count: usize),
    Open(path: *const u8, flags: u32),
    Close(fd: u32),
    Mmap(addr: usize, length: usize, prot: u32, flags: u32),
    SendMessage(target_pid: ProcessId, msg: *const Message),
    ReceiveMessage(msg: *mut Message),
}

pub fn syscall_handler(syscall_num: u64, arg1: u64, arg2: u64, arg3: u64) -> i64;
```

**Механизм:**
- Userspace помещает номер syscall в RAX, аргументы в RDI, RSI, RDX
- Выполняется инструкция `syscall`
- CPU переключается в Ring 0 и передает управление обработчику
- Обработчик валидирует параметры и выполняет запрошенную операцию
- Результат возвращается в RAX

### 6. IPC Subsystem

**Язык:** Rust

**Ответственность:**
- Межпроцессное взаимодействие
- Shared memory для больших данных
- Message passing для сигналов и команд

**Структура данных:**
```rust
pub struct IpcManager {
    shared_regions: HashMap<SharedMemoryId, SharedMemoryRegion>,
    message_queues: HashMap<ProcessId, MessageQueue>,
}

pub struct SharedMemoryRegion {
    id: SharedMemoryId,
    physical_addr: PhysicalAddress,
    size: usize,
    owners: Vec<ProcessId>,
}

pub struct Message {
    sender: ProcessId,
    msg_type: MessageType,
    data: [u8; 256],
}

pub enum MessageType {
    MouseEvent { x: i32, y: i32, buttons: u8 },
    KeyboardEvent { scancode: u8, pressed: bool },
    RenderFrame { buffer_id: SharedMemoryId },
    WindowCreate { width: u32, height: u32 },
    WindowClose { window_id: u32 },
}

impl IpcManager {
    pub fn create_shared_memory(size: usize) -> SharedMemoryId;
    pub fn attach_shared_memory(id: SharedMemoryId, pid: ProcessId);
    pub fn send_message(target: ProcessId, msg: Message);
    pub fn receive_message(pid: ProcessId) -> Option<Message>;
}
```

### 7. Video Driver

**Язык:** Rust

**Ответственность:**
- Управление framebuffer
- Double buffering для устранения мерцания
- Копирование данных из shared memory в framebuffer

**Структура данных:**
```rust
pub struct VideoDriver {
    framebuffer: &'static mut [u8],
    back_buffer: Vec<u8>,
    width: u32,
    height: u32,
    pitch: u32,
    bpp: u8,
}

impl VideoDriver {
    pub fn init(info: FramebufferInfo) -> Self;
    pub fn swap_buffers();
    pub fn blit(x: u32, y: u32, width: u32, height: u32, data: &[u8]);
    pub fn clear(color: u32);
}
```

**Протокол взаимодействия с Display Server:**
1. Display Server создает shared memory для framebuffer
2. Display Server отрисовывает в shared memory
3. Display Server отправляет сообщение RenderFrame с buffer_id
4. Video Driver копирует данные из shared memory в back_buffer
5. Video Driver выполняет swap_buffers

### 8. Display Server + Window Manager

**Язык:** Rust + egui

**Ответственность:**
- Композитинг окон приложений
- Обработка событий ввода
- Управление фокусом окон
- Отрисовка через egui

**Структура данных:**
```rust
pub struct DisplayServer {
    windows: HashMap<WindowId, Window>,
    focused_window: Option<WindowId>,
    egui_ctx: egui::Context,
    input_state: InputState,
}

pub struct Window {
    id: WindowId,
    owner_pid: ProcessId,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    title: String,
    buffer: SharedMemoryId,
    visible: bool,
    focused: bool,
}

pub struct InputState {
    mouse_x: i32,
    mouse_y: i32,
    mouse_buttons: u8,
    keyboard_modifiers: u8,
}

impl DisplayServer {
    pub fn create_window(pid: ProcessId, width: u32, height: u32) -> WindowId;
    pub fn destroy_window(id: WindowId);
    pub fn handle_mouse_event(x: i32, y: i32, buttons: u8);
    pub fn handle_keyboard_event(scancode: u8, pressed: bool);
    pub fn render_frame();
}
```

**Алгоритм композитинга:**
1. Очистить back buffer
2. Для каждого видимого окна (от заднего к переднему):
   - Прочитать содержимое из shared memory окна
   - Отрисовать рамку окна через egui
   - Скопировать содержимое окна в позицию (x, y)
3. Отрисовать курсор мыши
4. Отправить RenderFrame в Video Driver

### 9. Virtual File System

**Язык:** Rust

**Ответственность:**
- Абстракция файловой системы
- Управление файловыми дескрипторами
- Монтирование различных FS

**Структура данных:**
```rust
pub struct VirtualFileSystem {
    mount_points: HashMap<String, Box<dyn FileSystem>>,
    open_files: HashMap<FileDescriptor, OpenFile>,
}

pub trait FileSystem {
    fn open(&self, path: &str, flags: OpenFlags) -> Result<FileHandle>;
    fn read(&self, handle: FileHandle, buf: &mut [u8]) -> Result<usize>;
    fn write(&self, handle: FileHandle, buf: &[u8]) -> Result<usize>;
    fn close(&self, handle: FileHandle);
}

pub struct OpenFile {
    fs: &'static dyn FileSystem,
    handle: FileHandle,
    position: usize,
}

impl VirtualFileSystem {
    pub fn mount(path: &str, fs: Box<dyn FileSystem>);
    pub fn open(path: &str, flags: OpenFlags) -> Result<FileDescriptor>;
    pub fn read(fd: FileDescriptor, buf: &mut [u8]) -> Result<usize>;
    pub fn write(fd: FileDescriptor, buf: &[u8]) -> Result<usize>;
    pub fn close(fd: FileDescriptor);
}
```

**Структура директорий:**
```
/
├── dev/          # Device files
│   ├── fb0       # Framebuffer
│   ├── kbd       # Keyboard
│   └── mouse     # Mouse
├── bin/          # Executables
│   ├── init
│   ├── netsurf
│   └── terminal
└── proc/         # Process information
    ├── 1/
    ├── 2/
    └── ...
```

### 10. Global Allocator

**Язык:** Rust

**Ответственность:**
- Управление heap памятью ядра
- Реализация GlobalAlloc trait
- Поддержка Box, Vec, String в no_std

**Структура данных:**
```rust
pub struct KernelAllocator {
    heap_start: usize,
    heap_size: usize,
    free_list: *mut FreeBlock,
}

struct FreeBlock {
    size: usize,
    next: *mut FreeBlock,
}

unsafe impl GlobalAlloc for KernelAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8;
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout);
}

#[global_allocator]
static ALLOCATOR: KernelAllocator = KernelAllocator::new();
```

**Алгоритм:** First-fit with free list
- Поддерживается связный список свободных блоков
- При аллокации ищется первый блок достаточного размера
- При деаллокации блок возвращается в список и объединяется с соседними

## Data Models

### Process State Machine

```
    ┌─────────┐
    │  Ready  │◄──────────┐
    └────┬────┘           │
         │                │
         │ schedule()     │ unblock()
         │                │
         ▼                │
    ┌─────────┐           │
    │ Running │           │
    └────┬────┘           │
         │                │
         │ block()        │
         │                │
         ▼                │
    ┌─────────┐           │
    │ Blocked │───────────┘
    └─────────┘
         │
         │ exit()
         │
         ▼
    ┌─────────┐
    │  Dead   │
    └─────────┘
```

### Memory Layout

**Kernel Space (Higher Half):**
```
0xFFFFFFFF_FFFFFFFF  ┌──────────────┐
                     │   Reserved   │
0xFFFFFFFF_80000000  ├──────────────┤
                     │  Kernel Code │
                     │  Kernel Data │
                     │  Kernel Heap │
0xFFFF8000_00000000  ├──────────────┤
                     │   Page Tables│
0xFFFF0000_00000000  └──────────────┘
```

**User Space (Lower Half):**
```
0x00007FFF_FFFFFFFF  ┌──────────────┐
                     │    Stack     │
                     │      ↓       │
                     ├──────────────┤
                     │              │
                     │     Free     │
                     │              │
                     ├──────────────┤
                     │      ↑       │
                     │    Heap      │
0x0000000000400000   ├──────────────┤
                     │  Code + Data │
0x0000000000000000   └──────────────┘
```

### ELF64 Loading

**Процесс загрузки исполняемого файла:**
1. Прочитать ELF header, проверить magic number (0x7F 'E' 'L' 'F')
2. Проверить архитектуру (x86_64) и тип (executable)
3. Прочитать program headers
4. Для каждого LOAD сегмента:
   - Выделить физические страницы
   - Создать маппинг в page table процесса
   - Скопировать данные из файла
   - Установить права доступа (R/W/X)
5. Установить entry point из ELF header
6. Создать стек процесса
7. Переключиться в Ring 3 и передать управление на entry point

## Correctness Properties

*Свойство корректности (correctness property) - это характеристика или поведение, которое должно выполняться во всех допустимых состояниях системы. По сути, это формальное утверждение о том, что система должна делать. Свойства служат мостом между человекочитаемыми спецификациями и машинно-проверяемыми гарантиями корректности.*

### Property 1: Virtual Memory Mapping Consistency

*For any* memory allocation request from a process, when the VMM maps a virtual page to a physical frame, translating that virtual address should return the same physical address that was mapped.

**Validates: Requirements 2.3**

### Property 2: Timer Interrupt Triggers Scheduling

*For any* timer interrupt occurrence, the scheduler should be invoked to perform context switching.

**Validates: Requirements 3.2**

### Property 3: Keyboard Interrupt Processing

*For any* keyboard interrupt, the kernel should process the keyboard input and make it available to userspace.

**Validates: Requirements 3.3**

### Property 4: Process Isolation on Crash

*For any* process that crashes or triggers a fault, the kernel should remain stable, terminate only the faulty process, and allow other processes to continue execution.

**Validates: Requirements 4.4**

### Property 5: Context Switch Preservation

*For any* process, saving its CPU context then restoring it should preserve all register values (rax, rbx, rcx, rdx, rsi, rdi, rbp, rsp, r8-r15, rip, rflags).

**Validates: Requirements 4.5**

### Property 6: Syscall Parameter Validation

*For any* syscall invoked from userspace with invalid parameters (null pointers, out-of-bounds values, invalid file descriptors), the kernel should detect the invalid parameters and return an error code without crashing.

**Validates: Requirements 5.2, 5.4**

### Property 7: Shared Memory Visibility

*For any* two processes sharing a memory region, when one process writes data to the shared memory, the other process should be able to read the same data from the same offset.

**Validates: Requirements 6.2**

### Property 8: IPC Message Delivery

*For any* message sent via IPC to a target process, the target process should receive the message with the same content and sender information.

**Validates: Requirements 6.5**

### Property 9: Framebuffer Render Completion

*For any* render request from Display Server to Video Driver, the Video Driver should copy the frame data to the framebuffer.

**Validates: Requirements 7.3**

### Property 10: Window Position Update on Drag

*For any* window being dragged, the window's position coordinates should be updated to reflect the new position after the drag operation.

**Validates: Requirements 8.2**

### Property 11: Window Focus Management

*For any* window that receives a focus event, that window should become the focused window and other windows should lose focus.

**Validates: Requirements 8.3**

### Property 12: Window Resource Allocation

*For any* window creation request from an application, the WM should allocate window resources (window ID, buffer, metadata) and return a valid window handle.

**Validates: Requirements 8.4**

### Property 13: Window Compositing Completeness

*For any* set of visible windows, the Display Server's composited output should contain pixels from all visible windows in the correct z-order.

**Validates: Requirements 8.5**

### Property 14: Input Event Forwarding to egui

*For any* mouse or keyboard event received by Display Server, the event should be forwarded to egui's input system.

**Validates: Requirements 9.3, 9.4**

### Property 15: File Open Returns Descriptor

*For any* valid file path that exists in the VFS, opening the file should return a valid file descriptor that can be used for subsequent read/write operations.

**Validates: Requirements 10.2**

### Property 16: File Not Found Error

*For any* file path that does not exist in the VFS, attempting to open it should return an error code indicating the file was not found.

**Validates: Requirements 10.5**

### Property 17: Malloc/Free Round Trip

*For any* memory allocation via relibc's malloc, the allocated memory should be usable, and calling free on that pointer should not cause corruption or crashes.

**Validates: Requirements 11.2**

### Property 18: Network Packet Transmission

*For any* network packet sent by an application through the Network Driver, the packet should be transmitted through the hardware interface.

**Validates: Requirements 12.3**

### Property 19: Render Surface Provision

*For any* rendering request from NetSurf, the Display Server should provide a valid drawing surface (window buffer).

**Validates: Requirements 13.3**

### Property 20: ELF Header Parsing

*For any* valid ELF64 file, the kernel should successfully parse the ELF headers and extract program segments, entry point, and load addresses.

**Validates: Requirements 14.2, 14.3, 14.4**

### Property 21: Corrupted ELF Rejection

*For any* ELF file with corrupted headers (invalid magic number, unsupported architecture, malformed segments), the kernel should refuse to load it and return an error.

**Validates: Requirements 14.5**

### Property 22: Allocator Provides Memory

*For any* allocation request via Box::new or Vec::push in kernel code, the global allocator should provide valid memory that can be written to and read from.

**Validates: Requirements 15.3, 15.4**

## Error Handling

### HAL Error Handling

**C/Assembly Layer:**
- **CPU Exceptions**: Triple fault → System reset (unavoidable)
- **Invalid GDT/IDT**: Halt system with error code in specific register
- **Port I/O failures**: Return error codes to Rust layer

**Error Propagation to Rust:**
```c
// C function returns error codes
int hal_init_gdt(void) {
    // ... setup GDT
    if (error) return -1;
    return 0;
}
```

```rust
// Rust checks return values
unsafe {
    if hal_init_gdt() != 0 {
        panic!("Failed to initialize GDT");
    }
}
```

### Kernel Error Handling

**Memory Allocation Failures:**
- PMM out of memory → Return None, caller handles gracefully
- VMM mapping failure → Return error, process receives ENOMEM

**Process Errors:**
- Invalid syscall → Return error code to userspace
- Page fault in userspace → Send SIGSEGV to process, terminate if unhandled
- Page fault in kernel → Panic with diagnostic information

**IPC Errors:**
- Invalid target PID → Return ESRCH (No such process)
- Message queue full → Return EAGAIN (Try again)
- Shared memory attach failure → Return ENOMEM

**File System Errors:**
- File not found → Return ENOENT
- Permission denied → Return EACCES
- Invalid file descriptor → Return EBADF

### Driver Error Handling

**Video Driver:**
- Invalid framebuffer address → Log error, use fallback text mode
- Render request with invalid buffer → Ignore request, log warning

**Network Driver:**
- Hardware not responding → Retry with exponential backoff
- Packet transmission failure → Return error to application
- Invalid packet format → Drop packet, log warning

### Error Recovery Strategies

1. **Graceful Degradation**: If GUI fails, fall back to text mode
2. **Process Isolation**: Process crashes don't affect kernel or other processes
3. **Resource Cleanup**: On error, free allocated resources before returning
4. **Logging**: All errors logged to kernel buffer for debugging
5. **Panic Handling**: Kernel panics display diagnostic info and halt

## Testing Strategy

### Dual Testing Approach

Система будет тестироваться двумя комплементарными подходами:

**Unit Tests:**
- Тестирование конкретных примеров и граничных случаев
- Проверка интеграционных точек между компонентами
- Тестирование обработки ошибок
- Примеры: тест на деление на ноль, тест на null pointer, тест на переполнение буфера

**Property-Based Tests:**
- Проверка универсальных свойств на множестве входных данных
- Генерация случайных тестовых данных для широкого покрытия
- Каждый тест выполняется минимум 100 итераций
- Примеры: любая аллокация памяти должна быть освобождаема, любое сообщение IPC должно доставляться

### Testing Framework

**Для Rust компонентов:**
- Фреймворк: `proptest` для property-based testing
- Конфигурация: минимум 100 итераций на тест
- Теги: каждый property test помечается комментарием:
  ```rust
  // Feature: microkernel-os, Property 5: Context Switch Preservation
  #[test]
  fn prop_context_switch_preserves_registers() { ... }
  ```

**Для C/Assembly компонентов:**
- Unit tests на C с использованием простого test harness
- Тестирование через вызовы из Rust тестов
- Проверка корректности GDT/IDT через чтение дескрипторов

### Test Categories

**1. HAL Tests (C/Assembly):**
- Unit: Проверка корректности GDT entries
- Unit: Проверка корректности IDT entries
- Unit: Тест port I/O операций (loopback)
- Integration: Вызов HAL функций из Rust

**2. Memory Management Tests:**
- Property: Аллокация и деаллокация не вызывают утечек
- Property: Маппинг виртуальной памяти консистентен
- Unit: Аллокация при нехватке памяти возвращает ошибку
- Unit: Двойная деаллокация обнаруживается

**3. Scheduler Tests:**
- Property: Все процессы получают CPU время
- Property: Переключение контекста сохраняет регистры
- Unit: Планировщик корректно обрабатывает один процесс
- Unit: Блокированные процессы не получают CPU

**4. IPC Tests:**
- Property: Сообщения доставляются без потерь
- Property: Shared memory видна обоим процессам
- Unit: Отправка сообщения несуществующему процессу возвращает ошибку
- Unit: Переполнение очереди сообщений обрабатывается корректно

**5. Syscall Tests:**
- Property: Невалидные параметры всегда возвращают ошибку
- Property: Валидные syscalls выполняются успешно
- Unit: Syscall с null pointer возвращает EFAULT
- Unit: Syscall с невалидным fd возвращает EBADF

**6. VFS Tests:**
- Property: Открытие существующего файла возвращает дескриптор
- Property: Открытие несуществующего файла возвращает ошибку
- Unit: Чтение из закрытого дескриптора возвращает ошибку
- Unit: Запись в read-only файл возвращает ошибку

**7. Display Server Tests:**
- Property: Все видимые окна присутствуют в композите
- Property: События ввода форвардятся в egui
- Unit: Создание окна возвращает валидный ID
- Unit: Закрытие окна освобождает ресурсы

**8. ELF Loader Tests:**
- Property: Валидные ELF файлы загружаются корректно
- Property: Невалидные ELF файлы отклоняются
- Unit: ELF с неправильным magic number отклоняется
- Unit: ELF для другой архитектуры отклоняется

### Testing Environment

**QEMU для интеграционных тестов:**
- Запуск ОС в QEMU с автоматическими тестами
- Проверка вывода через serial port
- Автоматическое определение успеха/провала тестов

**Unit тесты:**
- Запуск на host системе где возможно
- Мокирование HAL для тестирования Rust компонентов
- Изолированное тестирование каждого модуля

### Test Execution

```bash
# Запуск всех тестов
make test

# Запуск только unit тестов
cargo test --lib

# Запуск property-based тестов
cargo test --test properties

# Запуск интеграционных тестов в QEMU
make test-integration

# Запуск с coverage
cargo tarpaulin --out Html
```

### Coverage Goals

- **Kernel code**: минимум 80% покрытие
- **HAL code**: минимум 70% покрытие (сложно тестировать Assembly)
- **Drivers**: минимум 75% покрытие
- **Critical paths** (syscalls, memory management): 90%+ покрытие

