#!/usr/bin/env python3
"""
Local Build and Test Script for Dunit OS
Builds the project locally, runs QEMU with serial logging, and validates boot automatically
"""

import os
import sys
from pathlib import Path
import time
import subprocess
import argparse
import json
import socket
import select
import re

DEFAULT_QEMU_TIMEOUT = 45
DEFAULT_QEMU_LOG = "qemu_serial.log"
DEFAULT_QMP_PORT = 4444
LOCAL_ISO_NAME = "microkernel.iso"


def parse_args():
    parser = argparse.ArgumentParser(
        description="Build Dunit OS locally and run QEMU with automatic testing."
    )
    parser.add_argument(
        "--no-qemu",
        action="store_true",
        help="Build the ISO, but do not launch QEMU."
    )
    parser.add_argument(
        "--qemu-timeout",
        type=int,
        default=DEFAULT_QEMU_TIMEOUT,
        help=f"QEMU timeout in seconds (default: {DEFAULT_QEMU_TIMEOUT})"
    )
    parser.add_argument(
        "--qemu-log",
        type=str,
        default=DEFAULT_QEMU_LOG,
        help=f"QEMU serial log file (default: {DEFAULT_QEMU_LOG})"
    )
    parser.add_argument(
        "--qemu-test-commands",
        type=str,
        default=None,
        help="Send test commands via QMP (comma-separated, e.g. 'help,ls,cat /etc/motd')"
    )
    parser.add_argument(
        "--qmp-port",
        type=int,
        default=DEFAULT_QMP_PORT,
        help=f"QMP TCP port for QEMU control when --qmp-transport=tcp (default: {DEFAULT_QMP_PORT})"
    )
    parser.add_argument(
        "--qmp-transport",
        type=str,
        choices=["stdio", "unix", "tcp"],
        default="stdio",
        help="QMP transport for automatic testing (default: stdio, no socket bind needed)"
    )
    parser.add_argument(
        "--qmp-socket",
        type=str,
        default=None,
        help="QMP Unix socket path (default: /tmp/dunit-os-qmp-<pid>.sock)"
    )
    parser.add_argument(
        "--mode",
        type=str,
        choices=["gui", "terminal", "test-gui", "test-terminal"],
        default="terminal",
        help="Boot mode: gui (normal GUI), terminal (normal terminal), test-gui, test-terminal (default: terminal)"
    )
    parser.add_argument(
        "--no-build",
        action="store_true",
        help="Skip build phase, only run QEMU with existing ISO"
    )
    parser.add_argument(
        "--clean",
        action="store_true",
        help="Clean build artifacts before building"
    )
    parser.add_argument(
        "--display",
        type=str,
        choices=["none", "sdl", "gtk"],
        default=None,
        help="QEMU display backend for GUI modes (default: none for automation)"
    )
    parser.add_argument(
        "--accel",
        type=str,
        choices=["auto", "kvm", "tcg"],
        default="auto",
        help="QEMU accelerator (default: auto, falls back to TCG when /dev/kvm is unavailable)"
    )
    parser.add_argument(
        "--net",
        type=str,
        choices=["e1000", "rtl8139", "virtio", "none"],
        default="e1000",
        help="Emulated NIC for network-stack bring-up tests (default: e1000)"
    )
    return parser.parse_args()


def run_command(cmd, cwd=None, env=None, stream_output=True):
    """Execute a shell command and return exit code"""
    print(f"[CMD] {cmd}")

    if stream_output:
        process = subprocess.Popen(
            cmd,
            shell=True,
            cwd=cwd,
            env=env,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            bufsize=1
        )

        for line in process.stdout:
            print(line, end='')

        process.wait()
        return process.returncode
    else:
        result = subprocess.run(
            cmd,
            shell=True,
            cwd=cwd,
            env=env,
            capture_output=True,
            text=True
        )
        if result.stdout:
            print(result.stdout)
        if result.stderr:
            print(result.stderr, file=sys.stderr)
        return result.returncode


def check_dependencies():
    """Check if required build tools are installed"""
    required = {
        'gcc': 'C compiler',
        'nasm': 'Assembler',
        'cargo': 'Rust toolchain',
        'xorriso': 'ISO creation tool',
        'ld.lld': 'LLVM linker',
        'qemu-system-x86_64': 'QEMU x86_64 emulator'
    }

    missing = []
    for tool, description in required.items():
        result = subprocess.run(['which', tool], capture_output=True)
        if result.returncode != 0:
            missing.append(f"{tool} ({description})")

    if missing:
        print("[ERROR] Missing required dependencies:")
        for item in missing:
            print(f"  - {item}")
        print("\nPlease install missing tools and try again.")
        sys.exit(1)

    print("[DEPS] All build dependencies found")


