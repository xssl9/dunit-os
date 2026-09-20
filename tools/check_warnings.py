#!/usr/bin/env python3
"""Compiler-warnings budget for Dunit OS (M0 CI budget).

Builds the kernel (Rust) and the userspace apps (Rust), counts compiler
warnings that originate in *our* source (not dependencies or the Rust
sysroot), and compares the total against a committed budget in
`tools/warnings_budget.json`. The C HAL is compiled with -Wall -Wextra by the
Makefile and is not part of this Rust warnings budget.

Semantics
---------
- The build FAILS (exit 2) if the measured count is ABOVE the budget. The bar
  can only be held or lowered, never silently raised.
- `--update` ratchets the budget to the freshly measured count and writes it
  back. Use it to lock in a reduction, or to establish the first baseline.
- Warnings are re-emitted deterministically by touching each crate root before
  building, so a warm Cargo cache does not hide them.

This is deliberately a *budget*, not a hard `-D warnings` gate: the kernel
carries a known backlog of warnings today, and the point of M0 is to stop that
backlog from growing while the honest baseline is paid down.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
BUDGET_FILE = REPO_ROOT / "tools" / "warnings_budget.json"

# Userspace apps, kept in sync with the Makefile USERSPACE_APPS list.
USERSPACE_APPS = [
    "elf_demo", "fs_test", "exit_test", "args_test", "cwd_test", "path_test",
    "image_demo", "bmp_viewer", "scheduler_test", "spawn_ready_test",
    "yield_child", "yield_test", "resumable_child", "resumable_test",
    "ipc_child", "ipc_parent", "runtime_stress", "input_test", "file_api_test",
    "env_test", "calc", "gui_ping", "gui_terminal_stub", "gui_calculator",
    "gui_stats", "gui_file_manager", "stdin_test", "fault_pf", "fault_ud",
    "dtop", "preempt_child", "preempt_test",
]

KERNEL_BUILD = [
    "cargo", "build", "--release", "--features", "boot-smoke-tests",
    "-Z", "build-std=core,alloc,compiler_builtins",
    "-Z", "build-std-features=compiler-builtins-mem",
    "-Z", "json-target-spec", "--message-format=json",
]
USERSPACE_BUILD = [
    "cargo", "build", "--release",
    "--target", "../../../userspace/x86_64-unknown-none.json",
    "-Z", "build-std=core,alloc",
    "-Z", "build-std-features=compiler-builtins-mem",
    "-Z", "json-target-spec", "--message-format=json",
]


def touch_crate(crate_dir: Path) -> None:
    """Bump mtime of the crate root so Cargo recompiles and re-emits warnings."""
    for candidate in ("src/lib.rs", "src/main.rs"):
        p = crate_dir / candidate
        if p.is_file():
            p.touch()


def count_rust_warnings(crate_dir: Path, build_cmd: list[str]) -> int:
    """Build a crate with JSON output; count warnings whose spans are in-repo."""
    touch_crate(crate_dir)
    proc = subprocess.run(build_cmd, cwd=crate_dir, capture_output=True, text=True)
    warns = 0
    for line in proc.stdout.splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            msg = json.loads(line)
        except json.JSONDecodeError:
            continue
        if msg.get("reason") != "compiler-message":
            continue
        m = msg.get("message", {})
        if m.get("level") != "warning":
            continue
        spans = m.get("spans", [])
        # Only warnings anchored in our own source, not sysroot/deps.
        if any(("/src/" in s.get("file_name", "") or s.get("file_name", "").startswith("src/"))
               and "/.cargo/" not in s.get("file_name", "")
               for s in spans):
            warns += 1
    if proc.returncode != 0:
        sys.stderr.write(f"[check_warnings] build failed in {crate_dir}:\n")
        sys.stderr.write(proc.stderr[-2000:] + "\n")
        raise SystemExit(3)
    return warns


def measure() -> dict:
    counts: dict[str, int] = {}
    counts["kernel"] = count_rust_warnings(REPO_ROOT / "kernel", KERNEL_BUILD)
    userspace_total = 0
    for app in USERSPACE_APPS:
        app_dir = REPO_ROOT / "userspace" / "system_apps" / app
        if not app_dir.is_dir():
            continue
        userspace_total += count_rust_warnings(app_dir, USERSPACE_BUILD)
    counts["userspace"] = userspace_total
    counts["total"] = counts["kernel"] + counts["userspace"]
    return counts


def load_budget() -> dict | None:
    if BUDGET_FILE.is_file():
        return json.loads(BUDGET_FILE.read_text(encoding="utf-8"))
    return None


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--update", action="store_true",
                    help="ratchet the budget down to the measured count and write it back")
    ap.add_argument("--json", action="store_true", help="machine-readable output")
    args = ap.parse_args()

    counts = measure()
    budget = load_budget()
    budget_total = budget.get("total") if budget else None

    over = budget_total is not None and counts["total"] > budget_total

    if args.update:
        payload = {
            "description": "Compiler-warnings budget (M0). tools/check_warnings.py "
                           "fails if the measured count exceeds these. Ratchet down only.",
            "kernel": counts["kernel"],
            "userspace": counts["userspace"],
            "total": counts["total"],
        }
        BUDGET_FILE.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")

    if args.json:
        print(json.dumps({"measured": counts, "budget": budget_total, "over": over}, indent=2))
    else:
        print(f"kernel    warnings: {counts['kernel']}")
        print(f"userspace warnings: {counts['userspace']}")
        print(f"total     warnings: {counts['total']}")
        if budget_total is not None:
            print(f"budget    total   : {budget_total}")
        if args.update:
            print(f"[check_warnings] budget written to {BUDGET_FILE}")
        elif over:
            print(f"[check_warnings] FAIL: {counts['total']} > budget {budget_total}. "
                  f"Fix new warnings, or run --update if you intentionally lowered/raised the bar.")
        elif budget_total is not None:
            print("[check_warnings] OK: within budget.")

    return 2 if (over and not args.update) else 0


if __name__ == "__main__":
    raise SystemExit(main())
