"""Part 4 of the release smoke: kill-and-resume, at $0.

A scratch daemon on its own state directory and socket, driven by the stand-in
model (`theseus-sim fake-model`), runs one scripted six-step task: step k is a
`proc_run` that appends k to `starts`, then (steps 2, 4 and 6) waits for a gate
file, then appends k to `effects`. At each gated step the driver waits for the
step's start, SIGKILLs the daemon while the tool runs, opens the gate, waits
for the job (its wrapper outlives the daemon) to leave its effect, and starts
the daemon again on the same state: startup requeues the interrupted turn and
resumes it with the job's own settled result (the kernel's startup,
toolrun/resume.rs; steps.rs' a_restart_with_a_step_running_settles_the_call).
So every kill lands mid-tool by construction, never on a timer. The gate opens
before the restart, not after, because a job still running at the restart is
adopted as background and its late result comes back as a message of its own,
which the stand-in, matching a turn's last text, cannot follow.

Each step also prints `done-k`, and the stand-in keeps every request it gets
(`--dump`), so the driver checks that the job's own output, not "the outcome
cannot be established", reached the model after each restart: the stand-in
picks its next step by counting tool results and never reads them, so without
this a resume that lost every result would pass. This covers the
settled-while-down path of the resume; a job still running at the restart
(adopted as background) is not covered here.

The record (`--out`): `kills`, `resumed` (kills after which the task went on:
the next step started, or after the last, the turn ended), `starts` and
`effects` (the lines on disk: a step run twice shows twice), `steps`,
`settled`, `seen` (the `done-k` the model saw), and from the scratch ledger
`interrupted_rows`, `turn_ended`, `turn_failed`, `unknown_rows` and
`succeeded_rows`; `wall_s`.

    python3 bench/smoke/resume.py --bin-dir DIR --sim PATH --work DIR --out FILE

Standard library only; Python >= 3.11.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import signal
import subprocess
import sys
import time
import tomllib
from pathlib import Path
from typing import Any

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
sys.path.insert(0, str(REPO / "bench" / "recall"))
from drive import toml_dumps  # noqa: E402

STEPS = 6
KILLED = (2, 4, 6)
PROMPT = "release smoke: run the six steps"
WAIT_S = 60.0
# A gate a driver never opens ends on its own (1200 x 0.05 s), so no job outlives a dead driver for long.
GATE_TICKS = 1200


def rules(work: Path) -> list[dict[str, Any]]:
    steps = []
    for k in range(1, STEPS + 1):
        gate = (f"n=0; while [ ! -e {work}/go-{k} ] && [ $n -lt {GATE_TICKS} ]; do sleep 0.05; n=$((n+1)); done; "
                if k in KILLED else "")
        script = f"echo {k} >> {work}/starts; {gate}echo {k} >> {work}/effects; echo done-{k}"
        steps.append({"calls": [{"name": "proc_run", "input": {"argv": ["sh", "-c", script], "timeout_secs": 600}}]})
    steps.append({"text": "All six steps done."})
    return [{"when": PROMPT, "steps": steps}, {"when": "hello", "text": "Hello."}]


def config(work: Path, api_base: str) -> dict[str, Any]:
    """The bench profile (no vault, every tool open), pointed at the stand-in, with the judge off."""
    with open(REPO / "bench" / "theseus-bench.toml", "rb") as f:
        t = tomllib.load(f)
    t["model"]["api_base"] = api_base
    for p in t.get("providers", {}).values():
        p["api_base"] = api_base
    t["tools"]["projects_dir"] = str(work)
    t["tools"]["proc_sync_secs"] = 120
    t["judge"] = {"enabled": False}
    return t


def seen(dump: Path) -> list[int]:
    """The steps whose `done-k` reached the model, from the requests the stand-in kept."""
    found: set[int] = set()
    for f in dump.rglob("*"):
        if f.is_file():
            found.update(int(m) for m in re.findall(r"done-(\d)", f.read_text(errors="replace")))
    return sorted(found)


def lines(path: Path) -> list[int]:
    return [int(x) for x in path.read_text().split()] if path.exists() else []


def until(cond, timeout: float = WAIT_S) -> bool:
    end = time.monotonic() + timeout
    while time.monotonic() < end:
        if cond():
            return True
        time.sleep(0.05)
    return cond()


class Scratch:
    def __init__(self, bin_dir: Path, work: Path, api_base: str):
        self.bin, self.work = bin_dir, work
        self.state, self.sock = work / "state", work / "sock"
        self.cfg = work / "config.toml"
        self.cfg.write_text(toml_dumps(config(work / "projects", api_base)))
        (work / "projects").mkdir(exist_ok=True)
        drop = ("THESEUS_CONFIG", "THESEUS_STATE_DIR", "THESEUS_SOCKET", "THESEUS_SESSION")
        self.env = {k: v for k, v in os.environ.items() if k not in drop}
        self.env["ANTHROPIC_API_KEY"] = "stand-in"  # the stand-in model reads none
        self.env["XDG_STATE_HOME"] = str(work / "xdg-state")
        self.proc: subprocess.Popen | None = None

    def cli(self, *args: str, timeout: float = 30.0) -> subprocess.CompletedProcess:
        return subprocess.run([str(self.bin / "theseus"), "--socket", str(self.sock), *args],
                              capture_output=True, text=True, timeout=timeout, env=self.env)

    def start(self) -> None:
        log = (self.work / "theseusd.log").open("a")
        self.proc = subprocess.Popen(
            [str(self.bin / "theseusd"), "--config", str(self.cfg), "--state-dir", str(self.state),
             "--socket", str(self.sock)],
            stdin=subprocess.DEVNULL, stdout=log, stderr=log, env=self.env, start_new_session=True)

        def serving() -> bool:
            if self.proc.poll() is not None:
                raise RuntimeError(f"theseusd exited {self.proc.returncode}: see {self.work}/theseusd.log")
            try:
                return self.cli("health", timeout=10).returncode == 0
            except subprocess.TimeoutExpired:
                return False
        if not until(serving, 60):
            raise RuntimeError(f"theseusd did not serve in 60 s: see {self.work}/theseusd.log")

    def kill(self) -> None:
        self.proc.send_signal(signal.SIGKILL)
        self.proc.wait(timeout=30)

    def stop(self) -> None:
        if self.proc and self.proc.poll() is None:
            try:
                self.cli("shutdown", timeout=30)
                self.proc.wait(timeout=30)
            except subprocess.TimeoutExpired:
                self.kill()


def start_model(sim: Path, work: Path) -> tuple[subprocess.Popen, str]:
    (work / "rules.json").write_text(json.dumps(rules(work)))
    p = subprocess.Popen([str(sim), "fake-model", "--addr", "127.0.0.1:0", "--rules", str(work / "rules.json"),
                          "--log", str(work / "fake-model.log"), "--dump", str(work / "requests")],
                         stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True, start_new_session=True)
    first = p.stdout.readline().strip()
    if not first.startswith("fake model on "):
        p.kill()
        raise RuntimeError(f"fake-model said {first!r}")
    return p, first.removeprefix("fake model on ")


def count(d: "Scratch", session: str, kind: str, producer: str | None = None) -> int | None:
    """The session's ledger rows of `kind` (whose producer starts with `producer`, when named: a job's
    wrapper is `wrapper:<pid>`, a model call `provider:<name>`); None when the ledger could not be read."""
    r = d.cli("--json", "ledger", "-n", "1000", "-s", session, "-k", kind, timeout=30)
    if r.returncode != 0:
        return None
    rows = json.loads(r.stdout).get("rows", [])
    if producer:
        rows = [x for x in rows if str((x.get("data") or {}).get("producer", "")).startswith(producer)]
    return len(rows)


def run(bin_dir: Path, sim: Path, work: Path) -> dict[str, Any]:
    work = work.resolve()
    work.mkdir(parents=True, exist_ok=True)
    t0 = time.monotonic()
    model, base = start_model(sim, work)
    d = Scratch(bin_dir, work, base)
    rec: dict[str, Any] = {"kills": 0, "resumed": 0, "steps": STEPS, "settled": False, "notes": []}
    asker = None
    try:
        d.start()
        hello = d.cli("--json", "ask", "hello", timeout=60)
        session = json.loads(hello.stdout)["session_id"]
        rec["session"] = session
        asker = subprocess.Popen([str(bin_dir / "theseus"), "--socket", str(d.sock), "--json", "ask", "-s", session,
                                  PROMPT], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, env=d.env)
        starts, effects = work / "starts", work / "effects"
        for k in KILLED:
            if not until(lambda: k in lines(starts)):
                rec["notes"].append(f"step {k} never started")
                break
            d.kill()
            rec["kills"] += 1
            (work / f"go-{k}").touch()
            if not until(lambda: k in lines(effects)):
                rec["notes"].append(f"step {k}'s job left no effect while the daemon was down")
            d.start()
            if k < STEPS:
                went_on = lambda: k + 1 in lines(starts)  # noqa: E731
            else:
                went_on = lambda: bool(count(d, session, "turn.ended"))  # noqa: E731
            if until(went_on):
                rec["resumed"] += 1
            else:
                rec["notes"].append(f"no progress after the kill at step {k}")
                break
        w = d.cli("--json", "wait", session, "--until", "settled", "--timeout", "90s", timeout=120)
        rec["settled"] = w.returncode == 0
        # Let any late result land before counting what is on disk.
        time.sleep(3)
        rec["interrupted_rows"] = count(d, session, "execution.interrupted")
        rec["turn_ended"] = count(d, session, "turn.ended")
        rec["turn_failed"] = count(d, session, "turn.failed")
        rec["unknown_rows"] = count(d, session, "action.outcome_unknown")
        rec["succeeded_rows"] = count(d, session, "action.succeeded", producer="wrapper:")
        rec["seen"] = seen(work / "requests")
        rec["starts"], rec["effects"] = lines(starts), lines(effects)
    finally:
        for k in KILLED:
            (work / f"go-{k}").touch()
        if asker and asker.poll() is None:
            asker.kill()
        d.stop()
        model.kill()
    rec.setdefault("starts", lines(work / "starts"))
    rec.setdefault("effects", lines(work / "effects"))
    rec["wall_s"] = round(time.monotonic() - t0, 1)
    return rec


def main(argv: list[str]) -> int:
    ap = argparse.ArgumentParser(prog="resume.py", description=__doc__.split("\n\n")[0])
    ap.add_argument("--bin-dir", required=True, type=Path, help="where theseus and theseusd are")
    ap.add_argument("--sim", required=True, type=Path, help="theseus-sim, for its fake-model")
    ap.add_argument("--work", required=True, type=Path)
    ap.add_argument("--out", required=True, type=Path)
    a = ap.parse_args(argv)

    def stop(signum, frame):  # a SIGTERM or SIGHUP runs the cleanup in `run`'s finally, as Ctrl-C does
        raise KeyboardInterrupt
    signal.signal(signal.SIGTERM, stop)
    signal.signal(signal.SIGHUP, stop)
    try:
        rec = run(a.bin_dir, a.sim, a.work)
    except (RuntimeError, OSError, ValueError, subprocess.TimeoutExpired, KeyboardInterrupt) as e:
        rec = {"kills": 0, "resumed": 0, "steps": STEPS, "settled": False, "starts": [], "effects": [],
               "seen": [], "notes": [f"the driver failed: {e!r}"], "wall_s": 0.0}
    a.out.write_text(json.dumps(rec, indent=1) + "\n")
    print(f"resume: {rec['kills']} kills, {rec['resumed']} resumed, effects {rec['effects']}"
          + (f"; {'; '.join(rec['notes'])}" if rec["notes"] else ""))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
