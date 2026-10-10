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


class Endings(unittest.TestCase):
    def test_a_pi_provider_failure_and_an_abort_are_endings_of_their_own(self):
        """Harbor records no exception for them (Pi exits 0): the report's
        loader names the trial's error from the record's `end` (theseus-bpeg),
        and the drafting tool sorts it into an ending, not the catch-all."""
        self.assertEqual(draft.ending("PiProviderError"), "provider failure (Pi)")
        self.assertEqual(draft.ending("PiAbortedError"), "aborted (Pi)")
        self.assertEqual(draft.ending("AgentTimeoutError"), "timeout")
        self.assertEqual(draft.ending("SomethingElse"), "error (exit or harness)")
        with tempfile.TemporaryDirectory() as d:
            job = Path(d) / "pi"
            for i, stop in enumerate(("stop", "error", "aborted")):
                trial(job, f"fix-git__{i}", "fix-git", 0, 0.05, "2026-10-04T10:00:00-07:00", agent="pi")
                rec = {"schema": "bench-efficiency/1", "arm": "pi", "tokens": {}, "cost_usd": 0.05,
                       "end": {"stop_reason": stop, "error": None, "answers": 3, "turns": 3}}
                (job / f"fix-git__{i}" / "agent" / "efficiency.json").write_text(json.dumps(rec))
            out = Path(d) / "docs"
            run(["harbor", "--suite", "terminal-bench@2.0", "--date", "2026-10-04", "--slug", "pi ends",
                 "--arm", f"theseus={job}", "--model", "anthropic/claude-sonnet-5-5", "--out", str(out)])
            data = json.loads((out / "2026-10-04-terminal-bench-pi-ends.json").read_text())
            self.assertEqual(data["summary"]["theseus"]["endings"],
                             {"ended by the agent": 1, "provider failure (Pi)": 1, "aborted (Pi)": 1})


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

    def test_a_pi_job_drafts_and_is_charted_in_its_own_colour(self):
        """`--arm pi=<job>` was refused (no `pi` key in `charts.ARMS`, theseus-3lqk): a Pi job drafts, every figure
        draws it in Pi's colour and name, and the palette figure lists it."""
        job = self.root / "jobs-pi"
        trial(job, "fix-git__1", "fix-git", 1, 0.06, "2026-10-04T10:00:00-07:00", agent="pi")
        trial(job, "fix-git__2", "fix-git", 0, 0.07, "2026-10-04T11:00:00-07:00", agent="pi")
        run(["harbor", "--suite", "terminal-bench@2.0", "--date", "2026-10-04", "--slug", "pi",
             "--arm", f"theseus={self.root / 'jobs-a'}", "--arm", f"pi={job}", "--out", str(self.out)])
        name = "2026-10-04-terminal-bench-pi"
        data = json.loads((self.out / f"{name}.json").read_text())
        self.assertEqual([a["key"] for a in data["arms"]], ["theseus", "pi"])
        self.assertEqual(data["summary"]["pi"]["label"], "Pi")
        self.assertEqual(data["summary"]["pi"]["pass"]["k"], 1)
        svg = (self.out / "img" / name / "pass-rates.svg").read_text()
        self.assertIn("Pi", svg)
        self.assertIn(draft.charts.SLOTS["light"][4], svg)
        self.assertIn(draft.charts.SLOTS["dark"][4], (self.out / "img" / name / "pass-rates-dark.svg").read_text())
        self.assertEqual(draft.arm_key("pi"), "pi")
        self.assertEqual(draft.arm_key("pi_agent"), "pi")
        self.assertEqual(draft.arm_key("pinned-thing"), "other")

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


