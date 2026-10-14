#!/usr/bin/env python3
"""The async bench's `interrupt` and `cancel` families on Theseus, with a
scripted stand-in model (theseus-qu1lc). Standard library only.

A message sent while Theseus works must be heard while it works. The real
model waits on a long job with `job_wait`, 60 s at a time and again, and
before the steer a message queued behind that turn was answered only when
the turn ended (157 s in the `interrupt` family, 182 s in `cancel`). This
runs both families' shape on `theseus-sim fake-model --rules`, so it costs
nothing and every run sees the same model: the stand-in starts the long job in
the foreground (`proc_run`, which goes to the background at `proc_sync_secs`),
then calls `job_wait` again and again until the job ends. At the injection's
time the driver sends the family's message to the same session. The numbers:

- `answer_s`: from the message's send to the model's answer to it in the
  session's transcript (the interrupt family's ticket count; the cancel
  family's "stopped");
- `gone_s` (cancel): from the send to the job's process being gone.

`--arm after` is the daemon as it ships (`[kernel] steer_queued = true`);
`--arm before` is the same binary with the switch off, which is how a message
queued before the steer. The real-model run is the Harbor one (README.md), not
this: the stand-in answers instantly, so the numbers here are the daemon's
alone.

    python3 bench/async/steer_stand_in.py --bin-dir target/release-thin \\
        --family interrupt --arm before --arm after --inject-at 20 --inject-at 90

`--scale full` runs the bench's own timings (a 150 s job, `proc_sync_secs`
60, injections at 20 s and, in a `job_wait`, at 90 s); the default `quick`
runs a 40 s job, 10 s syncs, injections at 3 s and 17 s.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import time
from pathlib import Path

REPLY = {
    "interrupt": "There are 12 open tickets.",
    "cancel": "Stopped the migration; nothing of it is left running.",
}
ASK = {
    "interrupt": "Reply with how many support tickets are open now.",
    "cancel": "Cancel the migration now and leave nothing of it running.",
}
TASK = "Run the long job and tell me when it is done."
MARK = "steerbench-job"

SCALES = {
    "quick": {"job": 40, "sync": 10, "wait": 10, "inject": [3.0, 17.0]},
    "full": {"job": 150, "sync": 60, "wait": 60, "inject": [20.0, 90.0]},
}


def free_port() -> int:
    s = socket.socket()
    s.bind(("127.0.0.1", 0))
    port = s.getsockname()[1]
    s.close()
    return port


def rules(job: int, wait: int, cwd: str) -> list[dict]:
    """The stand-in: the long job, then `job_wait` until it ends; the family's
    message is answered whenever it is the last thing the model reads."""
    waits = job // wait + 3
    steps = [{"calls": [{"name": "proc_run", "input": {
        "argv": ["bash", "-c", f"exec -a {MARK} sleep {job}"], "cwd": cwd,
        "timeout_secs": job + 60}}]}]
    steps += [{"calls": [{"name": "job_wait", "input": {"job": "j1", "timeout_secs": wait}}]}] * waits
    steps.append({"text": "The job finished."})
    return [
        # A message that joins a running turn arrives with that turn's tool results, which the
        # stand-in reads as the answer to its calls: so each message's rule asks for one call
        # (`repeat`) and then says its text, whether the message came alone or joined the turn.
        {"when": ASK["interrupt"], "repeat": 1, "text": REPLY["interrupt"],
         "calls": [{"name": "job_read", "input": {"job": "j1", "lines": 1}}]},
        {"when": ASK["cancel"], "repeat": 1, "text": REPLY["cancel"],
         "calls": [{"name": "job_stop", "input": {"job": "j1"}}]},
        {"when": TASK, "steps": steps},
    ]


def config(state: Path, sock: Path, work: Path, base: str, sync: int, steer: bool) -> str:
    return f"""# the steer bench's scratch daemon: only what differs from the defaults.
[model]
live = "bench"
api_base = "{base}"
api_key_secret = "anthropic_api_key"

[profiles.bench]
provider = "anthropic"
model = "claude-sonnet-5-5"

[secrets]
anthropic_api_key = "env:STEER_STAND_IN_KEY"

[server]
state_dir = "{state}"
socket = "{sock}"
disk_warn_mb = 0
disk_floor_mb = 0

[tools]
projects_dir = "{work}"
roots = ["{work}"]
proc_sync_secs = {sync}

[policy]
enforcement = "open"

[kernel]
steer_queued = {str(steer).lower()}

[discord]
enabled = false

[web]
enabled = false

[index]
enabled = false

[judge]
enabled = false

