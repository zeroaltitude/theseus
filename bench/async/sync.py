#!/usr/bin/env python3
"""Write each task's copies of the tool library, and its tools' wrappers
(theseus-7gir.16). A task's image is built from its own `environment/`, and
its verifier reads only its own `tests/`, so each holds a copy of
`tools/asyncbench.py`; `test_async.py` fails when a copy differs.

    python3 bench/async/sync.py
"""

from __future__ import annotations

from pathlib import Path

HERE = Path(__file__).resolve().parent
LIB = HERE / "tools/asyncbench.py"
TASKS = HERE / "tasks"

# Each family's tools, on the agent's PATH as /opt/async/bin/<tool>.
FAMILY_TOOLS = {
    "parallel": ["digest"],
    "wait-tax": ["build-index"],
    "interrupt": ["train-model", "ticket-count"],
    "fanout": ["ingest"],
    "cancel": ["migrate"],
    "contention": ["deposit", "balance"],
}


def wrapper(tool: str) -> str:
    return f'#!/bin/sh\nexec python3 "$(dirname "$0")/../lib/asyncbench.py" tool {tool} "$@"\n'


def expected() -> dict[Path, str]:
    """Every file this script writes, with its text."""
    lib = LIB.read_text()
    files: dict[Path, str] = {}
    for family, tools in FAMILY_TOOLS.items():
        task = TASKS / family
        files[task / "environment/async/lib/asyncbench.py"] = lib
        files[task / "tests/asyncbench.py"] = lib
        for t in tools:
            files[task / f"environment/async/bin/{t}"] = wrapper(t)
    return files


def main() -> None:
    for path, text in expected().items():
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)
        if "/bin/" in path.as_posix():
            path.chmod(0o755)
        print(path.relative_to(HERE))


if __name__ == "__main__":
    main()