def build_project(project_root, mode, clean=False):
    """Build the Dunit OS project locally"""
    print("=" * 80)
    print("[BUILD] Starting local build")
    print("=" * 80)

    if clean:
        print("\n[CLEAN] Cleaning build artifacts...")
        exit_code = run_command("make clean", cwd=project_root)
        if exit_code != 0:
            print("[ERROR] Clean failed")
            sys.exit(1)
        print("[CLEAN] Done")

    # Determine make target based on mode
    if mode == "test-terminal":
        make_target = "iso-test-terminal"
    elif mode == "test-gui":
        make_target = "iso-test-gui"
    else:
        make_target = "iso"

    print(f"\n[BUILD] Building with target: {make_target}")
    print("[BUILD] This may take 5-15 minutes on first run...")
    print()

    exit_code = run_command(f"make {make_target}", cwd=project_root)

    if exit_code != 0:
        print(f"\n[ERROR] Build failed with exit code {exit_code}")
        sys.exit(1)

    print("=" * 80)
    print("[BUILD] Build successful!")
    print("=" * 80)

    iso_path = project_root / "build" / "microkernel.iso"
    if not iso_path.exists():
        print(f"[ERROR] ISO not found at {iso_path}")
        sys.exit(1)

    return iso_path


def send_qmp_command(sock, command):
    """Send a QMP command and receive response"""
    payload = (json.dumps(command) + '\n').encode()
    if isinstance(sock, tuple):
        reader, writer = sock
        writer.write(payload)
        writer.flush()
    else:
        sock.sendall(payload)
    time.sleep(0.1)

    return read_qmp_available(sock)


def read_qmp_available(sock):
    response = b''
    while True:
        target = sock[0] if isinstance(sock, tuple) else sock
        ready = select.select([target], [], [], 0.5)
        if not ready[0]:
            break
        if isinstance(sock, tuple):
            chunk = os.read(target.fileno(), 4096)
        else:
            chunk = sock.recv(4096)
        if not chunk:
            break
        response += chunk

    if response:
        try:
            return json.loads(response.decode())
        except json.JSONDecodeError:
            pass
    return None


def qmp_send_keys(sock, text):
    """Send text as keyboard input via QMP"""
    for char in text:
        if char == '\n':
            keys = [{"type": "qcode", "data": "ret"}]
        elif char == ' ':
            keys = [{"type": "qcode", "data": "spc"}]
        elif char.isalpha():
            if char.isupper():
                keys = [
                    {"type": "qcode", "data": "shift"},
                    {"type": "qcode", "data": char.lower()},
                ]
            else:
                keys = [{"type": "qcode", "data": char}]
        elif char.isdigit():
            keys = [{"type": "qcode", "data": char}]
        elif char == '/':
            keys = [{"type": "qcode", "data": "slash"}]
        elif char == '.':
            keys = [{"type": "qcode", "data": "dot"}]
        elif char == '-':
            keys = [{"type": "qcode", "data": "minus"}]
        elif char == '_':
            keys = [
                {"type": "qcode", "data": "shift"},
                {"type": "qcode", "data": "minus"},
            ]
        elif char == ':':
            keys = [
                {"type": "qcode", "data": "shift"},
                {"type": "qcode", "data": "semicolon"},
            ]
        elif char == ';':
            keys = [{"type": "qcode", "data": "semicolon"}]
        else:
            continue  # Skip unsupported characters

        send_qmp_command(sock, {"execute": "send-key", "arguments": {"keys": keys}})
        time.sleep(0.05)


