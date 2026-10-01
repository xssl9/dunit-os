#!/usr/bin/env python3
"""Pack userspace ELFs + assets + DWM config into a Limine-module initrd archive.

The kernel no longer embeds application binaries or desktop assets via
`include_bytes!`; instead Limine loads this archive as a boot module and the
kernel populates its VFS from it (kernel/src/initrd.rs -> kernel/src/fs/vfs.rs).

Archive format "DUNITRD1" (all integers little-endian, no padding):

    magic:  8 bytes  b"DUNITRD1"
    count:  u32      number of file entries
    entry * count:
        name_len: u32          length of the VFS path in bytes
        data_len: u32          length of the file payload in bytes
        name:     name_len     UTF-8 VFS path, e.g. "/app/calc"
        data:     data_len     raw file bytes

Entries are sorted by path for a deterministic image. Directories are implied
by the paths and recreated by the kernel; they are not stored.
"""

from __future__ import annotations

import argparse
import os
from pathlib import Path
import struct
import sys


MAGIC = b"DUNITRD1"

# Configs that build the kernel with `boot-smoke-tests`; for those the desktop
# default must be test.toml (autostarts gui_clients), otherwise default.toml
# (clean desktop). This rule MUST stay in sync with the Makefile KERNEL_FEATURES
# filter (same two basenames) so the kernel feature and the shipped config agree.
SMOKE_CONFIGS = {"limine_test_terminal.conf", "limine_test_gui.conf"}


def dwm_default_source(assets_dir: Path, config: str | None) -> Path:
    smoke = config is not None and os.path.basename(config) in SMOKE_CONFIGS
    return assets_dir / "dwm" / ("test.toml" if smoke else "default.toml")


def collect(args: argparse.Namespace) -> list[tuple[str, bytes]]:
    entries: dict[str, bytes] = {}

    def add(vfs_path: str, host_path: Path) -> None:
        entries[vfs_path] = host_path.read_bytes()

    userspace = Path(args.userspace_dir)
    for source in sorted(userspace.iterdir()):
        if source.is_file():
            add(f"/app/{source.name}", source)

    assets = Path(args.assets_dir)
    excludes = {e.strip("/") for e in args.exclude}
    for host_path in sorted(p for p in assets.rglob("*") if p.is_file()):
        rel = host_path.relative_to(assets).as_posix()
        if rel in excludes:
            continue
        add(f"/assets/{rel}", host_path)

    # DWM system config tree read by gui_server at /system/share/dwm. default.toml
    # is selected per-config (clean vs smoke); apps/* and widgets/* are globbed
    # generically so no application name is hardcoded in the packer.
    default_src = dwm_default_source(assets, args.config)
    if not default_src.is_file():
        raise RuntimeError(f"missing DWM default config: {default_src}")
    add("/system/share/dwm/default.toml", default_src)
    for sub in ("apps", "widgets"):
        src_dir = assets / "dwm" / sub
        if src_dir.is_dir():
            for host_path in sorted(src_dir.glob("*.toml")):
                add(f"/system/share/dwm/{sub}/{host_path.name}", host_path)

    return sorted(entries.items())


def pack(entries: list[tuple[str, bytes]]) -> bytes:
    out = bytearray()
    out += MAGIC
    out += struct.pack("<I", len(entries))
    for name, data in entries:
        name_bytes = name.encode("utf-8")
        out += struct.pack("<II", len(name_bytes), len(data))
        out += name_bytes
        out += data
    return bytes(out)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--userspace-dir", type=Path, required=True)
    parser.add_argument("--assets-dir", type=Path, required=True)
    parser.add_argument("--config", type=str, default=None,
                        help="limine config path; selects default vs test DWM toml")
    parser.add_argument("--exclude", action="append", default=[],
                        help="asset path (relative to assets dir) to omit, repeatable")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    entries = collect(args)
    image = pack(entries)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_bytes(image)
    print(f"[INITRD] packed {len(entries)} files -> {args.out} ({len(image)} bytes)")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (RuntimeError, OSError) as error:
        print(f"[INITRD] pack failed: {error}", file=sys.stderr)
        raise SystemExit(1)
