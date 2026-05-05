#!/bin/bash
qemu-system-x86_64 \
    -cdrom build/microkernel.iso \
    -m 512M \
    -serial file:serial.log \
    -boot menu=on,splash-time=1000 \
    -display gtk &

sleep 3
xdotool search --name "QEMU" key Down Return
wait
