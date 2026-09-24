.PHONY: all clean hal kernel userspace iso disk-image run run-gui

CC = gcc
AS = nasm
CARGO = cargo
QEMU = qemu-system-x86_64
QEMU_DISPLAY ?= sdl
QEMU_USB_INPUT ?= -device qemu-xhci -device usb-mouse
# Hardware acceleration: q35 machine + KVM (CPU virtualization) + host CPU passthrough.
QEMU_ACCEL ?= -enable-kvm -cpu host -machine q35,accel=kvm
# std VGA with 32 MiB VRAM, plenty for the Limine linear framebuffer.
QEMU_VGA ?= -vga std -global VGA.vgamem_mb=32
QEMU_EXTRA ?= -no-reboot
QEMU_MEM ?= 512M
LIMINE_CONFIG ?= limine.conf

HAL_DIR = hal
KERNEL_DIR = kernel
USERSPACE_DIR = userspace
BUILD_DIR = build
ISO_DIR = $(BUILD_DIR)/iso
USERSPACE_BUILD_DIR = $(BUILD_DIR)/userspace

USERSPACE_APPS = \
	elf_demo fs_test exit_test args_test cwd_test path_test image_demo bmp_viewer \
	scheduler_test spawn_ready_test yield_child yield_test resumable_child resumable_test \
	ipc_child ipc_parent runtime_stress input_test file_api_test env_test calc gui_ping \
	gui_terminal_stub gui_calculator gui_stats gui_file_manager stdin_test fault_pf fault_ud dtop \
	preempt_child preempt_test kill_target thread_test wait_test vm_test vm_peer vm_guard_fault vm_protect_fault tls_test futex_test handle_test gui_server gui_shbuf_peer
USERSPACE_CARGO_FLAGS = --release --target ../../../userspace/x86_64-unknown-none.json \
	-Z build-std=core,alloc -Z build-std-features=compiler-builtins-mem -Z json-target-spec

KERNEL_FEATURES =
ifneq ($(filter limine_test_terminal.conf limine_test_gui.conf,$(notdir $(LIMINE_CONFIG))),)
KERNEL_FEATURES = --features boot-smoke-tests
endif

HAL_OBJS = $(BUILD_DIR)/boot.o $(BUILD_DIR)/boot_main.o $(BUILD_DIR)/limine.o $(BUILD_DIR)/hal.o $(BUILD_DIR)/ports.o \
           $(BUILD_DIR)/gdt.o $(BUILD_DIR)/gdt_asm.o \
           $(BUILD_DIR)/idt.o $(BUILD_DIR)/idt_asm.o $(BUILD_DIR)/interrupts.o \
           $(BUILD_DIR)/context_switch.o $(BUILD_DIR)/syscall.o \
           $(BUILD_DIR)/hal_test.o

CFLAGS = -ffreestanding -fno-stack-protector -fno-pic -mno-red-zone \
         -mcmodel=kernel -mno-sse -mno-sse2 -O2 -Wall -Wextra
ASFLAGS = -f elf64

all: $(BUILD_DIR)/kernel.elf

$(BUILD_DIR):
	mkdir -p $(BUILD_DIR)

$(BUILD_DIR)/boot.o: $(HAL_DIR)/src/boot32.asm | $(BUILD_DIR)
	$(AS) $(ASFLAGS) $< -o $@

$(BUILD_DIR)/boot_main.o: $(HAL_DIR)/src/boot_main.c | $(BUILD_DIR)
	$(CC) $(CFLAGS) -c $< -o $@

$(BUILD_DIR)/limine.o: $(HAL_DIR)/src/limine.c | $(BUILD_DIR)
	$(CC) $(CFLAGS) -c $< -o $@

$(BUILD_DIR)/hal.o: $(HAL_DIR)/src/hal.c | $(BUILD_DIR)
	$(CC) $(CFLAGS) -c $< -o $@

