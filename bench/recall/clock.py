"""The clock the runner pins each session to (theseus-kyrgn).

The progression narrates a dated calendar ("It's Sunday, 2026-11-15.") and no arm's CLI takes a clock, so
the arm's system prompt and its tools' `date` read the machine's own: Mon 2026-10-26 in the 10-26 seeds.
The model noticed, said so, and was graded confident-wrong for preferring the machine: 22 of the 32
confident-wrong probes, and `date` recall at 0.367. The narration is the thing under test, so the repair
is to make the machine agree with it: libfaketime, preloaded into every process the arm starts, reads its
time from a one-line file (`@YYYY-MM-DD HH:MM:SS`, ticking from there) that the runner rewrites when a
session opens. The runner then reads the date back from a fresh process under the same environment and
refuses to run a session whose machine date is not the narrated one.

`mode`: `pin` requires the library, `off` never pins, `auto` (the default) pins when the library is found
and otherwise runs unpinned and says so: `run.json`'s `clock` records which, and the scorer sets the date
probes of an unpinned run aside (`score.py`).

A long-lived daemon ticks from the pin as of its first clock read, so a run that lasts past the pinned
day's remaining hours would roll past midnight and read another date: `enter` refuses it.
"""

from __future__ import annotations

import datetime as dt
import os
import subprocess
import sys
import time
from pathlib import Path
from typing import Callable

import progression as pg

LIB_ENV = "LIBFAKETIME_LIB"
LIB_PATHS = (
    "/usr/lib/x86_64-linux-gnu/faketime/libfaketime.so.1",
    "/usr/lib/aarch64-linux-gnu/faketime/libfaketime.so.1",
    "/usr/lib/faketime/libfaketime.so.1",
    "/usr/local/lib/faketime/libfaketime.so.1",
)
# From 08:00, sixteen hours to midnight, less an hour's margin.
MAX_RUN_S = 15 * 3600
MODES = ("auto", "pin", "off")


class ClockDrift(RuntimeError):
    """The machine's date is not the date the progression narrates."""


def find_lib(explicit: str | os.PathLike | None = None) -> str | None:
    """The libfaketime to preload: `explicit` (taken at its word: a path that is not there is None, never
    another library), else `$LIBFAKETIME_LIB`, else the distribution's."""
    if explicit:
        return str(explicit) if Path(explicit).is_file() else None
    for cand in (os.environ.get(LIB_ENV), *LIB_PATHS):
        if cand and Path(cand).is_file():
            return str(cand)
    return None


def machine_date(env: dict) -> str:
    """The date a fresh process under `env` reads: the machine's, as the arm's tools would."""
    r = subprocess.run(["date", "+%F"], env=env, capture_output=True, text=True, timeout=20)
    if r.returncode != 0:
        raise ClockDrift(f"`date` failed under the pinned environment: {r.stderr.strip()}")
    return r.stdout.strip()


def pin_env(base: dict, lib: str, spec: Path) -> dict:
    """`base` with the pin in it: the library preloaded ahead of any other, and the spec file."""
    out = dict(base)
    out["LD_PRELOAD"] = lib + (":" + base["LD_PRELOAD"] if base.get("LD_PRELOAD") else "")
    out["FAKETIME_TIMESTAMP_FILE"] = str(spec)
    out["FAKETIME_NO_CACHE"] = "1"
    return out


def _host_date() -> str:
    return time.strftime("%F")


def smoke(pin: str, lib: str | None = None, workdir: Path | None = None,
          runner: Callable[[dict], str] | None = None, host: Callable[[], str] = _host_date) -> dict:
    """The pin at smoke level, with a control: read the date of a fresh process under the pin and of the
    same process without it. `pin` (`YYYY-MM-DD`) must be at least a month from the host's date, so a pin
    that did nothing cannot read as one that worked. Raises `ClockDrift` when the pinned arm does not read
    the pin or the control does, and `ValueError` when the pin is too near the real date to tell."""
    lib = lib or find_lib()
    if lib is None:
        raise SystemExit("smoke: no libfaketime found (set LIBFAKETIME_LIB or pass --faketime-lib)")
    runner = runner if runner is not None else machine_date
    real = host()
    if abs((dt.date.fromisoformat(pin) - dt.date.fromisoformat(real)).days) < 30:
        raise ValueError(f"the pin {pin} is within a month of the host's {real}: pick a date far from today")
    workdir = Path(workdir) if workdir else Path.cwd()
    workdir.mkdir(parents=True, exist_ok=True)
    spec = workdir / "smoke.faketime.spec"
    spec.write_text(f"@{pin} {pg.CLOCK_TIME}\n")
    base = {"PATH": os.environ.get("PATH", "")}
    pinned = runner(pin_env(base, lib, spec))
    control = runner(dict(base))
    if pinned != pin:
        raise ClockDrift(f"the pinned arm reads {pinned}, not the pin {pin}: libfaketime did not take")
    if control == pin:
        raise ClockDrift(f"the control arm (no pin) also reads {pin}: the pin is not what moved the clock")
    return {"pin": pin, "lib": lib, "host": real, "pinned": pinned, "control": control,
            "proof": pinned == pin and control != pin}


