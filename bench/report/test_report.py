"""Tests for the efficiency report (theseus-7gir.12).

    python3 -m unittest discover -s bench/report

Fixture job directories, three arms, and every number worked by hand in the
comments:

- **alpha**, sampled: 4 trials, 2 solved, one timed out with no reward;
  $0.60; 6,300 tokens; harness 4.0 s over 20 tool calls.
- **beta**, sampled: 2 trials, both solved; $0.80; 20,200 tokens.
- **gamma**, an old job (no records): one trial with Theseus's own files,
  one with only its trajectory; Harbor's dollars, $0.60; not sampled.

Two Pi jobs beside them (theseus-jp9p): one whose records hold the other
arms' caps, one trial past them, and an old trial read from Pi's session log.
"""

from __future__ import annotations

import contextlib
import importlib.util
import io
import json
import sys
import tempfile
import unittest
import xml.etree.ElementTree as ET
from pathlib import Path

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("efficiency_report", HERE / "efficiency.py")
rp = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = rp
spec.loader.exec_module(rp)
ef = rp.ef

SVG = "{http://www.w3.org/2000/svg}"


def toks(i, r, w, o):
    return {"input": i, "cache_read": r, "cache_write": w, "output": o}


def rec(cost, tokens, calls, tools, h_cpu, rss, work_cpu=10.0):
    return {"schema": ef.SCHEMA, "arm": "x", "tokens": tokens, "by_model": {}, "cost_usd": cost,
            "spend_from": "turn", "model_calls": calls, "tool_calls": tools, "wall_s": None,
            "wall_from": None,
            "harness": {"cpu_s": h_cpu, "peak_rss_kb": rss, "peak_hwm_kb": rss, "processes": 2},
            "work": {"cpu_s": work_cpu, "peak_rss_kb": 1, "peak_hwm_kb": 1, "cpu_from": "cgroup"},
            "wrappers": None, "container": None,
            "sampler": {"status": "ok", "reason": None, "interval_ms": 250, "samples": 10}}


def trial(job: Path, name: str, reward, wall_s: int, *, record=None, harbor=None, error=None,
          files: dict | None = None, in_metadata=False):
    d = job / name
    (d / "agent").mkdir(parents=True)
    agent_result = dict(harbor or {})
    if record is not None and in_metadata:
        agent_result["metadata"] = {"efficiency": record}
    elif record is not None:
        (d / "agent" / ef.RECORD).write_text(json.dumps(record))
    for fname, content in (files or {}).items():
        (d / "agent" / fname).write_text(json.dumps(content))
    result = {
        "task_name": name.rsplit("__", 1)[0], "trial_name": name,
        "agent_result": agent_result,
        "verifier_result": None if reward is None else {"rewards": {"reward": reward}},
        "exception_info": None if error is None else {"exception_type": error},
        "agent_execution": {"started_at": "2026-10-05T10:00:00.000000Z",
                            "finished_at": f"2026-10-05T10:{wall_s // 60:02d}:{wall_s % 60:02d}.000000Z"},
    }
    (d / "result.json").write_text(json.dumps(result))


TURN = {"turn_id": "turn_1", "model": "claude-sonnet-5-5", "cost_usd": 0.03, "tool_calls": 1,
        "usage": {"input_tokens": 150, "output_tokens": 60, "cache_read_input_tokens": 2500,
                  "cache_creation_input_tokens": 500}}
TRAJECTORY = {"agent": {"model_name": "claude-sonnet-5-5"}, "final_metrics": {"total_cost_usd": 0.03},
              "steps": [{"source": "user"},
                        {"source": "agent", "tool_calls": [{"tool_call_id": "t1"}],
                         "metrics": {"prompt_tokens": 1600, "cached_tokens": 1000, "completion_tokens": 40,
                                     "extra": {"cache_creation_input_tokens": 500}}},
                        {"source": "agent", "metrics": {"prompt_tokens": 1550, "cached_tokens": 1500,
                                                        "completion_tokens": 20}}]}


