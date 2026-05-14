#!/bin/bash
qemu-system-x86_64 -cdrom build/microkernel.iso -m 512M -drive file=build/disk.img,format=raw,if=ide
