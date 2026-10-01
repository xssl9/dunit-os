#!/usr/bin/env python3
"""Create a BIOS/UEFI bootable Dunit OS disk."""

from __future__ import annotations

import argparse
import os
from pathlib import Path
import re
import shutil
import stat
import struct
import subprocess
import sys
import zlib


SECTOR_SIZE = 512
ESP_START_MIB = 1
ESP_END_MIB = 129
MIN_DISK_MIB = 128
ESP_SIZE_MIB = ESP_END_MIB - ESP_START_MIB
DUNITFS_MAGIC = b"DUNITFS1"
DUNITFS_VERSION = 1
DUNITFS_METADATA_BLOCKS = 16
DUNITFS_DATA_START = 17

# Dunit GPT partition policy — single source of truth is kernel/src/storage/policy.rs.
# These on-disk bytes MUST stay identical to policy::DUNIT_SYSTEM_TYPE_GUID so the kernel's
# dunitfs::auto_mount recognises the installed root by its Dunit System type GUID. The bytes
# spell "DUNIT\0SYSTEM\0\0\0\x01"; they replace the generic Linux-data GUID that parted would
# otherwise assign (which the kernel policy no longer treats as a DunitFS root).
DUNIT_SYSTEM_TYPE_GUID = bytes(
    [0x44, 0x55, 0x4E, 0x49, 0x54, 0x00, 0x53, 0x59, 0x53, 0x54, 0x45, 0x4D, 0x00, 0x00, 0x00, 0x01]
)


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


def build_initrd(root: Path, config: Path) -> Path:
    """Pack userspace ELFs + assets + DWM config into build/initrd.img.

    The kernel embeds no application binaries or desktop assets; they ship in
    this Limine-module archive (tools/pack_initrd.py -> kernel/src/initrd.rs).
    gui/boot_blur.bmp is excluded: it is the one asset still embedded in the
    kernel (the boot-background pre-allocator needs it before the VFS exists),
    so shipping it in the archive too would only waste image space.
    """
    out = root / "build/initrd.img"
    run([
        sys.executable,
        str(root / "tools/pack_initrd.py"),
        "--out", str(out),
        "--userspace-dir", str(root / "build/userspace"),
        "--assets-dir", str(root / "assets"),
        "--config", str(config),
        "--exclude", "gui/boot_blur.bmp",
    ])
    return out


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
            "DUNIT-SYSTEM",
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


def set_partition_type_guid(path: Path, part_number: int, type_guid: bytes) -> None:
    """Overwrite partition `part_number`'s GPT type GUID and fix every affected CRC.

    parted assigns a generic type GUID to a bare `mkpart`; the Dunit GPT policy
    (kernel/src/storage/policy.rs) requires the installed root to carry the Dunit System
    type GUID so the kernel auto-mounts it by identity. We rewrite the type GUID in both
    the primary and backup partition-entry arrays and recompute the entry-array CRC and
    both GPT header CRCs, exactly like the in-kernel installer's patch path.
    """
    if len(type_guid) != 16:
        raise RuntimeError("GPT type GUID must be 16 bytes")

    def read_header(disk, lba: int) -> bytearray:
        disk.seek(lba * SECTOR_SIZE)
        header = bytearray(disk.read(SECTOR_SIZE))
        if header[:8] != b"EFI PART":
            raise RuntimeError(f"missing GPT header at LBA {lba}")
        return header

    def patch_entry_array(disk, entries_lba: int, count: int, size: int) -> int:
        disk.seek(entries_lba * SECTOR_SIZE)
        entries = bytearray(disk.read(count * size))
        offset = (part_number - 1) * size
        entries[offset : offset + 16] = type_guid
        disk.seek(entries_lba * SECTOR_SIZE)
        disk.write(entries)
        return zlib.crc32(entries) & 0xFFFFFFFF

    def rewrite_header(disk, header: bytearray, lba: int, entries_crc: int) -> None:
        header_size = struct.unpack_from("<I", header, 12)[0]
        struct.pack_into("<I", header, 88, entries_crc)
        struct.pack_into("<I", header, 16, 0)
        header_crc = zlib.crc32(header[:header_size]) & 0xFFFFFFFF
        struct.pack_into("<I", header, 16, header_crc)
        disk.seek(lba * SECTOR_SIZE)
        disk.write(header)

    with path.open("r+b", buffering=0) as disk:
        primary = read_header(disk, 1)
        primary_entries_lba = struct.unpack_from("<Q", primary, 72)[0]
        backup_header_lba = struct.unpack_from("<Q", primary, 32)[0]
        entry_count = struct.unpack_from("<I", primary, 80)[0]
        entry_size = struct.unpack_from("<I", primary, 84)[0]
        if part_number < 1 or part_number > entry_count or entry_size < 128:
            raise RuntimeError("invalid GPT geometry for type-GUID patch")

        backup = read_header(disk, backup_header_lba)
        backup_entries_lba = struct.unpack_from("<Q", backup, 72)[0]

        entries_crc = patch_entry_array(disk, primary_entries_lba, entry_count, entry_size)
        patch_entry_array(disk, backup_entries_lba, entry_count, entry_size)
        rewrite_header(disk, primary, 1, entries_crc)
        rewrite_header(disk, backup, backup_header_lba, entries_crc)
        os.fsync(disk.fileno())


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


