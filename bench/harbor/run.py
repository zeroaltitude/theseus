"""Start a benchmark run the fair way (theseus-w052 3 and 4).

    python3 bench/harbor/run.py --plan bench/plans/<plan>.json \\
        -- harbor run -d terminal-bench@2.0 -a theseus_agent:Theseus \\
           -m anthropic/claude-sonnet-5-5 -o jobs --job-name theseus-r0

What it checks before it starts anything, and refuses (exit 3, with every
reason) when one fails:

- the attempt plan (`plan.py`): a file under bench/plans/, committed and clean
  in git. Its attempts (`-k`, and `-i` for a per-task list) replace the command
  line's, which must not name them;
- a quiet host (`quiet.py`): the gate's lock free, the CPU's pressure under the
  line, and at most 4 trials at once (`-n` is given 4 when the line has none);

then it runs one `harbor run` per attempt group, in order, and writes the run's
record beside the job, `<jobs dir>/<job name>.run.json`: the plan, the commit
that holds it, the commit the code is at, the host's reading, and the lines it
ran. `--dry-run` checks and prints without running.

Standard library only; Harbor is the command it is given.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path
from typing import Any, Callable

import plan as planmod
import quiet


def option(args: list[str], *names: str, default: str | None = None) -> str | None:
    """The value of the first of `names` in `args` (`--job-name x`, `--job-name=x`)."""
    for i, a in enumerate(args):
        if a in names and i + 1 < len(args):
            return args[i + 1]
        for n in names:
            if a.startswith(n + "=") and n.startswith("--"):
                return a.split("=", 1)[1]
    return default


def without(args: list[str], name: str) -> list[str]:
    out, skip = [], False
    for a in args:
        if skip:
            skip = False
        elif a == name:
            skip = True
        elif not a.startswith(name + "="):
            out.append(a)
    return out


def prepare(plan_file: str, harbor: list[str], *, repo: Path = planmod.REPO, plans: Path = planmod.PLANS,
            lock: Path | None = None, pressure_file: str = quiet.PRESSURE_FILE,
            max_pressure: float = quiet.MAX_CPU_PRESSURE) -> dict[str, Any]:
    """The run's record and lines, or `planmod.PlanRefused` / `Refused` with
    every reason."""
    reasons = quiet.refusals(harbor, lock=lock, pressure_file=pressure_file, max_pressure=max_pressure)
    plan, plan_reason = None, None
    try:
        plan = planmod.load(plan_file, repo, plans)
    except planmod.PlanRefused as e:
        plan_reason = str(e)
    if plan_reason:
        reasons.append(plan_reason)
    if reasons:
        raise Refused(reasons)
    job = option(harbor, "--job-name") or "job"
    base = quiet.with_parallel(without(harbor, "--job-name"))
    lines = planmod.commands(plan, base, job)
    return {"job_name": job, "jobs_dir": option(harbor, "-o", "--jobs-dir", default="jobs"), "plan": plan,
            "host": quiet.reading(pressure_file), "parallel": quiet.parallel(base), "lines": lines}


class Refused(Exception):
    def __init__(self, reasons: list[str]):
        super().__init__("; ".join(reasons))
        self.reasons = reasons


def main(argv: list[str], runner: Callable[[list[str]], int] | None = None) -> int:
    if "--" not in argv:
        print("usage: run.py --plan bench/plans/<plan>.json [--dry-run] -- harbor run ...", file=sys.stderr)
        return 2
    cut = argv.index("--")
    ap = argparse.ArgumentParser(prog="run.py")
    ap.add_argument("--plan", required=True)
    ap.add_argument("--dry-run", action="store_true")
    a = ap.parse_args(argv[:cut])
    try:
        rec = prepare(a.plan, argv[cut + 1:])
    except Refused as e:
        for r in e.reasons:
            print(f"run.py: refusing to start: {r}", file=sys.stderr)
        return 3
    path = Path(rec["jobs_dir"]) / f"{rec['job_name']}.run.json"
    for line in rec["lines"]:
        print(" ".join(line))
    if a.dry_run:
        return 0
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(rec, indent=2) + "\n")
    run = runner or (lambda line: subprocess.run(line).returncode)
    code = 0
    for line in rec["lines"]:
        code = run(line) or code
    return code


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
