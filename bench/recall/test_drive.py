"""The drivers' tests: the Claude Code driver against a stand-in `claude` on
PATH, and the Theseus driver end to end on this workspace's binaries and
`theseus-sim fake-model --rules` (skipped when they are missing; nothing is
left running). Standard library only:

    python3 -m unittest discover -s bench/recall

The Theseus test finds `theseus`, `theseusd`, `theseus-index` and
`theseus-sim` in `THESEUS_RECALL_BIN_DIR`, else in the workspace's
`target/debug` (a `cargo build --workspace` makes them).
"""

from __future__ import annotations

import io
import json
import os
import re
import signal
import socket
import subprocess
import sys
import tempfile
import time
import tomllib
import unittest
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
sys.path.insert(0, str(HERE))

import drive  # noqa: E402
import generate  # noqa: E402
import progression as pg  # noqa: E402
import score  # noqa: E402
import standin  # noqa: E402

STANDIN = r'''#!/usr/bin/env python3
# A stand-in `claude -p --output-format json`: its sessions under
# CLAUDE_CONFIG_DIR, one JSONL log each; a scripted answer per prompt.
import json, os, re, subprocess, sys
from pathlib import Path
args = sys.argv[1:]
prompt = sys.stdin.read()
cfg = Path(os.environ["CLAUDE_CONFIG_DIR"])
cwd = Path.cwd()
name = "".join(c if c.isalnum() else "-" for c in str(cwd))
logs = cfg / "projects" / name
logs.mkdir(parents=True, exist_ok=True)
def arg(flag):
    return args[args.index(flag) + 1] if flag in args else None
sid = arg("--session-id") or arg("--resume")
with open(os.environ["STANDIN_LOG"], "a") as f:
    f.write(json.dumps({"argv": args, "cwd": str(cwd), "prompt": prompt, "parent": os.environ.get("CLAUDECODE")}) + "\n")
log = logs / f"{sid}.jsonl"
if "--resume" in args and not log.exists():
    print(json.dumps({"type": "result", "subtype": "error", "is_error": True, "result": "no such session"}))
    sys.exit(1)
answers = json.loads(Path(os.environ["STANDIN_ANSWERS"]).read_text())
lines = []
if prompt.strip() == "/compact":
    lines.append({"type": "system", "subtype": "compact_boundary"})
    reply = "Compacted."
else:
    lines.append({"type": "user", "message": {"role": "user", "content": prompt}})
    m = re.search(r"Run (\./scripts/[\w.-]+\.sh)", prompt)
    if m:
        r = subprocess.run([m.group(1)], capture_output=True, text=True)
        lines.append({"type": "user", "message": {"content": [{"type": "tool_result", "content": r.stdout}]}})
    a = answers.get(prompt, {})
    if a.get("file"):
        p = cwd / a["file"]
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(a["content"])
    reply = a.get("reply", "ok")
    lines.append({"type": "assistant", "message": {"role": "assistant", "content": [{"type": "text", "text": reply}]}})
with log.open("a") as f:
    for x in lines:
        f.write(json.dumps(x) + "\n")
print(json.dumps({"type": "result", "subtype": "success", "is_error": False, "result": reply, "session_id": sid,
                  "usage": {"input_tokens": 10, "output_tokens": 5, "cache_read_input_tokens": 100,
                            "cache_creation_input_tokens": 20}, "total_cost_usd": 0.001, "duration_ms": 3,
                  "num_turns": 1}))
'''


def answers_for(prog: pg.Progression) -> dict[str, dict]:
    """What a perfect arm answers each probe: the value, written where an
    indirect task says; an admission for an abstention."""
    facts = prog.facts_by_id()
    out = {}
    for p in prog.probes:
        if p.kind == "abstention":
            out[p.text] = {"reply": "I don't know: that was never mentioned."}
        elif p.kind == "indirect":
            out[p.text] = {"file": p.file, "content": facts[p.fact].value + "\n", "reply": "Written."}
        else:
            out[p.text] = {"reply": f"It's {facts[p.fact].value}."}
    return out


