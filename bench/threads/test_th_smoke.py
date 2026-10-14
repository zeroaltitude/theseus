"""threadbench's drivers and its stand-in smoke (theseus-9ofg): the arms table, the layouts, Claude Code's
sessions carried per stream against a stand-in `claude`, and A3 and A5 end to end on a scratch daemon
(`bench/recall/standin.py` counting like a provider) with the scorer and the report rendered into a scratch
directory. The Theseus tests are skipped when the binaries are missing (`cargo build --workspace`, or
THESEUS_RECALL_BIN_DIR). Nothing is committed under docs/benchmarks/.
"""

from __future__ import annotations

import io
import json
import os
import sys
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
sys.path.insert(0, str(HERE))

import th_format as tf  # noqa: E402
import standin  # noqa: E402
import th_drive as td  # noqa: E402
import th_generate as gen  # noqa: E402
import th_score as ts  # noqa: E402
import th_standin as tsi  # noqa: E402

SMOKE = gen.build(11, "smoke")

FAKE_CLAUDE = r'''#!/usr/bin/env python3
# A stand-in `claude -p --output-format json` for threadbench: a scripted answer and writes per prompt.
import json, os, sys
from pathlib import Path
args = sys.argv[1:]
if args == ["--version"]:
    print("2.1.290 (Claude Code)")
    sys.exit(0)
prompt = sys.stdin.read()
cfg = Path(os.environ["CLAUDE_CONFIG_DIR"])
cwd = Path.cwd()
logs = cfg / "projects" / "".join(c if c.isalnum() else "-" for c in str(cwd))
logs.mkdir(parents=True, exist_ok=True)
arg = lambda f: args[args.index(f) + 1] if f in args else None
sid = arg("--session-id") or arg("--resume")
with open(os.environ["STANDIN_LOG"], "a") as f:
    f.write(json.dumps({"flag": "--session-id" if "--session-id" in args else "--resume", "sid": sid}) + "\n")
log = logs / f"{sid}.jsonl"
if "--resume" in args and not log.exists():
    print(json.dumps({"type": "result", "subtype": "error", "is_error": True, "result": "no such session"}))
    sys.exit(1)
a = json.loads(Path(os.environ["STANDIN_ANSWERS"]).read_text()).get(prompt, {})
for w in a.get("writes", []):
    p = cwd / w["path"]
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(w["content"])
reply = a.get("reply", "ok")
with log.open("a") as f:
    f.write(json.dumps({"type": "user", "message": {"role": "user", "content": prompt}}) + "\n")
    f.write(json.dumps({"type": "assistant", "message": {"role": "assistant", "content": [{"type": "text", "text": reply}]}}) + "\n")
print(json.dumps({"type": "result", "subtype": "success", "is_error": False, "result": reply, "session_id": sid,
                  "usage": {"input_tokens": 10, "output_tokens": 5, "cache_read_input_tokens": 100,
                            "cache_creation_input_tokens": 20}, "total_cost_usd": 0.001, "duration_ms": 3,
                  "num_turns": 1}))
'''


def bin_dir() -> Path | None:
    d = Path(os.environ.get("THESEUS_RECALL_BIN_DIR") or REPO / "target" / "debug")
    need = ("theseus", "theseusd", "theseus-index")
    return d if all((d / b).is_file() for b in need) else None


