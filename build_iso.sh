#!/bin/bash
set -e

# Проверка и настройка Limine
if [ ! -d "limine" ]; then
    echo "==> Загрузка Limine bootloader..."
    git clone https://github.com/limine-bootloader/limine.git --branch=v8.x-binary --depth=1
fi

if [ ! -f "limine/limine" ]; then
    echo "==> Сборка Limine executable..."
    (cd limine && gcc -O2 -o limine limine.c)
fi

# Поиск доступного линкера
if command -v ld.lld &> /dev/null; then
    LINKER="ld.lld"
elif command -v ld.lld-19 &> /dev/null; then
    LINKER="ld.lld-19"
elif command -v ld.lld-18 &> /dev/null; then
    LINKER="ld.lld-18"
else
    echo "Ошибка: lld линкер не найден. Установите: sudo apt install lld"
    exit 1
fi

# Обновление Makefile с правильным линкером
sed -i.bak "s|/usr/bin/ld\.lld[^[:space:]]*|$(command -v $LINKER)|g" Makefile

echo "==> Сборка ISO образа..."
make iso

echo "==> Создание образа диска с Ext2..."
./create_disk.sh

echo "==> ISO образ создан: build/microkernel.iso"
echo "==> Образ диска создан: build/disk.img"
