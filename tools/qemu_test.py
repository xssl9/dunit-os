#!/usr/bin/env python3
"""AI-driven QEMU test harness for Dunit OS.

Purpose
-------
Lets an AI (or a human) build the OS, boot it head-less inside QEMU, type
commands into the emulated terminal, capture what the system prints on the
serial line, optionally grab a framebuffer screenshot, and force the VM to
shut down after a chosen time budget. This makes it possible to keep working
on the OS unattended.

Why keystrokes instead of "just pipe into serial"
--------------------------------------------------
The Dunit terminal reads its input from the emulated PS/2 keyboard (IRQ1),
NOT from the serial port. There is no serial console. So to "run a command"
we inject scancodes through the QEMU QMP `send-key` command, exactly as if a
user typed them.

What is observable on serial
----------------------------
- All boot/init logs (`serial_write` in the kernel).
- Everything a userspace program writes to stdout/stderr while it runs under
  `exec` (the kernel mirrors STDOUT/STDERR to serial).
- Process / scheduler / syscall / IPC diagnostics.

What is NOT on serial
---------------------
Pure kernel-terminal builtins (`ls`, `ps`, `cat`, ...) render only to the
framebuffer. Use `--screenshot` to capture those visually, or drive `exec`
programs whose output is mirrored to serial.

Examples
--------
Build the terminal-test ISO, boot it, run the canonical regression app, grab a
screenshot, and hard-quit after 60s:

    python3 tools/qemu_test.py --build \
        --cmd "exec runtime_stress" \
        --cmd "exec ipc_parent" \
        --screenshot out.ppm \
        --timeout 60

Use an already-built ISO and just watch boot for 20s with no commands:

    python3 tools/qemu_test.py --iso build/microkernel.iso --timeout 20

Machine-readable result:

    python3 tools/qemu_test.py --build --cmd "exec args_test one two" --json
"""

from __future__ import annotations

import argparse
import json
import os
import selectors
import shutil
import signal
import socket
import subprocess
import sys
import time
from dataclasses import dataclass, field, asdict
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]

# Marker the kernel prints right before it enters the interactive terminal
# loop. Once we see it, keystrokes will be accepted.
DEFAULT_READY_MARKER = "[TERM-007] Starting main loop"

# Default LIMINE_CONFIG for --build. A markers manifest may override it.
DEFAULT_CONFIG = "limine_test_terminal.conf"

# ---------------------------------------------------------------------------
# ASCII -> QEMU qcode mapping for send-key. Values are (qcode, needs_shift).
# ---------------------------------------------------------------------------
_BASE = {
    "a": "a", "b": "b", "c": "c", "d": "d", "e": "e", "f": "f", "g": "g",
    "h": "h", "i": "i", "j": "j", "k": "k", "l": "l", "m": "m", "n": "n",
    "o": "o", "p": "p", "q": "q", "r": "r", "s": "s", "t": "t", "u": "u",
    "v": "v", "w": "w", "x": "x", "y": "y", "z": "z",
    "0": "0", "1": "1", "2": "2", "3": "3", "4": "4",
    "5": "5", "6": "6", "7": "7", "8": "8", "9": "9",
    " ": "spc", "-": "minus", "=": "equal", "[": "bracket_left",
    "]": "bracket_right", ";": "semicolon", "'": "apostrophe",
    "`": "grave_accent", ",": "comma", ".": "dot", "/": "slash",
    "\\": "backslash",
}
# Characters that require Shift on a US layout, mapped to the un-shifted qcode.
_SHIFTED = {
    "!": "1", "@": "2", "#": "3", "$": "4", "%": "5", "^": "6", "&": "7",
    "*": "8", "(": "9", ")": "0", "_": "minus", "+": "equal",
    "{": "bracket_left", "}": "bracket_right", ":": "semicolon",
    '"': "apostrophe", "~": "grave_accent", "<": "comma", ">": "dot",
    "?": "slash", "|": "backslash",
}


