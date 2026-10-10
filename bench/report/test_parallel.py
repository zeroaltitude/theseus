"""Tests for the parallel-calls report's generator (theseus-2wxa), on a fixture.

    python3 -m unittest discover -s bench/report

Fixture trials for six arms, two Layer 2 families each (host-logs, independent; pipeline, a control), with ledgers and
trajectories worked by hand (bench/async's test fixtures), and Layer 1 results for four harnesses, one of them not
measurable. The generator must write the data file, the CSV, both modes of every figure and the report, answer
first, and the omnibus's rows; and it must never overwrite a report that stands.
"""

from __future__ import annotations

import contextlib
import importlib.util
import io
import json
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
ASYNC = HERE.parent / "async"
sys.path.insert(0, str(ASYNC))
sys.path.insert(0, str(ASYNC / "tools"))
sys.path.insert(0, str(HERE.parent / "harbor"))

spec = importlib.util.spec_from_file_location("bench_report_parallel", HERE / "parallel.py")
parallel = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = parallel
spec.loader.exec_module(parallel)

from test_score import trial  # noqa: E402
from test_score_layer2 import HOSTS, PIPELINE, claude_code_atif  # noqa: E402

# Each arm: its agent's wall on host-logs (ideal 20 s), its calls in each of four responses, its cost, and whether
# its pipeline trial ran a step early.
FIXTURE = {
    "theseus-before": ("theseus-async", 120.0, [1, 1, 1, 0], 0.30, False),
    "theseus-d1hi": ("theseus-async", 60.0, [3, 3, 0], 0.20, False),
    "theseus-after": ("theseus-async", 30.0, [6, 0], 0.10, False),
    "claude-code": ("claude-code-async", 40.0, [4, 2, 0], 0.15, True),
    "pi": ("pi-async", 90.0, [2, 1, 1, 1, 0], 0.12, False),
    "opencode": ("opencode", 100.0, [1, 1, 1, 1, 1, 1, 0], 0.11, False),
}
LAYER1 = {
    "theseus-before": [(1, 2.2), (2, 4.3), (4, 8.3), (8, 16.4), (16, 32.5)],
    "theseus-after": [(1, 2.2), (2, 2.3), (4, 2.3), (8, 2.4), (16, 4.6)],
    "claude-code": [(1, 3.1), (2, 3.2), (4, 3.3), (8, 3.6), (16, 6.0)],
}


def layer1_file(path: Path, harness: str, walls: list[tuple[int, float]] | None) -> None:
    if walls is None:
        d = {"harness": harness, "ok": False, "why": "a run took 0.4 s, less than one call's 2 s: its calls never ran",
             "counts": [{"n": 1, "ok": False, "why": "never ran", "wall_s": None, "serial_s": 2}]}
    else:
        d = {"harness": harness, "ok": True, "counts": [{"n": n, "ok": True, "wall_s": w, "min_s": w, "max_s": w,
                                                         "serial_s": 2 * n} for n, w in walls]}
    path.write_text(json.dumps(d))