class ClaudeCodeDriver(unittest.TestCase):
    def test_sessions_are_carried_a_boundary_opens_a_new_one_and_marks_compact(self):
        prog = generate.build(7, "smoke")
        with tempfile.TemporaryDirectory() as d:
            d = Path(d)
            bin_ = d / "bin"
            bin_.mkdir()
            (bin_ / "claude").write_text(STANDIN)
            (bin_ / "claude").chmod(0o755)
            gen = d / "gen"
            prog.save(gen)
            (d / "answers.json").write_text(json.dumps(answers_for(prog)))
            env = {"PATH": f"{bin_}:{os.environ['PATH']}", "STANDIN_LOG": str(d / "calls.jsonl"),
                   "STANDIN_ANSWERS": str(d / "answers.json"), "CLAUDECODE": "1"}
            old = {k: os.environ.get(k) for k in env}
            os.environ.update(env)
            try:
                with redirect_stdout(io.StringIO()):
                    rc = drive.main(["--arm", "claude-code", "--progression", str(gen), "--out", str(d / "run")])
            finally:
                for k, v in old.items():
                    if v is None:
                        os.environ.pop(k, None)
                    else:
                        os.environ[k] = v
            self.assertEqual(rc, 0)
            calls = [json.loads(x) for x in (d / "calls.jsonl").read_text().splitlines()]
            # One call per turn, and one /compact after the mark (the
            # smoke's window is under the 100k --autocompact takes).
            self.assertEqual(len(calls), len(prog.turns) + 1)
            turns = [c for c in calls if c["prompt"] != "/compact"]
            compact = [i for i, c in enumerate(calls) if c["prompt"] == "/compact"]
            self.assertEqual(compact, [prog.marks()[0] + 1])
            ids = []
            for t, c in zip(prog.turns, turns):
                a = c["argv"]
                self.assertEqual(c["cwd"], str((d / "run" / "workspace").resolve()))
                self.assertIsNone(c["parent"], "a parent session's variables are taken out")
                self.assertIn("--output-format", a)
                self.assertEqual(a[a.index("--effort") + 1], "medium", "every arm's default is medium")
                self.assertEqual(a[a.index("--tools") + 1], drive.CC_TOOLS)
                first = t.index == prog.session_turns(t.session)[0].index
                if first:
                    self.assertIn("--session-id", a)
                    self.assertNotIn("--resume", a)
                    ids.append(a[a.index("--session-id") + 1])
                else:
                    self.assertEqual(a[a.index("--resume") + 1], ids[-1])
            self.assertEqual(len(ids), 2)
            self.assertNotEqual(ids[0], ids[1])
            run = json.loads((d / "run" / "run.json").read_text())
            self.assertEqual(run["sessions"], ids)
            # The boundary lands before the turn after the mark's: that
            # turn's request is the compacted one.
            self.assertEqual(run["compactions"], [prog.marks()[0] + 1])
            self.assertEqual(run["cc_compact"], "marks")
            self.assertEqual(run["effort"], "medium")
            self.assertEqual(run["delivered"], len(prog.facts))
            rows = [json.loads(x) for x in (d / "run" / "turns.jsonl").read_text().splitlines()]
            self.assertEqual(len(rows), len(prog.turns))
            self.assertTrue(all(r["exit"] == 0 and r["cost_usd"] == 0.001 for r in rows))
            # The perfect arm scores perfectly.
            runs = [score.load_run(d / "run")]
            s = score.summarize(score.score_run(runs[0]))
            self.assertEqual((s["recall_accuracy"], s["abstention_accuracy"]), (1.0, 1.0))
            self.assertEqual(s["undelivered"] + s["failed"] + s["confident_wrong"], 0)

    def test_a_window_of_100k_or_more_is_claude_codes_own_autocompact(self):
        prog = generate.build(7, "smoke")
        with tempfile.TemporaryDirectory() as d:
            run = drive.Run(Path(d) / "run", prog, "claude-code", {})

            class A:
                claude, context_window, cc_compact, model, claude_arg = "claude", 145350, "auto", "m", []

            cc = drive.ClaudeCode(A, run)
            self.assertEqual(cc.mode, "window")
            run.turns_f.close()


PI_STANDIN = r'''#!/usr/bin/env python3
# A stand-in `pi --print --mode json`: its sessions under --session-dir, one
# JSONL log each, named as Pi names them; a scripted answer per prompt, and a
# compaction before each prompt listed in STANDIN_COMPACT_BEFORE. The first
# prompt's first answer reports STANDIN_FIRST_INPUT input tokens in all
# (input, cache read and cache write), as a provider counts a new session's
# first call.
import json, os, re, subprocess, sys, time
from pathlib import Path
args = sys.argv[1:]
prompt = sys.stdin.read()
def arg(flag):
    return args[args.index(flag) + 1] if flag in args else None
sid, sessions = arg("--session-id"), Path(arg("--session-dir"))
cfg = Path(os.environ["PI_CODING_AGENT_DIR"])
settings = json.loads((cfg / "settings.json").read_text())
with open(os.environ["STANDIN_LOG"], "a") as f:
    f.write(json.dumps({"argv": args, "cwd": str(Path.cwd()), "prompt": prompt, "settings": settings,
                        "env": {k: os.environ.get(k) for k in ("PI_SKIP_VERSION_CHECK", "PI_TELEMETRY", "PI_OFFLINE")},
                        "parent": [k for k in ("PI_SESSION_ID", "AI_AGENT", "CLAUDECODE") if k in os.environ]})
            + "\n")
sessions.mkdir(parents=True, exist_ok=True)
found = sorted(sessions.glob(f"*_{sid}.jsonl"))
log = found[0] if found else sessions / f"2026-10-06T12-00-00-000Z_{sid}.jsonl"
dump = lambda x: json.dumps(x, separators=(",", ":"))
entries, out = [], []
if not found:
    entries.append({"type": "session", "version": 3, "id": sid, "timestamp": "2026-10-06T12:00:00.000Z"})
if prompt in json.loads(os.environ["STANDIN_COMPACT_BEFORE"]):
    entries.append({"type": "compaction", "id": f"c{time.monotonic_ns()}", "summary": "Earlier work.",
                    "firstKeptEntryId": "x", "tokensBefore": 40000})
user = {"role": "user", "content": [{"type": "text", "text": prompt}]}
entries.append({"type": "message", "id": f"u{time.monotonic_ns()}", "message": user})
answers = json.loads(Path(os.environ["STANDIN_ANSWERS"]).read_text())
calls = []
m = re.search(r"Run (\./scripts/[\w.-]+\.sh)", prompt)
if m:
    r = subprocess.run([m.group(1)], capture_output=True, text=True)
    calls = [{"type": "toolCall", "id": "toolu_1", "name": "bash", "arguments": {"command": m.group(1)}}]
    first = {"role": "assistant", "content": calls, "model": "claude-sonnet-5-5", "stopReason": "toolUse",
             "usage": {"input": 10, "output": 5, "cacheRead": 100, "cacheWrite": 20, "cost": {"total": 0.0005}}}
    if prompt == os.environ.get("STANDIN_FIRST_PROMPT"):
        first["usage"]["input"] = int(os.environ["STANDIN_FIRST_INPUT"]) - 120
    result = {"role": "toolResult", "toolCallId": "toolu_1", "toolName": "bash",
              "content": [{"type": "text", "text": r.stdout}], "isError": False}
    entries += [{"type": "message", "id": f"a{time.monotonic_ns()}", "message": first},
                {"type": "message", "id": f"r{time.monotonic_ns()}", "message": result}]
    out += [first, result]
a = answers.get(prompt, {})
if a.get("file"):
    p = Path.cwd() / a["file"]
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(a["content"])
reply = {"role": "assistant", "content": [{"type": "text", "text": a.get("reply", "ok")}],
         "model": "claude-sonnet-5-5", "stopReason": "stop",
         "usage": {"input": 10, "output": 5, "cacheRead": 100, "cacheWrite": 20, "cost": {"total": 0.0005}}}
if prompt == os.environ.get("STANDIN_FIRST_PROMPT") and not calls:
    reply["usage"]["input"] = int(os.environ["STANDIN_FIRST_INPUT"]) - 120
entries.append({"type": "message", "id": f"a{time.monotonic_ns()}", "message": reply})
out.append(reply)
with log.open("a") as f:
    for x in entries:
        f.write(dump(x) + "\n")
print(dump({"type": "session", "version": 3, "id": sid}))
for msg in [user] + out:
    print(dump({"type": "message_end", "message": msg}))
print(dump({"type": "agent_settled"}))
'''