def char_to_keys(ch: str) -> list[str] | None:
    """Return the list of qcodes needed to type `ch`, or None if unsupported."""
    if ch in _BASE:
        return [_BASE[ch]]
    if ch.isalpha() and ch.isupper():
        return ["shift", _BASE[ch.lower()]]
    if ch in _SHIFTED:
        return ["shift", _SHIFTED[ch]]
    return None


@dataclass
class CommandResult:
    command: str
    serial_output: str
    duration_s: float
    timed_out: bool


@dataclass
class RunResult:
    ok: bool
    reason: str
    booted: bool
    iso: str
    boot_serial: str = ""
    commands: list[CommandResult] = field(default_factory=list)
    screenshot: str | None = None
    full_serial_len: int = 0
    exit_code: int | None = None
    # Marker budget (M0): required markers that were missing and forbidden
    # markers that showed up. Empty lists mean the marker budget passed.
    markers_checked: bool = False
    missing_markers: list[str] = field(default_factory=list)
    forbidden_hits: list[str] = field(default_factory=list)


class Qmp:
    """Minimal QMP client over a UNIX socket."""

    def __init__(self, path: Path):
        self.path = path
        self.sock: socket.socket | None = None
        self._buf = b""

    def connect(self, timeout: float = 20.0) -> None:
        deadline = time.monotonic() + timeout
        last_err: Exception | None = None
        while time.monotonic() < deadline:
            try:
                s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
                s.connect(str(self.path))
                self.sock = s
                break
            except OSError as err:
                last_err = err
                time.sleep(0.1)
        if self.sock is None:
            raise RuntimeError(f"could not connect to QMP socket: {last_err}")
        # Read the greeting then negotiate capabilities.
        self._read_json()
        self.execute("qmp_capabilities")

    def _read_json(self, timeout: float = 20.0) -> dict:
        assert self.sock is not None
        self.sock.settimeout(timeout)
        while True:
            nl = self._buf.find(b"\n")
            if nl >= 0:
                line = self._buf[:nl]
                self._buf = self._buf[nl + 1:]
                if line.strip():
                    return json.loads(line.decode("utf-8", "replace"))
                continue
            chunk = self.sock.recv(65536)
            if not chunk:
                raise RuntimeError("QMP socket closed")
            self._buf += chunk

    def execute(self, command: str, arguments: dict | None = None) -> dict:
        assert self.sock is not None
        msg = {"execute": command}
        if arguments:
            msg["arguments"] = arguments
        self.sock.sendall((json.dumps(msg) + "\n").encode())
        # Skip async events until we get a return/error.
        while True:
            reply = self._read_json()
            if "return" in reply or "error" in reply:
                return reply

    def send_key(self, qcodes: list[str], hold_ms: int = 40) -> None:
        keys = [{"type": "qcode", "data": code} for code in qcodes]
        self.execute("send-key", {"keys": keys, "hold-time": hold_ms})

    def screendump(self, path: Path) -> None:
        self.execute("screendump", {"filename": str(path)})

    def quit(self) -> None:
        try:
            self.execute("quit")
        except Exception:
            pass

    def close(self) -> None:
        if self.sock is not None:
            try:
                self.sock.close()
            finally:
                self.sock = None