class Arms(unittest.TestCase):
    def test_the_arms_that_need_threads_refuse_with_words(self):
        for arm in ("A1", "A1n", "A1p", "A2"):
            self.assertEqual(td.ARMS[arm]["requires"], "threads")
            with self.assertRaises(SystemExit) as cm:
                td.main(["--arm", arm, "--out", "/tmp/never"])
            msg = str(cm.exception)
            self.assertIn("needs threads", msg)
            self.assertIn("not built", msg)
            self.assertIn("A3", msg)
        self.assertEqual(td.MEASURABLE, ("A3", "A4", "A5", "A6", "A7"))

    def test_opencode_is_probed_and_its_answer_is_the_finding(self):
        p = td.probe_opencode("/no/such/opencode")
        self.assertFalse(p["measurable"])
        self.assertIn("could not be run", p["why"])
        with tempfile.TemporaryDirectory() as d:
            for name, text, ok in (("a", "Usage: opencode run [--session <id>] [--continue]\n", True),
                                   ("b", "Usage: opencode run [--continue]\n", False)):
                f = Path(d) / name
                f.write_text(f"#!/bin/sh\necho '{text.strip()}'\n")
                f.chmod(0o755)
                self.assertEqual(td.probe_opencode(str(f))["measurable"], ok, name)
        with self.assertRaises(SystemExit) as cm:
            td.main(["--arm", "A8", "--opencode", "/no/such/opencode", "--out", "/tmp/never"])
        self.assertIn("not measurable", str(cm.exception))

    def test_the_pins_are_the_harbor_arms(self):
        self.assertEqual(td.pinned("claude-code"), "2.1.290")
        self.assertEqual(td.pinned("pi"), "1.0.4")
        with tempfile.TemporaryDirectory() as d:
            f = Path(d) / "claude"
            f.write_text("#!/bin/sh\necho '2.1.296 (Claude Code)'\n")
            f.chmod(0o755)
            with self.assertRaises(SystemExit) as cm:
                td.check_pin("claude-code", str(f), False)
            self.assertIn("2.1.290", str(cm.exception))
            self.assertIn("2.1.296", td.check_pin("claude-code", str(f), True))


class Plan(unittest.TestCase):
    PLAN = REPO / "bench" / "plans" / "threadbench-t0r.json"

    def test_the_attempt_plan_pins_the_arms_and_the_progressions(self):
        sys.path.insert(0, str(td.HARBOR))
        import plan as hp
        plan = json.loads(self.PLAN.read_text())
        hp.check(plan, "threadbench-t0r")  # the Harbor plan's own rule: a name, and `k`
        self.assertEqual(plan["arms"], list(td.MEASURABLE))
        self.assertEqual(plan["probe_only"], ["A8"])
        self.assertEqual((plan["model"], plan["effort"], plan["size"]), ("anthropic/claude-sonnet-5-5", "medium", "standard"))
        import th_prefix
        for name, v in plan["progressions"].items():
            built = th_prefix.build(v["seed"]) if name == "prefix" else gen.build(v["seed"], "standard")
            self.assertEqual(built.digest(), v["digest"], f"regenerate the plan's {name} digest")
        self.assertEqual(plan["progressions"]["dev"]["seed"], gen.DEV_SEED)
        self.assertEqual(plan["progressions"]["held_out"]["seed"], gen.HELD_OUT_SEED)
        self.assertEqual((plan["budget_usd"]["expected_low"], plan["budget_usd"]["expected_high"],
                          plan["budget_usd"]["pause_with_after_run"]), (45, 75, 170))

    def test_a_run_refuses_an_arm_or_a_progression_the_plan_does_not_name(self):
        from unittest import mock
        sys.path.insert(0, str(td.HARBOR))
        import plan as hp
        plan = json.loads(self.PLAN.read_text())
        ok = {**plan, "file": "bench/plans/threadbench-t0r.json", "sha256": "x", "commit": "c", "head": "h"}
        dev = gen.build(gen.DEV_SEED, "standard")
        with mock.patch.object(hp, "load", return_value=ok):
            rec = td.check_plan(self.PLAN, "A3", dev)
            self.assertEqual(rec["commit"], "c")
            with self.assertRaises(SystemExit) as cm:
                td.check_plan(self.PLAN, "A8", dev)
            self.assertIn("does not name arm A8", str(cm.exception))
            with self.assertRaises(SystemExit) as cm:
                td.check_plan(self.PLAN, "A3", gen.build(3, "standard"))
            self.assertIn("not one the plan", str(cm.exception))
        with self.assertRaises(SystemExit) as cm:  # the real loader: not committed (or not clean) refuses
            with mock.patch.object(hp, "_git", return_value=type("R", (), {"returncode": 1, "stdout": ""})()):
                td.check_plan(self.PLAN, "A3", dev)
        self.assertIn("th_drive:", str(cm.exception))


