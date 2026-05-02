#!/bin/bash
cd "$(dirname "$0")"
rm -rf /tmp/iso_build
mkdir -p /tmp/iso_build/boot
cp build/kernel.elf /tmp/iso_build/boot/
cp limine.conf /tmp/iso_build/boot/
cp limine/limine-bios.sys limine/limine-bios-cd.bin limine/limine-uefi-cd.bin /tmp/iso_build/boot/
mkdir -p /tmp/iso_build/EFI/BOOT
cp limine/BOOTX64.EFI /tmp/iso_build/EFI/BOOT/
xorriso -as mkisofs -b boot/limine-bios-cd.bin -no-emul-boot -boot-load-size 4 -boot-info-table --efi-boot boot/limine-uefi-cd.bin -efi-boot-part --efi-boot-image --protective-msdos-label /tmp/iso_build -o build/microkernel.iso
echo "ISO created at build/microkernel.iso"