class Report(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.out = self.root / "out"
        self.args = ["--date", "2026-10-11", "--out", str(self.out)]
        for key, (agent, wall, counts, cost, early) in FIXTURE.items():
            job = self.root / f"pc-{key}"
            job.mkdir()
            trial(job, "host-logs__a", arm=agent, family="host-logs", start=1000.0, end=1000.0 + wall, reward=1.0,
                  events=HOSTS)
            events = PIPELINE if early else [e for e in PIPELINE if not (e[1] == "run-app" and e[3] == 2)]
            trial(job, "pipeline__b", arm=agent, family="pipeline", start=1000.0, end=1060.0,
                  reward=0.0 if early else 1.0, events=events)
            for t in ("host-logs__a", "pipeline__b"):
                (job / t / "agent/trajectory.json").write_text(json.dumps(claude_code_atif(counts)))
                r = json.loads((job / t / "result.json").read_text())
                r["agent_result"]["cost_usd"] = cost
                (job / t / "result.json").write_text(json.dumps(r))
            self.args += ["--arm", f"{key}={job}"]
        for key in ("theseus-before", "theseus-after", "claude-code", "pi"):
            f = self.root / f"l1-{key}.json"
            layer1_file(f, parallel.HARNESS_OF[key], LAYER1.get(key))
            self.args += ["--layer1", f"{key}={f}"]

    def tearDown(self):
        self.tmp.cleanup()

    def run_it(self, extra: list[str] | None = None) -> dict:
        with contextlib.redirect_stdout(io.StringIO()) as out:
            self.assertEqual(parallel.main(self.args + (extra or [])), 0)
        self.printed = out.getvalue()
        return json.loads((self.out / "2026-10-11-asyncbench-parallel-calls.json").read_text())

    def test_the_report_and_its_data(self):
        data = self.run_it()
        name = "2026-10-11-asyncbench-parallel-calls"
        self.assertEqual([a["key"] for a in data["arms"]], list(FIXTURE))
        s = data["summary"]
        # host-logs: wall / ideal (20 s): before 6, after 1.5; pipeline (ideal 43 s, or 31 s with no early run) 60 s.
        self.assertEqual(s["theseus-before"]["over_ideal_independent"]["median"], 6.0)
        self.assertEqual(s["theseus-after"]["over_ideal_independent"]["median"], 1.5)
        self.assertEqual(s["theseus-after"]["calls_per_response"], 6.0)
        self.assertEqual(s["theseus-before"]["multi_call_share"]["rate"], 0.0)
        self.assertEqual(s["claude-code"]["multi_call_share"]["rate"], 1.0)
        self.assertEqual((s["claude-code"]["control_trials"], s["claude-code"]["control_trials_with_violations"]),
                         (1, 1))
        self.assertEqual(s["theseus-after"]["control_trials_with_violations"], 0)
        # Round trips per solved: two trials of two responses each, both solved: 2; $0.10 a trial, $0.10 a solve.
        self.assertEqual(s["theseus-after"]["round_trips_per_solved"], 2.0)
        self.assertAlmostEqual(s["theseus-after"]["usd_per_solved"], 0.10)
        # Claude Code solved one of two: its $0.30 over one.
        self.assertAlmostEqual(s["claude-code"]["usd_per_solved"], 0.30)
        figs = {f["name"]: f for f in data["figures"]}
        self.assertEqual(sorted(figs), ["calls-per-response", "layer1-wall", "multi-call-share", "over-ideal"])
        panels = {p["title"].split(" (")[0]: p for p in figs["layer1-wall"]["panels"]}
        self.assertEqual(sorted(panels), ["Claude Code", "Pi", "Theseus"])
        self.assertIn("not measurable", panels["Pi"]["title"])
        self.assertEqual([sr["label"] for sr in panels["Theseus"]["series"]],
                         ["in turn: 2 s × N", "Theseus before", "Theseus, grouped + the sentence"])
        for f in figs:
            for mode in ("", "-dark"):
                svg = (self.out / "img" / name / f"{f}{mode}.svg").read_text()
                self.assertTrue(svg.startswith("<svg"), f)
        md = (self.out / f"{name}.md").read_text()
        self.assertTrue(md.splitlines()[2].startswith("> **The answer.** On Layer 2's 16 families"), md[:400])
        self.assertIn("Theseus before 6.00×", md)
        self.assertIn("Layer 1, eight calls of `sleep 2` in one response: Theseus before 16.4 s", md)
        self.assertIn("Pi not measurable", md)
        for head in ("## Threats to validity", "### The projection to Terminal-Bench", "### The controls' order "
                     "violations", "| `pipeline` (control) |", "Sleep-based durations", "0.2% of tool time"):
            self.assertIn(head, md)
        csv = (self.out / f"{name}.csv").read_text().splitlines()
        self.assertEqual(len(csv), 1 + 2 * len(FIXTURE))
        self.assertIn("calls_per_response", csv[0])
        om = data["omnibus"]
        self.assertEqual(om["every_report_row"].split(" | ")[0], "| Speed")
        self.assertTrue(om["every_report_row"].endswith(f"({name}.md) | Oct 11 |"))
        self.assertIn("`asyncbench`", om["index_row"])
        self.assertEqual(om["data"]["arms"]["theseus-after"]["over_ideal_independent"], 1.5)
        self.assertIn("omnibus: | Speed |", self.printed)

    def test_a_report_that_stands_is_kept(self):
        self.run_it()
        md = self.out / "2026-10-11-asyncbench-parallel-calls.md"
        md.write_text("the maintainer's words")
        with contextlib.redirect_stderr(io.StringIO()):
            self.run_it()
        self.assertEqual(md.read_text(), "the maintainer's words")
        self.run_it(["--force"])
        self.assertNotEqual(md.read_text(), "the maintainer's words")

    def test_an_unknown_arm_is_refused(self):
        with self.assertRaises(SystemExit), contextlib.redirect_stderr(io.StringIO()):
            parallel.main(["--date", "2026-10-11", "--arm", f"codex={self.root}"])


if __name__ == "__main__":
    unittest.main()
