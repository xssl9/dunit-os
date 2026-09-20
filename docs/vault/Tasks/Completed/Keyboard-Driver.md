# Keyboard Input Foundation

**Status:** COMPLETED PS/2 FOUNDATION

## Delivered

- IRQ1 PS/2 keyboard path.
- Scancode decoding for terminal input and common special keys.
- Input path used by Terminal Mode and automated QEMU command injection.
- Userspace/GUI input bridges built on current kernel input model.

## Boundary

This is not general keyboard support. USB xHCI enumeration/HID transfers, layouts, text composition, IME and final GUI Server routing remain unfinished. Target input ownership is capability-gated delivery through GUI Server/session services.