def pi_input(prog: pg.Progression, overhead: int, model: str = "anthropic/claude-sonnet-5-5") -> int:
    """What a provider reports for Pi's first call at a system prompt and
    tools of `overhead` tokens: those, and the first turn's words."""
    return overhead + drive.tk.user_text(prog.turns[0].text).tokens(drive.tk.rates_of(model))


def run_pi(prog: pg.Progression, d: Path, argv: list[str] = (), overhead: int = drive.PI_OVERHEAD_TOKENS,
           compact_before: list[str] = ()) -> tuple[int, str]:
    """`drive.main` for Pi on the stand-in `pi`, whose first answer counts
    `overhead` tokens beyond the first turn's words: the exit code and what
    it said to stderr."""
    bin_ = d / "bin"
    bin_.mkdir()
    (bin_ / "pi").write_text(PI_STANDIN)
    (bin_ / "pi").chmod(0o755)
    gen = d / "gen"
    prog.save(gen)
    (d / "answers.json").write_text(json.dumps(answers_for(prog)))
    env = {"PATH": f"{bin_}:{os.environ['PATH']}", "STANDIN_LOG": str(d / "calls.jsonl"),
           "STANDIN_ANSWERS": str(d / "answers.json"), "STANDIN_COMPACT_BEFORE": json.dumps(list(compact_before)),
           "STANDIN_FIRST_PROMPT": prog.turns[0].text, "STANDIN_FIRST_INPUT": str(pi_input(prog, overhead)),
           "PI_SESSION_ID": "a-parent", "AI_AGENT": "pi"}
    old = {k: os.environ.get(k) for k in env}
    os.environ.update(env)
    err = io.StringIO()
    try:
        with redirect_stdout(io.StringIO()), redirect_stderr(err):
            rc = drive.main(["--arm", "pi", "--progression", str(gen), "--out", str(d / "run"), *argv])
    finally:
        for k, v in old.items():
            if v is None:
                os.environ.pop(k, None)
            else:
                os.environ[k] = v
    return rc, err.getvalue()