def run_qemu(
    iso_path,
    timeout,
    log_file,
    test_commands=None,
    qmp_port=DEFAULT_QMP_PORT,
    qmp_transport="unix",
    qmp_socket=None,
    mode="terminal",
    display=None,
    accel="auto",
    net="e1000",
):
    """Run QEMU with serial logging and automatic shutdown"""
    print("\n" + "=" * 80)
    print("[QEMU] Starting QEMU")
    print("=" * 80)

    log_path = Path(log_file).resolve()
    print(f"[QEMU] Serial log: {log_path}")
    print(f"[QEMU] Timeout: {timeout}s")
    print(f"[QEMU] Mode: {mode}")

    if accel == "auto":
        selected_accel = "kvm" if Path("/dev/kvm").exists() else "tcg"
    else:
        selected_accel = accel
    print(f"[QEMU] Accelerator: {selected_accel}")

    accel_args = (
        ["-enable-kvm", "-cpu", "host", "-machine", "q35,accel=kvm"]
        if selected_accel == "kvm"
        else ["-cpu", "max", "-machine", "q35,accel=tcg"]
    )

    # Build QEMU command based on mode
    qemu_base = [
        "qemu-system-x86_64",
        *accel_args,
        "-m", "512M",
        "-boot", "d",
        "-cdrom", str(iso_path),
        "-serial", f"file:{log_path}",
        "-no-reboot"
    ]

    if net != "none":
        qemu_base.extend(["-netdev", "user,id=net0"])
        if net == "virtio":
            qemu_base.extend(["-device", "virtio-net-pci,netdev=net0"])
        else:
            qemu_base.extend(["-device", f"{net},netdev=net0"])
        print(f"[QEMU] NIC: {net}")
    else:
        print("[QEMU] NIC: none")

    if mode in ["gui", "test-gui"]:
        display_backend = display or "none"
        qemu_cmd = qemu_base + [
            "-vga", "std",
            "-global", "VGA.vgamem_mb=32",
            "-device", "qemu-xhci",
            "-device", "usb-mouse",
            "-display", display_backend
        ]
    else:  # terminal modes
        qemu_cmd = qemu_base + [
            "-nographic"
        ]

    # Add QMP if test commands are specified
    qmp_path = None
    if test_commands:
        if qmp_transport == "stdio":
            qemu_cmd.extend(["-qmp", "stdio"])
            print("[QEMU] QMP enabled on stdio")
        elif qmp_transport == "unix":
            qmp_path = Path(qmp_socket or f"/tmp/dunit-os-qmp-{os.getpid()}.sock")
            if qmp_path.exists():
                qmp_path.unlink()
            qemu_cmd.extend(["-qmp", f"unix:{qmp_path},server,nowait"])
            print(f"[QEMU] QMP enabled on Unix socket {qmp_path}")
        else:
            qemu_cmd.extend(["-qmp", f"tcp:127.0.0.1:{qmp_port},server,nowait"])
            print(f"[QEMU] QMP enabled on TCP port {qmp_port}")

    # Clear old log
    if log_path.exists():
        log_path.unlink()

    print(f"[QEMU] Command: {' '.join(qemu_cmd)}")
    print("[QEMU] Starting VM...")
    print()

    qemu_stdout = b""
    qemu_stderr = b""
    # Start QEMU
    qemu_process = subprocess.Popen(
        qemu_cmd,
        stdin=subprocess.PIPE if test_commands and qmp_transport == "stdio" else subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE
    )

    start_time = time.time()

    # If test commands specified, wait for QMP and send them
    if test_commands:
        if qmp_transport == "stdio":
            print("[QMP] Waiting for QMP stdio...")
        else:
            print("[QMP] Waiting for QMP socket...")
        time.sleep(3)

        try:
            if qmp_transport == "stdio":
                qmp_sock = (qemu_process.stdout, qemu_process.stdin)
                read_qmp_available(qmp_sock)
            elif qmp_transport == "unix":
                qmp_sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
                qmp_sock.connect(str(qmp_path))
            else:
                qmp_sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
                qmp_sock.connect(("127.0.0.1", qmp_port))
            print("[QMP] Connected")

            # QMP handshake
            if isinstance(qmp_sock, tuple):
                read_qmp_available(qmp_sock)
            else:
                qmp_sock.recv(4096)
            send_qmp_command(qmp_sock, {"execute": "qmp_capabilities"})

            # Wait for boot
            print("[QMP] Waiting for system boot (10s)...")
            time.sleep(10)

            # Send test commands
            commands = [cmd.strip() for cmd in re.split(r"[,;]", test_commands) if cmd.strip()]
            for cmd in commands:
                print(f"[QMP] Sending command: {cmd}")
                qmp_send_keys(qmp_sock, cmd + '\n')
                time.sleep(1)

            print("[QMP] Test commands sent")
            if not isinstance(qmp_sock, tuple):
                qmp_sock.close()

        except Exception as e:
            print(f"[QMP] Error: {e}")

    # Monitor QEMU process
    print(f"[QEMU] Running for {timeout}s...")
    try:
        qemu_stdout, qemu_stderr = qemu_process.communicate(timeout=timeout)
        print("[QEMU] Process exited naturally")
    except subprocess.TimeoutExpired:
        print(f"[QEMU] Timeout reached ({timeout}s), terminating...")
        qemu_process.terminate()
        try:
            qemu_stdout, qemu_stderr = qemu_process.communicate(timeout=5)
        except subprocess.TimeoutExpired:
            print("[QEMU] Force killing...")
            qemu_process.kill()
            qemu_stdout, qemu_stderr = qemu_process.communicate()

    if qemu_stdout:
        print("[QEMU] stdout:")
        print(qemu_stdout.decode("utf-8", errors="ignore").strip())
    if qemu_stderr:
        print("[QEMU] stderr:")
        print(qemu_stderr.decode("utf-8", errors="ignore").strip())
    if qemu_process.returncode not in (0, -15):
        print(f"[QEMU] Exit code: {qemu_process.returncode}")
    print("[QEMU] VM stopped")
    if qmp_path and qmp_path.exists():
        qmp_path.unlink()