class Layouts(unittest.TestCase):
    def test_one_session_or_one_per_stream_in_arrival_order(self):
        one = td.view(SMOKE, "one")
        self.assertEqual({t.session for t in one.turns}, {0})
        self.assertEqual(len(one.sessions), 1)
        per = td.view(SMOKE, "stream")
        self.assertEqual(len(per.sessions), 5)
        for t, m in zip(per.turns, SMOKE.messages):
            self.assertEqual(per.sessions[t.session].label, f"threads {m.stream}-0")
        # a stream's session is revisited, not reopened, when the owner returns to it
        seq = [t.session for t in per.turns]
        self.assertTrue(any(seq[i] != seq[i - 1] and seq[i] in seq[:i] for i in range(1, len(seq))))
        # each day's first message opens with the date, in every layout
        for v in (one, per):
            firsts = [t.text for t, m in zip(v.turns, SMOKE.messages) if m.index == 0 or m.day != SMOKE.messages[m.index - 1].day]
            self.assertTrue(all(x.startswith("It's ") for x in firsts), firsts)
            self.assertTrue(all(not t.text.startswith("It's") for t, m in zip(v.turns, SMOKE.messages)
                                if m.index and m.day == SMOKE.messages[m.index - 1].day))
        self.assertEqual([f.marker for f in one.facts], [f.value for f in SMOKE.facts])

    def test_the_prefix_tracks_cold_gap_starts_a_fresh_session(self):
        import th_prefix as pf
        p = pf.build(11)
        per = td.view(p, "stream")
        one = td.view(p, "one")
        self.assertGreater(len(per.sessions), len(p.streams))
        self.assertEqual(len(one.sessions), 2)


@unittest.skipIf(not (REPO / "bench" / "recall").is_dir(), "no recall bench")
class ClaudeCodeSessions(unittest.TestCase):
    def run_arm(self, d: Path, arm: str, prog: tf.Progression = SMOKE) -> tuple[int, Path, list[dict]]:
        fake = d / "claude"
        fake.write_text(FAKE_CLAUDE)
        fake.chmod(0o755)
        layout = td.ARMS[arm]["layout"]
        v = td.view(prog, layout)
        plan = tsi.plan(prog)
        answers = {t.text: plan[t.index] for t in v.turns}
        (d / "answers.json").write_text(json.dumps(answers))
        gen_dir = d / "gen"
        prog.save(gen_dir)
        old = {k: os.environ.get(k) for k in ("STANDIN_LOG", "STANDIN_ANSWERS")}
        os.environ.update(STANDIN_LOG=str(d / "calls.jsonl"), STANDIN_ANSWERS=str(d / "answers.json"))
        try:
            buf = io.StringIO()
            with redirect_stdout(buf), redirect_stderr(buf):
                rc = td.main(["--arm", arm, "--claude", str(fake), "--progression", str(gen_dir), "--out", str(d / "run"),
                              "--model", "anthropic/claude-sonnet-5-5"])
        finally:
            for k, val in old.items():
                os.environ.pop(k, None) if val is None else os.environ.__setitem__(k, val)
        calls = [json.loads(x) for x in (d / "calls.jsonl").read_text().splitlines()]
        return rc, d / "run", calls

    def test_a_session_per_stream_is_resumed_not_reopened(self):
        with tempfile.TemporaryDirectory() as d:
            rc, run, calls = self.run_arm(Path(d), "A6")
            self.assertEqual(rc, 0)
            self.assertEqual(len(calls), len(SMOKE.messages))
            by: dict[str, list[str]] = {}
            for m, c in zip(SMOKE.messages, calls):
                by.setdefault(m.stream, []).append((c["flag"], c["sid"]))
            for s, rows in by.items():
                self.assertEqual(rows[0][0], "--session-id", s)
                self.assertTrue(all(f == "--resume" for f, _ in rows[1:]), s)
                self.assertEqual(len({sid for _, sid in rows}), 1, s)
            self.assertEqual(len({c["sid"] for c in calls}), 5)
            meta = json.loads((run / "run.json").read_text())
            self.assertEqual((meta["arm_id"], meta["layout"], len(meta["sessions"])), ("A6", "stream", 5))
            self.assertIn("resources", meta)
            sc = ts.score_run(ts.load_run(run))
            self.assertEqual(sc["oracle_mean"], 1.0)
            self.assertEqual(sc["embargo"]["leak_count"], 0)
            self.assertEqual(sc["contamination"]["k"], 0)
            self.assertEqual(sc["excluded"], 0)
            self.assertTrue(all(r["ok"] for r in sc["probes"]), [r for r in sc["probes"] if not r["ok"]])
            self.assertEqual(sc["routing"], {"owner_routed": True})

    def test_the_prefix_track_reopens_each_family_stream_as_a_fresh_session_and_counts_false_fits_apart(self):
        import th_prefix
        prog = th_prefix.build(11)
        with tempfile.TemporaryDirectory() as d:
            rc, run, calls = self.run_arm(Path(d), "A6", prog)
            self.assertEqual(rc, 0)
            self.assertEqual(len({c["sid"] for c in calls}), 5)  # alpha, alpha again, beta, beta again, the sibling
            first = {}
            for m, c in zip(prog.messages, calls):
                first.setdefault((m.stream, c["sid"]), c["flag"])
            self.assertTrue(all(f == "--session-id" for f in first.values()))
            sc = ts.score_run(ts.load_run(run))
            self.assertEqual(sc["oracle_mean"], 1.0, sc["streams"])
            self.assertEqual(sc["track"], "prefix")
            self.assertEqual((sc["header"]["n"], sc["header"]["false_fit"], sc["header"]["other_wrong"]), (4, 0, 0))
            self.assertEqual(sc["header"]["correct"], 4)
            self.assertEqual(sc["embargo"]["leak_count"], 0)
            rc, run, calls = self.run_arm(Path(d) / "one", "A4", prog) if (Path(d) / "one").mkdir() is None else (0, 0, [])
            self.assertEqual(len({c["sid"] for c in calls}), 2)  # one cold gap, one fresh session

    def test_one_session_starts_once_and_resumes_every_turn(self):
        with tempfile.TemporaryDirectory() as d:
            rc, run, calls = self.run_arm(Path(d), "A4")
            self.assertEqual(rc, 0)
            self.assertEqual(calls[0]["flag"], "--session-id")
            self.assertTrue(all(c["flag"] == "--resume" for c in calls[1:]))
            self.assertEqual(len({c["sid"] for c in calls}), 1)


