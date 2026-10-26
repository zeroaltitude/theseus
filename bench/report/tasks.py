"""The task catalogue (theseus-hbh4m): `docs/benchmarks/tasks.json`, what each task of a benchmark is, in its own facts.

The Benchmarks page's per-test drawer reads it to say what a task asks and how hard its authors rate it. No benchmark
CSV carries a description column, so this file must exist. It runs LOCALLY, over a task tree that is not in the
repository (one directory a task, each with a `task.toml` and an `instruction.md`):

    python3 bench/report/tasks.py build --tasks <dir> [--no-summaries] [--source-base <url>] [--suite NAME] [--out FILE]

and writes `{version: 1, suite, about, tasks: {<id>: {title, difficulty, category, tags, expert_min, junior_min,
timeout_s, cpus, memory, summary?, source?}}}`, byte for byte the same on every run (sorted keys, `indent=1`, a
trailing newline).

- The facts come from `task.toml`: `[metadata]` (difficulty, category, tags, the expert and junior time estimates),
  `[agent]` or `[verifier]` `timeout_sec`, `[environment]` `cpus` and `memory`. They go in whole. The `author` field is
  never copied: it may name a person, and a public file carries none.
- `summary` is upstream's prose, so only an excerpt: the first sentence of `instruction.md`, cut to 200 characters. The
  page shows it as an attributed excerpt. `--no-summaries` omits every `summary` and nothing else, so the owner's answer
  on publishing upstream excerpts is one flag.
- `source` is written only from an explicit `--source-base` (`<base>/<id>`); no address is invented, and the page links
  only when it is present.
- A task whose `task.toml` is missing, unreadable or has no `[metadata]` is listed with its id and what it does have,
  never failed on. Standard library only.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
import tomllib
from pathlib import Path
from typing import Any

VERSION = 1
SUITE = "terminal-bench@2.0"
ABOUT = ("What each task of the benchmark is, from the task tree's own task.toml files (difficulty, category, tags, the "
         "authors' time estimates, the timeouts) and, unless left out, the first sentence of each instruction as an "
         "excerpt of the upstream text. Written by bench/report/tasks.py; no author name is copied.")
SUMMARY_MAX = 200

# [metadata] keys copied whole, by the name they take in the file.
METADATA = {"difficulty": "difficulty", "category": "category", "expert_time_estimate_min": "expert_min", "junior_time_estimate_min": "junior_min"}


def _title(task_id: str) -> str:
    return task_id.replace("-", " ").replace("_", " ")


def first_sentence(text: str, limit: int = SUMMARY_MAX) -> str:
    """The first sentence of an instruction, whitespace collapsed, cut to `limit` characters (an ellipsis counts)."""
    flat = " ".join(text.split())
    m = re.search(r"[.!?](?=\s|$)", flat)
    out = flat[: m.end()] if m else flat
    return out if len(out) <= limit else out[: limit - 1].rstrip() + "…"


def _table(doc: dict[str, Any], key: str) -> dict[str, Any]:
    v = doc.get(key)
    return v if isinstance(v, dict) else {}


def _num(v: Any) -> bool:
    return isinstance(v, (int, float)) and not isinstance(v, bool)


def entry_of(task_dir: Path, summaries: bool, source_base: str | None) -> dict[str, Any]:
    """One task's entry: whatever its files hold, and its title from its id."""
    out: dict[str, Any] = {"title": _title(task_dir.name)}
    try:
        doc = tomllib.loads((task_dir / "task.toml").read_text())
    except (OSError, ValueError):
        doc = {}
    meta = _table(doc, "metadata")
    for src, name in METADATA.items():
        v = meta.get(src)
        if isinstance(v, str) and v or _num(v):
            out[name] = v
    tags = meta.get("tags")
    if isinstance(tags, list) and all(isinstance(t, str) for t in tags) and tags:
        out["tags"] = sorted(tags)
    for table in ("agent", "verifier"):
        t = _table(doc, table).get("timeout_sec")
        if _num(t):
            out["timeout_s"] = t
            break
    env = _table(doc, "environment")
    for key in ("cpus", "memory"):
        v = env.get(key)
        if (isinstance(v, str) and v) or _num(v):
            out[key] = v
    if summaries:
        try:
            s = first_sentence((task_dir / "instruction.md").read_text())
        except OSError:
            s = ""
        if s:
            out["summary"] = s
    if source_base:
        out["source"] = f"{source_base.rstrip('/')}/{task_dir.name}"
    return out


def build(tasks_dir: Path, summaries: bool = True, source_base: str | None = None, suite: str = SUITE) -> str:
    """The whole catalogue over every task directory in `tasks_dir`."""
    tasks = {d.name: entry_of(d, summaries, source_base) for d in sorted(tasks_dir.iterdir()) if d.is_dir() and not d.name.startswith(".")}
    doc = {"version": VERSION, "suite": suite, "about": ABOUT, "tasks": tasks}
    return json.dumps(doc, indent=1, ensure_ascii=False, sort_keys=True) + "\n"


def main(argv: list[str]) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    sub = ap.add_subparsers(dest="cmd", required=True)
    b = sub.add_parser("build", help="write tasks.json from a task tree")
    b.add_argument("--tasks", required=True, help="the directory holding one directory a task")
    b.add_argument("--no-summaries", action="store_true", help="omit every summary (upstream's prose) and nothing else")
    b.add_argument("--source-base", help="an address each task id is appended to, written as `source`; none is invented")
    b.add_argument("--suite", default=SUITE)
    b.add_argument("--out", default=str(Path(__file__).resolve().parents[2] / "docs" / "benchmarks" / "tasks.json"))
    args = ap.parse_args(argv)
    tasks = Path(args.tasks)
    if not tasks.is_dir():
        print(f"tasks: {tasks} is not a directory", file=sys.stderr)
        return 1
    text = build(tasks, summaries=not args.no_summaries, source_base=args.source_base, suite=args.suite)
    Path(args.out).write_text(text)
    print(f"wrote {args.out}: {len(json.loads(text)['tasks'])} tasks")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
