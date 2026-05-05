# Dunit OS (Green Tea) - Build and Run Guide

## Prerequisites

### Required Tools
- **Rust Nightly Toolchain** (specified in `rust-toolchain.toml`)
- **GCC** (for C compilation)
- **NASM** (for assembly)
- **ld.lld** (LLVM linker)
- **xorriso** (for ISO creation)
- **QEMU** (for running the OS)

### Installation on Linux

```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Install build tools
sudo apt install build-essential nasm lld xorriso qemu-system-x86

# Add Rust source (for building std)
rustup component add rust-src
```

## Building the OS

### Clean Build
```bash
make clean
make
```

### Quick Rebuild
```bash
make
```

### Build Steps Explained
1. **HAL (Hardware Abstraction Layer)**: Compiles C and assembly files
2. **Kernel**: Builds Rust kernel with custom target
3. **Linking**: Links all components into `build/kernel.elf`

## Creating Bootable ISO

```bash
./create_iso.sh
```

This creates `build/microkernel.iso` with Limine bootloader.

## Running the OS

### GUI Mode (Default)
```bash
make run
```

Features:
- Graphical desktop environment
- Window manager with Plank dock
- Mouse cursor support
- 5 application icons (Terminal, Files, Settings, Monitor, Editor)
- Solarized Dark theme

### Terminal Mode (Text-only via Serial)
```bash
make run-terminal
```

Then select **"Dunit OS - Terminal Mode (Text Only)"** from the Limine boot menu.

Features:
- Text-only interface via serial port (output in console)
- Interactive shell with commands: `help`, `ls`, `pwd`, `clear`, `exit`
- No GUI initialization
- Lower resource usage

Exit: Press `Ctrl+A` then `X`

### With Serial Logging
```bash
qemu-system-x86_64 -cdrom build/microkernel.iso -m 512M -serial file:serial.log
```

View logs:
```bash
tail -f serial.log
```

## Boot Menu Options

The Limine bootloader provides two options:

1. **Dunit OS - GUI Mode (with Desktop Environment)**
   - Full graphical interface
   - Window manager and desktop
   - Mouse and keyboard support

2. **Dunit OS - Terminal Mode (Text Only)**
   - Serial port terminal
   - Command-line interface
   - Minimal initialization

## Available Commands (Terminal Mode)

- `help` - Show available commands
- `ls` - List directory contents
- `pwd` - Print working directory
- `clear` - Clear screen
- `exit` - Show exit instructions

## Troubleshooting

### Build Errors

**Error: `rustc` not found**
```bash
rustup default nightly
```

**Error: `rust-src` component missing**
```bash
rustup component add rust-src
```

**Error: `ld.lld` not found**
```bash
sudo apt install lld
```

### Runtime Issues

**Black screen in GUI mode**
- Check `serial.log` for errors
- Ensure QEMU has graphics support

**Terminal mode not responding**
- Make sure you selected "Terminal Mode" in boot menu
- Use `-serial mon:stdio` flag
- Check keyboard is captured by QEMU window

**System hangs**
- Check `serial.log` for last message
- Try increasing memory: `-m 1024M`

## Project Structure

```
.
├── hal/                    # Hardware Abstraction Layer (C/ASM)
│   └── src/
│       ├── boot.asm       # Boot code
│       ├── boot_main.c    # C boot entry
│       ├── gdt.c/asm      # Global Descriptor Table
│       ├── idt.c/asm      # Interrupt Descriptor Table
│       └── interrupts.asm # Interrupt handlers
├── kernel/                 # Kernel (Rust)
│   └── src/
│       ├── lib.rs         # Main kernel code
│       ├── drivers/       # Device drivers
│       ├── memory/        # Memory management
│       ├── process/       # Process scheduler
│       ├── fs/            # File systems
│       └── ipc/           # Inter-process communication
├── limine/                 # Limine bootloader
├── Makefile               # Build system
├── create_iso.sh          # ISO creation script
└── limine.conf            # Bootloader configuration
```

## Development

### Running Tests
```bash
cd kernel
cargo test
```

### Checking Code
```bash
cd kernel
cargo check
```

### Cleaning Build Artifacts
```bash
make clean
```

## Performance Tips

- Use `-enable-kvm` on Linux for hardware acceleration:
  ```bash
  qemu-system-x86_64 -cdrom build/microkernel.iso -m 512M -enable-kvm
  ```

- Increase memory for better performance:
  ```bash
  qemu-system-x86_64 -cdrom build/microkernel.iso -m 1024M
  ```

## Exit QEMU

- **GUI Mode**: Close window or press `Ctrl+Alt+G` then `Ctrl+C`
- **Terminal Mode**: Press `Ctrl+A` then `X`
- **Monitor Mode**: Type `quit` in QEMU monitor

## License

See LICENSE file for details.