[memory]
mode = "off"
"""


class Run:
    def __init__(self, bin_dir: Path, arm: str, family: str, scale: dict, inject: float, root: Path):
        self.bin = bin_dir
        self.arm, self.family, self.scale, self.inject = arm, family, scale, inject
        self.root = root
        self.procs: list[subprocess.Popen] = []
        self.sock = root / "theseus.sock"
        self.env = dict(os.environ, STEER_STAND_IN_KEY="stand-in", TZ="UTC",
                        THESEUS_SOCKET=str(self.sock), XDG_STATE_HOME=str(root / "xdg"))

    def cli(self, *args, timeout=30):
        work = self.root / "work"
        return subprocess.run([self.bin / "theseus", "--socket", str(self.sock), *args],
                              capture_output=True, text=True, env=self.env, timeout=timeout,
                              cwd=work if work.exists() else None)

    def start(self):
        r = self.root
        work, state = r / "work", r / "state"
        work.mkdir()
        port = free_port()
        (r / "rules.json").write_text(json.dumps(rules(self.scale["job"], self.scale["wait"], str(work))))
        cfg = r / "theseus.toml"
        cfg.write_text(config(state, self.sock, work, f"http://127.0.0.1:{port}",
                              self.scale["sync"], self.arm == "after"))
        self.procs.append(subprocess.Popen(
            [self.bin / "theseus-sim", "fake-model", "--addr", f"127.0.0.1:{port}",
             "--rules", r / "rules.json"], stdout=open(r / "model.log", "w"), stderr=subprocess.STDOUT))
        self.daemon = subprocess.Popen(
            [self.bin / "theseusd", "--config", str(cfg), "--socket", str(self.sock),
             "--state-dir", str(state)], env=self.env, stdout=open(r / "theseusd.log", "w"),
            stderr=subprocess.STDOUT, start_new_session=True)
        for _ in range(400):
            if self.cli("health").returncode == 0:
                return
            time.sleep(0.05)
        raise SystemExit(f"theseusd did not answer; see {r}/theseusd.log")

    def stop(self):
        try:
            self.cli("shutdown", timeout=20)
            self.daemon.wait(20)
        except Exception:
            os.killpg(self.daemon.pid, signal.SIGKILL)
        for p in self.procs:
            p.terminate()
        # Only the pids this run's marker names.
        for pid in subprocess.run(["pgrep", "-f", MARK], capture_output=True, text=True).stdout.split():
            try:
                os.kill(int(pid), signal.SIGKILL)
            except OSError:
                pass

    def session(self) -> str | None:
        r = self.cli("--json", "sessions")
        try:
            v = json.loads(r.stdout)
        except ValueError:
            return None
        rows = v.get("sessions", v) if isinstance(v, dict) else v
        for s in rows or []:
            if isinstance(s, dict) and (s.get("session_id") or s.get("id")):
                return s.get("session_id") or s.get("id")
        return None

    def said(self, sid: str, text: str) -> bool:
        r = self.cli("--json", "history", sid)
        return text in r.stdout

    def job_alive(self) -> bool:
        return subprocess.run(["pgrep", "-f", MARK], capture_output=True).returncode == 0

    def measure(self) -> dict:
        reply = REPLY[self.family]
        work = self.root / "work"
        task = subprocess.Popen([self.bin / "theseus", "--socket", str(self.sock), "ask", TASK],
                                env=self.env, cwd=work, stdout=subprocess.DEVNULL,
                                stderr=open(self.root / "ask-task.log", "w"))
        sid = None
        t_task = time.monotonic()
        while sid is None and time.monotonic() - t_task < 30:
            sid = self.session()
            time.sleep(0.1)
        if sid is None:
            return {"error": "no session"}
        # The job starts a model call after the task; the injection counts from the job.
        while not self.job_alive() and time.monotonic() - t_task < 30:
            time.sleep(0.05)
        started = time.monotonic()
        while time.monotonic() - started < self.inject:
            time.sleep(0.05)
        t0 = time.monotonic()
        msg = subprocess.Popen(
            [self.bin / "theseus", "--socket", str(self.sock), "ask", "--session", sid, ASK[self.family]],
            env=self.env, cwd=work, stdout=subprocess.DEVNULL, stderr=open(self.root / "ask-msg.log", "w"))
        out: dict = {"arm": self.arm, "family": self.family, "inject_at_s": self.inject,
                     "job_s": self.scale["job"]}
        limit = self.scale["job"] * 2 + 60
        while time.monotonic() - t0 < limit:
            if "answer_s" not in out and self.said(sid, reply):
                out["answer_s"] = round(time.monotonic() - t0, 2)
            if self.family == "cancel" and "gone_s" not in out and not self.job_alive():
                out["gone_s"] = round(time.monotonic() - t0, 2)
            done = "answer_s" in out and (self.family != "cancel" or "gone_s" in out)
            if done:
                break
            time.sleep(0.1)
        for p in (task, msg):
            if p.poll() is None:
                p.terminate()
        return out


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--bin-dir", required=True, type=Path, help="holds theseus, theseusd and theseus-sim")
    ap.add_argument("--family", choices=("interrupt", "cancel"), action="append")
    ap.add_argument("--arm", choices=("before", "after"), action="append")
    ap.add_argument("--scale", choices=tuple(SCALES), default="quick")
    ap.add_argument("--inject-at", type=float, action="append", help="seconds after the job starts")
    ap.add_argument("--out", type=Path, help="write the results as JSON here")
    ap.add_argument("--keep", action="store_true", help="keep each run's directory (logs, rules, the store)")
    a = ap.parse_args()
    scale = SCALES[a.scale]
    injects = a.inject_at or scale["inject"]
    results = []
    for family in a.family or ["interrupt", "cancel"]:
        for arm in a.arm or ["before", "after"]:
            for inject in injects:
                d = tempfile.mkdtemp(prefix="steer-")
                run = Run(a.bin_dir.resolve(), arm, family, scale, inject, Path(d))
                try:
                    run.start()
                    r = run.measure()
                finally:
                    run.stop()
                if a.keep:
                    r["dir"] = d
                else:
                    shutil.rmtree(d, ignore_errors=True)
                print(json.dumps(r), flush=True)
                results.append(r)
    if a.out:
        a.out.write_text(json.dumps(results, indent=1) + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
