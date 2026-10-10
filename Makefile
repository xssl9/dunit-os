.PHONY: all clean hal kernel userspace iso disk-image run run-gui run-dwm iso-dwm

CC = gcc
AS = nasm
# Prefer the rustup shim so the build works even when a distro-packaged stable
# cargo shadows it on PATH. The shim honors rust-toolchain.toml (nightly +
# rust-src), which the -Z build-std flags below require; fall back to plain
# `cargo` if the shim is absent (e.g. CI with nightly as the PATH default).
CARGO ?= $(shell test -x $(HOME)/.cargo/bin/cargo && echo $(HOME)/.cargo/bin/cargo || echo cargo)
QEMU = qemu-system-x86_64
QEMU_DISPLAY ?= sdl
QEMU_USB_INPUT ?= -device qemu-xhci -device usb-mouse
# Hardware acceleration: q35 machine + KVM (CPU virtualization) + host CPU passthrough.
QEMU_ACCEL ?= -enable-kvm -cpu host -machine q35,accel=kvm
# std VGA with 32 MiB VRAM. `edid=on,xres=1920,yres=1080` advertises 1920x1080 as
# the preferred EDID mode so the Limine GOP path under UEFI/OVMF actually offers
# it (plain `-vga std` GOP tops out ~1280x800 and Limine silently falls back to a
# smaller mode). BIOS/VBE already offered 1920x1080 and ignores the EDID hint.
QEMU_VGA ?= -device VGA,vgamem_mb=32,edid=on,xres=1920,yres=1080
QEMU_EXTRA ?= -no-reboot
QEMU_MEM ?= 512M
LIMINE_CONFIG ?= limine.conf

HAL_DIR = hal
KERNEL_DIR = kernel
USERSPACE_DIR = userspace
BUILD_DIR = build
ISO_DIR = $(BUILD_DIR)/iso
USERSPACE_BUILD_DIR = $(BUILD_DIR)/userspace

# Dunit musl port (M6): the x86_64-dunit libc fork lives in the submodule
# toolchains/dunit-musl. build-libc.sh produces a static libc.a + crt objects
# into this sysroot (built once; the recipe is skipped while libc.a exists).
MUSL_DIR = toolchains/dunit-musl
MUSL_SYSROOT = $(BUILD_DIR)/dunit-sysroot
MUSL_LIBC = $(MUSL_SYSROOT)/lib/libc.a
# C programs linked against the Dunit musl port (userspace/ctests/<name>.c),
# packed into the initrd as /app/<name>.
MUSL_CTESTS = musl_hello musl_stdio musl_malloc musl_file musl_thread musl_read musl_stat musl_dir musl_cwd musl_time

USERSPACE_APPS = \
	elf_demo fs_test exit_test args_test cwd_test path_test image_demo bmp_viewer color_test \
	scheduler_test spawn_ready_test yield_child yield_test resumable_child resumable_test \
	ipc_child ipc_parent runtime_stress input_test file_api_test env_test calc \
	stdin_test fault_pf fault_ud dtop \
	preempt_child preempt_test kill_target thread_test wait_test vm_test vm_peer vm_guard_fault vm_protect_fault tls_test futex_test handle_test gui_server gui_shbuf_peer gui_client gui_demo gui_calc gui_stat gui_files pty_echo pty_test dsh_calc_test dsh gui_terminal gui_settings init init_probe fsck_dunit abi_test
USERSPACE_CARGO_FLAGS = --release --target ../../../userspace/x86_64-unknown-none.json \
	-Z build-std=core,alloc -Z build-std-features=compiler-builtins-mem -Z json-target-spec