$(BUILD_DIR)/ports.o: $(HAL_DIR)/src/ports.c | $(BUILD_DIR)
	$(CC) $(CFLAGS) -c $< -o $@

$(BUILD_DIR)/gdt.o: $(HAL_DIR)/src/gdt.c | $(BUILD_DIR)
	$(CC) $(CFLAGS) -c $< -o $@

$(BUILD_DIR)/gdt_asm.o: $(HAL_DIR)/src/gdt.asm | $(BUILD_DIR)
	$(AS) $(ASFLAGS) $< -o $@

$(BUILD_DIR)/idt.o: $(HAL_DIR)/src/idt.c | $(BUILD_DIR)
	$(CC) $(CFLAGS) -c $< -o $@

$(BUILD_DIR)/idt_asm.o: $(HAL_DIR)/src/idt.asm | $(BUILD_DIR)
	$(AS) $(ASFLAGS) $< -o $@

$(BUILD_DIR)/interrupts.o: $(HAL_DIR)/src/interrupts.asm | $(BUILD_DIR)
	$(AS) $(ASFLAGS) $< -o $@

$(BUILD_DIR)/context_switch.o: $(HAL_DIR)/src/context_switch.asm | $(BUILD_DIR)
	$(AS) $(ASFLAGS) $< -o $@

$(BUILD_DIR)/syscall.o: $(HAL_DIR)/src/syscall.asm | $(BUILD_DIR)
	$(AS) $(ASFLAGS) $< -o $@

$(BUILD_DIR)/hal_test.o: $(HAL_DIR)/src/hal_test.c | $(BUILD_DIR)
	$(CC) $(CFLAGS) -c $< -o $@

hal: $(HAL_OBJS)

$(BUILD_DIR)/kernel.o: hal userspace
	cd $(KERNEL_DIR) && $(CARGO) build --release $(KERNEL_FEATURES) -Z build-std=core,alloc,compiler_builtins -Z build-std-features=compiler-builtins-mem -Z json-target-spec
	cp $(KERNEL_DIR)/target/x86_64-unknown-none/release/libkernel.a $(BUILD_DIR)/kernel.o

kernel: $(BUILD_DIR)/kernel.o

$(BUILD_DIR)/kernel.elf: kernel
	/usr/bin/ld.lld -T $(KERNEL_DIR)/linker.ld -o $@ $(HAL_OBJS) $(BUILD_DIR)/kernel.o

