"""Tests for the report drafting tool (theseus-qla4).

    python3 -m unittest discover -s bench/report

Fixtures written here, numbers worked by hand: two arms of Harbor jobs (two tasks, two attempts each), a bench
history whose header changes shape mid-file, a recall scorer's output, and two async trials. Each subcommand must
write the data file, the CSV, both modes of every figure and the skeleton, never overwrite a report that stands, and
number the figures in the order the report shows them.
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
spec = importlib.util.spec_from_file_location("bench_report_draft", HERE / "draft.py")
draft = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = draft
spec.loader.exec_module(draft)


def trial(job: Path, name: str, task: str, reward: float | None, cost: float, start: str, exc: str | None = None,
          agent: str = "theseus") -> None:
    d = job / name
    (d / "agent").mkdir(parents=True)
    (d / "result.json").write_text(json.dumps({
        "task_name": task, "trial_name": name, "agent_info": {"name": agent},
        "agent_result": {"cost_usd": cost, "n_input_tokens": 1000, "n_cache_tokens": 800, "n_output_tokens": 100},
        "verifier_result": None if reward is None else {"rewards": {"reward": reward}},
        "exception_info": {"exception_type": exc} if exc else None,
        "agent_execution": {"started_at": start, "finished_at": start.replace(":00:00", ":02:00")},
    }))


def run(argv: list[str]) -> str:
    out = io.StringIO()
    with contextlib.redirect_stdout(out), contextlib.redirect_stderr(io.StringIO()):
        assert draft.main(argv) == 0
    return out.getvalue()


class Draft(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.out = self.root / "docs"
        # theseus: fix-git solved twice; hard-task once solved, once a timeout (reward 0) -> 3 of 4.
        a = self.root / "jobs-a"
        trial(a, "fix-git__1", "fix-git", 1, 0.04, "2026-10-04T10:00:00-07:00")
        trial(a, "fix-git__2", "fix-git", 1, 0.05, "2026-10-04T11:00:00-07:00")
        trial(a, "hard-task__1", "hard-task", 1, 0.30, "2026-10-04T10:00:00-07:00")
        trial(a, "hard-task__2", "hard-task", 0, 0.50, "2026-10-04T11:00:00-07:00", exc="AgentTimeoutError")
        # claude-code, a directory of jobs: fix-git once solved, once not; hard-task never -> 1 of 4.
        for i, (task, r, c, t) in enumerate((("fix-git", 1, 0.03, "10"), ("fix-git", 0, 0.03, "11"),
                                             ("hard-task", 0, 0.20, "10"), ("hard-task", 0, 0.25, "11"))):
            trial(self.root / "jobs-c" / f"job{i}", f"{task}__{i}", task, r, c, f"2026-10-04T{t}:00:00-07:00",
                  agent="claude-code")

    def tearDown(self):
        self.tmp.cleanup()

    def test_a_harbor_run_drafts_every_part_of_its_report(self):
        out = run(["harbor", "--suite", "terminal-bench@2.0", "--date", "2026-10-04", "--slug", "Fixture Run",
                   "--arm", f"theseus={self.root / 'jobs-a'}", "--arm", f"claude-code={self.root / 'jobs-c'}",
                   "--model", "anthropic/claude-sonnet-5-5", "--out", str(self.out)])
        name = "2026-10-04-terminal-bench-fixture-run"
        self.assertIn(f"{name}.md", out)
        data = json.loads((self.out / f"{name}.json").read_text())
        th, cc = data["summary"]["theseus"], data["summary"]["claude-code"]
        self.assertEqual((th["pass"]["k"], th["pass"]["n"], cc["pass"]["k"]), (3, 4, 1))
        self.assertEqual((th["solved_every_attempt"], th["solved_any_attempt"]), (1, 2))
        self.assertEqual(th["endings"], {"ended by the agent": 3, "timeout": 1})
        self.assertAlmostEqual(th["cost_usd"], 0.89)
        self.assertEqual(th["cost_per_trial"]["method"], "range", "four trials: a range, not a bootstrap")
        # paired by task and attempt: (1,1) (1,0) (1,0) (0,0) -> 2 pairs only theseus, none only claude-code
        p = data["paired"]["claude-code"]
        self.assertEqual((p["only_ref"], p["only_arm"], p["both"], p["neither"]), (2, 0, 1, 1))
        self.assertEqual(p["tasks_only_ref"], ["hard-task"])
        rows = (self.out / f"{name}.csv").read_text().splitlines()
        self.assertEqual(len(rows), 1 + 8)
        self.assertTrue(rows[0].startswith("arm,task,attempt,trial,reward,ending"))
        matrix = next(f for f in data["figures"] if f["name"] == "outcomes")
        self.assertEqual([r["cells"] for r in matrix["rows"]], ["PPPF", "PEFF"])
        for f in data["figures"]:
            for suffix in ("", "-dark"):
                self.assertTrue((self.out / "img" / name / f"{f['name']}{suffix}.svg").exists())
        md = (self.out / f"{name}.md").read_text()
        heads = [ln for ln in md.splitlines() if ln.startswith("## ")]
        self.assertEqual(heads, ["## The question", "## The setup", "## Results", "## Analysis",
                                 "## Threats to validity", "## What it cost", "## Reproduction", "## Data"])
        figs = [ln.split(".")[0] for ln in md.splitlines() if ln.startswith("*Figure ")]
        self.assertEqual(figs, [f"*Figure {i}" for i in range(1, len(data["figures"]) + 1)])
        self.assertIn("3/4 = 75.0% [30.1%, 95.4%]", md)

    def test_a_report_that_stands_is_never_overwritten(self):
        args = ["harbor", "--suite", "swe-bench", "--date", "2026-10-04", "--arm", f"theseus={self.root / 'jobs-a'}",
                "--out", str(self.out)]
        run(args)
        md = self.out / "2026-10-04-swe-bench.md"
        md.write_text("the lane's narrative")
        run(args)
        self.assertEqual(md.read_text(), "the lane's narrative")
        run(args + ["--force"])
        self.assertNotEqual(md.read_text(), "the lane's narrative")

    def test_an_unknown_arm_or_a_bad_date_is_refused(self):
        with self.assertRaises(SystemExit), contextlib.redirect_stderr(io.StringIO()):
            draft.main(["harbor", "--suite", "x", "--date", "2026-10-04", "--arm", f"gpt={self.root / 'jobs-a'}",
                        "--out", str(self.out)])
        with self.assertRaises(ValueError):
            draft.report_name("10/04/2026", "x")
        self.assertEqual(draft.report_name("2026-10-04", "terminal-bench@2.0", "First Full Run"),
                         "2026-10-04-terminal-bench-first-full-run")

    def test_the_bench_history_is_read_block_by_block(self):
        csv = self.root / "history.csv"
        csv.write_text(
            "time,label,load1,cold_p50,cold_p95,cold_limit,passed\n"
            "2026-10-01T10:00:00-07:00,main abc,4.1,20,25,57.1,true\n"
            "2026-10-01T11:00:00-07:00,lane/x def,9.0,40,90,57.1,false\n"
            "time,label,load1,cold_p50,cold_p95,cold_limit,turn_plain_p50,turn_plain_p95,turn_plain_limit,passed,allowance\n"
            "2026-10-02T10:00:00-07:00,\"main 123 (dirty)\",3.0,21,24,57.1,75,90,,true,\n"
            "2026-10-02T12:00:00-07:00,main 124,12.0,30,80,57.1,80,1600,,false,0.65\n")
        rows = draft.read_history(csv)
        self.assertEqual(len(rows), 4)
        self.assertEqual(rows[2]["label"], "main 123 (dirty)")
        self.assertIsNone(rows[0].get("turn_plain_p95"), "an older block has no turn bench")
        self.assertEqual(rows[3]["allowance"], 0.65)
        run(["history", "--csv", str(csv), "--branch", "main", "--date", "2026-10-02", "--out", str(self.out)])
        data = json.loads((self.out / "2026-10-02-gate-bench.json").read_text())
        self.assertEqual(data["span"]["rows"], 3, "main's rows only")
        self.assertEqual(data["rows_failed"], 1)
        self.assertEqual(data["phases"]["cold"]["p95_max"], 80)
        life = next(f for f in data["figures"] if f["name"] == "lifecycle")
        self.assertEqual(len(life["panels"][0]["marks"]), 1, "the run over its budget is marked")
        self.assertTrue((self.out / "img" / "2026-10-02-gate-bench" / "turn-dark.svg").exists())

    def test_the_recall_scorers_output_drafts_its_curve(self):
        scores = self.root / "scores.json"
        curve = [{"bucket": "near", "n": 4, "correct": 4, "accuracy": 1.0, "turns_since": 2, "tokens_since": 900},
                 {"bucket": "compaction", "n": 4, "correct": 1, "accuracy": 0.25, "turns_since": 40,
                  "tokens_since": 60000}]
        scores.write_text(json.dumps({"digest": "d30943acf7bfd1ac", "stale_rule": "strict", "arms": {"theseus": {
            "probes": 9, "scored": 8, "undelivered": 1, "failed": 0, "recall_accuracy": 0.625,
            "abstention_accuracy": 0.5, "confident_wrong": 1, "stale": 0, "stale_of": 2,
            "half_life": {"incidental": {"turns": 30.0, "tokens": 40000}}, "cost_usd_per_probe": 0.04,
            "curves": {"incidental": curve, "central": []}}},
            "probes": [{"arm": "theseus", "id": "p001", "kind": "direct", "status": "scored", "correct": True}]}))
        run(["recall", "--scores", str(scores), "--date", "2026-10-05", "--slug", "smoke", "--out", str(self.out)])
        data = json.loads((self.out / "2026-10-05-recall-smoke.json").read_text())
        rows = data["figures"][0]["rows"]
        self.assertEqual([r["label"] for r in rows], ["near · theseus", "compaction · theseus"])
        self.assertEqual(rows[1]["value"], 0.25)
        self.assertIn("| Recall accuracy | 62.5% |", (self.out / "2026-10-05-recall-smoke.md").read_text())

    def test_async_trials_draft_through_the_async_scorers_own_reading(self):
        for i, (agent, reward) in enumerate((("theseus-async", 1), ("claude-code-async", 0))):
            job = self.root / f"async-{i}"
            trial(job, f"parallel__{i}", "parallel", reward, 0.03, "2026-10-05T10:00:00-07:00", agent=agent)
            (job / f"parallel__{i}" / "verifier").mkdir()
            (job / f"parallel__{i}" / "verifier" / "problems.json").write_text(json.dumps({"family": "parallel"}))
        run(["async", "--date", "2026-10-05", "--out", str(self.out), str(self.root / "async-0"),
             str(self.root / "async-1")])
        data = json.loads((self.out / "2026-10-05-async.json").read_text())
        self.assertEqual({a["key"] for a in data["arms"]}, {"theseus", "claude-code"})
        self.assertEqual({r["label"]: r["value"] for r in data["figures"][0]["rows"]},
                         {"claude-code-async": 0.0, "theseus-async": 1.0})

    def test_plot_redraws_a_reports_figures_from_its_data_file(self):
        summary = self.out / "2026-10-04-x.json"
        self.out.mkdir(parents=True)
        fig = {"name": "rates", "form": "intervals", "title": "t", "question": "q?",
               "rows": [{"label": "A", "arm": "theseus", "value": 0.5, "lo": 0.4, "hi": 0.6}]}
        summary.write_text(json.dumps({"figures": [fig]}))
        printed = run(["plot", str(summary)])
        self.assertIn('srcset="img/2026-10-04-x/rates-dark.svg"', printed)
        self.assertTrue((self.out / "img" / "2026-10-04-x" / "rates.svg").exists())
        summary.write_text(json.dumps({"figures": [fig, fig]}))
        with self.assertRaises(ValueError):
            draft.plot(summary)


if __name__ == "__main__":
    unittest.main()