class PiArm(unittest.TestCase):
    def test_a7_is_one_session_planned_from_the_theseus_overhead(self):
        import test_drive as rtd  # the recall bench's stand-in `pi` and its helpers
        with tempfile.TemporaryDirectory() as d:
            d = Path(d)
            bin_ = d / "bin"
            bin_.mkdir()
            (bin_ / "pi-real").write_text(rtd.PI_STANDIN)
            (bin_ / "pi-real").chmod(0o755)
            (bin_ / "pi").write_text(f'#!/bin/sh\n[ "$1" = "--version" ] && {{ echo 1.0.4; exit 0; }}\nexec {bin_}/pi-real "$@"\n')
            (bin_ / "pi").chmod(0o755)
            gen_dir = d / "gen"
            SMOKE.save(gen_dir)
            plan = tsi.plan(SMOKE)
            v = td.view(SMOKE, "one")
            (d / "answers.json").write_text(json.dumps({t.text: plan[t.index] for t in v.turns}))
            env = {"STANDIN_LOG": str(d / "calls.jsonl"), "STANDIN_ANSWERS": str(d / "answers.json"),
                   "STANDIN_COMPACT_BEFORE": "[]",
                   "STANDIN_COUNTED": json.dumps({
                       td.rd.PROBE_TEXT: rtd.pi_input(td.rd.PROBE_TEXT, rtd.PI_OVERHEAD),
                       v.turns[0].text: rtd.pi_input(v.turns[0].text, rtd.PI_OVERHEAD)})}
            old = {k: os.environ.get(k) for k in env}
            os.environ.update(env)
            try:
                buf = io.StringIO()
                with redirect_stdout(buf), redirect_stderr(buf):
                    rc = td.main(["--arm", "A7", "--pi", str(bin_ / "pi"), "--progression", str(gen_dir),
                                  "--out", str(d / "run"), "--theseus-overhead", "13640"])
            finally:
                for k, val in old.items():
                    os.environ.pop(k, None) if val is None else os.environ.__setitem__(k, val)
            self.assertEqual(rc, 0, buf.getvalue())
            meta = json.loads((d / "run" / "run.json").read_text())
            self.assertEqual((meta["arm_id"], meta["harness"], len(meta["sessions"])), ("A7", "pi", 1))
            self.assertEqual(meta["overhead"]["planned"], rtd.PI_OVERHEAD)
            rows = [json.loads(x) for x in (d / "run" / "turns.jsonl").read_text().splitlines()]
            self.assertEqual(len(rows), len(SMOKE.messages))
            # the window is Sonnet 5.5's, so Pi's compaction threshold is that less what the plan holds beyond Pi's
            self.assertEqual(meta["pi_compact"]["threshold"], td.WINDOW - (13640 - rtd.PI_OVERHEAD))
            with self.assertRaises(SystemExit) as cm:
                td.main(["--arm", "A7", "--pi", str(bin_ / "pi"), "--progression", str(gen_dir), "--out", str(d / "none")])
            self.assertIn("--theseus-overhead", str(cm.exception))


