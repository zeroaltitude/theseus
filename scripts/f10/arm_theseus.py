"""F10's Theseus arm (theseus-qy2a): a scratch `theseusd` on the scripted
stand-in model (`theseus-sim fake-model --rules`, so a run costs $0), driven
the way a person new to it would, through the clients it ships.

How each step reaches Theseus (the rules the baseline was measured by):
- S1 types the bare `theseus`. Where no conversation opens, the driver opens
  one the documented way, `theseus ask "<S2's question>"`, and from S3 on uses
  the terminal UI, `theseus tui`: enter opens the session, `i` the input
  line, `y` answers a question.
- S7's interrupt is Ctrl-C, the key a person presses in a terminal; the stop
  that B3 verifies is the TUI's own (`s`, then `s`).
- An arm with no key or command for a step (S5's mode, S9's undo, S11's
  reopen and picker, S12's plan mode) records none, and its checks say so.

The daemon's config is the defaults, with the stand-in as the model, the
scratch project as its workspace, Discord, the web UI, the index and the
judge off, and `proc.run` and the file writers asking first (the template's
own advice for the shell), so the approval steps have a question to answer.
"""

import json
import os
import re
import signal
import socket
import subprocess
import time
from pathlib import Path

import steps
from driver import wait_until

SHELL_PROMPT = re.compile(r"^\$\s*$")


def free_port():
    s = socket.socket()
    s.bind(("127.0.0.1", 0))
    port = s.getsockname()[1]
    s.close()
    return port


def config_text(state, sock, project, model_base):
    return f"""# F10's scratch daemon (scripts/f10/arm_theseus.py): only what differs from the defaults.
[model]
live = "f10"
api_base = "{model_base}"
api_key_secret = "anthropic_api_key"

[profiles.f10]
provider = "anthropic"
model = "{steps_model()}"

[secrets]
anthropic_api_key = "env:F10_STANDIN_KEY"

[server]
state_dir = "{state}"
socket = "{sock}"
disk_warn_mb = 0
disk_floor_mb = 0

[tools]
projects_dir = "{project}"
roots = ["{project}"]
proc_sync_secs = 5

[policy.tools]
"proc.run" = "approve"
"fs.edit" = "approve"
"fs.write" = "approve"
"fs.patch" = "approve"

[discord]
enabled = false

[web]
enabled = false

[index]
enabled = false

[judge]
enabled = false
"""


def steps_model():
    return "claude-sonnet-5-5"


