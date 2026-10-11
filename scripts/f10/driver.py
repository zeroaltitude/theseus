#!/usr/bin/env python3
"""F10, the first ten minutes: the driver (theseus-qy2a).

The same twelve steps for Theseus and for Claude Code, on a scratch copy of
`scripts/f10/project`, in a 120x40 terminal and again at 80x24, typed into a
pane of a private tmux server. After each step the driver captures the screen
(colours kept) and keeps the few facts a screen cannot hold (a key count, a
process alive, a file's bytes, what the session recorded) in `run.json`; then
`checks.py` answers each of the 47 checks and `scorecard.py` writes the table.

    # Theseus, on the stand-in model ($0): a built tree's binaries
    python3 scripts/f10/driver.py theseus --bin-dir target/release --out /tmp/f10-out

    # Claude Code, the real model: LOCALLY ONLY, capped at $1 a run
    python3 scripts/f10/driver.py claude-code --out /tmp/f10-out --expect-version 2.1.296

`--size` picks one size (`120x40` or `80x24`); both by default. The output
directory gets one run directory per arm and size, and `scorecard.md` and
`scorecard.json` over them. docs/benchmarks/f10/README.md says how a report is
written from a run. Standard library only.
"""

import argparse
import hashlib
import json
import os
import shutil
import sys
import tempfile
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import procs  # noqa: E402
import scorecard  # noqa: E402
import steps  # noqa: E402
from tmuxio import Tmux  # noqa: E402

SIZES = ("120x40", "80x24")


class Ctx:
    """One run: the pane, the scratch project, the run directory, and the
    records the checks read."""

    def __init__(self, arm, size, root, out):
        self.arm, self.size = arm, size
        self.cols, self.rows = (int(x) for x in size.split("x"))
        self.root = Path(root)  # the run's scratch: project, state, logs
        self.project = self.root / "project"
        self.out = Path(out)
        (self.out / "screens").mkdir(parents=True, exist_ok=True)
        self.records = {}
        self.meta = {"arm": arm.name, "size": size, "project_dir": str(self.project), "records": self.records}
        self.tmux = None
        self.log = open(self.out / "driver.log", "a")

    def say(self, line):
        stamp = time.strftime("%H:%M:%S")
        print(f"f10 {self.arm.name} {self.size} {stamp} {line}", flush=True)
        self.log.write(f"{stamp} {line}\n")
        self.log.flush()

    def rec(self, step, **kv):
        self.records.setdefault(step, {}).update(kv)

    def capture(self, name):
        screen = self.tmux.capture()
        (self.out / "screens" / f"{name}.ansi").write_text(screen)
        return screen

    def save_screen(self, name, screen):
        (self.out / "screens" / f"{name}.ansi").write_text(screen)

    def sample_client(self):
        """The client's resident set now: the program in front in the pane
        (the shell's child), not a daemon behind it nor the work it runs."""
        pid, rss, comm = procs.client_rss(self.tmux.pane_pid())
        if rss:
            best = self.records.setdefault("B5", {})
            if rss > best.get("client_rss_kb", 0):
                best.update(client_rss_kb=rss, client=f"{comm} (pid {pid})")

    def file_digest(self, name="tides.py"):
        return hashlib.sha256((self.project / name).read_bytes()).hexdigest()

    def survey_pids(self):
        return procs.survey_pids(self.project)

    def write(self):
        (self.out / "run.json").write_text(json.dumps(self.meta, indent=1) + "\n")


def wait_until(pred, timeout, every=0.2):
    end = time.monotonic() + timeout
    while True:
        v = pred()
        if v:
            return v
        if time.monotonic() > end:
            return None
        time.sleep(every)


def run_steps(ctx):
    """The twelve steps, in order, each through the arm; a step that fails is
    logged and the run goes on, so one broken step costs only its checks."""
    arm = ctx.arm
    for step in ("s1", "s2", "s3", "s4", "s5", "s6", "s7", "s8", "s9", "s10", "s11", "s12"):
        ctx.say(f"{step.upper()} begins")
        try:
            getattr(arm, step)(ctx)
        except Exception as e:  # a broken step is a finding of the run, not the driver's end
            ctx.say(f"{step.upper()} broke: {type(e).__name__}: {e}")
            ctx.rec(step.upper(), broke=f"{type(e).__name__}: {e}")
            try:
                ctx.capture(f"{step.upper()}.broke")
            except Exception:
                pass
        ctx.sample_client()
        ctx.write()


def one_run(arm, size, out_root, keep):
    out = Path(out_root) / f"{arm.name}-{size}"
    if out.exists():
        shutil.rmtree(out)
    root = Path(tempfile.mkdtemp(prefix=f"f10-{arm.name}-{size}-"))
    ctx = Ctx(arm, size, root, out)
    shutil.copytree(HERE / "project", ctx.project, ignore=shutil.ignore_patterns("__pycache__"))
    try:
        env = arm.setup(ctx)
        ctx.tmux = Tmux(ctx.cols, ctx.rows, ctx.project, env=env)
        ctx.meta["tmux_socket"] = ctx.tmux.name
        run_steps(ctx)
    finally:
        try:
            arm.teardown(ctx)
        finally:
            if ctx.tmux:
                ctx.tmux.kill()
            procs.stop_all(ctx.survey_pids())
            ctx.write()
            if not keep:
                shutil.rmtree(root, ignore_errors=True)
    return out


def main(argv=None):
    ap = argparse.ArgumentParser(description="F10's driver: the first ten minutes, scripted (theseus-qy2a).")
    ap.add_argument("arm", choices=("theseus", "claude-code"))
    ap.add_argument("--out", required=True, help="where the runs and the scorecard go")
    ap.add_argument("--size", action="append", choices=SIZES, help="a size to run (both by default)")
    ap.add_argument("--bin-dir", default="target/release", help="theseus: where theseus, theseusd, theseus-tui and theseus-sim are")
    ap.add_argument("--claude", default="claude", help="claude-code: the CLI to run")
    ap.add_argument("--expect-version", help="claude-code: refuse any other version (the pin)")
    ap.add_argument("--budget-usd", type=float, default=1.0, help="claude-code: stop the run past this spend (default 1)")
    ap.add_argument("--keep", action="store_true", help="keep each run's scratch directory")
    a = ap.parse_args(argv)
    if a.arm == "theseus":
        from arm_theseus import Theseus

        arm = Theseus(Path(a.bin_dir).resolve())
    else:
        from arm_claude import ClaudeCode

        arm = ClaudeCode(a.claude, a.expect_version, a.budget_usd)
    outs = [one_run(arm, size, a.out, a.keep) for size in (a.size or SIZES)]
    scorecard.main([str(o) for o in outs] + ["--out", str(Path(a.out) / f"scorecard-{arm.name}")])
    print((Path(a.out) / f"scorecard-{arm.name}.md").read_text())
    return 0


if __name__ == "__main__":
    sys.exit(main())
