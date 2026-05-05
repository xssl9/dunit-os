#!/bin/bash

echo "=== Запуск Dunit OS в Terminal Mode ==="
echo ""
echo "Сейчас откроется окно QEMU с меню загрузчика."
echo "Выберите стрелкой ВНИЗ пункт 'Dunit OS - Terminal Mode'"
echo "Нажмите ENTER"
echo ""
echo "Вывод терминала будет в этой консоли (serial port)"
echo ""
echo "Нажмите Enter для продолжения..."
read

qemu-system-x86_64 -cdrom build/microkernel.iso -m 512M -serial stdio