@unittest.skipIf(bin_dir() is None, "no theseus binaries (cargo build --workspace, or THESEUS_RECALL_BIN_DIR)")
class TheseusSmoke(unittest.TestCase):
    """A3 and A5 end to end on a scratch daemon with the counting stand-in model: 5 streams, about 8 turns each."""

    def run_arm(self, d: Path, arm: str) -> tuple[int, str, Path]:
        rules = tsi.rules_for(SMOKE)
        self.assertEqual(tsi.ambiguous_rules(rules), [])
        gen_dir = d / "gen"
        SMOKE.save(gen_dir)
        model = standin.StandIn(rules, tsi.REPLY)
        try:
            out = d / "run"
            buf = io.StringIO()
            with redirect_stdout(buf), redirect_stderr(buf):
                rc = td.main(["--arm", arm, "--bin-dir", str(bin_dir()), "--progression", str(gen_dir), "--out", str(out),
                              "--api-base", model.base, "--turn-timeout", "120", "--spend-limit", "5"])
        finally:
            model.close()
        return rc, buf.getvalue(), out

    def test_a3_and_a5_run_end_to_end_score_and_render(self):
        import report as rep
        scored = {}
        with tempfile.TemporaryDirectory() as d:
            d = Path(d)
            for arm in ("A3", "A5"):
                (d / arm).mkdir()
                rc, said, out = self.run_arm(d / arm, arm)
                self.assertEqual(rc, 0, said)
                meta = json.loads((out / "run.json").read_text())
                self.assertEqual(meta["left_running"], [])
                self.assertEqual(meta["killed"], [])
                self.assertEqual(len(meta["sessions"]), 1 if arm == "A3" else 5, arm)
                rows = [json.loads(x) for x in (out / "turns.jsonl").read_text().splitlines()]
                self.assertEqual([r["exit"] for r in rows], [0] * len(SMOKE.messages), arm)
                delivered = json.loads((out / "delivered.json").read_text())
                self.assertEqual([k for k, v in delivered.items() if not v["delivered"]], [], arm)
                sc = ts.score_run(ts.load_run(out))
                self.assertEqual(sc["oracle_mean"], 1.0, (arm, sc["streams"]))
                self.assertEqual(sc["embargo"]["leak_count"], 0)
                self.assertEqual(sc["contamination"]["k"], 0)
                self.assertEqual(sc["excluded"], 0)
                bad = [r for r in sc["probes"] if not r["ok"]]
                self.assertEqual(bad, [], arm)
                self.assertIsNotNone(sc["tokens"]["request_mean"])
                scored[arm] = sc
            # One session's requests carry the whole history; a session per stream carries its own stream's only.
            self.assertLess(scored["A5"]["tokens"]["request_mean"], scored["A3"]["tokens"]["request_mean"])
            self.assertGreater(scored["A3"]["tokens"]["request_curve"][-1], scored["A3"]["tokens"]["request_curve"][2])
            # The report renders into a scratch directory.
            doc = rep.write_report([scored["A3"], scored["A5"]], SMOKE, d / "docs", slug="smoke", date="2026-10-14",
                                   scratch=True)
            self.assertTrue((d / "docs" / "2026-10-14-threadbench-smoke.md").is_file())
            self.assertTrue(list((d / "docs" / "img" / "2026-10-14-threadbench-smoke").glob("*.svg")))
            self.assertIn("Improvement assessment", doc)


if __name__ == "__main__":
    unittest.main()
