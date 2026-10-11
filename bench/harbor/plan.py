"""The attempt plan, committed before the run (theseus-w052 4).

How many attempts each task gets (`k` a task, or a list per task, such as a
Neyman allocation from a pilot) is decided before the data is read, and a file
under `bench/plans/` holds it:

    {"name": "r0-sonnet-5-5", "k": 3}
    {"name": "r0-neyman", "attempts": {"fix-git": 2, "build-pmars": 5}}

A run reads its plan from that file and only from it (`load`), and refuses to
start unless the file is committed and clean: tracked (`git ls-files
--error-unmatch`) and with no difference from HEAD, staged or not (`git diff
HEAD`). The commit that holds it is named in the run's record (`commit`, the
last commit that touched the file; `head`, the commit the run's code is at), so
a plan edited after the fact, or after the first trial, cannot pass for the
one the run was decided on.

`attempt_groups` turns a plan into what Harbor takes: its `-k` is one number
for a run, so tasks with the same number of attempts form a group, one
`harbor run` each (`-i <task>` per task). `bench/harbor/run.py` runs them.

Standard library only.
"""

from __future__ import annotations

import hashlib
import json
import subprocess
from pathlib import Path
from typing import Any

PLANS = Path(__file__).resolve().parent.parent / "plans"
REPO = Path(__file__).resolve().parents[2]


class PlanRefused(Exception):
    """The plan cannot start a run; the message says why."""


def _git(repo: Path, *args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(["git", "-C", str(repo), *args], capture_output=True, text=True)


def load(path: str | Path, repo: Path = REPO, plans: Path = PLANS) -> dict[str, Any]:
    """The plan in `path`, checked: under `plans`, tracked, clean, and well
    formed. Returns the plan with `file`, `sha256`, `commit` and `head` added."""
    p = Path(path).resolve()
    if not p.is_file():
        raise PlanRefused(f"no plan file at {path}")
    if plans.resolve() not in p.parents:
        raise PlanRefused(f"{path} is not under {plans}: a plan lives in bench/plans/")
    rel = str(p.relative_to(repo.resolve()))
    if _git(repo, "ls-files", "--error-unmatch", "--", rel).returncode != 0:
        raise PlanRefused(f"{rel} is not committed: commit the attempt plan before the run")
    if _git(repo, "diff", "HEAD", "--quiet", "--", rel).returncode != 0:
        raise PlanRefused(f"{rel} differs from its commit: commit or discard the change before the run")
    commit = _git(repo, "log", "-1", "--format=%H", "--", rel).stdout.strip()
    head = _git(repo, "rev-parse", "HEAD").stdout.strip()
    if not commit:
        raise PlanRefused(f"{rel} has no commit in this checkout's history")
    try:
        plan = json.loads(p.read_text())
    except ValueError as e:
        raise PlanRefused(f"{rel} is not JSON: {e}") from e
    check(plan, rel)
    return {**plan, "file": rel, "sha256": hashlib.sha256(p.read_bytes()).hexdigest(), "commit": commit,
            "head": head}


def check(plan: Any, where: str = "the plan") -> None:
    if not isinstance(plan, dict) or not plan.get("name"):
        raise PlanRefused(f"{where}: a plan is an object with a name")
    k, att = plan.get("k"), plan.get("attempts")
    if (k is None) == (att is None):
        raise PlanRefused(f"{where}: give `k` (attempts a task) or `attempts` (a list per task), not both or neither")
    if k is not None and not (isinstance(k, int) and not isinstance(k, bool) and k >= 1):
        raise PlanRefused(f"{where}: k must be a whole number, 1 or more")
    if att is not None:
        if not isinstance(att, dict) or not att:
            raise PlanRefused(f"{where}: attempts is an object of task name to attempts")
        for t, n in att.items():
            if not (isinstance(n, int) and not isinstance(n, bool) and n >= 1):
                raise PlanRefused(f"{where}: {t}: attempts must be a whole number, 1 or more")


def attempt_groups(plan: dict[str, Any]) -> list[tuple[int, list[str]]]:
    """`(k, tasks)` per distinct attempt count, in order of k; a task list is
    empty for a plan with one `k` (every task of the dataset)."""
    if plan.get("k") is not None:
        return [(plan["k"], [])]
    groups: dict[int, list[str]] = {}
    for task, n in sorted(plan["attempts"].items()):
        groups.setdefault(n, []).append(task)
    return sorted(groups.items())


def commands(plan: dict[str, Any], harbor: list[str], job_name: str) -> list[list[str]]:
    """The `harbor run` lines for the plan's groups: the caller's arguments
    (which must name neither `-k` nor `-i`: the plan decides those), with each
    group's `-k`, its tasks, and a job name that says the group."""
    for a in harbor:
        if a in ("-k", "--n-attempts", "-i", "--include-task-name") or a.startswith(("--n-attempts=", "-k")):
            raise PlanRefused(f"{a}: the attempt plan decides attempts and tasks, not the command line")
    groups = attempt_groups(plan)
    out = []
    for k, tasks in groups:
        name = job_name if len(groups) == 1 else f"{job_name}-k{k}"
        line = [*harbor, "--job-name", name, "-k", str(k)]
        for t in tasks:
            line += ["-i", t]
        out.append(line)
    return out