def build_esp_image(path: Path, root: Path, config: Path) -> None:
    sectors = ESP_SIZE_MIB * 1024 * 1024 // SECTOR_SIZE
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("wb") as image:
        image.truncate(sectors * SECTOR_SIZE)
    run(
        [
            "mkfs.fat",
            "-F",
            "32",
            "-n",
            "DUNITBOOT",
            "-I",
            str(path),
            str(sectors // 2),
        ]
    )
    copy_boot_files(root, str(path), config)
    bios_header = (root / "limine/limine-bios-hdd.h").read_text(encoding="ascii")
    bios_payload = bytes(
        int(value, 16) for value in re.findall(r"0x([0-9a-fA-F]{2})", bios_header)
    )
    if len(bios_payload) <= SECTOR_SIZE or bios_payload[510:512] != b"\x55\xaa":
        raise RuntimeError("invalid Limine BIOS HDD payload")
    path.with_name("installer-bios.bin").write_bytes(bios_payload)


def copy_boot_files(root: Path, fat_image: str, config: Path) -> None:
    env = os.environ.copy()
    env["MTOOLS_SKIP_CHECK"] = "1"
    for directory in ("EFI", "EFI/BOOT", "boot", "boot/limine"):
        run(["mmd", "-i", fat_image, f"::/{directory}"], env=env)

    # The installed system boots from its own ESP, which carries only the initrd
    # module (apps + assets); the installer-only payloads (installer-esp.img and
    # installer-bios.bin) are not copied to the target, so their module_path
    # lines are stripped while the initrd module_path line is preserved.
    def keep_config_line(line: str) -> bool:
        stripped = line.lstrip()
        if not stripped.startswith("module_path:"):
            return True
        return "initrd.img" in stripped

    installed_config = root / "build/installed-limine.conf"
    installed_config.write_text(
        "".join(
            line
            for line in config.read_text(encoding="utf-8").splitlines(keepends=True)
            if keep_config_line(line)
        ),
        encoding="utf-8",
    )
    files = [
        (root / "limine/BOOTX64.EFI", "::/EFI/BOOT/BOOTX64.EFI"),
        (root / "build/kernel.elf", "::/boot/kernel.elf"),
        (root / "build/initrd.img", "::/boot/initrd.img"),
        (installed_config, "::/boot/limine/limine.conf"),
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
    parser.add_argument("--esp-image-only", action="store_true")
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
    tools = ["mkfs.fat", "mmd", "mcopy"]
    if not args.esp_image_only:
        tools.extend(["parted", "lsblk"])
    if not args.no_build:
        tools.append("make")
    require_tools(tools)
    if not args.no_build:
        subprocess.run(["make", "all", "userspace"], cwd=root, check=True)
    validate_payload(root, config)
    build_initrd(root, config)
    if args.esp_image_only:
        if block_device:
            raise RuntimeError("ESP payload target must be a regular image file")
        build_esp_image(target, root, config)
        print(f"[INSTALL] ESP payload ready: {target}")
        return 0
    prepare_target(target, args.image_size_mib, block_device)
    partition_disk(target)
    ranges = partition_ranges(target)
    set_partition_type_guid(target, 2, DUNIT_SYSTEM_TYPE_GUID)
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