KERNEL_FEATURES =
ifneq ($(filter limine_test_terminal.conf limine_test_gui.conf,$(notdir $(LIMINE_CONFIG))),)
KERNEL_FEATURES = --features boot-smoke-tests
endif
# `boot-smoke-tests` is enabled ONLY for the two smoke ISOs above; it switches on
# the in-kernel smoke assertions. The DWM config that ships as default.toml is NO
# LONGER an embedded kernel blob: tools/pack_initrd.py selects assets/dwm/test.toml
# (its [startup] autostarts two gui_clients for the M3 isolation invariant) vs
# assets/dwm/default.toml (clean, windowless boot) by the SAME config basename and
# ships it in the initrd archive. The userspace side is built ONCE for every ISO;
# gui_server still learns nothing about which desktop it serves. The in-kernel
# legacy GUI has been removed entirely; the default build is the userspace-DWM
# kernel, and the kernel embeds no application binaries or desktop assets.

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
	@echo "[USERSPACE] building c_hello (freestanding C, Dunit ABI v0 conformance)"
	$(CC) -ffreestanding -nostdlib -nostartfiles -static -no-pie -fno-pic -m64 \
		-fno-stack-protector -fno-asynchronous-unwind-tables -fcf-protection=none \
		-I abi/include -O2 \
		-o $(USERSPACE_BUILD_DIR)/c_hello \
		$(USERSPACE_DIR)/ctests/crt0.s $(USERSPACE_DIR)/ctests/hello.c \
		-Wl,-T,$(USERSPACE_DIR)/userspace.ld -Wl,--build-id=none
	@echo "[USERSPACE] building musl ctests ($(MUSL_CTESTS)) against the Dunit port"
	@if [ ! -f $(MUSL_DIR)/tools/dunit/build-libc.sh ] && [ -f .gitmodules ] && \
	   { [ -d .git ] || [ -f .git ]; } && command -v git >/dev/null 2>&1; then \
		echo "[USERSPACE] fetching $(MUSL_DIR) submodule (git submodule update --init)"; \
		git submodule update --init --recursive $(MUSL_DIR) || \
			echo "[USERSPACE] WARNING: submodule fetch failed (offline?)"; \
	fi
	@if [ -f $(MUSL_DIR)/tools/dunit/build-libc.sh ]; then set -e; \
		[ -f $(abspath $(MUSL_LIBC)) ] || $(MUSL_DIR)/tools/dunit/build-libc.sh $(abspath $(MUSL_SYSROOT)); \
		for prog in $(MUSL_CTESTS); do \
			echo "  [MUSL] $$prog"; \
			CC=$(CC) $(abspath $(MUSL_SYSROOT))/bin/dunit-cc -O2 \
				-o $(USERSPACE_BUILD_DIR)/$$prog \
				$(USERSPACE_DIR)/ctests/$$prog.c; \
		done; \
	else echo "[USERSPACE] WARNING: $(MUSL_DIR) unavailable (not a git checkout or offline); musl ctests skipped. Fetch with: git submodule update --init --recursive"; fi
	@echo "Userspace programs built in $(USERSPACE_BUILD_DIR)/"

iso: $(BUILD_DIR)/kernel.elf userspace
	test -f $(LIMINE_CONFIG)
	python3 tools/install_disk.py $(BUILD_DIR)/installer-esp.img --esp-image-only --no-build --config $(LIMINE_CONFIG) --yes-i-know-this-erases-the-disk
	mkdir -p $(ISO_DIR)/boot/limine
	cp $(BUILD_DIR)/kernel.elf $(ISO_DIR)/boot/
	cp $(BUILD_DIR)/initrd.img $(ISO_DIR)/boot/
	cp $(BUILD_DIR)/installer-esp.img $(ISO_DIR)/boot/
	cp $(BUILD_DIR)/installer-bios.bin $(ISO_DIR)/boot/
	cp $(LIMINE_CONFIG) $(ISO_DIR)/boot/limine/limine.conf
	test -f assets/gui/background.png && cp assets/gui/background.png $(ISO_DIR)/boot/background.png || true
	test -f assets/boot/limine.png && cp assets/boot/limine.png $(ISO_DIR)/boot/limine.png || true
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

iso-dwm:
	$(MAKE) iso LIMINE_CONFIG=limine_dwm.conf

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

run-dwm: iso-dwm
	$(QEMU) $(QEMU_ACCEL) $(QEMU_EXTRA) -boot d -cdrom $(BUILD_DIR)/microkernel.iso -m $(QEMU_MEM) -serial stdio -display $(QEMU_DISPLAY) $(QEMU_VGA) $(QEMU_USB_INPUT) -boot menu=on

run-terminal: iso
	$(QEMU) $(QEMU_ACCEL) $(QEMU_EXTRA) -boot d -cdrom $(BUILD_DIR)/microkernel.iso -m $(QEMU_MEM) -serial stdio -nographic $(QEMU_USB_INPUT) -boot menu=on

clean:
	rm -rf $(BUILD_DIR)
	cd $(KERNEL_DIR) && $(CARGO) clean