def analyze_log(log_file):
    """Analyze serial log for boot success and errors"""
    print("\n" + "=" * 80)
    print("[ANALYSIS] Analyzing serial log")
    print("=" * 80)

    log_path = Path(log_file)
    if not log_path.exists():
        print(f"[ERROR] Log file not found: {log_path}")
        return False

    with open(log_path, 'r', encoding='utf-8', errors='ignore') as f:
        log_content = f.read()

    if not log_content.strip():
        print("[ERROR] Log file is empty - VM may have failed to start")
        return False

    # Boot success indicators
    success_indicators = [
        "[KERNEL] OK",
        "[ OK ] Dunit OS (Green Tea) ready",
        "[TERM-003] Console initialized",
        "Kernel initialized successfully",
        "Dunit OS",  # Matches "Dunit OS 1.0.0"
        "Terminal initialized",
        "GUI initialized",
        "Shell ready",
        "Welcome to Dunit OS",
        "root@dunit",  # Shell prompt visible
        "Console initialized",
        "tty1"  # Terminal device
    ]

    # Error indicators
    error_indicators = [
        "PANIC",
        "kernel panic",
        "FATAL",
        "Page fault",
        "General protection fault",
        "Double fault",
        "Triple fault"
    ]

    found_success = any(indicator in log_content for indicator in success_indicators)
    found_errors = [err for err in error_indicators if err in log_content]

    # Show last 50 lines of log
    lines = log_content.splitlines()
    tail_lines = lines[-50:] if len(lines) > 50 else lines

    print("\n[LOG] Last 50 lines:")
    print("-" * 80)
    for line in tail_lines:
        print(line)
    print("-" * 80)

    print("\n[RESULT] Boot Analysis:")
    if found_success:
        print("  ✓ System booted successfully")
        print(f"  ✓ Found indicators: {', '.join([i for i in success_indicators if i in log_content])}")
    else:
        print("  ✗ No clear boot success indicator found")

    if found_errors:
        print(f"  ✗ Errors detected: {', '.join(found_errors)}")
        return False
    else:
        print("  ✓ No critical errors detected")

    return found_success


def main():
    args = parse_args()
    project_root = Path(__file__).parent.resolve()

    print("\n" + "=" * 80)
    print("Dunit OS - Local Build and Test System")
    print("=" * 80)
    print(f"Project root: {project_root}")
    print(f"Mode: {args.mode}")
    print()

    try:
        # Check dependencies
        check_dependencies()

        # Build phase
        if not args.no_build:
            iso_path = build_project(project_root, args.mode, clean=args.clean)
        else:
            print("\n[BUILD] Skipped (--no-build)")
            iso_path = project_root / "build" / "microkernel.iso"
            if not iso_path.exists():
                print(f"[ERROR] ISO not found at {iso_path}")
                sys.exit(1)

        # Test phase
        if args.no_qemu:
            print("\n[QEMU] Skipped (--no-qemu)")
        else:
            run_qemu(
                iso_path,
                args.qemu_timeout,
                args.qemu_log,
                test_commands=args.qemu_test_commands,
                qmp_port=args.qmp_port,
                qmp_transport=args.qmp_transport,
                qmp_socket=args.qmp_socket,
                display=args.display,
                accel=args.accel,
                net=args.net,
                mode=args.mode
            )

            # Analyze results
            success = analyze_log(args.qemu_log)

            if not success:
                print("\n[FAIL] Boot verification failed")
                sys.exit(1)

    except KeyboardInterrupt:
        print("\n[ABORT] Interrupted by user")
        sys.exit(1)
    except Exception as e:
        print(f"\n[ERROR] {e}")
        import traceback
        traceback.print_exc()
        sys.exit(1)

    print("\n" + "=" * 80)
    print("[DONE] All operations completed successfully!")
    print("=" * 80)


if __name__ == "__main__":
    main()
