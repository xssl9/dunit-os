# Requirements Document

## Introduction

Микроядерная операционная система на базе Rust с раздельными процессами для Display Environment и Window Manager. Система предназначена для x86_64 архитектуры и использует современный подход к изоляции компонентов через IPC.

## Glossary

- **Kernel**: Микроядро системы, управляющее потоками, планировщиком и IPC
- **HAL**: Hardware Abstraction Layer - слой абстракции оборудования
- **IPC**: Inter-Process Communication - механизм межпроцессного взаимодействия
- **PMM**: Physical Memory Manager - менеджер физической памяти
- **VMM**: Virtual Memory Manager - менеджер виртуальной памяти
- **Scheduler**: Планировщик задач
- **Display_Server**: Сервер отображения, собирающий картинку из окон приложений
- **WM**: Window Manager - менеджер окон
- **VFS**: Virtual File System - виртуальная файловая система
- **Syscall**: Системный вызов для взаимодействия userspace с ядром
- **Framebuffer**: Область памяти для хранения изображения экрана

## Requirements

### Requirement 1: Bootloader Integration

**User Story:** As a system developer, I want to initialize the system through Limine bootloader, so that I can receive framebuffer pointer and memory map without writing 16-bit assembly code.

#### Acceptance Criteria

1. WHEN the system boots THEN THE Kernel SHALL receive control from Limine bootloader
2. WHEN Limine transfers control THEN THE Kernel SHALL receive valid framebuffer pointer
3. WHEN Limine transfers control THEN THE Kernel SHALL receive complete memory map
4. THE Kernel SHALL implement _start entry point with #[no_mangle] attribute
5. THE Kernel SHALL operate in x86_64 Long Mode

### Requirement 2: Memory Management

**User Story:** As a kernel developer, I want to manage physical and virtual memory, so that processes can safely allocate and use memory without conflicts.

#### Acceptance Criteria

1. THE PMM SHALL implement physical memory allocation using bitmap or stack-based approach
2. THE VMM SHALL implement page mapping for virtual memory
3. WHEN a process requests memory THEN THE VMM SHALL map virtual pages to physical frames
4. THE Kernel SHALL configure GDT for memory protection
5. THE Kernel SHALL configure paging tables for virtual memory translation

### Requirement 3: Interrupt Handling

**User Story:** As a kernel developer, I want to handle hardware interrupts, so that the system can respond to timer, keyboard, and other hardware events.

#### Acceptance Criteria

1. THE Kernel SHALL configure IDT with interrupt handlers
2. WHEN a timer interrupt occurs THEN THE Kernel SHALL invoke scheduler
3. WHEN a keyboard interrupt occurs THEN THE Kernel SHALL process keyboard input
4. THE Kernel SHALL implement exception handlers for CPU faults
5. IF an unhandled interrupt occurs THEN THE Kernel SHALL log error and halt safely

### Requirement 4: Process Management and Scheduling

**User Story:** As a kernel developer, I want to implement multitasking with process isolation, so that multiple applications can run concurrently without interfering with each other.

#### Acceptance Criteria

1. THE Scheduler SHALL implement Round-robin scheduling algorithm
2. WHEN timer interrupt occurs THEN THE Scheduler SHALL switch process contexts
3. THE Kernel SHALL support Ring 3 userspace execution
4. WHEN a process crashes THEN THE Kernel SHALL remain stable and terminate only the faulty process
5. THE Kernel SHALL save and restore CPU register state during context switches

### Requirement 5: System Calls

**User Story:** As an application developer, I want to use system calls, so that I can request kernel services from userspace.

#### Acceptance Criteria

1. THE Kernel SHALL implement syscall instruction handler
2. WHEN userspace invokes syscall THEN THE Kernel SHALL validate parameters
3. THE Kernel SHALL provide syscalls for memory allocation, process creation, and IPC
4. IF invalid syscall parameters are provided THEN THE Kernel SHALL return error code
5. THE Kernel SHALL switch from Ring 3 to Ring 0 during syscall execution

### Requirement 6: Inter-Process Communication

**User Story:** As a system architect, I want fast IPC mechanism, so that Display Server and applications can exchange data efficiently.

#### Acceptance Criteria

1. THE Kernel SHALL implement shared memory mechanism for IPC
2. WHEN processes share memory THEN THE Kernel SHALL map same physical pages to both processes
3. THE Kernel SHALL implement message passing system for lightweight communication
4. THE IPC SHALL support sending mouse events and render commands
5. WHEN IPC message is sent THEN THE Kernel SHALL deliver it to target process

