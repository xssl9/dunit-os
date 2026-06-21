#!/usr/bin/env python3
import json
import subprocess
from pathlib import Path


CODE_EXTENSIONS = {
    ".asm",
    ".c",
    ".h",
    ".html",
    ".json",
    ".ld",
    ".py",
    ".rs",
    ".sh",
    ".toml",
}

CODE_FILENAMES = {
    "Makefile",
    "Makefile.bak",
}

EXCLUDED_PREFIXES = (
    ".git/",
    ".github/",
    "assets/",
    "build/",
    "docs/",
    "limine/",
    "WIKI/",
)

OUTPUT = Path(".github/loc-badge.json")


def tracked_files() -> list[Path]:
    result = subprocess.run(
        ["git", "ls-files"],
        check=True,
        text=True,
        stdout=subprocess.PIPE,
    )
    return [Path(line) for line in result.stdout.splitlines() if line]


def is_code_file(path: Path) -> bool:
    text = path.as_posix()
    if text.startswith(EXCLUDED_PREFIXES):
        return False
    return path.name in CODE_FILENAMES or path.suffix in CODE_EXTENSIONS


def count_lines(path: Path) -> int:
    try:
        return len(path.read_text(encoding="utf-8", errors="ignore").splitlines())
    except OSError:
        return 0


def main() -> None:
    total = sum(count_lines(path) for path in tracked_files() if is_code_file(path))
    OUTPUT.parent.mkdir(parents=True, exist_ok=True)
    OUTPUT.write_text(
        json.dumps(
            {
                "schemaVersion": 1,
                "label": "lines of code",
                "message": f"{total:,}",
                "color": "2ea043",
            },
            indent=2,
        )
        + "\n",
        encoding="utf-8",
    )


if __name__ == "__main__":
    main()