class SerialTail:
    """Follows the QEMU serial log file and lets callers read incremental text."""

    def __init__(self, path: Path):
        self.path = path
        self._fh = None
        self._data = ""

    def open(self, timeout: float = 20.0) -> None:
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if self.path.exists():
                self._fh = self.path.open("r", encoding="utf-8", errors="replace")
                return
            time.sleep(0.05)
        raise RuntimeError(f"serial log never appeared: {self.path}")

    def pump(self) -> str:
        """Read whatever new text is available, append to buffer, return delta."""
        if self._fh is None:
            return ""
        delta = self._fh.read()
        if delta:
            self._data += delta
        return delta

    @property
    def text(self) -> str:
        return self._data

    def wait_for_marker(self, marker: str, timeout: float) -> bool:
        deadline = time.monotonic() + timeout
        start = len(self._data)
        while time.monotonic() < deadline:
            self.pump()
            if marker in self._data[max(0, start - len(marker)):]:
                return True
            time.sleep(0.05)
        return False

    def collect_until_idle(self, idle_s: float, max_s: float) -> str:
        """Collect output until the serial line is quiet for `idle_s`, or `max_s`."""
        start_len = len(self._data)
        deadline = time.monotonic() + max_s
        last_change = time.monotonic()
        while time.monotonic() < deadline:
            delta = self.pump()
            if delta:
                last_change = time.monotonic()
            elif time.monotonic() - last_change >= idle_s:
                break
            time.sleep(0.05)
        else:
            self.pump()
        return self._data[start_len:]


def load_markers(markers_file: str | None,
                 require: list[str] | None,
                 forbid: list[str] | None) -> tuple[list[str], list[str], dict]:
    """Merge a markers manifest file with CLI --require/--forbid markers.

    Returns (required, forbidden, run_defaults). `run_defaults` carries an
    optional {"config", "cmd"} block from the manifest so a single file both
    describes how to boot and what must be observed.
    """
    required: list[str] = []
    forbidden: list[str] = []
    run_defaults: dict = {}
    if markers_file:
        data = json.loads(Path(markers_file).read_text(encoding="utf-8"))
        for entry in data.get("required", []):
            required.append(entry["marker"] if isinstance(entry, dict) else entry)
        for entry in data.get("forbidden", []):
            forbidden.append(entry["marker"] if isinstance(entry, dict) else entry)
        run_defaults = data.get("run", {}) or {}
    required.extend(require or [])
    forbidden.extend(forbid or [])
    return required, forbidden, run_defaults


def build_iso(config: str) -> Path:
    """Build the ISO via the Makefile. Returns the ISO path."""
    if shutil.which("make") is None:
        raise RuntimeError("`make` not found; cannot --build")
    print(f"[qemu_test] building ISO (LIMINE_CONFIG={config}) ...", file=sys.stderr)
    subprocess.run(
        ["make", "iso", f"LIMINE_CONFIG={config}"],
        cwd=REPO_ROOT,
        check=True,
    )
    iso = REPO_ROOT / "build" / "microkernel.iso"
    if not iso.is_file():
        raise RuntimeError("build succeeded but ISO not found")
    return iso


def build_disk(config: str, workdir: Path) -> Path:
    """Build a raw BIOS/UEFI bootable disk image via install_disk.py.

    Unlike build_iso(), this path needs NO xorriso: it uses parted, mkfs.fat,
    mtools, and the bundled Limine binary. That makes a fully self-contained,
    zero-touch AI build+boot+test loop possible even on hosts without xorriso.
    Returns the disk image path.
    """
    if shutil.which("make") is None:
        raise RuntimeError("`make` not found; cannot --build-disk")
    print(f"[qemu_test] building disk image (config={config}) ...", file=sys.stderr)
    # Compile kernel + userspace artifacts the installer consumes.
    subprocess.run(["make", "all", "userspace"], cwd=REPO_ROOT, check=True)
    workdir.mkdir(parents=True, exist_ok=True)
    image = workdir / "dunit-test-disk.img"
    subprocess.run(
        [
            sys.executable,
            str(REPO_ROOT / "tools" / "install_disk.py"),
            str(image),
            "--no-build",
            "--config",
            config,
            "--yes-i-know-this-erases-the-disk",
        ],
        cwd=REPO_ROOT,
        check=True,
    )
    if not image.is_file():
        raise RuntimeError("disk build succeeded but image not found")
    return image