class Clock:
    def __init__(self, prog: pg.Progression, mode: str = "auto", lib: str | None = None,
                 workdir: Path | None = None, runner: Callable[[dict], str] | None = None,
                 now: Callable[[], float] = time.monotonic):
        if mode not in MODES:
            raise ValueError(f"the clock mode is one of {', '.join(MODES)}, not {mode!r}")
        self.prog, self.plan, self.now = prog, pg.clock_plan(prog), now
        self.runner = runner if runner is not None else (lambda env: machine_date(env))
        self.lib, self.file, self.t0 = lib, None, now()
        reason = None
        if mode == "off":
            reason = "--clock off: the arm reads the machine's own clock"
        elif lib is None:
            reason = ("no libfaketime found (set LIBFAKETIME_LIB or pass --faketime-lib): the arm reads the "
                      "machine's own clock")
            if mode == "pin":
                raise SystemExit(f"drive: --clock pin needs libfaketime, and {reason}")
            print(f"drive: WARNING {reason}; the run's date probes will be set aside by the scorer",
                  file=sys.stderr)
        self.pinned = reason is None
        self.meta = {"mode": mode, "pinned": self.pinned, "lib": lib if self.pinned else None,
                     "time": pg.CLOCK_TIME, "reason": reason, "sessions": []}
        if self.pinned:
            workdir = Path(workdir) if workdir else Path.cwd()
            workdir.mkdir(parents=True, exist_ok=True)
            self.file = workdir / "faketime.spec"
            self._write(0)  # a process started before the first session already has a clock to read

    def _write(self, session: int) -> None:
        tmp = self.file.with_name(self.file.name + ".tmp")
        tmp.write_text(f"@{self.plan[session]}\n")
        os.replace(tmp, self.file)

    def env(self, base: dict) -> dict:
        """`base` with the pin in it: the library preloaded ahead of any other, and the spec file."""
        return pin_env(base, self.lib, self.file) if self.pinned else dict(base)

    def enter(self, session: int) -> dict:
        """Pin the clock to `session`'s narrated date and read it back. Raises `ClockDrift` when the
        machine disagrees, or when the run has outlived the pinned day."""
        narrated = self.prog.sessions[session].date
        rec = {"session": session, "narrated": narrated, "machine": None}
        if self.pinned:
            if session and self.now() - self.t0 > MAX_RUN_S:
                raise ClockDrift(f"session {session}: the run has lasted past {MAX_RUN_S // 3600} hours, "
                                 f"so a clock ticking from {pg.CLOCK_TIME} would roll past midnight and "
                                 f"read another date than {narrated}")
            self._write(session)
            machine = self.runner(self.env({"PATH": os.environ.get("PATH", "")}))
            rec["machine"] = machine
            if machine != narrated:
                raise ClockDrift(f"session {session}: the machine clock reads {machine} but the progression "
                                 f"narrates {narrated}; the model would be graded for believing the machine")
        self.meta["sessions"].append(rec)
        return rec


def main(argv: list[str] | None = None) -> int:
    import argparse
    ap = argparse.ArgumentParser(description="Smoke-prove the clock pin: a pinned arm, an unpinned control, the host.")
    ap.add_argument("--smoke", metavar="YYYY-MM-DD", required=True, help="the date to pin to (far from today)")
    ap.add_argument("--faketime-lib", default=None)
    a = ap.parse_args(argv)
    import tempfile
    with tempfile.TemporaryDirectory() as t:
        r = smoke(a.smoke, lib=find_lib(a.faketime_lib), workdir=Path(t))
    print(f"host    date: {r['host']}\ncontrol date: {r['control']}   (same path, no pin)\n"
          f"pinned  date: {r['pinned']}   (pin {r['pin']}, {r['lib']})\nproof: {r['proof']}")
    return 0 if r["proof"] else 1


if __name__ == "__main__":
    sys.exit(main())
