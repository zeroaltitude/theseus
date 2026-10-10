#!/usr/bin/env python3
"""Write each task's copies of the tool library, and its tools' wrappers
(theseus-7gir.16). A task's image is built from its own `environment/`, and
its verifier reads only its own `tests/`, so each holds a copy of
`tools/asyncbench.py`; `test_tasks.py` fails when a copy differs.

Layer 2's tasks (theseus-2wxa) are written whole from `families.py`: their
task.toml, instruction, Dockerfile, files, oracle and verifier, and a copy
of `tools/layer2.py` beside each copy of the library.

    python3 bench/async/sync.py
"""

from __future__ import annotations

from pathlib import Path

HERE = Path(__file__).resolve().parent
LIB = HERE / "tools/asyncbench.py"
LAYER2 = HERE / "tools/layer2.py"
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


def wrapper(tool: str, lib: str = "asyncbench.py") -> str:
    return f'#!/bin/sh\nexec python3 "$(dirname "$0")/../lib/{lib}" tool {tool} "$@"\n'


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
    files.update(layer2_files())
    return files


def layer2_files() -> dict[Path, str]:
    """Layer 2's tasks, each whole (theseus-2wxa)."""
    import families

    lib, l2 = LIB.read_text(), LAYER2.read_text()
    files: dict[Path, str] = {}
    for f in families.FAMILIES:
        task = TASKS / f.name
        files[task / "task.toml"] = families.task_toml(f)
        files[task / "instruction.md"] = f.instruction
        files[task / "environment/Dockerfile"] = families.dockerfile(f)
        for rel, text in f.app.items():
            files[task / "environment/app" / rel] = text
        for d in ("environment/async/lib", "tests"):
            files[task / d / "asyncbench.py"] = lib
            files[task / d / "layer2.py"] = l2
        for t in f.tools:
            files[task / f"environment/async/bin/{t}"] = wrapper(t, "layer2.py")
        files[task / "solution/solve.sh"] = f.solve
        files[task / "tests/test.sh"] = families.test_sh(f)
    return files


def executable(path: Path) -> bool:
    p = path.as_posix()
    return "/bin/" in p or p.endswith(("/solve.sh", "/test.sh"))


def main() -> None:
    for path, text in expected().items():
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)
        if executable(path):
            path.chmod(0o755)
        print(path.relative_to(HERE))


if __name__ == "__main__":
    main()
