#!/bin/bash
set -e

# Dunit musl fork (M6) lives in the submodule toolchains/dunit-musl. A plain
# `git clone` without --recurse-submodules leaves it empty, so fetch it here
# (idempotent; a no-op once present). Skipped gracefully if this is not a git
# checkout (e.g. a source tarball) — `make` then just skips the musl programs.
if [ -f .gitmodules ] && { [ -d .git ] || [ -f .git ]; } && command -v git &> /dev/null; then
    if [ ! -f toolchains/dunit-musl/configure ]; then
        echo "==> Загрузка submodule toolchains/dunit-musl..."
        git submodule update --init --recursive toolchains/dunit-musl || \
            echo "ВНИМАНИЕ: не удалось получить dunit-musl (нет сети?); musl-программы будут пропущены."
    fi
fi

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

echo "==> ISO образ создан: build/microkernel.iso"