userspace:
	mkdir -p $(USERSPACE_BUILD_DIR)
	rm -f $(USERSPACE_BUILD_DIR)/*
	@set -e; for app in $(USERSPACE_APPS); do \
		echo "[USERSPACE] building $$app"; \
		(cd $(USERSPACE_DIR)/system_apps/$$app && $(CARGO) build $(USERSPACE_CARGO_FLAGS)); \
		cp $(USERSPACE_DIR)/system_apps/$$app/target/x86_64-unknown-none/release/$$app $(USERSPACE_BUILD_DIR)/$$app; \
	done
	@echo "Userspace programs built in $(USERSPACE_BUILD_DIR)/"

iso: $(BUILD_DIR)/kernel.elf userspace
	test -f $(LIMINE_CONFIG)
	python3 tools/install_disk.py $(BUILD_DIR)/installer-esp.img --esp-image-only --no-build --config $(LIMINE_CONFIG) --yes-i-know-this-erases-the-disk
	mkdir -p $(ISO_DIR)/boot/limine
	mkdir -p $(ISO_DIR)/boot/userspace
	cp $(BUILD_DIR)/kernel.elf $(ISO_DIR)/boot/
	cp $(BUILD_DIR)/installer-esp.img $(ISO_DIR)/boot/
	cp $(BUILD_DIR)/installer-bios.bin $(ISO_DIR)/boot/
	cp $(LIMINE_CONFIG) $(ISO_DIR)/boot/limine/limine.conf
	test -f assets/gui/background.png && cp assets/gui/background.png $(ISO_DIR)/boot/background.png || true
	test -f assets/boot/limine.png && cp assets/boot/limine.png $(ISO_DIR)/boot/limine.png || true
	cp $(USERSPACE_BUILD_DIR)/* $(ISO_DIR)/boot/userspace/ 2>/dev/null || true
	cp limine/limine-bios.sys $(ISO_DIR)/boot/limine/
	cp limine/limine-bios-cd.bin $(ISO_DIR)/boot/limine/
	cp limine/limine-uefi-cd.bin $(ISO_DIR)/boot/limine/
	mkdir -p $(ISO_DIR)/EFI/BOOT
	cp limine/BOOTX64.EFI $(ISO_DIR)/EFI/BOOT/
	xorriso -as mkisofs -b boot/limine/limine-bios-cd.bin \
		-no-emul-boot -boot-load-size 4 -boot-info-table \
		--efi-boot boot/limine/limine-uefi-cd.bin \
		-efi-boot-part --efi-boot-image --protective-msdos-label \
		$(ISO_DIR) -o $(BUILD_DIR)/microkernel.iso 2>/dev/null || \
	xorriso -as mkisofs -b boot/limine/limine-bios-cd.bin \
		-no-emul-boot -boot-load-size 4 -boot-info-table \
		--efi-boot boot/limine/limine-uefi-cd.bin \
		-efi-boot-part --efi-boot-image --protective-msdos-label \
		"$$(pwd)/$(ISO_DIR)" -o "$$(pwd)/$(BUILD_DIR)/microkernel.iso"
	./limine/limine bios-install $(BUILD_DIR)/microkernel.iso
	@echo "ISO created at $(BUILD_DIR)/microkernel.iso using $(LIMINE_CONFIG)"

iso-test-terminal:
	$(MAKE) iso LIMINE_CONFIG=limine_test_terminal.conf

iso-test-gui:
	$(MAKE) iso LIMINE_CONFIG=limine_test_gui.conf

disk-image: all userspace
	python3 tools/install_disk.py $(BUILD_DIR)/dunit-disk.img --no-build --yes-i-know-this-erases-the-disk

grub-iso: $(BUILD_DIR)/kernel.elf
	rm -rf $(BUILD_DIR)/iso_grub
	mkdir -p $(BUILD_DIR)/iso_grub/boot/grub
	cp $(BUILD_DIR)/kernel.elf $(BUILD_DIR)/iso_grub/boot/
	cp grub.cfg $(BUILD_DIR)/iso_grub/boot/grub/
	grub-mkrescue -o $(BUILD_DIR)/os.iso $(BUILD_DIR)/iso_grub
	@echo "GRUB ISO created at $(BUILD_DIR)/os.iso"

run: iso
	$(QEMU) $(QEMU_ACCEL) $(QEMU_EXTRA) -boot d -cdrom $(BUILD_DIR)/microkernel.iso -m $(QEMU_MEM) -serial stdio -display $(QEMU_DISPLAY) $(QEMU_VGA) $(QEMU_USB_INPUT) -boot menu=on

run-gui: iso-test-gui
	$(QEMU) $(QEMU_ACCEL) $(QEMU_EXTRA) -boot d -cdrom $(BUILD_DIR)/microkernel.iso -m $(QEMU_MEM) -serial stdio -display $(QEMU_DISPLAY) $(QEMU_VGA) $(QEMU_USB_INPUT) -boot menu=on

run-terminal: iso
	$(QEMU) $(QEMU_ACCEL) $(QEMU_EXTRA) -boot d -cdrom $(BUILD_DIR)/microkernel.iso -m $(QEMU_MEM) -serial stdio -nographic $(QEMU_USB_INPUT) -boot menu=on

clean:
	rm -rf $(BUILD_DIR)
	cd $(KERNEL_DIR) && $(CARGO) clean