def build(root: Path) -> list[tuple[str, Path]]:
    alpha, beta, gamma = root / "alpha", root / "beta", root / "gamma"
    for j in (alpha, beta, gamma):
        j.mkdir()
        (j / "result.json").write_text(json.dumps({"id": "a job's own result"}))
    (alpha / "not-a-trial").mkdir()
    trial(alpha, "fix-git__a1", 1.0, 60, record=rec(0.10, toks(100, 800, 100, 50), 5, 4, 0.8, 40000))
    trial(alpha, "build-pmars__a2", 1.0, 120, record=rec(0.20, toks(200, 1600, 200, 100), 10, 8, 1.6, 50000))
    trial(alpha, "dna-insert__a3", 0.0, 90, record=rec(0.10, toks(100, 800, 100, 50), 5, 4, 0.8, 45000),
          in_metadata=True)
    trial(alpha, "qemu-startup__a4", None, 150, error="AgentTimeoutError",
          record=rec(0.20, toks(200, 1600, 200, 100), 10, 4, 0.8, 55000))
    for n in ("fix-git__b1", "build-pmars__b2"):
        trial(beta, n, 1.0, 240, record=rec(0.40, toks(50, 9000, 950, 100), 20, 10, 3.0, 300000))
    trial(gamma, "fix-git__g1", 0.0, 30, harbor={"cost_usd": 0.30, "n_input_tokens": 3150},
          files={"theseus-turn.json": TURN})
    trial(gamma, "build-pmars__g2", 1.0, 30, harbor={"cost_usd": 0.30},
          files={"trajectory.json": TRAJECTORY})
    return [("alpha", alpha), ("beta", beta), ("gamma", gamma)]


