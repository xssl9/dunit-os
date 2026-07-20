#!/usr/bin/env python3
"""Create a BIOS/UEFI bootable Dunit OS disk."""

from __future__ import annotations

import argparse
import os
from pathlib import Path
import shutil
import stat
import struct
import subprocess
import sys
import zlib


SECTOR_SIZE = 512
ESP_START_MIB = 1
ESP_END_MIB = 65
MIN_DISK_MIB = 128
DUNITFS_MAGIC = b"DUNITFS1"
DUNITFS_VERSION = 1
DUNITFS_METADATA_BLOCKS = 16
DUNITFS_DATA_START = 17


def run(command: list[str], *, env: dict[str, str] | None = None) -> None:
    print("[INSTALL]", " ".join(command))
    subprocess.run(command, check=True, env=env)


def require_tools(names: list[str]) -> None:
    missing = [name for name in names if shutil.which(name) is None]
    if missing:
        raise RuntimeError(f"missing tools: {', '.join(missing)}")


def is_block_device(path: Path) -> bool:
    try:
        return stat.S_ISBLK(path.stat().st_mode)
    except FileNotFoundError:
        return False


def reject_mounted_disk(path: Path) -> None:
    device_type = subprocess.run(
        ["lsblk", "-dnro", "TYPE", str(path)],
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()
    if device_type not in ("disk", "loop"):
        raise RuntimeError("target must be a whole disk, not a partition")
    result = subprocess.run(
        ["lsblk", "-nrpo", "NAME,MOUNTPOINT", str(path)],
        check=True,
        capture_output=True,
        text=True,
    )
    mounted = [line for line in result.stdout.splitlines() if len(line.split(None, 1)) == 2]
    if mounted:
        raise RuntimeError("target or one of its partitions is mounted")


def prepare_target(path: Path, size_mib: int, block_device: bool) -> None:
    if block_device:
        reject_mounted_disk(path)
        if not os.access(path, os.R_OK | os.W_OK):
            raise RuntimeError("block device is not readable and writable; run as root")
        return
    if size_mib < MIN_DISK_MIB:
        raise RuntimeError(f"image must be at least {MIN_DISK_MIB} MiB")
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("wb") as image:
        image.truncate(size_mib * 1024 * 1024)


def validate_payload(root: Path, config: Path) -> None:
    required = [
        root / "limine/limine",
        root / "limine/BOOTX64.EFI",
        root / "limine/limine-bios.sys",
        root / "build/kernel.elf",
        config,
    ]
    missing = [str(path) for path in required if not path.is_file()]
    userspace = root / "build/userspace"
    if not userspace.is_dir() or not any(path.is_file() for path in userspace.iterdir()):
        missing.append(str(userspace))
    if missing:
        raise RuntimeError(f"missing install payload: {', '.join(missing)}")


def partition_disk(path: Path) -> None:
    run(["parted", "-s", str(path), "mklabel", "gpt"])
    run(
        [
            "parted",
            "-s",
            str(path),
            "mkpart",
            "DUNIT-ESP",
            "fat32",
            f"{ESP_START_MIB}MiB",
            f"{ESP_END_MIB}MiB",
        ]
    )
    run(["parted", "-s", str(path), "set", "1", "esp", "on"])
    run(
        [
            "parted",
            "-s",
            str(path),
            "mkpart",
            "DUNIT-ROOT",
            f"{ESP_END_MIB}MiB",
            "100%",
        ]
    )


def partition_ranges(path: Path) -> dict[int, tuple[int, int]]:
    result = subprocess.run(
        ["parted", "-sm", str(path), "unit", "s", "print"],
        check=True,
        capture_output=True,
        text=True,
    )
    ranges: dict[int, tuple[int, int]] = {}
    for line in result.stdout.splitlines():
        fields = line.rstrip(";").split(":")
        if not fields or not fields[0].isdigit() or len(fields) < 4:
            continue
        index = int(fields[0])
        start = int(fields[1].removesuffix("s"))
        end = int(fields[2].removesuffix("s"))
        ranges[index] = (start, end - start + 1)
    if 1 not in ranges or 2 not in ranges:
        raise RuntimeError("failed to read the new GPT partition table")
    return ranges


def format_esp(path: Path, start: int, sectors: int) -> str:
    if sectors % 2:
        raise RuntimeError("ESP size must be aligned to 1024-byte FAT blocks")
    run(
        [
            "mkfs.fat",
            "-F",
            "32",
            "-n",
            "DUNITBOOT",
            "-I",
            f"--offset={start}",
            str(path),
            str(sectors // 2),
        ]
    )
    return f"{path}@@{start * SECTOR_SIZE}"


def copy_boot_files(root: Path, fat_image: str, config: Path) -> None:
    env = os.environ.copy()
    env["MTOOLS_SKIP_CHECK"] = "1"
    for directory in ("EFI", "EFI/BOOT", "boot", "boot/limine", "boot/userspace"):
        run(["mmd", "-i", fat_image, f"::/{directory}"], env=env)

    files = [
        (root / "limine/BOOTX64.EFI", "::/EFI/BOOT/BOOTX64.EFI"),
        (root / "build/kernel.elf", "::/boot/kernel.elf"),
        (config, "::/boot/limine/limine.conf"),
        (root / "limine/limine-bios.sys", "::/boot/limine/limine-bios.sys"),
    ]
    optional = [
        (root / "assets/gui/background.png", "::/boot/background.png"),
        (root / "assets/boot/limine.png", "::/boot/limine.png"),
    ]
    for source, destination in files + [item for item in optional if item[0].is_file()]:
        if not source.is_file():
            raise RuntimeError(f"missing install payload: {source}")
        run(["mcopy", "-o", "-i", fat_image, str(source), destination], env=env)

    userspace = root / "build/userspace"
    for source in sorted(userspace.iterdir()):
        if source.is_file():
            run(["mcopy", "-o", "-i", fat_image, str(source), "::/boot/userspace/"], env=env)


def format_dunitfs(path: Path, start: int, sectors: int) -> None:
    if sectors <= DUNITFS_DATA_START:
        raise RuntimeError("DunitFS partition is too small")
    block = bytearray(SECTOR_SIZE)
    with path.open("r+b", buffering=0) as disk:
        disk.seek(start * SECTOR_SIZE)
        disk.write(block)
        disk.seek((start + 1) * SECTOR_SIZE)
        disk.write(block * DUNITFS_METADATA_BLOCKS)

        block[:8] = DUNITFS_MAGIC
        struct.pack_into("<II", block, 8, DUNITFS_VERSION, SECTOR_SIZE)
        struct.pack_into("<QQ", block, 16, sectors, 1)
        struct.pack_into("<II", block, 32, DUNITFS_METADATA_BLOCKS, 64)
        struct.pack_into("<QQ", block, 40, DUNITFS_DATA_START, 1)
        struct.pack_into("<I", block, 56, zlib.crc32(block[:56]))
        disk.seek(start * SECTOR_SIZE)
        disk.write(block)
        os.fsync(disk.fileno())


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("target", type=Path, help="disk image or whole block device")
    parser.add_argument("--image-size-mib", type=int, default=256)
    parser.add_argument("--config", type=Path, default=Path("limine.conf"))
    parser.add_argument("--no-build", action="store_true")
    parser.add_argument(
        "--yes-i-know-this-erases-the-disk",
        action="store_true",
        help="required destructive-operation confirmation",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    if not args.yes_i_know_this_erases_the_disk:
        print("Refusing to erase target without --yes-i-know-this-erases-the-disk", file=sys.stderr)
        return 2

    root = Path(__file__).resolve().parents[1]
    target = args.target.expanduser().absolute()
    config = args.config if args.config.is_absolute() else root / args.config
    block_device = is_block_device(target)
    if str(target).startswith("/dev/") and not block_device:
        raise RuntimeError("target under /dev is not an existing whole block device")
    tools = ["parted", "mkfs.fat", "mmd", "mcopy", "lsblk"]
    if not args.no_build:
        tools.append("make")
    require_tools(tools)
    if not args.no_build:
        subprocess.run(["make", "all", "userspace"], cwd=root, check=True)
    validate_payload(root, config)
    prepare_target(target, args.image_size_mib, block_device)
    partition_disk(target)
    ranges = partition_ranges(target)
    fat_image = format_esp(target, *ranges[1])
    copy_boot_files(root, fat_image, config)
    format_dunitfs(target, *ranges[2])
    run([str(root / "limine/limine"), "bios-install", str(target)])
    print(f"[INSTALL] complete: {target}")
    print("[INSTALL] firmware support: BIOS and x86_64 UEFI")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (RuntimeError, subprocess.CalledProcessError, OSError) as error:
        print(f"[INSTALL] failed: {error}", file=sys.stderr)
        raise SystemExit(1)
