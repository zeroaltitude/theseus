"""F10's Theseus arm (theseus-qy2a): a scratch `theseusd` on the scripted
stand-in model (`theseus-sim fake-model --rules`, so a run costs $0), driven
the way a person new to it would, through the clients it ships.

How each step reaches Theseus (the rules the baseline was measured by):
- S1 types the bare `theseus`, which opens the chat (theseus-cyha); S2 is asked
  in it, S6 runs its job in it and S8 pastes into it (`theseus chat SESSION`).
  The other steps use the terminal UI, `theseus tui`, until chat-ask moves
  them: enter opens the session, `i` the input line, `y` answers a question.
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
        """The bare `theseus`: on a terminal it opens the chat (theseus-cyha),
        a conversation in this directory, inline. Ready is the prompt (`›`)
        on the screen, timed from Enter."""
        ctx.rec("S1", open_cmd="theseus")
        ctx.tmux.type("theseus")
        t0 = time.monotonic()
        ctx.tmux.keys("Enter")
        first = ctx.tmux.wait(lambda t: "›" in t and ctx.tmux.foreground() == "theseus-tui", 20, every=0.01)
        ready = round((time.monotonic() - t0) * 1000) if first else None
        # The banner follows the prompt by the daemon's answer to `health`.
        ctx.tmux.wait(lambda t: "? help" in t, 10)
        time.sleep(0.3)
        ctx.capture("S1.open")
        ctx.rec("S1", foreground=ctx.tmux.foreground(), ready_ms=ready)

    def s2(self, ctx):
        """S2 is asked in the chat S1 opened, and the chat is left after the
        reply: S3 opens the board from the shell."""
        q = steps.PROMPTS["S2"]
        ctx.tmux.type(q)
        ctx.tmux.keys("Enter")
        time.sleep(0.8)
        ctx.capture("S2.wait1")
        time.sleep(1.0)
        ctx.capture("S2.wait2")
        # The reply is whole when its turn line (time, model, cost) is on screen.
        ctx.tmux.wait(lambda t: re.search(r"^\s+\d+(\.\d)? (ms|s) · ", t, re.M) is not None, 60)
        time.sleep(0.3)
        ctx.capture("S2.reply")
        ctx.sample_client()
        ctx.rec("S2", opened_with="theseus")
        self.leave_chat(ctx)

    def leave_chat(self, ctx):
        """`/quit`, back at the shell's prompt."""
        ctx.tmux.type("/quit")
        ctx.tmux.keys("Enter")
        ctx.tmux.wait(lambda t: self.shell_back(t), 10)

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
        # The card's grant (theseus-drjs): `a` approves, and stops asking for
        # this command's prefix for the rest of the conversation.
        ctx.tmux.keys("a")
        self.settle(ctx)
        ctx.rec("S5", grant_keys=["a"], mode_keys=None)

    def s6(self, ctx):
        """S6 runs in the chat, where the dock shows the elapsed time of a
        job that runs (theseus-cyha); the board is opened again after it, for
        S7."""
        self.open_chat(ctx)
        self.chat_say(ctx, steps.PROMPTS["S6"])
        self.card(ctx)
        ctx.tmux.keys("y")
        wait_until(ctx.survey_pids, 30)
        time.sleep(5)
        ctx.capture("S6.run1")
        time.sleep(3)
        ctx.capture("S6.run2")
        wait_until(lambda: not self.busy(), 20, every=0.5)
        self.chat_say(ctx, steps.PROMPTS["S6.talk"])
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
        self.back_to_board(ctx)

    def open_chat(self, ctx):
        """Leave the board and open the chat on this conversation."""
        sid = self.session_id()
        ctx.tmux.keys("q")
        time.sleep(0.5)
        ctx.tmux.keys("C-l")
        ctx.tmux.type(f"theseus chat {sid}")
        ctx.tmux.keys("Enter")
        ctx.tmux.wait(lambda t: "› " in t and ctx.tmux.foreground() == "theseus-tui", 20, every=0.05)
        time.sleep(0.5)

    def chat_say(self, ctx, text):
        ctx.tmux.type(text)
        ctx.tmux.keys("Enter")

    def back_to_board(self, ctx):
        """Leave the chat and open the board on the conversation, where the
        steps after this one expect to be."""
        self.leave_chat(ctx)
        ctx.tmux.keys("C-l")
        self.open_tui(ctx)

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
        """S8 pastes into the chat, on the conversation the board had open
        (`theseus chat SESSION`); the board is opened again for S9."""
        before = len(self.history())
        self.open_chat(ctx)
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
        self.back_to_board(ctx)

    def s9(self, ctx):
        """S9 edits tides.py, then takes the edit back in the chat: Esc Esc on
        the empty line opens the picker (theseus-23rk), the latest prompt is
        chosen (Enter), and `f` puts back the files and the conversation. The
        keys are recorded; the driver compares tides.py's bytes."""
        prompt = steps.PROMPTS["S9.edit"]
        before = ctx.file_digest()
        self.say(ctx, prompt)
        self.card(ctx)
        ctx.tmux.keys("y")
        self.settle(ctx)
        changed = ctx.file_digest() != before
        self.open_chat(ctx)
        keys = ["Escape", "Escape"]
        ctx.tmux.keys(*keys)
        ctx.tmux.wait(lambda t: "Rewind to before which message?" in t, 10)
        ctx.capture("S9.menu")
        for k in ("Enter", "f"):
            ctx.tmux.keys(k)
            keys.append(k)
            time.sleep(0.3)
        # The fork's history is shown on a clean screen and the prompt is
        # back in the input line.
        ctx.tmux.wait(
            lambda t: prompt in t and "Added the comment" not in t and "Rewind to before" not in t, 20
        )
        time.sleep(0.5)
        after = ctx.capture("S9.after")
        ctx.rec(
            "S9",
            undo_keys=keys,
            file_restored=changed and ctx.file_digest() == before,
            answer_gone="Added the comment" not in after,
        )
        # The prompt waits in the input line; clear it, then back to the board.
        ctx.tmux.keys("C-u")
        self.back_to_board(ctx)

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