class Report(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.arms = build(self.root)
        self.out = self.root / "out"
        self.result = rp.report(self.arms, self.out)
        self.by = {a["arm"]: a for a in self.result["arms"]}

    def tearDown(self):
        self.tmp.cleanup()

    def test_an_arms_numbers_are_worked_by_hand(self):
        a = self.by["alpha"]
        self.assertEqual((a["trials"], a["solved"], a["errors"], a["sampled"]), (4, 2, 1, 4))
        self.assertEqual(a["mean_reward"], 0.5, "a trial with no reward counts 0")
        self.assertAlmostEqual(a["cost_usd"], 0.60)
        self.assertAlmostEqual(a["cost_per_trial"], 0.15)
        self.assertAlmostEqual(a["solved_per_dollar"], 2 / 0.60)
        self.assertEqual(a["tokens"], toks(600, 4800, 600, 300))
        self.assertEqual(a["tokens_per_trial"], 6300 / 4)
        self.assertEqual(a["tokens_per_solved"], {"input": 300, "cache_read": 2400,
                                                  "cache_write": 300, "output": 150})
        self.assertAlmostEqual(a["cache_hit_share"], 4800 / 6000)
        self.assertEqual((a["model_calls_per_trial"], a["tool_calls_per_trial"]), (7.5, 5.0))
        # 0.8 + 1.6 + 0.8 + 0.8 = 4.0 s of harness over 4 + 8 + 4 + 4 = 20 calls.
        self.assertAlmostEqual(a["harness_cpu_ms_per_tool_call"], 200.0)
        self.assertAlmostEqual(a["peak_harness_rss_mb"], 55000 / 1024)
        self.assertAlmostEqual(a["mean_peak_harness_rss_mb"], 47500 / 1024)
        self.assertEqual(a["wall_s_per_trial"], (60 + 120 + 90 + 150) / 4)
        b = self.by["beta"]
        self.assertAlmostEqual(b["harness_cpu_ms_per_tool_call"], 300.0)
        self.assertEqual((b["mean_reward"], b["tokens_per_trial"]), (1.0, 10100))

    def test_an_old_job_reports_what_harbor_kept(self):
        """No records: Harbor's dollars, the cache write from Theseus's own
        turn and from the trajectory, CPU and RAM not sampled."""
        g = self.by["gamma"]
        self.assertEqual((g["trials"], g["solved"], g["sampled"]), (2, 1, 0))
        self.assertAlmostEqual(g["cost_usd"], 0.60)
        self.assertEqual(g["tokens"], toks(300, 5000, 1000, 120))
        self.assertTrue(g["writes_kept"])
        self.assertIsNone(g["harness_cpu_ms_per_tool_call"])
        self.assertIsNone(g["mean_peak_harness_rss_mb"])
        md = (self.out / "report.md").read_text()
        self.assertIn("| Harness CPU per tool call | 200.0 ms | 300.0 ms | not sampled |", md)
        self.assertIn("Left out (not sampled): gamma.", md)

    def test_an_old_trial_with_empty_files_has_unknown_calls_and_the_mean_is_the_rest(self):
        """A Theseus trial whose turn and history files were left empty read
        0 model calls; unknown now, so the arm's mean is over the others."""
        job = self.root / "epsilon"
        job.mkdir()
        trial(job, "fix-git__e1", 1.0, 30, files={"theseus-turn.json": dict(TURN, loops=8)})
        trial(job, "build-pmars__e2", 1.0, 30, files={"theseus-turn.json": {}})
        trial(job, "dna-insert__e3", 0.0, 30, files={"theseus-turn.json": {}})
        for n in ("build-pmars__e2", "dna-insert__e3"):
            for f in ("theseus-turn.json", "theseus-history.json"):
                (job / n / "agent" / f).write_text("")
        trials = {t["trial"]: t["record"] for t in rp.load_job(job)}
        self.assertEqual(trials["fix-git__e1"]["model_calls"], 8)
        for n in ("build-pmars__e2", "dna-insert__e3"):
            self.assertIsNone(trials[n]["model_calls"])
            self.assertIsNone(trials[n]["tool_calls"])
        e = rp.summarize("epsilon", rp.load_job(job))
        self.assertEqual((e["model_calls_per_trial"], e["tool_calls_per_trial"]), (8.0, 1.0))

    def test_harbors_three_counters_alone_say_the_write_was_not_kept(self):
        job = self.root / "delta"
        job.mkdir()
        trial(job, "fix-git__d1", 1.0, 30,
              harbor={"cost_usd": 0.05, "n_input_tokens": 1000, "n_cache_tokens": 800, "n_output_tokens": 9})
        d = rp.summarize("delta", rp.load_job(job))
        self.assertFalse(d["writes_kept"])
        self.assertEqual(d["tokens"], toks(200, 800, 0, 9))
        self.assertIn("not kept", rp.markdown([d], [("delta", str(job))]))

    def test_the_front_is_every_arm_no_other_dominates(self):
        self.assertEqual(rp.front([("alpha", 0.15, 0.5), ("beta", 0.40, 1.0), ("gamma", 0.30, 0.5)]),
                         {"alpha", "beta"})
        self.assertEqual(rp.front([("a", 1, 0.5), ("b", 1, 0.5)]), {"a", "b"}, "a tie dominates neither")
        self.assertEqual(rp.front([("a", 1, 0.5), ("b", 1, 0.6)]), {"b"})
        self.assertEqual(rp.front([("a", 2, 0.9), ("b", 1, 0.5), ("c", 3, 0.95)]), {"a", "b", "c"})
        md = (self.out / "report.md").read_text()
        self.assertIn("| alpha | 0.500 | $0.150 | yes |", md)
        self.assertIn("| gamma | 0.500 | $0.300 |  |", md)
        self.assertIn("| beta | 1.000 | $0.400 | yes |", md)
        self.assertIn("| gamma | 0.500 | 3,210 |  |", md)  # (3,150 + 60) × 2 / 2 a trial

    def test_three_charts_that_parse_one_point_per_arm(self):
        for _, _, _, file in rp.PARETO:
            for f in (file, file.replace(".svg", "-dark.svg")):
                root = ET.parse(self.out / f).getroot()
                self.assertEqual(root.tag, SVG + "svg")
                dots = [c for c in root.iter(SVG + "circle") if c.get("r") == "6"]
                rings = [c for c in root.iter(SVG + "circle") if c.get("r") == "10"]
                labels = {t.text for t in root.iter(SVG + "text")}
                want = {"alpha", "beta"} if file == "pareto-ram.svg" else {"alpha", "beta", "gamma"}
                self.assertEqual(len(dots), len(want), f)
                self.assertTrue(want <= labels, f)
                self.assertEqual(len(rings), 2, f"{f}: the front, alpha and beta, is ringed")
                hover = [g.find(SVG + "title").text for g in root.iter(SVG + "g") if g.find(SVG + "title") is not None]
                for name in want:
                    self.assertTrue(any(h.startswith(name + ":") for h in hover), f"{f}: {name} has its hover text")
        md = (self.out / "report.md").read_text()
        for _, _, _, file in rp.PARETO:
            self.assertIn(f"({file})", md)

    def test_an_arm_outside_the_palette_is_named_in_the_legend(self):
        pts = [("theseus", 0.14, 0.72), ("claude-code", 0.13, 0.81), ("pi", 0.10, 0.70)]
        texts = [t.text for t in ET.fromstring(rp.svg("Score", "dollars", pts, rp.front(pts))).iter(SVG + "text")]
        self.assertEqual(texts.count("pi"), 2, "its point's label and its legend entry")
        self.assertNotIn("other arm", texts)

    def test_the_command_line_and_the_trials_csv(self):
        out = self.root / "cli"
        argv = [f"--arm={n}={p}" for n, p in self.arms] + ["--out", str(out)]
        with contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(rp.main(argv), 0)
        self.assertTrue((out / "report.md").exists())
        rows = (out / "trials.csv").read_text().splitlines()
        self.assertEqual(len(rows), 1 + 4 + 2 + 2)
        self.assertTrue(rows[0].startswith("arm,trial,task,reward"))
        with self.assertRaises(SystemExit), contextlib.redirect_stderr(io.StringIO()):
            rp.main(["--arm", "nojob", "--out", str(out)])

    def test_dollars_read_as_cents_and_a_small_run_not_as_free(self):
        self.assertEqual((rp._dollars(24.72), rp._dollars(0.6), rp._dollars(0.00064), rp._dollars(0)),
                         ("$24.72", "$0.60", "$0.0006", "$0.00"))
        self.assertIn("| Cost, total | $0.60 | $0.80 | $0.60 |", (self.out / "report.md").read_text())

    def test_a_pi_arm_counts_its_trials_past_the_others_caps(self):
        """Pi enforces no cap: its records hold the others' and whether each
        trial passed them, and only then does the report say so."""
        self.assertNotIn("caps", (self.out / "report.md").read_text())
        job = self.root / "pi"
        job.mkdir()
        for n, cost, over in (("fix-git__p1", 0.10, False), ("build-pmars__p2", 2.50, True)):
            r = dict(rec(cost, toks(100, 800, 100, 50), 5, 4, 0.5, 100000), arm="pi")
            r["limits"] = {"enforced": False, "max_budget_usd": 2.0, "max_turns": 200, "answers": 5,
                           "over_budget": over, "over_turns": False}
            trial(job, n, 1.0, 60, record=r)
        result = rp.report(self.arms + [("pi", job)], self.root / "with-pi")
        p = {a["arm"]: a for a in result["arms"]}["pi"]
        self.assertEqual((p["past_caps"], p["solved"]), (1, 2))
        self.assertIsNone({a["arm"]: a for a in result["arms"]}["alpha"]["past_caps"])
        md = (self.root / "with-pi" / "report.md").read_text()
        self.assertIn("| Trials past the others' caps (not enforced) | – | – | – | 1/2 |", md)
        self.assertIn("| pi | 1.000 | $1.300 |", md)

    def test_an_old_pi_trial_is_read_from_its_session_log(self):
        job = self.root / "pi-old"
        job.mkdir()
        trial(job, "fix-git__q1", 1.0, 30, harbor={"cost_usd": 0.0058})
        logs = job / "fix-git__q1" / "agent" / "pi" / "sessions"
        logs.mkdir(parents=True)
        cost = {"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0}
        lines = [{"type": "session", "version": 3, "id": "s", "timestamp": "2026-10-05T10:00:00.000Z"},
                 {"type": "message", "id": "a1", "timestamp": "2026-10-05T10:00:01.000Z", "message": {
                     "role": "assistant", "model": "claude-sonnet-5-5", "stopReason": "toolUse",
                     "content": [{"type": "toolCall", "id": "t1", "name": "bash", "arguments": {}}],
                     "usage": {"input": 120, "output": 45, "cacheRead": 0, "cacheWrite": 1800,
                               "cost": dict(cost, total=0.00519)}}},
                 {"type": "message", "id": "a2", "timestamp": "2026-10-05T10:00:02.000Z", "message": {
                     "role": "assistant", "model": "claude-sonnet-5-5", "stopReason": "stop", "content": [],
                     "usage": {"input": 60, "output": 12, "cacheRead": 1800, "cacheWrite": 0,
                               "cost": dict(cost, total=0.0006)}}}]
        (logs / "s.jsonl").write_text("\n".join(json.dumps(x) for x in lines) + "\n")
        [t] = rp.load_job(job)
        self.assertEqual((t["record_from"], t["record"]["arm"]), ("pi files", "pi"))
        self.assertEqual(t["record"]["tokens"], toks(180, 1800, 1800, 57))
        self.assertEqual((t["record"]["model_calls"], t["record"]["tool_calls"]), (2, 1))
        # Harbor's dollars are the job's own account of an old trial.
        self.assertEqual(t["record"]["cost_usd"], 0.0058)

    def test_an_svg_escapes_an_arms_name(self):
        text = rp.svg("Score <&>", "dollars", [("a<b>&c", 0.1, 0.5)], {"a<b>&c"})
        root = ET.fromstring(text)
        self.assertIn("a<b>&c", {t.text for t in root.findall(SVG + "text")})


if __name__ == "__main__":
    unittest.main()
