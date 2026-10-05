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
import socket
import subprocess
import sys
import tempfile
import time
import tomllib
import unittest
from contextlib import redirect_stdout
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
sys.path.insert(0, str(HERE))

import drive  # noqa: E402
import generate  # noqa: E402
import progression as pg  # noqa: E402
import score  # noqa: E402

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


class LeftRunning(unittest.TestCase):
    def test_a_process_naming_the_run_or_working_in_it_is_found(self):
        with tempfile.TemporaryDirectory() as d:
            d = Path(d)
            (d / "workspace").mkdir()
            by_cwd = subprocess.Popen(["sleep", "30"], cwd=d / "workspace")
            elsewhere = subprocess.Popen(["sleep", "30"], cwd="/")
            named = subprocess.Popen(["sh", "-c", "sleep 30", str(d / "daemon" / "sock")], cwd="/")
            try:
                found = {pid for pid, _ in drive.processes_naming(d)}
                self.assertIn(by_cwd.pid, found)
                self.assertIn(named.pid, found)
                self.assertNotIn(elsewhere.pid, found)
            finally:
                for p in (by_cwd, elsewhere, named):
                    p.kill()
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
        t = drive.theseus_config({"mcp_server": {"enabled": True}}, model="m", memory_arm="bm25",
                                 workspace=Path("/w"), window=32000, spend_limit=3, max_loops=9,
                                 api_base="http://127.0.0.1:1")
        self.assertFalse(t["mcp_server"]["enabled"])
        self.assertEqual(t["model"]["api_base"], "http://127.0.0.1:1")


def bin_dir() -> Path | None:
    d = Path(os.environ.get("THESEUS_RECALL_BIN_DIR") or REPO / "target" / "debug")
    need = ("theseus", "theseusd", "theseus-index", "theseus-sim")
    return d if all((d / b).is_file() for b in need) else None


def free_port() -> int:
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def rules_for(prog: pg.Progression) -> list[dict]:
    """The stand-in model's script for a progression: each fact turn that
    runs a script runs it, a bulk turn reads its log, and each probe gets a
    perfect arm's answer. The latest turn's rule comes first, so a recall
    note that quotes an earlier turn never takes its rule."""
    rules = []
    answers = answers_for(prog)
    facts = prog.facts_by_id()
    for t in prog.turns:
        if t.role == "fact" and facts and any(f.turn == t.index and f.carrier in ("output", "error") for f in prog.facts):
            f = next(f for f in prog.facts if f.turn == t.index)
            rules.append({"when": t.text, "calls": [{"name": "proc_run", "input": {"argv": [f"./{f.source}"]}}]})
        elif t.role == "bulk":
            log = t.text.split()[1]
            rules.append({"when": t.text, "calls": [{"name": "proc_run", "input": {"argv": ["cat", log]}}]})
        elif t.role == "probe":
            a = answers[t.text]
            if a.get("file"):
                rules.append({"when": t.text, "calls": [{"name": "fs_write",
                                                         "input": {"path": a["file"], "content": a["content"]}}]})
            else:
                rules.append({"when": t.text, "text": a["reply"]})
    return list(reversed(rules))


@unittest.skipIf(bin_dir() is None, "no theseus binaries (cargo build --workspace, or THESEUS_RECALL_BIN_DIR)")
class TheseusDriver(unittest.TestCase):
    def drive(self, d: Path, prog: pg.Progression, rules: list[dict], timeout: str = "120") -> tuple[int, str, Path]:
        """`prog` driven through a scratch daemon on the stand-in model."""
        bins = bin_dir()
        gen = d / "gen"
        prog.save(gen)
        (d / "rules.json").write_text(json.dumps(rules))
        port = free_port()
        fake = subprocess.Popen([str(bins / "theseus-sim"), "fake-model", "--addr", f"127.0.0.1:{port}",
                                 "--rules", str(d / "rules.json")],
                                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        try:
            deadline = time.monotonic() + 10
            while True:
                try:
                    socket.create_connection(("127.0.0.1", port), 0.2).close()
                    break
                except OSError:
                    if time.monotonic() > deadline:
                        raise
                    time.sleep(0.05)
            out = d / "run"
            buf = io.StringIO()
            with redirect_stdout(buf):
                rc = drive.main(["--arm", "theseus", "--memory-arm", "baseline", "--bin-dir", str(bins),
                                 "--progression", str(gen), "--out", str(out), "--turn-timeout", timeout,
                                 "--api-base", f"http://127.0.0.1:{port}"])
        finally:
            fake.kill()
            fake.wait()
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
        with tempfile.TemporaryDirectory() as d:
            rc, said, out = self.drive(Path(d), prog, rules + rules_for(prog), timeout="8")
            self.assertEqual(rc, 0, said)
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

    def test_the_smoke_runs_end_to_end_on_a_scratch_daemon_and_leaves_nothing(self):
        prog = generate.build(7, "smoke")
        with tempfile.TemporaryDirectory() as d:
            d = Path(d)
            rc, said, out = self.drive(d, prog, rules_for(prog))
            self.assertEqual(rc, 0, said)
            run = json.loads((out / "run.json").read_text())
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
            rows_c = run.get("compaction_rows", [])
            self.assertEqual([r["turn"] for r in rows_c], run["compactions"])
            s = score.summarize(score.score_run(score.load_run(out)))
            self.assertEqual(s["scored"], len(prog.probes))
            self.assertEqual((s["recall_accuracy"], s["abstention_accuracy"]), (1.0, 1.0))


if __name__ == "__main__":
    unittest.main()