def type_command(qmp: Qmp, text: str) -> None:
    for ch in text:
        keys = char_to_keys(ch)
        if keys is None:
            # Unsupported char: skip but keep going.
            print(f"[qemu_test] warning: cannot type {ch!r}", file=sys.stderr)
            continue
        qmp.send_key(keys)
    qmp.send_key(["ret"])


def build_qemu_command(image: Path, is_disk: bool, accel: str, mem: str,
                       qmp_sock: Path, serial_log: Path, extra: list[str]) -> list[str]:
    cmd = [
        "qemu-system-x86_64",
        "-no-reboot",
        "-m", mem,
        "-display", "none",
        "-serial", f"file:{serial_log}",
        "-qmp", f"unix:{qmp_sock},server,nowait",
        "-vga", "std", "-global", "VGA.vgamem_mb=32",
    ]
    if is_disk:
        cmd += ["-drive", f"file={image},format=raw,if=ide", "-boot", "c"]
    else:
        cmd += ["-cdrom", str(image), "-boot", "d"]
    if accel == "kvm":
        cmd += ["-machine", "q35,accel=kvm", "-cpu", "host"]
    elif accel == "tcg":
        cmd += ["-machine", "q35,accel=tcg"]
    else:  # auto
        if os.access("/dev/kvm", os.W_OK):
            cmd += ["-machine", "q35,accel=kvm", "-cpu", "host"]
        else:
            cmd += ["-machine", "q35,accel=tcg"]
    cmd += extra
    return cmd


