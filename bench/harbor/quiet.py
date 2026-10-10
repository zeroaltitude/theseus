"""A quiet host for the graders (theseus-w052 3).

A verifier's timeout is the task's clock on the host's: a test that takes 40 s
on an idle machine can take 120 s beside a build, and the trial reads as
failed for the host's reasons. A run starts only on a quiet host, and with few
enough trials at once that its own trials don't make the noise:

- `MAX_PARALLEL` (4): the most trials at once by default. `-n` above it is
  refused, and a `harbor run` line with no `-n` is given it (`parallel`);
- the shared gate lock (`~/.cache/theseus-gate.lock`, `$THESEUS_GATE_LOCK_FILE`,
  scripts/gate.sh) held means a gate is running its tests or its benches on this
  host: `lock_held` tries the lock without waiting and refuses;
- the CPU pressure `/proc/pressure/cpu`, its `some avg10` (the share of the last
  ten seconds in which some task waited for a core), above `MAX_CPU_PRESSURE`
  (5.0, a percent): a host that is not under load reads under 1, and the four
  trials a run starts make about as much themselves, so 5 means something else
  is running. A host without the file (no PSI) is not refused, and the run's
  record says the reading was unavailable.

`refusals` returns every reason; `bench/harbor/run.py` refuses to start a run
that has any. A verifier's own timeout is the grader's, never the agent's
(`efficiency.GRADER_TIMEOUT_ERRORS`; `report/efficiency.py` reads it).

Standard library only.
"""

from __future__ import annotations

import fcntl
import os
import re
from pathlib import Path
from typing import Any

MAX_PARALLEL = 4
# `some avg10`, in percent, above which a run does not start.
MAX_CPU_PRESSURE = 5.0
PRESSURE_FILE = "/proc/pressure/cpu"


def lock_path() -> Path:
    """The gate's shared lock, where scripts/gate.sh keeps it."""
    return Path(os.environ.get("THESEUS_GATE_LOCK_FILE") or Path.home() / ".cache" / "theseus-gate.lock")


def lock_held(path: Path | None = None) -> bool:
    """Whether another process holds the lock: a non-blocking exclusive take
    that is given back at once. A lock file that does not exist is not held."""
    p = path or lock_path()
    try:
        fd = os.open(p, os.O_RDONLY)
    except OSError:
        return False
    try:
        try:
            fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except OSError:
            return True
        fcntl.flock(fd, fcntl.LOCK_UN)
        return False
    finally:
        os.close(fd)


def cpu_pressure(path: str | os.PathLike[str] = PRESSURE_FILE) -> float | None:
    """The `some` line's avg10 of a pressure file, or None when the file is
    missing or unreadable."""
    try:
        text = Path(path).read_text()
    except OSError:
        return None
    m = re.search(r"^some\b.*?\bavg10=([0-9.]+)", text, re.M)
    return float(m.group(1)) if m else None


def parallel(args: list[str]) -> int | None:
    """The `-n` of a `harbor run` line (`-n 4`, `-n4`, `--n-concurrent 4`,
    `--n-concurrent=4`), or None when it has none."""
    for i, a in enumerate(args):
        if a in ("-n", "--n-concurrent") and i + 1 < len(args):
            return int(args[i + 1])
        m = re.fullmatch(r"(?:-n|--n-concurrent=)(\d+)", a)
        if m:
            return int(m.group(1))
    return None


def with_parallel(args: list[str], default: int = MAX_PARALLEL) -> list[str]:
    """`args`, with `-n <default>` added when it has no `-n`."""
    return list(args) if parallel(args) is not None else [*args, "-n", str(default)]


def refusals(args: list[str], *, lock: Path | None = None, pressure_file: str = PRESSURE_FILE,
             max_pressure: float = MAX_CPU_PRESSURE, max_parallel: int = MAX_PARALLEL) -> list[str]:
    """Every reason this run should not start: too many trials at once, the
    gate lock held, the CPU's pressure above the line."""
    out = []
    n = parallel(args)
    if n is not None and n > max_parallel:
        out.append(f"-n {n}: more than {max_parallel} trials at once (a busy host times graders out)")
    if lock_held(lock):
        out.append(f"the gate's lock is held ({lock or lock_path()}): a gate is running on this host")
    p = cpu_pressure(pressure_file)
    if p is not None and p > max_pressure:
        out.append(f"CPU pressure is {p:.2f} (some avg10 of {pressure_file}), above {max_pressure:g}: "
                   "something else is running")
    return out


def reading(pressure_file: str = PRESSURE_FILE) -> dict[str, Any]:
    """What the run's record keeps of the host at the start."""
    p = cpu_pressure(pressure_file)
    return {"cpu_pressure_avg10": p, "pressure_available": p is not None, "max_cpu_pressure": MAX_CPU_PRESSURE,
            "max_parallel": MAX_PARALLEL, "lock_held": lock_held()}