class Flagged(unittest.TestCase):
    """What a reader must see apart: the grader's timeouts, the trials past $2.00, and the overlap column."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.out = self.root / "docs"
        job = self.root / "jobs"
        trial(job, "fix-git__1", "fix-git", 1, 0.04, "2026-10-04T10:00:00-07:00")
        # A verifier's own timeout: no reward, Harbor's VerifierTimeoutError.
        trial(job, "fix-git__2", "fix-git", None, 0.05, "2026-10-04T10:01:00-07:00", exc="VerifierTimeoutError")
        trial(job, "hard-task__1", "hard-task", 0, 2.40, "2026-10-04T12:00:00-07:00")
        for name, begin, end in (("fix-git__1", "10:00:00", "10:04:00"), ("fix-git__2", "10:01:00", "10:06:00"),
                                 ("hard-task__1", "12:00:00", "12:03:00")):
            f = job / name / "result.json"
            r = json.loads(f.read_text())
            r["agent_execution"] = {"started_at": f"2026-10-04T{begin}-07:00", "finished_at": f"2026-10-04T{end}-07:00"}
            r["verifier"] = {"started_at": f"2026-10-04T{end}-07:00", "finished_at": f"2026-10-04T{end}-07:00"}
            f.write_text(json.dumps(r))
        self.name = "2026-10-04-terminal-bench"
        run(["harbor", "--suite", "terminal-bench@2.0", "--date", "2026-10-04", "--arm", f"theseus={job}",
             "--out", str(self.out)])
        self.data = json.loads((self.out / f"{self.name}.json").read_text())
        self.md = (self.out / f"{self.name}.md").read_text()

    def tearDown(self):
        self.tmp.cleanup()

    def test_a_verifier_timeout_is_the_graders_and_listed_apart_from_the_agents_failures(self):
        s = self.data["summary"]["theseus"]
        self.assertEqual(s["endings"], {"ended by the agent": 2, "grader timeout": 1})
        self.assertEqual([(g["task"], g["attempt"]) for g in s["grader_timeouts"]], [("fix-git", 2)])
        self.assertIn("**Grader timeouts**", self.md)
        self.assertIn("`fix-git` attempt 2 (`fix-git__2`)", self.md)
        self.assertEqual(draft.ending("VerifierTimeoutError"), "grader timeout")
        rows = (self.out / f"{self.name}.csv").read_text().splitlines()
        self.assertIn("grader_timeout", rows[0])
        cell = next(r for r in rows if "fix-git__2" in r)
        self.assertIn("grader timeout", cell)
        self.assertNotIn("VerifierTimeoutError", cell, "not an exception of the agent's")
        matrix = next(f for f in self.data["figures"] if f["name"] == "outcomes")
        fix = next(r for r in matrix["rows"] if r["label"] == "fix-git")
        self.assertEqual(fix["cells"], "P-")
        self.assertTrue(fix["tips"][1].startswith("grader timeout"))

    def test_every_trial_past_two_dollars_gets_a_line(self):
        s = self.data["summary"]["theseus"]
        self.assertEqual([(o["task"], o["cost_usd"]) for o in s["over_budget"]], [("hard-task", 2.4)])
        self.assertIn("**Trials past $2.00 of real spend**", self.md)
        self.assertIn("`hard-task` attempt 1 (`hard-task__1`): $2.40", self.md)

    def test_the_overlap_column_is_in_the_data_and_named_in_the_threats(self):
        rows = (self.out / f"{self.name}.csv").read_text().splitlines()
        head = rows[0].split(",")
        i = head.index("overlap")
        got = {r.split(",")[3]: r.split(",")[i] for r in rows[1:]}
        self.assertEqual(got, {"fix-git__1": "1", "fix-git__2": "1", "hard-task__1": "0"})
        self.assertEqual(self.data["summary"]["theseus"]["overlap_max"], 1)
        threats = self.md.split("## Threats to validity")[1].split("## What it cost")[0]
        self.assertIn("`overlap`", threats)
        self.assertIn("Overlap per arm", threats)

    def test_every_report_prints_t4_and_t5_from_the_trials_trajectories(self):
        """T4 and T5 (theseus-w052 5) in the data file and the skeleton, read from each trial's
        `agent/trajectory.json`; T5 over the pairs both arms solved."""
        root = Path(tempfile.mkdtemp())
        self.addCleanup(lambda: __import__("shutil").rmtree(root, ignore_errors=True))
        for arm, calls in (("theseus", [("a", "fs.read", "Unknown tool `fs.grep`. Available: x.", {"status": "error"})]),
                           ("claude-code", [("a", "Bash", "Exit code 1\nfailed", {"tool_result_is_error": True})])):
            for i, (reward, cost) in enumerate(((1, 0.10), (1, 0.30)), 1):
                trial(root / arm, f"fix-git__{i}", "fix-git", reward, cost, f"2026-10-04T1{i}:00:00-07:00", agent=arm)
                d = root / arm / f"fix-git__{i}" / "agent"
                (d / "trajectory.json").write_text(json.dumps({"steps": [{
                    "step_id": 1, "source": "agent", "message": "m",
                    "tool_calls": [{"tool_call_id": c[0], "function_name": c[1], "arguments": {}} for c in calls],
                    "observation": {"results": [{"source_call_id": c[0], "content": c[2], "extra": c[3]}
                                                for c in calls]}}]}))
        out = root / "docs"
        run(["harbor", "--suite", "terminal-bench@2.0", "--date", "2026-10-04", "--arm", f"theseus={root / 'theseus'}",
             "--arm", f"claude-code={root / 'claude-code'}", "--out", str(out)])
        name = "2026-10-04-terminal-bench"
        data = json.loads((out / f"{name}.json").read_text())
        th, cc = data["summary"]["theseus"]["t4"], data["summary"]["claude-code"]["t4"]
        self.assertEqual((th["trials_with_tool_error"], th["invented"]), (2, 2))
        self.assertEqual((cc["trials_with_tool_error"], cc["tool_errors"]), (0, 0), "an exit code is the command's")
        self.assertEqual(len(data["t5"]["pairs"]), 2)
        self.assertAlmostEqual(data["t5"]["arms"]["theseus"]["cost_mean"], 0.20)
        md = (out / f"{name}.md").read_text()
        self.assertIn("**T4, tool friction.**", md)
        self.assertIn("| Invented names, by tool | fs.read 2 | none |", md)
        self.assertIn("| Dollars per solve, mean | $0.200 | $0.200 |", md)


if __name__ == "__main__":
    unittest.main()