### Requirement 7: Video Driver

**User Story:** As a display server developer, I want to access framebuffer, so that I can render graphics to screen.

#### Acceptance Criteria

1. THE Video_Driver SHALL run as userspace process with elevated privileges
2. THE Video_Driver SHALL provide interface for writing to framebuffer
3. WHEN Display_Server requests frame render THEN THE Video_Driver SHALL copy data to framebuffer
4. THE Video_Driver SHALL implement double buffering to prevent screen tearing
5. THE Video_Driver SHALL support VBE and GOP protocols

### Requirement 8: Display Server and Window Manager

**User Story:** As a user, I want graphical interface with window management, so that I can run multiple applications with visual feedback.

#### Acceptance Criteria

1. THE Display_Server SHALL run as separate userspace process
2. THE WM SHALL implement window dragging functionality
3. THE WM SHALL implement window focus management
4. WHEN application requests window creation THEN THE WM SHALL allocate window resources
5. THE Display_Server SHALL composite all windows into single framebuffer output

### Requirement 9: GUI Framework Integration

**User Story:** As a GUI developer, I want to use egui framework, so that I can create user interfaces without implementing rendering from scratch.

#### Acceptance Criteria

1. THE Display_Server SHALL integrate egui library
2. THE Display_Server SHALL implement egui::RawInput trait for input events
3. WHEN mouse event occurs THEN THE Display_Server SHALL forward it to egui
4. WHEN keyboard event occurs THEN THE Display_Server SHALL forward it to egui
5. THE Display_Server SHALL render egui textures through Video_Driver

### Requirement 10: Virtual File System

**User Story:** As an application developer, I want file system interface, so that I can read and write files using standard paths.

#### Acceptance Criteria

1. THE VFS SHALL implement /dev, /bin, and /proc directories
2. WHEN application opens file THEN THE VFS SHALL resolve path and return file descriptor
3. THE VFS SHALL support read and write operations
4. THE VFS SHALL implement device files in /dev for hardware access
5. IF file does not exist THEN THE VFS SHALL return appropriate error code

### Requirement 11: C Library Support

**User Story:** As a system integrator, I want C library support, so that I can port existing C applications like NetSurf.

#### Acceptance Criteria

1. THE System SHALL integrate relibc from Redox OS
2. THE relibc SHALL provide malloc and free implementations
3. THE relibc SHALL provide standard C functions for file operations
4. WHEN C application calls malloc THEN THE relibc SHALL allocate memory through kernel syscalls
5. THE relibc SHALL be compiled as shared library for userspace

### Requirement 12: Network Stack

**User Story:** As a network application developer, I want TCP/IP stack, so that I can implement network communication.

#### Acceptance Criteria

1. THE System SHALL integrate smoltcp library for networking
2. THE Network_Driver SHALL run as userspace process
3. WHEN application sends network packet THEN THE Network_Driver SHALL transmit it through hardware
4. THE Network_Driver SHALL support e1000 network card emulated by QEMU
5. THE smoltcp SHALL operate without standard library dependencies

### Requirement 13: Browser Support

**User Story:** As a user, I want to run web browser, so that I can access internet content.

#### Acceptance Criteria

1. THE System SHALL support NetSurf browser compilation
2. THE NetSurf SHALL render through egui container instead of X11
3. WHEN NetSurf requests rendering THEN THE Display_Server SHALL provide drawing surface
4. THE NetSurf SHALL use relibc for C library functions
5. THE NetSurf SHALL communicate with Network_Driver for HTTP requests

### Requirement 14: Binary Format

**User Story:** As a system developer, I want standard executable format, so that I can load and execute programs consistently.

#### Acceptance Criteria

1. THE System SHALL use ELF64 format for executables
2. WHEN loading executable THEN THE Kernel SHALL parse ELF headers
3. THE Kernel SHALL load program segments into memory according to ELF specification
4. THE Kernel SHALL set entry point from ELF header
5. IF ELF file is corrupted THEN THE Kernel SHALL refuse to load it

### Requirement 15: Global Allocator

**User Story:** As a Rust developer, I want custom allocator, so that I can use Box, Vec, and String in no_std environment.

#### Acceptance Criteria

1. THE Kernel SHALL implement GlobalAlloc trait
2. THE Allocator SHALL manage heap memory for kernel data structures
3. WHEN Box::new is called THEN THE Allocator SHALL provide memory
4. WHEN Vec grows THEN THE Allocator SHALL reallocate memory
5. THE Allocator SHALL work without standard library support
