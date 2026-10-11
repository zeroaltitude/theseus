"""F10's Claude Code arm (theseus-qy2a): the real model, run LOCALLY ONLY,
never in a cloud session, capped at $1 a run.

The cap. Claude Code's `--max-budget-usd` works only with `--print` (its
`--help`, 2.1.296), and S1 must open the conversation with the bare command,
so the driver caps the run itself: a watchdog reads the session's transcript
(`~/.claude/projects/<cwd>/<session>.jsonl`) every two seconds, prices each
answer's usage at the rates given (by default above any current model's, so
the estimate is high), and past the cap stops the client (two Ctrl-C) and
every step after. The run's `run.json` keeps the estimate and whether the cap
stopped it.

The version is pinned by `--expect-version`: another version refuses to run,
and the version run is recorded either way.
"""

import json
import re
import subprocess
import threading
import time
from pathlib import Path

import steps
from driver import wait_until

READY = re.compile(r"(\? for shortcuts|^\s*│\s*>\s|^>\s)", re.M)
BUSY = re.compile(r"esc to interrupt", re.I)
ASK = re.compile(r"(Do you want to (proceed|make this edit|create)|❯ 1\. Yes)", re.I)
TRUST = re.compile(r"Do you trust the files in this folder|trust this folder", re.I)

# Dollars per million tokens: input, output, cache write, cache read. High on
# purpose: the cap must stop a run before the real spend reaches it.
RATES = (15.0, 75.0, 18.75, 1.5)