class PiDriver(unittest.TestCase):
    def test_a_session_id_per_session_its_reserve_at_the_planned_threshold_and_compactions_from_its_logs(self):
        prog = generate.build(7, "smoke")
        mark = prog.marks()[0]
        after = prog.turns[mark + 1].text
        # Pi's own overhead is smaller than the plan's: it compacts where its
        # context holds what the plan's holds at the window.
        threshold = prog.context_window - (generate.planned_overhead(prog) - drive.PI_OVERHEAD_TOKENS)
        self.assertLess(threshold, prog.context_window)
        with tempfile.TemporaryDirectory() as d:
            d = Path(d)
            rc, _ = run_pi(prog, d, compact_before=[after])
            self.assertEqual(rc, 0)
            calls = [json.loads(x) for x in (d / "calls.jsonl").read_text().splitlines()]
            self.assertEqual(len(calls), len(prog.turns), "one call a turn, and no /compact")
            ids = []
            for t, c in zip(prog.turns, calls):
                a = c["argv"]
                self.assertEqual(c["cwd"], str((d / "run" / "workspace").resolve()))
                self.assertEqual(c["parent"], [], "a parent's variables are taken out")
                self.assertEqual(a[:3], ["--print", "--mode", "json"])
                self.assertEqual((a[a.index("--provider") + 1], a[a.index("--model") + 1]),
                                 ("anthropic", "claude-sonnet-5-5"))
                self.assertEqual(a[a.index("--tools") + 1], drive.PI_TOOLS)
                self.assertEqual(a[a.index("--thinking") + 1], "medium", "every arm's default is medium")
                self.assertEqual(c["prompt"], t.text)
                # No version check, no telemetry, and no overlay of newer catalog
                # data (its prices, its thinking map): the pinned Pi's own.
                self.assertEqual(c["env"], {"PI_SKIP_VERSION_CHECK": "1", "PI_TELEMETRY": "0", "PI_OFFLINE": "1"})
                sid = a[a.index("--session-id") + 1]
                if t.index == prog.session_turns(t.session)[0].index:
                    ids.append(sid)
                self.assertEqual(sid, ids[-1])
                # Pi compacts past its model's window less the reserve: at
                # the progression's window.
                self.assertEqual(c["settings"], {"compaction": {"modelOverrides": {
                    "anthropic/claude-sonnet-5-5": {"reserveTokens": drive.PI_MODEL_WINDOW - threshold,
                                                    "keepRecentTokens": threshold // 4}}}})
            self.assertEqual(len(set(ids)), 2)
            run = json.loads((d / "run" / "run.json").read_text())
            self.assertEqual((run["arm"], run["sessions"]), ("pi", ids))
            self.assertEqual(run["compactions"], [mark + 1])
            self.assertEqual(run["pi_compact"], {"window": prog.context_window, "model_window": drive.PI_MODEL_WINDOW,
                                                 "threshold": threshold,
                                                 "reserve_tokens": drive.PI_MODEL_WINDOW - threshold,
                                                 "keep_recent_tokens": threshold // 4})
            self.assertGreater(drive.PI_MODEL_WINDOW - threshold, drive.PI_MODEL_WINDOW - prog.context_window,
                               "a smaller overhead than the plan's is a smaller threshold: a larger reserve")
            # Pi's overhead, in Theseus's record's shape and the threshold it was set at.
            oh = run["overhead"]
            self.assertEqual((oh["planned"], oh["measured"], oh["past_cushion"], oh["threshold"]),
                             (drive.PI_OVERHEAD_TOKENS, drive.PI_OVERHEAD_TOKENS, False, threshold))
            self.assertEqual(set(oh) - {"threshold"}, set(drive.overhead_record(prog, 0, False)))
            self.assertEqual(run["delivered"], len(prog.facts))
            rows = [json.loads(x) for x in (d / "run" / "turns.jsonl").read_text().splitlines()]
            self.assertEqual(len(rows), len(prog.turns))
            self.assertTrue(all(r["exit"] == 0 and r["error"] is None for r in rows))
            scripted = [r for r, t in zip(rows, prog.turns) if "Run ./scripts/" in t.text]
            self.assertTrue(scripted)
            # A scripted turn's two answers, summed.
            self.assertTrue(all(r["cost_usd"] == 0.001 and r["tool_calls"] == 1 for r in scripted))
            self.assertEqual(scripted[0]["tokens"], {"input": 20, "output": 10, "cache_read": 200, "cache_write": 40})
            # The perfect arm scores perfectly.
            s = score.summarize(score.score_run(score.load_run(d / "run")))
            self.assertEqual((s["recall_accuracy"], s["abstention_accuracy"]), (1.0, 1.0))
            self.assertEqual(s["undelivered"] + s["failed"] + s["confident_wrong"], 0)

    def test_an_effort_is_pis_thinking_level_and_run_json_names_it(self):
        prog = generate.build(7, "smoke")
        with tempfile.TemporaryDirectory() as d:
            rc, _ = run_pi(prog, Path(d), argv=["--effort", "high"])
            self.assertEqual(rc, 0)
            calls = [json.loads(x) for x in (Path(d) / "calls.jsonl").read_text().splitlines()]
            self.assertEqual({c["argv"][c["argv"].index("--thinking") + 1] for c in calls}, {"high"})
            self.assertEqual(json.loads((Path(d) / "run" / "run.json").read_text())["effort"], "high")

    def test_the_threshold_is_the_windows_less_what_theseuss_overhead_holds_beyond_pis(self):
        prog = generate.build(7, "smoke")
        plan = generate.planned_overhead(prog)
        self.assertEqual(drive.pi_threshold(prog, 45000, plan), 45000, "at the plan's own overhead: the window")
        self.assertEqual(drive.pi_threshold(prog, 45000, plan - 11340), 45000 - 11340)
        self.assertEqual(drive.pi_keep_recent(33660), 8415)
        self.assertEqual(drive.pi_keep_recent(200_000), drive.PI_KEEP_RECENT)

    def test_a_pi_more_than_the_cushion_off_its_plan_is_refused_and_one_at_it_is_not(self):
        """Pi's overhead is held to its own plan as Theseus's is held to the
        progression's: 50 off, either way, runs; 51 off exits 3 naming both
        numbers and how to move the plan; `--allow-overhead` runs on."""
        prog = generate.build(7, "smoke")
        plan, cushion = drive.PI_OVERHEAD_TOKENS, generate.OVERHEAD_CUSHION
        for measured in (plan + cushion, plan - cushion):
            with tempfile.TemporaryDirectory() as d:
                rc, _ = run_pi(prog, Path(d), overhead=measured)
                self.assertEqual(rc, 0, measured)
                oh = json.loads((Path(d) / "run" / "run.json").read_text())["overhead"]
                self.assertEqual((oh["measured"], oh["past_cushion"]), (measured, False))
        for measured in (plan + cushion + 1, plan - cushion - 1):
            with tempfile.TemporaryDirectory() as d:
                d = Path(d)
                rc, err = run_pi(prog, d, overhead=measured)
                self.assertEqual(rc, 3, measured)
                self.assertIn(f"{measured:,}", err)
                self.assertIn(f"{plan:,}", err)
                self.assertIn(f"--pi-overhead {measured}", err)
                self.assertIn("--allow-overhead", err)
                rows = (d / "run" / "turns.jsonl").read_text().splitlines()
                self.assertEqual(len(rows), 1, "stopped after the first turn")
                oh = json.loads((d / "run" / "run.json").read_text())["overhead"]
                self.assertEqual((oh["measured"], oh["past_cushion"], oh["allowed"]), (measured, True, False))
            with tempfile.TemporaryDirectory() as d:
                rc, _ = run_pi(prog, Path(d), argv=["--allow-overhead"], overhead=measured)
                self.assertEqual(rc, 0, measured)
                run = json.loads((Path(d) / "run" / "run.json").read_text())
                self.assertEqual((run["turns"], run["overhead"]["past_cushion"], run["overhead"]["allowed"]),
                                 (len(prog.turns), True, True))
        # Moving the plan moves the threshold and the verdict.
        with tempfile.TemporaryDirectory() as d:
            rc, _ = run_pi(prog, Path(d), argv=["--pi-overhead", "2500"], overhead=2500)
            self.assertEqual(rc, 0)
            run = json.loads((Path(d) / "run" / "run.json").read_text())
            self.assertEqual(run["overhead"]["planned"], 2500)
            self.assertEqual(run["pi_compact"]["threshold"], prog.context_window - (prog.overhead_tokens - 2500))

    def test_a_turn_ends_failed_when_its_last_answer_is_a_providers_error(self):
        events = drive.pi_events("\n".join(json.dumps(x) for x in [
            {"type": "session", "id": "s"},
            {"type": "message_end", "message": {"role": "assistant", "content": [], "stopReason": "error",
                                                "errorMessage": "529 overloaded",
                                                "usage": {"input": 0, "output": 0, "cost": {"total": 0}}}}]))
        v = drive.pi_turn(events)
        self.assertEqual((v["stop"], v["error"], v["answers"], v["cost_usd"]), ("error", "529 overloaded", 1, 0.0))
        self.assertEqual(drive.pi_turn([])["cost_usd"], None)
        self.assertTrue(drive.pi_failed(0, v), "print mode exits 0 on a provider's error")
        # An aborted answer (a request cut off) fails the turn as an error does.
        aborted = drive.pi_turn(drive.pi_events(json.dumps(
            {"type": "message_end", "message": {"role": "assistant", "content": [], "stopReason": "aborted",
                                                "usage": {"input": 0, "output": 0, "cost": {"total": 0}}}})))
        self.assertEqual((aborted["stop"], aborted["answers"]), ("aborted", 1))
        self.assertTrue(drive.pi_failed(0, aborted), "print mode exits 0 on an aborted answer too")
        # And a turn that ended well, or answered nothing, is told from them.
        stopped = dict(aborted, stop="stop")
        self.assertFalse(drive.pi_failed(0, stopped))
        self.assertTrue(drive.pi_failed(0, dict(stopped, answers=0)))
        self.assertTrue(drive.pi_failed(1, stopped))
        # A compaction's own summary call is the turn's spend too.
        answer = {"role": "assistant", "content": [{"type": "toolCall", "id": "t1"}], "stopReason": "stop",
                  "usage": {"input": 100, "output": 10, "cacheRead": 50, "cost": {"total": 0.002}}}
        summary = {"summary": "s", "usage": {"input": 3000, "output": 400, "cacheWrite": 5, "cost": {"total": 0.01}}}
        v = drive.pi_turn([{"type": "compaction_end", "reason": "threshold", "result": summary},
                           {"type": "message_end", "message": answer}])
        self.assertEqual((v["tokens"], v["cost_usd"], v["tool_calls"], v["answers"]),
                         ({"input": 3100, "output": 410, "cache_read": 50, "cache_write": 5}, 0.012, 1, 1))
        self.assertEqual(drive.pi_keep_recent(45_000), 11_250)
        self.assertEqual(drive.pi_keep_recent(200_000), drive.PI_KEEP_RECENT)
        self.assertTrue(drive.pi_failed(0, drive.pi_turn([])))
        self.assertTrue(drive.pi_failed(None, dict(v, stop="stop")))
        self.assertFalse(drive.pi_failed(0, dict(v, stop="stop")))


def exec_done(p: subprocess.Popen, argv0: str, timeout: float = 10.0) -> None:
    """Wait until `p` runs `argv0`: between its fork and its exec, its
    command line is still this process's, and names nothing of the run."""
    deadline = time.monotonic() + timeout
    while True:
        try:
            cmd = Path(f"/proc/{p.pid}/cmdline").read_bytes().split(b"\0", 1)[0].decode()
        except OSError:
            cmd = ""
        if Path(cmd).name == argv0:
            return
        if time.monotonic() > deadline:
            raise AssertionError(f"{p.pid} never ran {argv0} (it runs {cmd!r})")
        time.sleep(0.01)


class LeftRunning(unittest.TestCase):
    def test_a_process_naming_the_run_or_working_in_it_is_found(self):
        # Each fixture in a session of its own, and its whole group killed:
        # `sh -c` may fork its `sleep`, and a child that outlives its shell,
        # caught between its fork and its exec, names the run (theseus-523y).
        with tempfile.TemporaryDirectory() as d:
            d = Path(d)
            (d / "workspace").mkdir()
            by_cwd = subprocess.Popen(["sleep", "30"], cwd=d / "workspace", start_new_session=True)
            elsewhere = subprocess.Popen(["sleep", "30"], cwd="/", start_new_session=True)
            named = subprocess.Popen(["sh", "-c", "sleep 30", str(d / "daemon" / "sock")], cwd="/",
                                     start_new_session=True)
            try:
                for p, argv0 in ((by_cwd, "sleep"), (elsewhere, "sleep"), (named, "sh")):
                    exec_done(p, argv0)
                found = {pid for pid, _ in drive.processes_naming(d)}
                self.assertIn(by_cwd.pid, found)
                self.assertIn(named.pid, found)
                self.assertNotIn(elsewhere.pid, found)
            finally:
                for p in (by_cwd, elsewhere, named):
                    try:
                        os.killpg(p.pid, signal.SIGKILL)
                    except ProcessLookupError:
                        pass
                    p.wait()
            self.assertEqual(drive.processes_naming(d), [])


class Timeouts(unittest.TestCase):
    def test_a_turn_past_its_timeout_is_killed_with_its_children(self):
        with tempfile.TemporaryDirectory() as d:
            t0 = time.monotonic()
            out, err, code = drive.run_group(["sh", "-c", "echo started; sleep 30 & sleep 30"], "", 0.5, cwd=d)
            self.assertLess(time.monotonic() - t0, 10)
            self.assertIsNone(code)
            self.assertIn("timed out", err)
            self.assertEqual(out, "started\n")
            time.sleep(0.2)
            self.assertEqual(drive.processes_naming(Path(d)), [])
            self.assertEqual(drive.run_group(["cat"], "x", 5), ("x", "", 0))


class TheseusConfig(unittest.TestCase):
    def test_the_daemons_config_is_the_bench_profile_with_the_arm_set(self):
        base = tomllib.loads(drive.PROFILE.read_text())
        for arm in ("none", "bm25", "baseline", "+synthesis"):
            t = drive.theseus_config(base, model="anthropic/claude-sonnet-5-5", memory_arm=arm,
                                     workspace=Path("/w"), window=32000, spend_limit=3, max_loops=9)
            back = tomllib.loads(drive.toml_dumps(t))
            self.assertEqual(back, t)
            self.assertEqual(back["memory"], {"mode": "live", "arm": arm})
            self.assertEqual(back["index"]["enabled"], arm != "none")
            self.assertFalse(back["discord"]["enabled"])
            self.assertFalse(back["web"]["enabled"])
            self.assertEqual(back["secrets"]["anthropic_api_key"], "env:ANTHROPIC_API_KEY")
            self.assertEqual(back["catalog"]["claude-sonnet-5-5"]["context_window"], 32000)
            self.assertEqual(back["tools"]["projects_dir"], "/w")
            self.assertNotIn("api_base", back["model"])
        # Without an effort the profile's is left as the bench profile has it; with one, it is that.
        kw = dict(model="anthropic/claude-sonnet-5-5", memory_arm="baseline", workspace=Path("/w"), window=32000,
                  spend_limit=3, max_loops=9)
        self.assertEqual(drive.theseus_config(base, **kw)["profiles"]["bench"].get("effort"),
                         base["profiles"]["bench"].get("effort"))
        for effort in drive.EFFORTS:
            t = drive.theseus_config(base, effort=effort, **kw)
            self.assertEqual(tomllib.loads(drive.toml_dumps(t))["profiles"]["bench"]["effort"], effort)
        t = drive.theseus_config({"mcp_server": {"enabled": True}}, model="m", memory_arm="bm25",
                                 workspace=Path("/w"), window=32000, spend_limit=3, max_loops=9,
                                 api_base="http://127.0.0.1:1")
        self.assertFalse(t["mcp_server"]["enabled"])
        self.assertEqual(t["model"]["api_base"], "http://127.0.0.1:1")


class Overhead(unittest.TestCase):
    def test_the_overhead_is_the_first_compile_less_its_user_message(self):
        """`OVERHEAD_TOKENS`' own definition: the earliest `context.compiled`
        estimate, less its user message at the model's rates."""
        text = "It's Monday, 2026-11-09. Hi."
        user = drive.tk.user_text(text).tokens(drive.tk.rates_of("anthropic/claude-sonnet-5-5"))
        rows = [{"position": 9, "data": {"est_tokens": 20000}}, {"position": 4, "data": {"est_tokens": 13650 + user}},
                {"position": 2, "kind": "other"}]
        self.assertEqual(drive.overhead_of(rows, text, "anthropic/claude-sonnet-5-5"), 13650)
        self.assertIsNone(drive.overhead_of([], text, "m"))

    def test_past_the_cushion_is_refused_and_both_numbers_are_named(self):
        prog = generate.build(7, "smoke")
        planned, cushion = prog.overhead_tokens, generate.OVERHEAD_CUSHION
        for measured in (planned + cushion, planned - cushion, planned):
            at = drive.overhead_record(prog, measured, False)
            self.assertEqual((at["planned"], at["past_cushion"]), (planned, False), measured)
        past = drive.overhead_record(prog, planned + cushion + 1, False)
        self.assertTrue(past["past_cushion"])
        said = drive.overhead_refusal(past)
        self.assertIn(f"{planned + cushion + 1:,}", said)
        self.assertIn(f"{planned:,}", said)
        self.assertIn("--allow-overhead", said)

    def test_a_daemon_under_the_plan_is_refused_like_one_over_it(self):
        """theseus-tqa3: a lower overhead thins the reads' crossing (smoke
        seed 12 crosses by 31 tokens 101 under the default plan, and not at
        150 under it), so 51 under is refused as 51 over is, and 50 under is
        not. The refusal names both numbers and how to plan at the measured."""
        prog = generate.build(7, "smoke")
        planned, cushion = prog.overhead_tokens, generate.OVERHEAD_CUSHION
        self.assertFalse(drive.overhead_record(prog, planned - cushion, False)["past_cushion"])
        under = drive.overhead_record(prog, planned - cushion - 1, False)
        self.assertTrue(under["past_cushion"])
        said = drive.overhead_refusal(under)
        self.assertIn(f"{planned - cushion - 1:,}", said)
        self.assertIn(f"under the {planned:,}", said)
        self.assertIn(f"by {cushion + 1}", said)
        self.assertIn(f"--overhead {planned - cushion - 1}", said)
        self.assertIn("--allow-overhead", said)
        over = drive.overhead_refusal(drive.overhead_record(prog, planned + cushion + 1, False))
        self.assertIn(f"past the {planned:,}", over)
        self.assertIn(f"--overhead {planned + cushion + 1}", over)
        # An older file, with no record, is held to today's constant.
        prog.overhead_tokens = None
        old = drive.overhead_record(prog, 20000, True)
        self.assertEqual((old["planned"], old["planned_recorded"], old["allowed"]),
                         (generate.OVERHEAD_TOKENS, False, True))


def bin_dir() -> Path | None:
    d = Path(os.environ.get("THESEUS_RECALL_BIN_DIR") or REPO / "target" / "debug")
    need = ("theseus", "theseusd", "theseus-index", "theseus-sim")
    return d if all((d / b).is_file() for b in need) else None


def free_port() -> int:
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def work_for(text: str) -> list[dict]:
    """The tool calls a model makes for a turn's words: a script run, each
    file read with `fs_read` (a bulk log whole, as a model reads it), the
    listing asked for."""
    calls = []
    for m in re.finditer(r"Run (\./scripts/[\w.-]+\.sh)", text):
        calls.append({"name": "proc_run", "input": {"argv": [m.group(1)]}})
    for m in re.finditer(r"\b((?:src|docs|logs)/[\w.-]+\.(?:py|md|log))\b", text):
        calls.append({"name": "fs_read", "input": {"path": m.group(1)}})
    if "Which file under src/ is the largest?" in text:
        calls.append({"name": "fs_list", "input": {"path": "src"}})
    return calls


# A model's reply after its work, about REPLY_BYTES long: no value, no
# admission.
REPLY = ("That's done: the output above says what it found, and it needs nothing more from me right now. "
         * 5)[:generate.REPLY_BYTES]


def rules_for(prog: pg.Progression) -> list[dict]:
    """The stand-in model's script for a progression: each turn's work as a
    model does it (`work_for`), and each probe a perfect arm's answer. The
    latest turn's rule comes first, so a recall note that quotes an earlier
    turn never takes its rule."""
    rules = []
    answers = answers_for(prog)
    for t in prog.turns:
        if t.role == "probe":
            a = answers[t.text]
            if a.get("file"):
                rules.append({"when": t.text, "calls": [{"name": "fs_write",
                                                         "input": {"path": a["file"], "content": a["content"]}}]})
            else:
                rules.append({"when": t.text, "text": a["reply"]})
        elif work_for(t.text):
            rules.append({"when": t.text, "calls": work_for(t.text)})
    return list(reversed(rules))


@unittest.skipIf(bin_dir() is None, "no theseus binaries (cargo build --workspace, or THESEUS_RECALL_BIN_DIR)")
class TheseusDriver(unittest.TestCase):
    def drive(self, d: Path, prog: pg.Progression, rules: list[dict], timeout: str = "120",
              counting: bool = False, extra: tuple[str, ...] = ()) -> tuple[int, str, Path]:
        """`prog` driven through a scratch daemon on a stand-in model:
        theseus-sim's, which reports 40 input tokens a call, or with
        `counting`, `standin.py`'s, which reports the request's estimate.
        What it said is its stdout and its stderr."""
        bins = bin_dir()
        gen = d / "gen"
        prog.save(gen)
        (d / "rules.json").write_text(json.dumps(rules))
        fake = counted = None
        if counting:
            counted = standin.StandIn(rules, REPLY)
            base = counted.base
        else:
            port = free_port()
            base = f"http://127.0.0.1:{port}"
            fake = subprocess.Popen([str(bins / "theseus-sim"), "fake-model", "--addr", f"127.0.0.1:{port}",
                                     "--rules", str(d / "rules.json")],
                                    stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        try:
            deadline = time.monotonic() + 10
            while fake is not None:
                try:
                    socket.create_connection(("127.0.0.1", port), 0.2).close()
                    break
                except OSError:
                    if time.monotonic() > deadline:
                        raise
                    time.sleep(0.05)
            out = d / "run"
            buf = io.StringIO()
            with redirect_stdout(buf), redirect_stderr(buf):
                rc = drive.main(["--arm", "theseus", "--memory-arm", "baseline", "--bin-dir", str(bins),
                                 "--progression", str(gen), "--out", str(out), "--turn-timeout", timeout,
                                 "--api-base", base, *extra])
        finally:
            if fake is not None:
                fake.kill()
                fake.wait()
            if counted is not None:
                counted.close()
        return rc, buf.getvalue(), out

    def test_a_turn_past_its_timeout_is_stopped_and_the_next_one_runs(self):
        # The last turn runs a job that outlasts the turn's timeout, and one
        # more turn follows it. A recall note may quote a turn's text, and the
        # stand-in takes the first rule whose text it finds anywhere, so the
        # turn after the slow one has its own rule first.
        prog = generate.build(7, "smoke")
        slow = prog.turns[-1]
        prog.turns.append(pg.Turn(index=slow.index + 1, session=slow.session, block=slow.block, topic=slow.topic,
                                  text="Last one: are we done for the day?", role="filler", est_tokens=300))
        rules = [{"when": prog.turns[-1].text, "text": "We are."},
                 {"when": slow.text, "calls": [{"name": "proc_run", "input": {"argv": ["sleep", "30"]}}]}]
        # Planned far under the daemon's overhead, it runs on with
        # --allow-overhead, and run.json says so.
        prog.overhead_tokens = 1000
        with tempfile.TemporaryDirectory() as d:
            rc, said, out = self.drive(Path(d), prog, rules + rules_for(prog), timeout="8",
                                       extra=("--allow-overhead",))
            self.assertEqual(rc, 0, said)
            o = json.loads((out / "run.json").read_text())["overhead"]
            self.assertEqual((o["planned"], o["past_cushion"], o["allowed"]), (1000, True, True), o)
            rows = {r["index"]: r for r in map(json.loads, (out / "turns.jsonl").read_text().splitlines())}
            self.assertIsNone(rows[slow.index]["exit"])
            self.assertIn("timed out", rows[slow.index]["error"])
            self.assertEqual([i for i, r in rows.items() if r["exit"] != 0], [slow.index])
            # It ran, and answered: it carries the stopped call's result, which
            # the stand-in answers "Done.", as it answers any tool result.
            self.assertIn(rows[slow.index + 1]["reply"], ("We are.", "Done."))
            run = json.loads((out / "run.json").read_text())
            # The stop took its job too: nothing works in the workspace.
            self.assertEqual(run["left_running"], [])

    def test_mains_sizing_fails_the_mark_on_the_counting_stand_in_as_it_did_live(self):
        """The smoke as main sized it (theseus-523y): a 35000 window, and one
        log of 6,944 tokens at four bytes a token, which `fs_read` shows at
        its 30,000-character cap. Counted as a provider counts, the mark's
        turn fails as the first live smoke's did: its newest exchange alone
        passes the budget. (On theseus-sim's stand-in, 40 tokens a call, the
        same progression never compacts, and every turn exits 0.)"""
        prog = generate.build(7, "smoke")
        prog.context_window = 35000
        lines, size, i = [], 0, 0
        while size < 6944 * 4:
            line = f"step {i:05d} basalt-compile-{i:04d} {10 + (i * 7919) % 4000} ms"
            lines.append(line)
            size += len(line) + 1
            i += 1
        prog.workspace["logs/build-1.log"] = {"content": "\n".join(lines) + "\n", "executable": False}
        del prog.workspace["logs/build-1b.log"]
        after = prog.turns[prog.marks()[0] + 1]
        after.text, after.role = "How many lines are in src/basalt.py?", "filler"
        with tempfile.TemporaryDirectory() as d:
            rc, said, out = self.drive(Path(d), prog, rules_for(prog), counting=True)
            self.assertEqual(rc, 0, said)
            rows = {r["index"]: r for r in map(json.loads, (out / "turns.jsonl").read_text().splitlines())}
            mark = rows[prog.marks()[0]]
            self.assertEqual(mark["exit"], 1, mark)
            self.assertIn("class=context_overage", mark["error"])
            self.assertIn("against the 22,154 the window leaves", mark["error"])

    _measured: int | None = None

    def measured(self) -> int:
        """The scratch daemon's system prompt and tools, from a run refused
        at its first turn (planned at 1,000), once for the class."""
        if TheseusDriver._measured is None:
            prog = generate.build(7, "smoke")
            prog.overhead_tokens = 1000
            with tempfile.TemporaryDirectory() as d:
                rc, said, out = self.drive(Path(d), prog, rules_for(prog), counting=True)
                self.assertEqual(rc, 3, said)
                TheseusDriver._measured = json.loads((out / "run.json").read_text())["overhead"]["measured"]
        return TheseusDriver._measured

    def test_a_progression_planned_under_the_daemons_overhead_is_refused(self):
        """theseus-dp3y: planned 500 tokens under the daemon's real overhead,
        the run stops after its first turn, before any probe can move,
        naming both numbers; run.json records both."""
        real = self.measured()
        prog = generate.build(7, "smoke")
        prog.overhead_tokens = real - 500
        with tempfile.TemporaryDirectory() as d:
            rc, said, out = self.drive(Path(d), prog, rules_for(prog), counting=True)
            self.assertEqual(rc, 3, said)
            self.assertIn(f"tools are {real:,} tokens, past the {real - 500:,}", said)
            # Every run's effort is medium unless it says otherwise: the daemon's config has it.
            self.assertEqual(tomllib.loads((out / "daemon" / "config.toml").read_text())["profiles"]["bench"]["effort"],
                             "medium")
            self.assertEqual(json.loads((out / "run.json").read_text())["effort"], "medium")
            run = json.loads((out / "run.json").read_text())
            self.assertEqual({k: run["overhead"][k] for k in ("planned", "measured", "past_cushion", "allowed")},
                             {"planned": real - 500, "measured": real, "past_cushion": True, "allowed": False})
            self.assertEqual(run["turns"], 1)
            # Only the first turn ran: no probe's.
            self.assertGreater(min(p.turn for p in prog.probes), 0)
            self.assertEqual(run["left_running"], [])
            self.assertEqual(drive.processes_naming(out), [])

    def test_the_smoke_runs_end_to_end_on_a_scratch_daemon_and_leaves_nothing(self):
        """On the counting stand-in, each turn's history costs what it would
        live: the mark (or the turn after it) compacts, and no turn fails
        (theseus-523y: the live smoke's mark was an overage). Planned at the
        daemon's own overhead, it runs (theseus-dp3y), and its written bounds
        hold there however thin the plan's margin."""
        real = self.measured()
        self.assertLessEqual(real, generate.OVERHEAD_TOKENS + generate.OVERHEAD_CUSHION,
                             "the daemon's system prompt grew past the plan: raise OVERHEAD_TOKENS")
        self.assertGreaterEqual(real, generate.OVERHEAD_TOKENS - generate.OVERHEAD_CUSHION,
                                "the daemon's system prompt shrank under the plan: lower OVERHEAD_TOKENS")
        prog = generate.build(7, "smoke", real)
        with tempfile.TemporaryDirectory() as d:
            d = Path(d)
            rc, said, out = self.drive(d, prog, rules_for(prog), counting=True)
            self.assertEqual(rc, 0, said)
            run = json.loads((out / "run.json").read_text())
            self.assertEqual({k: run["overhead"][k] for k in ("planned", "measured", "past_cushion")},
                             {"planned": real, "measured": real, "past_cushion": False})
            self.assertEqual(run["left_running"], [])
            self.assertEqual(run["killed"], [])
            self.assertEqual(drive.processes_naming(out), [])
            self.assertEqual(run["memory_arm"], "baseline")
            self.assertEqual(len(run["sessions"]), 2)
            rows = [json.loads(x) for x in (out / "turns.jsonl").read_text().splitlines()]
            self.assertEqual(len(rows), len(prog.turns))
            bad = [(r["index"], r["exit"], r["error"]) for r in rows if r["exit"] != 0]
            self.assertEqual(bad, [])
            self.assertEqual({r["session_id"] for r in rows}, set(run["sessions"]))
            # Every fact reached the daemon: said in a turn, or in the output
            # of a script it ran.
            delivered = json.loads((out / "delivered.json").read_text())
            self.assertEqual([k for k, v in delivered.items() if not v["delivered"]], [])
            cfg = tomllib.loads((out / "daemon" / "config.toml").read_text())
            self.assertEqual(cfg["memory"], {"mode": "live", "arm": "baseline"})
            # Where it compacted is the ledger's word, whatever the marks say;
            # the scorer measures by it.
            rows_c = run["compaction_rows"]
            self.assertEqual([r["turn"] for r in rows_c], run["compactions"])
            mark = prog.marks()[0]
            self.assertIn(run["compactions"][0], (mark, mark + 1), run["compaction_rows"])
            # Its summary fit beside the kept turns: a compaction, not a ring.
            self.assertEqual(rows_c[0]["outcomes"], ["compaction"], rows_c[0])
            for r in rows_c:
                self.assertEqual(r["outcomes"], [c["outcome"] for c in r["cuts"]], r)
                for c in r["cuts"]:
                    self.assertIn(c["outcome"], score.MOVING_OUTCOMES, r)
                    self.assertGreater(c["messages"], 0, r)
            s = score.summarize(score.score_run(score.load_run(out)))
            self.assertEqual(s["scored"], len(prog.probes))
            self.assertEqual((s["recall_accuracy"], s["abstention_accuracy"]), (1.0, 1.0))
            self.assertEqual(s["moved"], 0)


if __name__ == "__main__":
    unittest.main()