def run(args: argparse.Namespace) -> RunResult:
    workdir = Path(args.workdir) if args.workdir else REPO_ROOT / "build"
    workdir.mkdir(parents=True, exist_ok=True)

    required, forbidden, run_defaults = load_markers(
        args.markers_file, args.require_marker, args.forbid_marker)
    # A markers manifest may define how to boot (config + commands). CLI flags
    # win; the manifest fills gaps so `--markers-file` alone is enough.
    if run_defaults.get("config") and args.config == DEFAULT_CONFIG and args.build:
        args.config = run_defaults["config"]
    if run_defaults.get("cmd") and not args.cmd:
        args.cmd = list(run_defaults["cmd"])

    is_disk = False
    if args.build:
        image = build_iso(args.config)
    elif args.build_disk:
        image = build_disk(args.config, workdir)
        is_disk = True
    elif args.disk:
        image = Path(args.disk)
        is_disk = True
        if not image.is_file():
            return RunResult(ok=False, reason=f"disk image not found: {image}",
                             booted=False, iso=str(image))
    else:
        image = Path(args.iso) if args.iso else REPO_ROOT / "build" / "microkernel.iso"
        if not image.is_file():
            return RunResult(ok=False, reason=f"ISO not found: {image}",
                             booted=False, iso=str(image))
    qmp_sock = workdir / "qemu-qmp.sock"
    serial_log = workdir / "qemu-serial.log"
    for stale in (qmp_sock, serial_log):
        try:
            stale.unlink()
        except FileNotFoundError:
            pass

    qemu_cmd = build_qemu_command(image, is_disk, args.accel, args.mem, qmp_sock,
                                  serial_log, args.qemu_arg or [])
    print("[qemu_test] launch:", " ".join(qemu_cmd), file=sys.stderr)

    result = RunResult(ok=False, reason="", booted=False, iso=str(image))
    proc = subprocess.Popen(qemu_cmd, cwd=REPO_ROOT,
                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    hard_deadline = time.monotonic() + args.timeout
    qmp = Qmp(qmp_sock)
    serial = SerialTail(serial_log)

    def remaining() -> float:
        return max(0.0, hard_deadline - time.monotonic())

    def force_quit(reason: str) -> None:
        result.reason = result.reason or reason
        try:
            qmp.quit()
        except Exception:
            pass
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.send_signal(signal.SIGKILL)
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            pass

    try:
        qmp.connect(timeout=min(20.0, args.timeout))
        serial.open(timeout=min(20.0, args.timeout))

        # Wait for the terminal to be ready to accept keystrokes.
        boot_budget = min(args.boot_timeout, remaining())
        booted = serial.wait_for_marker(args.ready_marker, boot_budget)
        result.booted = booted
        result.boot_serial = serial.text
        if not booted:
            force_quit(f"boot marker {args.ready_marker!r} not seen in {boot_budget:.0f}s")
            result.ok = False
            return result

        # The terminal drains its i8042 buffer right before the loop starts, so
        # give it a beat before typing or the first keystrokes get discarded.
        time.sleep(args.post_boot_delay)
        serial.pump()
        for command in args.cmd or []:
            if remaining() <= 1.0:
                result.reason = "hard timeout before running all commands"
                break
            before = len(serial.text)
            type_command(qmp, command)
            cmd_budget = min(args.cmd_timeout, remaining())
            t0 = time.monotonic()
            serial.collect_until_idle(args.settle, cmd_budget)
            out = serial.text[before:]
            dur = time.monotonic() - t0
            result.commands.append(CommandResult(
                command=command,
                serial_output=out,
                duration_s=round(dur, 2),
                timed_out=(dur >= cmd_budget - 0.05),
            ))

        if args.screenshot:
            shot = Path(args.screenshot)
            if not shot.is_absolute():
                shot = workdir / shot
            try:
                qmp.screendump(shot)
                time.sleep(0.3)
                result.screenshot = str(shot)
            except Exception as err:
                print(f"[qemu_test] screenshot failed: {err}", file=sys.stderr)

        # Optionally linger until the hard timeout to keep collecting serial.
        if args.linger:
            end = min(hard_deadline, time.monotonic() + args.linger)
            while time.monotonic() < end:
                serial.pump()
                time.sleep(0.1)

        force_quit("completed")
        result.ok = True
        if not result.reason:
            result.reason = "completed"
    except Exception as err:
        force_quit(f"error: {err}")
        result.ok = False
        result.reason = f"error: {err}"
    finally:
        serial.pump()
        qmp.close()
        result.full_serial_len = len(serial.text)
        result.exit_code = proc.returncode
        # Persist the full serial log alongside the run for later inspection.
        try:
            (workdir / "qemu-serial.full.log").write_text(serial.text, encoding="utf-8")
        except Exception:
            pass

        # Marker budget (M0): required markers must all appear and forbidden
        # markers must not. This is what ties a WORKING claim to real evidence.
        if required or forbidden:
            result.markers_checked = True
            text = serial.text
            result.missing_markers = [m for m in required if m not in text]
            result.forbidden_hits = [m for m in forbidden if m in text]
            if result.missing_markers or result.forbidden_hits:
                marker_reason = (
                    f"marker budget failed: {len(result.missing_markers)} missing, "
                    f"{len(result.forbidden_hits)} forbidden")
                # Keep an existing failure reason (e.g. boot failure) primary.
                if result.ok:
                    result.reason = marker_reason
                else:
                    result.reason = f"{result.reason}; {marker_reason}"
                result.ok = False

    return result


def print_human(result: RunResult) -> None:
    print("=" * 72)
    print(f"ISO       : {result.iso}")
    print(f"Booted    : {result.booted}")
    print(f"Result    : {'OK' if result.ok else 'FAIL'} ({result.reason})")
    print(f"Serial log: build/qemu-serial.full.log ({result.full_serial_len} bytes)")
    if result.markers_checked:
        if not result.missing_markers and not result.forbidden_hits:
            print("Markers   : OK (marker budget satisfied)")
        else:
            print("Markers   : FAIL")
            for m in result.missing_markers:
                print(f"  MISSING required: {m!r}")
            for m in result.forbidden_hits:
                print(f"  FORBIDDEN seen  : {m!r}")
    if result.screenshot:
        print(f"Screenshot: {result.screenshot}")
    if not result.booted:
        print("-" * 72)
        print("BOOT SERIAL (tail):")
        print("\n".join(result.boot_serial.splitlines()[-40:]))
    for i, cmd in enumerate(result.commands, 1):
        print("-" * 72)
        flag = " [TIMED OUT]" if cmd.timed_out else ""
        print(f"[{i}] $ {cmd.command}  ({cmd.duration_s}s){flag}")
        text = cmd.serial_output.strip("\n")
        if text:
            print(text)
        else:
            print("(no serial output; builtins render to framebuffer only)")
    print("=" * 72)


def main() -> int:
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    src = parser.add_mutually_exclusive_group()
    src.add_argument("--iso", help="path to an existing bootable ISO (cdrom boot)")
    src.add_argument("--disk", help="path to a raw bootable disk image (hdd boot)")
    src.add_argument("--build", action="store_true",
                     help="build the ISO with `make iso` before booting (needs xorriso)")
    src.add_argument("--build-disk", action="store_true",
                     help="build a raw bootable disk image and boot it (no xorriso needed)")
    parser.add_argument("--config", default="limine_test_terminal.conf",
                        help="LIMINE_CONFIG for --build (default: terminal test config)")
    parser.add_argument("--cmd", action="append", metavar="COMMAND",
                        help="a terminal command to type (repeatable, in order)")
    parser.add_argument("--timeout", type=float, default=90.0,
                        help="hard wall-clock budget in seconds; VM is force-quit after (default 90)")
    parser.add_argument("--boot-timeout", type=float, default=60.0,
                        help="max seconds to wait for the terminal ready marker (default 60)")
    parser.add_argument("--cmd-timeout", type=float, default=15.0,
                        help="max seconds to collect output per command (default 15)")
    parser.add_argument("--settle", type=float, default=1.5,
                        help="treat a command as done after this many seconds of serial silence (default 1.5)")
    parser.add_argument("--post-boot-delay", type=float, default=1.5,
                        help="seconds to wait after the ready marker before typing (default 1.5)")
    parser.add_argument("--linger", type=float, default=0.0,
                        help="after commands, keep collecting serial for N seconds")
    parser.add_argument("--ready-marker", default=DEFAULT_READY_MARKER,
                        help="serial substring that means the terminal is ready")
    parser.add_argument("--screenshot", metavar="FILE.ppm",
                        help="grab a framebuffer screenshot (PPM) before shutdown")
    parser.add_argument("--accel", choices=["auto", "kvm", "tcg"], default="auto",
                        help="QEMU acceleration (default auto: kvm if /dev/kvm writable)")
    parser.add_argument("--mem", default="512M", help="guest RAM (default 512M)")
    parser.add_argument("--workdir", help="where to place qmp socket / serial log (default build/)")
    parser.add_argument("--qemu-arg", action="append",
                        help="extra raw argument passed to qemu (repeatable)")
    parser.add_argument("--json", action="store_true",
                        help="print the result as JSON instead of human text")
    parser.add_argument("--markers-file", metavar="FILE.json",
                        help="markers manifest (required/forbidden serial markers + run defaults); "
                             "run FAILS if any required marker is missing or any forbidden one appears")
    parser.add_argument("--require-marker", action="append", metavar="TEXT",
                        help="serial substring that must appear (repeatable); merged with --markers-file")
    parser.add_argument("--forbid-marker", action="append", metavar="TEXT",
                        help="serial substring that must NOT appear (repeatable); merged with --markers-file")
    args = parser.parse_args()

    if (not args.build and not args.build_disk and not args.iso and not args.disk
            and not (REPO_ROOT / "build" / "microkernel.iso").is_file()):
        parser.error("no image: pass --build, --build-disk, --iso, or --disk")

    result = run(args)
    if args.json:
        print(json.dumps(asdict(result), indent=2, ensure_ascii=False))
    else:
        print_human(result)
    return 0 if result.ok else 1


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except KeyboardInterrupt:
        print("\n[qemu_test] interrupted", file=sys.stderr)
        raise SystemExit(130)