class ClaudeCode:
    name = "claude-code"

    def __init__(self, cli, expect_version, budget_usd, rates=RATES):
        self.cli, self.expect, self.budget, self.rates = cli, expect_version, budget_usd, rates
        self.stopped = threading.Event()
        self.watch = None

    # The run's frame

    def setup(self, ctx):
        version = subprocess.run([self.cli, "--version"], capture_output=True, text=True).stdout.strip()
        if self.expect and self.expect not in version:
            raise SystemExit(f"f10: {self.cli} is {version!r}, not the pinned {self.expect}")
        ctx.meta.update(version=version, model=self.model_name(), budget_usd=self.budget)
        self.stopped.clear()
        return {}

    def model_name(self):
        """The model the CLI will use: its settings' `model`, else the
        default it names (read from the first screen by S1c)."""
        try:
            return json.loads((Path.home() / ".claude" / "settings.json").read_text()).get("model", "")
        except (OSError, ValueError):
            return ""

    def teardown(self, ctx):
        self.stopped.set()
        if ctx.tmux:
            ctx.tmux.keys("C-c", "C-c")
        ctx.rec("cost", estimate_usd=round(self.spent(ctx), 4))

    def transcripts(self, ctx):
        slug = re.sub(r"[^A-Za-z0-9]", "-", str(ctx.project))
        d = Path.home() / ".claude" / "projects" / slug
        return sorted(d.glob("*.jsonl"), key=lambda p: p.stat().st_mtime) if d.exists() else []

    def entries(self, ctx):
        out = []
        for p in self.transcripts(ctx):
            for line in p.read_text(errors="replace").splitlines():
                try:
                    out.append(json.loads(line))
                except ValueError:
                    pass
        return out

    def spent(self, ctx):
        seen, usd = set(), 0.0
        rin, rout, rw, rr = self.rates
        for e in self.entries(ctx):
            m = e.get("message") or {}
            u = m.get("usage")
            if e.get("type") != "assistant" or not u or m.get("id") in seen:
                continue
            seen.add(m.get("id"))
            usd += (u.get("input_tokens", 0) * rin + u.get("output_tokens", 0) * rout
                    + u.get("cache_creation_input_tokens", 0) * rw + u.get("cache_read_input_tokens", 0) * rr) / 1e6
        return usd

    def start_watchdog(self, ctx):
        def watch():
            while not self.stopped.wait(2.0):
                usd = self.spent(ctx)
                if usd > self.budget:
                    ctx.say(f"the cap: about ${usd:.2f} spent, over ${self.budget:.2f}; stopping the run")
                    ctx.rec("cost", budget_stopped=True, estimate_usd=round(usd, 4))
                    self.stopped.set()
                    ctx.tmux.keys("C-c", "C-c")

        self.watch = threading.Thread(target=watch, daemon=True)
        self.watch.start()

    def check_cap(self):
        if self.stopped.is_set():
            raise RuntimeError("the run was stopped at its spend cap")

    # Helpers

    def ready(self, text):
        return bool(READY.search(text)) and not BUSY.search(text) and not TRUST.search(text)

    def prompt(self, ctx, text):
        self.check_cap()
        ctx.tmux.type(text)
        ctx.tmux.keys("Enter")

    def idle(self, ctx, timeout=240):
        time.sleep(1.0)
        return ctx.tmux.wait(lambda t: self.ready(t) and not ASK.search(t), timeout, every=0.5)

    def ask(self, ctx, timeout=240):
        return ctx.tmux.wait(lambda t: bool(ASK.search(t)), timeout, every=0.3)

    def user_messages_since(self, ctx, t0):
        out = []
        for e in self.entries(ctx):
            if e.get("type") != "user" or e.get("isMeta"):
                continue
            c = (e.get("message") or {}).get("content")
            text = c if isinstance(c, str) else "\n".join(b.get("text", "") for b in c or [] if b.get("type") == "text")
            if text and stamp(e) >= t0:
                out.append(text)
        return out

    # The steps

    def s1(self, ctx):
        ctx.rec("S1", open_cmd="claude")
        t0 = time.monotonic()
        ctx.tmux.type("claude")
        ctx.tmux.keys("Enter")
        first = ctx.tmux.wait(lambda t: self.ready(t) or bool(TRUST.search(t)), 30, every=0.01)
        if first and TRUST.search(first):
            # A first run in a new directory asks about trust; its answer is
            # not part of the ready time.
            ctx.capture("S1.trust")
            ctx.tmux.keys("Enter")
            t0 = time.monotonic()
            first = ctx.tmux.wait(self.ready, 30, every=0.01)
        ready_ms = round((time.monotonic() - t0) * 1000) if first else None
        time.sleep(1.0)
        ctx.capture("S1.open")
        ctx.rec("S1", ready_ms=ready_ms, foreground=ctx.tmux.foreground())
        self.start_watchdog(ctx)

    def s2(self, ctx):
        self.prompt(ctx, steps.PROMPTS["S2"])
        time.sleep(1.0)
        ctx.capture("S2.wait1")
        time.sleep(1.0)
        ctx.capture("S2.wait2")
        self.idle(ctx)
        ctx.capture("S2.reply")

    def s3(self, ctx):
        _clear_pycache(ctx)
        self.prompt(ctx, steps.PROMPTS["S3"])
        self.ask(ctx)
        ctx.capture("S3.ask")
        ctx.tmux.keys("Enter")
        self.idle(ctx)
        ctx.capture("S3.after")
        ctx.rec("S3", approve_keys=["Enter"], ran=_ran(ctx), ids_typed=0)

    def s4(self, ctx):
        self.prompt(ctx, steps.PROMPTS["S4"])
        self.ask(ctx)
        ctx.capture("S4.ask")
        ctx.tmux.keys("Enter")
        self.idle(ctx)
        ctx.capture("S4.after")

    def s5(self, ctx):
        self.prompt(ctx, steps.PROMPTS["S5"])
        self.ask(ctx)
        ctx.capture("S5.ask")
        ctx.tmux.keys("Enter")
        self.idle(ctx)
        ctx.tmux.keys("BTab")
        time.sleep(0.8)
        ctx.capture("S5.mode")
        ctx.rec("S5", mode_keys=["BTab"])
        # Back to the default mode (accept edits, plan, default), uncounted.
        ctx.tmux.keys("BTab")
        time.sleep(0.3)
        ctx.tmux.keys("BTab")
        time.sleep(0.5)

    def s6(self, ctx):
        self.prompt(ctx, steps.PROMPTS["S6"])
        self.ask(ctx)
        ctx.tmux.keys("Enter")
        wait_until(ctx.survey_pids, 30)
        time.sleep(5)
        ctx.capture("S6.run1")
        time.sleep(3)
        ctx.capture("S6.run2")
        self.prompt(ctx, steps.PROMPTS["S6.talk"])
        ctx.tmux.wait(lambda t: "12:00" in t and steps.PROMPTS["S6.talk"] not in t.splitlines()[-1], 90, every=0.5)
        ctx.rec("S6", ended_before_talk=not ctx.survey_pids())
        ctx.capture("S6.talk")
        wait_until(lambda: not ctx.survey_pids(), 150, every=1.0)
        t_end = time.time()
        self.idle(ctx, 120)
        ctx.capture("S6.end")
        ctx.rec("S6", went_on=self.went_on_after(ctx, t_end))

    def went_on_after(self, ctx, t_end):
        """An answer of the agent's after the survey ended, with no message
        from the person in between."""
        after = [e for e in self.entries(ctx) if stamp(e) >= t_end - 2]
        for e in after:
            if e.get("type") == "user" and isinstance((e.get("message") or {}).get("content"), str):
                return False
            if e.get("type") == "assistant":
                return True
        return False

    def s7(self, ctx):
        self.prompt(ctx, steps.PROMPTS["S7.start"])
        if self.ask(ctx, 120):
            ctx.tmux.keys("Enter")
        wait_until(ctx.survey_pids, 60)
        self.idle(ctx)
        self.prompt(ctx, steps.PROMPTS["S7.long"])
        ctx.tmux.wait(lambda t: bool(BUSY.search(t)), 30, every=0.1)
        ctx.capture("S7.busy")
        ctx.tmux.keys("Escape")
        time.sleep(2)
        ctx.capture("S7.after")
        ctx.rec("S7", interrupt_keys=["Escape"], foreground_after=ctx.tmux.foreground(),
                earlier_alive=bool(ctx.survey_pids()))
        self.prompt(ctx, "stop the background survey")
        if self.ask(ctx, 60):
            ctx.tmux.keys("Enter")
        self.idle(ctx)
        time.sleep(2)
        ctx.capture("S7.stop")
        ctx.rec("S7", stop_procs_left=len(ctx.survey_pids()))

    def s8(self, ctx):
        t0 = time.time()
        ctx.tmux.paste("\n".join(steps.survey_log()) + "\n")
        time.sleep(1.0)
        ctx.tmux.type(steps.PROMPTS["S8"])
        time.sleep(0.5)
        ctx.capture("S8.typed")
        ctx.tmux.keys("Enter")
        self.idle(ctx)
        ctx.capture("S8.after")
        ctx.rec("S8", messages=self.user_messages_since(ctx, t0))

    def s9(self, ctx):
        before = ctx.file_digest()
        self.prompt(ctx, steps.PROMPTS["S9.edit"])
        if self.ask(ctx, 120):
            ctx.tmux.keys("Enter")
        self.idle(ctx)
        changed = ctx.file_digest() != before
        keys = ["Escape", "Escape"]
        ctx.tmux.keys(*keys)
        time.sleep(1.0)
        ctx.capture("S9.menu")
        # The latest prompt is chosen; then "restore the code and the
        # conversation", the menu's first choice.
        for k in ("Enter", "Enter"):
            ctx.tmux.keys(k)
            keys.append(k)
            time.sleep(1.0)
        after = ctx.capture("S9.after")
        tail = "\n".join(after.splitlines()[-8:])
        ctx.rec("S9", undo_keys=keys, file_restored=changed and ctx.file_digest() == before,
                answer_gone=steps.PROMPTS["S9.edit"] in tail)

    def s10(self, ctx):
        self.prompt(ctx, "/context")
        time.sleep(3)
        a = ctx.tmux.capture()
        ctx.tmux.keys("Escape")
        self.prompt(ctx, "/cost")
        time.sleep(3)
        b = ctx.tmux.capture()
        ctx.save_screen("S10.screen", a + "\n" + b)

    def s11(self, ctx):
        self.prompt(ctx, steps.PROMPTS["S11"])
        if self.ask(ctx, 120):
            ctx.tmux.keys("Enter")
        wait_until(ctx.survey_pids, 60)
        self.idle(ctx)
        self.prompt(ctx, "/exit")
        ctx.tmux.wait(lambda t: t.rstrip().endswith("$"), 30)
        time.sleep(1)
        ctx.capture("S11.left")
        alive = bool(ctx.survey_pids())
        ctx.rec("S11", work_alive=alive, alive_after_restart=alive, restart_how="claude's exit")
        ctx.tmux.type("claude -c")
        ctx.tmux.keys("Enter")
        ctx.tmux.wait(self.ready, 30)
        time.sleep(1.5)
        ctx.capture("S11.reopen")
        ctx.rec("S11", reopen_cmd="claude -c", reopen_foreground=ctx.tmux.foreground())
        self.prompt(ctx, "/exit")
        ctx.tmux.wait(lambda t: t.rstrip().endswith("$"), 30)
        ctx.tmux.type("claude --resume")
        ctx.tmux.keys("Enter")
        time.sleep(3)
        ctx.tmux.type("log lines")
        time.sleep(1.5)
        ctx.capture("S11.picker")
        ctx.rec("S11", picker_cmd="claude --resume", picker_searched=True)
        ctx.tmux.keys("Escape")
        time.sleep(1)
        ctx.tmux.keys("C-c", "C-c")
        ctx.tmux.wait(lambda t: t.rstrip().endswith("$"), 30)
        ctx.tmux.type("claude -c")
        ctx.tmux.keys("Enter")
        ctx.tmux.wait(self.ready, 30)

    def s12(self, ctx):
        before = ctx.file_digest()
        ctx.tmux.keys("BTab")
        time.sleep(0.5)
        ctx.tmux.keys("BTab")
        time.sleep(0.8)
        ctx.capture("S12.mode")
        ctx.rec("S12", plan_keys=["BTab", "BTab"])
        self.prompt(ctx, steps.PROMPTS["S12"])
        ctx.tmux.wait(lambda t: bool(re.search(r"Would you like to proceed|ready to code", t, re.I)), 300, every=0.5)
        ctx.capture("S12.plan")
        ctx.rec("S12", unchanged_at_plan=ctx.file_digest() == before)
        ctx.tmux.keys("Enter")
        ctx.tmux.wait(lambda t: bool(re.search(r"[☐☒◼]", t)), 180, every=0.3)
        ctx.capture("S12.progress")
        self.idle(ctx, 300)


def stamp(entry):
    """An entry's time in Unix seconds (its ISO `timestamp`), 0 when absent."""
    ts = entry.get("timestamp")
    if not ts:
        return 0
    from datetime import datetime

    try:
        return datetime.fromisoformat(ts.replace("Z", "+00:00")).timestamp()
    except ValueError:
        return 0


def _clear_pycache(ctx):
    import shutil

    shutil.rmtree(ctx.project / "__pycache__", ignore_errors=True)


def _ran(ctx):
    """The tests ran: importing the module wrote its bytecode."""
    return any((ctx.project / "__pycache__").glob("tides.*.pyc"))