class Theseus:
    name = "theseus"

    def __init__(self, bin_dir):
        self.bin = Path(bin_dir)
        for b in ("theseus", "theseusd", "theseus-tui", "theseus-sim"):
            if not (self.bin / b).exists():
                raise SystemExit(f"f10: no {b} in {self.bin} (build it: scripts/build.sh --profile release-thin)")
        self.procs = []
        self.daemon = None

    # The run's frame

    def setup(self, ctx):
        r = ctx.root
        self.state, self.sock = r / "state", r / "theseus.sock"
        self.cfg, self.rules = r / "theseus.toml", r / "rules.json"
        self.xdg = r / "xdg"
        self.xdg.mkdir()
        port = free_port()
        self.rules.write_text(json.dumps(steps.stand_in_rules(str(ctx.project)), indent=1))
        self.cfg.write_text(config_text(self.state, self.sock, ctx.project, f"http://127.0.0.1:{port}"))
        self.env = {
            "PATH": f"{self.bin}:{os.environ.get('PATH', '/usr/bin:/bin')}",
            "THESEUS_SOCKET": str(self.sock),
            "THESEUS_CONFIG": str(self.cfg),
            "XDG_STATE_HOME": str(self.xdg),
            "TZ": "UTC",
            "TERM": "xterm-256color",
        }
        fake = subprocess.Popen(
            [self.bin / "theseus-sim", "fake-model", "--addr", f"127.0.0.1:{port}", "--rules", self.rules],
            stdout=open(r / "fake-model.log", "w"), stderr=subprocess.STDOUT)
        self.procs.append(fake)
        self.start_daemon(ctx)
        version = subprocess.run([self.bin / "theseus", "--version"], capture_output=True, text=True).stdout.strip()
        ctx.meta.update(version=version, model=steps_model())
        return self.env

    def start_daemon(self, ctx):
        env = dict(os.environ, F10_STANDIN_KEY="f10-stand-in", **self.env)
        log = open(ctx.root / "theseusd.log", "a")
        self.daemon = subprocess.Popen(
            [self.bin / "theseusd", "--config", self.cfg, "--socket", self.sock, "--state-dir", self.state],
            env=env, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
        if not wait_until(lambda: self.cli("health").returncode == 0, 20):
            raise RuntimeError(f"theseusd did not answer; see {ctx.root}/theseusd.log")

    def stop_daemon(self):
        if self.daemon and self.daemon.poll() is None:
            self.cli("shutdown")
            try:
                self.daemon.wait(15)
            except subprocess.TimeoutExpired:
                self.daemon.kill()
                self.daemon.wait()

    def teardown(self, ctx):
        self.stop_daemon()
        for p in self.procs:
            p.terminate()
            try:
                p.wait(5)
            except subprocess.TimeoutExpired:
                p.kill()
        self.procs = []

    def cli(self, *args, timeout=20):
        env = dict(os.environ, **self.env)
        return subprocess.run([self.bin / "theseus", "--socket", self.sock, *args],
                              capture_output=True, text=True, env=env, timeout=timeout)

    def sessions(self):
        r = self.cli("--json", "sessions", "list")
        try:
            v = json.loads(r.stdout)
        except ValueError:
            return []
        return v.get("sessions", v) if isinstance(v, dict) else v

    def session_id(self):
        ss = [s for s in self.sessions() if isinstance(s, dict)]
        return ss[0].get("session_id") or ss[0].get("id") if ss else None

    def history(self):
        sid = self.session_id()
        if not sid:
            return []
        r = self.cli("--json", "history", sid)
        try:
            v = json.loads(r.stdout)
        except ValueError:
            return []
        return v.get("nodes", v) if isinstance(v, dict) else v

    def busy(self):
        """Whether the session has a turn or a question open (its `status
        --short` is not empty)."""
        return bool(self.cli("status", "--short").stdout.strip())

    # Helpers

    def shell_back(self, text):
        lines = [l for l in text.splitlines() if l.strip()]
        return bool(lines) and SHELL_PROMPT.match(lines[-1]) is not None

    def tui_ready(self, text):
        return "theseus" in text.lower() and not self.shell_back(text)

    def open_tui(self, ctx):
        t0 = time.monotonic()
        ctx.tmux.type("theseus tui")
        ctx.tmux.keys("Enter")
        first = ctx.tmux.wait(lambda t: "$" in t and ctx.tmux.foreground() == "theseus-tui" and self.row_shown(t), 20, every=0.01)
        ms = round((time.monotonic() - t0) * 1000) if first else None
        time.sleep(0.5)
        ctx.tmux.keys("Enter")  # open the session
        time.sleep(0.5)
        return ms

    def row_shown(self, text):
        return bool(re.search(r"\$\d+\.\d\d", text))

    def say(self, ctx, text):
        """The input line: `i`, the text, enter."""
        ctx.tmux.keys("i")
        time.sleep(0.2)
        ctx.tmux.type(text)
        ctx.tmux.keys("Enter")

    def card(self, ctx, timeout=30):
        """Wait for the question: the confirm list has one."""
        def asked():
            r = self.cli("--json", "confirm")
            return r.returncode == 0 and '"id"' in r.stdout
        found = wait_until(asked, timeout, every=0.3)
        time.sleep(0.8)
        return found

    def settle(self, ctx, timeout=60):
        time.sleep(1.0)
        wait_until(lambda: not self.busy(), timeout, every=0.5)
        time.sleep(1.0)

    # The steps

    def s1(self, ctx):
        ctx.rec("S1", open_cmd="theseus")
        t0 = time.monotonic()
        ctx.tmux.type("theseus")
        ctx.tmux.keys("Enter")
        time.sleep(2.0)
        ctx.capture("S1.open")
        front = ctx.tmux.foreground()
        ready = None if front in ("bash", "sh") else round((time.monotonic() - t0) * 1000)
        ctx.rec("S1", foreground=front, ready_ms=ready)
        ctx.tmux.keys("C-l")

    def s2(self, ctx):
        q = steps.PROMPTS["S2"]
        ctx.tmux.type(f'theseus ask "{q}"')
        ctx.tmux.keys("Enter")
        time.sleep(0.8)
        ctx.capture("S2.wait1")
        time.sleep(1.0)
        ctx.capture("S2.wait2")
        ctx.tmux.wait(lambda t: self.shell_back(t) and q in t, 60)
        time.sleep(0.3)
        ctx.capture("S2.reply")
        ctx.sample_client()
        ctx.rec("S2", opened_with=f'theseus ask "{q}"')

    def s3(self, ctx):
        ctx.tmux.keys("C-l")
        ms = self.open_tui(ctx)
        ctx.rec("S3", open_cmd="theseus tui", ready_ms=ms)
        ctx.sample_client()
        import shutil

        shutil.rmtree(ctx.project / "__pycache__", ignore_errors=True)
        self.say(ctx, steps.PROMPTS["S3"])
        self.card(ctx)
        ctx.capture("S3.ask")
        ctx.tmux.keys("y")
        self.settle(ctx)
        ctx.capture("S3.after")
        ran = any((ctx.project / "__pycache__").glob("tides.*.pyc"))
        ctx.rec("S3", approve_keys=["y"], ran=ran, ids_typed=0)

    def s4(self, ctx):
        self.say(ctx, steps.PROMPTS["S4"])
        self.card(ctx)
        ctx.capture("S4.ask")
        ctx.tmux.keys("y")
        self.settle(ctx)
        ctx.capture("S4.after")

    def s5(self, ctx):
        self.say(ctx, steps.PROMPTS["S5"])
        self.card(ctx)
        ctx.capture("S5.ask")
        ctx.tmux.keys("y")
        self.settle(ctx)
        ctx.rec("S5", mode_keys=None)

    def s6(self, ctx):
        self.say(ctx, steps.PROMPTS["S6"])
        self.card(ctx)
        ctx.tmux.keys("y")
        wait_until(ctx.survey_pids, 30)
        time.sleep(5)
        ctx.capture("S6.run1")
        time.sleep(3)
        ctx.capture("S6.run2")
        wait_until(lambda: not self.busy(), 20, every=0.5)
        self.say(ctx, steps.PROMPTS["S6.talk"])
        ctx.tmux.wait(lambda t: "12:00 reading" in t, 30, every=0.3)
        time.sleep(0.5)
        ctx.rec("S6", ended_before_talk=not ctx.survey_pids())
        ctx.capture("S6.talk")
        wait_until(lambda: not ctx.survey_pids(), 150, every=1.0)
        t_end = time.time()
        self.settle(ctx)
        time.sleep(3)
        ctx.capture("S6.end")
        ctx.rec("S6", went_on=self.went_on_after(t_end))

    def went_on_after(self, t_end):
        """An answer written after the survey ended, with no person's message
        between: the session's history."""
        for n in self.history():
            if (n.get("at_unix_ms") or 0) / 1000 < t_end - 1:
                continue
            if n.get("kind") == "user_message":
                return False
            if n.get("kind") == "assistant_message":
                return True
        return False

    def s7(self, ctx):
        self.say(ctx, steps.PROMPTS["S7.start"])
        self.card(ctx)
        ctx.tmux.keys("y")
        wait_until(ctx.survey_pids, 30)
        self.settle(ctx, 30)
        self.say(ctx, steps.PROMPTS["S7.long"])
        time.sleep(2.0)
        ctx.capture("S7.busy")
        ctx.tmux.keys("C-c")
        time.sleep(1.5)
        ctx.capture("S7.after")
        ctx.rec("S7", interrupt_keys=["C-c"], foreground_after=ctx.tmux.foreground(),
                earlier_alive=bool(ctx.survey_pids()))
        # The stop B3 verifies: the TUI's own, on the session.
        ctx.tmux.keys("C-l")
        self.open_tui(ctx)
        ctx.tmux.keys("s")
        time.sleep(0.3)
        ctx.tmux.keys("s")
        wait_until(lambda: not ctx.survey_pids(), 15, every=0.3)
        ctx.tmux.wait(lambda t: "verified" in t, 10)
        ctx.capture("S7.stop")
        ctx.rec("S7", stop_procs_left=len(ctx.survey_pids()), stop_keys=["s", "s"])
        self.settle(ctx, 40)

    def s8(self, ctx):
        before = len(self.history())
        ctx.tmux.keys("i")
        time.sleep(0.2)
        ctx.tmux.paste("\n".join(steps.survey_log()) + "\n")
        time.sleep(0.5)
        ctx.tmux.type(steps.PROMPTS["S8"])
        time.sleep(0.5)
        ctx.capture("S8.typed")
        ctx.tmux.keys("Enter")
        self.settle(ctx)
        ctx.capture("S8.after")
        msgs = [n.get("text", "") for n in self.history()[before:] if n.get("kind") == "user_message"]
        ctx.rec("S8", messages=[m for m in msgs if m])

    def s9(self, ctx):
        self.say(ctx, steps.PROMPTS["S9.edit"])
        self.card(ctx)
        ctx.tmux.keys("y")
        self.settle(ctx)
        ctx.capture("S9.after")
        ctx.rec("S9", undo_keys=None)

    def s10(self, ctx):
        ctx.capture("S10.screen")

    def s11(self, ctx):
        self.say(ctx, steps.PROMPTS["S11"])
        self.card(ctx)
        ctx.tmux.keys("y")
        wait_until(ctx.survey_pids, 30)
        self.settle(ctx, 30)
        ctx.tmux.keys("q")
        time.sleep(1.0)
        ctx.capture("S11.left")
        ctx.rec("S11", work_alive=bool(ctx.survey_pids()), reopen_cmd=None, picker_cmd=None)
        # B4: the agent's own process, theseusd, stopped and started again.
        self.stop_daemon()
        self.start_daemon(ctx)
        time.sleep(2.0)
        ctx.rec("S11", alive_after_restart=bool(ctx.survey_pids()), restart_how="theseusd's stop and start")

    def s12(self, ctx):
        ctx.rec("S12", plan_keys=None)

