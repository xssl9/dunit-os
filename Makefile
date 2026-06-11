.PHONY: all clean hal kernel userspace iso run

CC = gcc
AS = nasm
CARGO = cargo
QEMU = qemu-system-x86_64
QEMU_DISPLAY = sdl

HAL_DIR = hal
KERNEL_DIR = kernel
USERSPACE_DIR = userspace
BUILD_DIR = build
ISO_DIR = $(BUILD_DIR)/iso
USERSPACE_BUILD_DIR = $(BUILD_DIR)/userspace

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
	cd $(KERNEL_DIR) && $(CARGO) build --release -Z build-std=core,alloc,compiler_builtins -Z build-std-features=compiler-builtins-mem -Z json-target-spec
	cp $(KERNEL_DIR)/target/x86_64-unknown-none/release/libkernel.a $(BUILD_DIR)/kernel.o

kernel: $(BUILD_DIR)/kernel.o

$(BUILD_DIR)/kernel.elf: kernel
	/usr/bin/ld.lld -T $(KERNEL_DIR)/linker.ld -o $@ $(HAL_OBJS) $(BUILD_DIR)/kernel.o

userspace:
	mkdir -p $(USERSPACE_BUILD_DIR)
	rm -f $(USERSPACE_BUILD_DIR)/*
	cd $(USERSPACE_DIR)/system_apps/elf_demo && $(CARGO) build --release --target ../../../userspace/x86_64-unknown-none.json -Z build-std=core,alloc -Z build-std-features=compiler-builtins-mem -Z json-target-spec
	cp $(USERSPACE_DIR)/system_apps/elf_demo/target/x86_64-unknown-none/release/elf_demo $(USERSPACE_BUILD_DIR)/elf_demo
	cd $(USERSPACE_DIR)/system_apps/fs_test && $(CARGO) build --release --target ../../../userspace/x86_64-unknown-none.json -Z build-std=core,alloc -Z build-std-features=compiler-builtins-mem -Z json-target-spec
	cp $(USERSPACE_DIR)/system_apps/fs_test/target/x86_64-unknown-none/release/fs_test $(USERSPACE_BUILD_DIR)/fs_test
	cd $(USERSPACE_DIR)/system_apps/exit_test && $(CARGO) build --release --target ../../../userspace/x86_64-unknown-none.json -Z build-std=core,alloc -Z build-std-features=compiler-builtins-mem -Z json-target-spec
	cp $(USERSPACE_DIR)/system_apps/exit_test/target/x86_64-unknown-none/release/exit_test $(USERSPACE_BUILD_DIR)/exit_test
	cd $(USERSPACE_DIR)/system_apps/args_test && $(CARGO) build --release --target ../../../userspace/x86_64-unknown-none.json -Z build-std=core,alloc -Z build-std-features=compiler-builtins-mem -Z json-target-spec
	cp $(USERSPACE_DIR)/system_apps/args_test/target/x86_64-unknown-none/release/args_test $(USERSPACE_BUILD_DIR)/args_test
	cd $(USERSPACE_DIR)/system_apps/cwd_test && $(CARGO) build --release --target ../../../userspace/x86_64-unknown-none.json -Z build-std=core,alloc -Z build-std-features=compiler-builtins-mem -Z json-target-spec
	cp $(USERSPACE_DIR)/system_apps/cwd_test/target/x86_64-unknown-none/release/cwd_test $(USERSPACE_BUILD_DIR)/cwd_test
	cd $(USERSPACE_DIR)/system_apps/path_test && $(CARGO) build --release --target ../../../userspace/x86_64-unknown-none.json -Z build-std=core,alloc -Z build-std-features=compiler-builtins-mem -Z json-target-spec
	cp $(USERSPACE_DIR)/system_apps/path_test/target/x86_64-unknown-none/release/path_test $(USERSPACE_BUILD_DIR)/path_test
	cd $(USERSPACE_DIR)/system_apps/scheduler_test && $(CARGO) build --release --target ../../../userspace/x86_64-unknown-none.json -Z build-std=core,alloc -Z build-std-features=compiler-builtins-mem -Z json-target-spec
	cp $(USERSPACE_DIR)/system_apps/scheduler_test/target/x86_64-unknown-none/release/scheduler_test $(USERSPACE_BUILD_DIR)/scheduler_test
	cd $(USERSPACE_DIR)/system_apps/spawn_ready_test && $(CARGO) build --release --target ../../../userspace/x86_64-unknown-none.json -Z build-std=core,alloc -Z build-std-features=compiler-builtins-mem -Z json-target-spec
	cp $(USERSPACE_DIR)/system_apps/spawn_ready_test/target/x86_64-unknown-none/release/spawn_ready_test $(USERSPACE_BUILD_DIR)/spawn_ready_test
	cd $(USERSPACE_DIR)/system_apps/stdin_test && $(CARGO) build --release --target ../../../userspace/x86_64-unknown-none.json -Z build-std=core,alloc -Z build-std-features=compiler-builtins-mem -Z json-target-spec
	cp $(USERSPACE_DIR)/system_apps/stdin_test/target/x86_64-unknown-none/release/stdin_test $(USERSPACE_BUILD_DIR)/stdin_test
	cd $(USERSPACE_DIR)/system_apps/fault_pf && $(CARGO) build --release --target ../../../userspace/x86_64-unknown-none.json -Z build-std=core,alloc -Z build-std-features=compiler-builtins-mem -Z json-target-spec
	cp $(USERSPACE_DIR)/system_apps/fault_pf/target/x86_64-unknown-none/release/fault_pf $(USERSPACE_BUILD_DIR)/fault_pf
	cd $(USERSPACE_DIR)/system_apps/fault_ud && $(CARGO) build --release --target ../../../userspace/x86_64-unknown-none.json -Z build-std=core,alloc -Z build-std-features=compiler-builtins-mem -Z json-target-spec
	cp $(USERSPACE_DIR)/system_apps/fault_ud/target/x86_64-unknown-none/release/fault_ud $(USERSPACE_BUILD_DIR)/fault_ud
	@echo "Userspace programs built in $(USERSPACE_BUILD_DIR)/"

iso: $(BUILD_DIR)/kernel.elf userspace
	mkdir -p $(ISO_DIR)/boot/limine
	mkdir -p $(ISO_DIR)/boot/userspace
	cp $(BUILD_DIR)/kernel.elf $(ISO_DIR)/boot/
	cp limine.conf $(ISO_DIR)/boot/limine/
	test -f background.png && cp background.png $(ISO_DIR)/boot/background.png || true
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
	@echo "ISO created at $(BUILD_DIR)/microkernel.iso"

grub-iso: $(BUILD_DIR)/kernel.elf
	rm -rf $(BUILD_DIR)/iso_grub
	mkdir -p $(BUILD_DIR)/iso_grub/boot/grub
	cp $(BUILD_DIR)/kernel.elf $(BUILD_DIR)/iso_grub/boot/
	cp grub.cfg $(BUILD_DIR)/iso_grub/boot/grub/
	grub-mkrescue -o $(BUILD_DIR)/os.iso $(BUILD_DIR)/iso_grub
	@echo "GRUB ISO created at $(BUILD_DIR)/os.iso"

run: iso
	$(QEMU) -boot d -cdrom $(BUILD_DIR)/microkernel.iso -m 512M -serial stdio -display $(QEMU_DISPLAY) -boot menu=on

run-terminal: iso
	$(QEMU) -boot d -cdrom $(BUILD_DIR)/microkernel.iso -m 512M -serial stdio -nographic -boot menu=on

clean:
	rm -rf $(BUILD_DIR)
	cd $(KERNEL_DIR) && $(CARGO) clean
