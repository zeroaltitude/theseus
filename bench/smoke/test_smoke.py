"""Tests for the release smoke (theseus-yf6pj): the slice against the data it was
picked from, the trial reader, the floor comparison, the verdict and the
report. Standard library only; Python >= 3.11 (tomllib)."""

from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

import smoke

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]


def trial(root: Path, task: str, reward: float | None, *, cost=0.10, model_cost=0.09, calls=5,
          wall=40.0, exception=None, by_model=None, suffix="aB3") -> Path:
    """One Harbor trial directory, as `harbor run` and the Theseus adapter leave it."""
    d = root / "job" / f"{task}__{suffix}"
    (d / "agent").mkdir(parents=True)
    res = {"task_name": task, "trial_name": d.name,
           "verifier_result": None if reward is None else {"rewards": {"reward": reward}},
           "exception_info": None if exception is None else {"exception_type": exception}}
    (d / "result.json").write_text(json.dumps(res))
    by_model = by_model or {"claude-sonnet-5-5": {"input": 1000, "cache_read": 100000, "cache_write": 10000,
                                                  "output": 5000, "calls": calls}}
    eff = {"cost_usd": cost, "model_cost_usd": model_cost, "model_calls": calls, "wall_s": wall,
           "by_model": by_model}
    (d / "agent" / "efficiency.json").write_text(json.dumps(eff))
    return d


class Slice(unittest.TestCase):
    def test_the_committed_slice_has_eight_stable_tasks_and_two_controls(self):
        s = smoke.load_slice(HERE / "tb-slice.toml")
        self.assertEqual(len(s["stable"]), 8)
        self.assertEqual(len(s["controls"]), 2)
        self.assertFalse(set(s["stable"]) & set(s["controls"]))
        for t in s["stable"] + s["controls"]:
            self.assertTrue(s["reasons"][t].strip(), t)

    def test_the_committed_slice_holds_against_the_data_it_was_picked_from(self):
        # Every stable task passed every trial of every named run that has it, and
        # every control failed every one: the slice cannot drift from its CSVs.
        s = smoke.load_slice(HERE / "tb-slice.toml")
        self.assertEqual(smoke.check_slice(s, REPO), [])

    def test_check_slice_names_a_stable_task_that_failed_once(self):
        with tempfile.TemporaryDirectory() as t:
            root = Path(t)
            (root / "a.csv").write_text("task,trial,reward,exception\n"
                                        "x,x__1,1,\nx,x__2,0,\ny,y__1,0,\ny,y__2,0,\n")
            s = {"stable": ["x"], "controls": ["y"], "reasons": {}, "sources": ["a.csv"]}
            bad = smoke.check_slice(s, root)
            self.assertEqual(len(bad), 1)
            self.assertIn("x", bad[0])

    def test_check_slice_names_a_control_that_passed_once_and_a_task_in_no_run(self):
        with tempfile.TemporaryDirectory() as t:
            root = Path(t)
            (root / "a.csv").write_text("task,trial,reward,exception\nx,x__1,1,\ny,y__1,1,\n")
            s = {"stable": ["x", "z"], "controls": ["y"], "reasons": {}, "sources": ["a.csv"]}
            bad = " ".join(smoke.check_slice(s, root))
            self.assertIn("y", bad)
            self.assertIn("z", bad)

    def test_the_plan_names_exactly_the_slice_once_each(self):
        s = smoke.load_slice(HERE / "tb-slice.toml")
        p = json.loads((REPO / "bench" / "plans" / "smoke-tb10.json").read_text())
        self.assertEqual(p["attempts"], {t: 1 for t in s["stable"] + s["controls"]})
        self.assertEqual(p["effort_bounds"], ["low", "high"])


class Trials(unittest.TestCase):
    def test_reads_reward_cost_wall_turns_and_model_mix(self):
        with tempfile.TemporaryDirectory() as t:
            root = Path(t)
            trial(root, "fix-git", 1.0, calls=7, wall=12.5)
            trial(root, "raman-fitting", 0.0, suffix="Zz9",
                  by_model={"claude-haiku-5-5": {"input": 10, "output": 10, "calls": 2},
                            "claude-sonnet-5-5": {"input": 10, "output": 10, "calls": 3}})
            rows = {r["task"]: r for r in smoke.read_trials(root / "job")}
            self.assertEqual(set(rows), {"fix-git", "raman-fitting"})
            self.assertTrue(rows["fix-git"]["passed"])
            self.assertEqual(rows["fix-git"]["turns"], 7)
            self.assertEqual(rows["fix-git"]["wall_s"], 12.5)
            self.assertFalse(rows["raman-fitting"]["passed"])
            self.assertEqual(rows["raman-fitting"]["models"], {"claude-haiku-5-5": 2, "claude-sonnet-5-5": 3})

    def test_cost_is_repriced_on_the_current_table_plus_jev(self):
        with tempfile.TemporaryDirectory() as t:
            root = Path(t)
            # 1M cache reads of Sonnet 5.5 at the current table's $0.20, and $0.01 of Jev.
            trial(root, "fix-git", 1.0, cost=0.11, model_cost=0.10,
                  by_model={"claude-sonnet-5-5": {"cache_read": 1_000_000, "calls": 1}})
            (row,) = smoke.read_trials(root / "job")
            self.assertAlmostEqual(row["cost_usd"], 0.21, places=6)
            self.assertEqual(row["price_digest"], smoke.price_digest())

    def test_a_trial_with_no_verifier_reward_did_not_pass(self):
        with tempfile.TemporaryDirectory() as t:
            root = Path(t)
            trial(root, "fix-git", None, exception="AgentTimeoutError")
            (row,) = smoke.read_trials(root / "job")
            self.assertFalse(row["passed"])
            self.assertEqual(row["exception"], "AgentTimeoutError")


def tb_rows(passes: dict[str, bool], cost=0.1, wall=50.0) -> list[dict]:
    return [{"task": t, "passed": p, "cost_usd": cost, "wall_s": wall, "turns": 3, "models": {},
             "exception": None, "price_digest": smoke.price_digest()} for t, p in passes.items()]


SLICE = {"stable": [f"s{i}" for i in range(8)], "controls": ["c0", "c1"], "reasons": {}, "sources": []}


class TbPart(unittest.TestCase):
    def ok_passes(self):
        return {**{t: True for t in SLICE["stable"]}, "c0": False, "c1": False}

    def test_all_stable_pass_and_both_controls_fail_is_a_pass_without_a_floor(self):
        p = smoke.tb_part(tb_rows(self.ok_passes()), SLICE, floor=None, wall_s=600)
        self.assertTrue(p["ok"], p)
        self.assertEqual(p["count"], 10)
        self.assertAlmostEqual(p["number"], 0.1 * 10 / 8)  # every trial's dollars over the tasks solved

    def test_no_trials_is_a_fail_never_a_vacuous_pass(self):
        p = smoke.tb_part([], SLICE, floor=None, wall_s=0)
        self.assertFalse(p["ok"])
        self.assertIn("0 of 10", p["note"])

    def test_a_missing_task_is_a_fail(self):
        rows = tb_rows(self.ok_passes())[:-1]
        self.assertFalse(smoke.tb_part(rows, SLICE, floor=None, wall_s=600)["ok"])

    def test_a_stable_task_that_fails_is_a_fail_and_named(self):
        ps = self.ok_passes()
        ps["s3"] = False
        p = smoke.tb_part(tb_rows(ps), SLICE, floor=None, wall_s=600)
        self.assertFalse(p["ok"])
        self.assertIn("s3", p["note"])

    def test_a_control_that_passes_is_a_fail(self):
        ps = self.ok_passes()
        ps["c1"] = True
        p = smoke.tb_part(tb_rows(ps), SLICE, floor=None, wall_s=600)
        self.assertFalse(p["ok"])
        self.assertIn("c1", p["note"])

    def test_cost_per_solved_task_over_the_floor_by_more_than_thirty_percent_fails(self):
        floor = {"tb": {"cost_per_solved_usd": 0.10, "wall_s": 600, "price_digest": smoke.price_digest()}}
        within = smoke.tb_part(tb_rows(self.ok_passes(), cost=0.104), SLICE, floor=floor, wall_s=600)
        self.assertTrue(within["ok"], within)
        over = smoke.tb_part(tb_rows(self.ok_passes(), cost=0.11), SLICE, floor=floor, wall_s=600)
        self.assertFalse(over["ok"])
        self.assertIn("cost", over["note"])

    def test_wall_over_the_floor_by_more_than_thirty_percent_fails(self):
        floor = {"tb": {"cost_per_solved_usd": 1.0, "wall_s": 600, "price_digest": smoke.price_digest()}}
        self.assertTrue(smoke.tb_part(tb_rows(self.ok_passes()), SLICE, floor=floor, wall_s=780)["ok"])
        p = smoke.tb_part(tb_rows(self.ok_passes()), SLICE, floor=floor, wall_s=781)
        self.assertFalse(p["ok"])
        self.assertIn("wall", p["note"])

    def test_a_floor_on_another_price_table_does_not_compare_dollars(self):
        floor = {"tb": {"cost_per_solved_usd": 0.01, "wall_s": 600, "price_digest": "000000000000"}}
        p = smoke.tb_part(tb_rows(self.ok_passes()), SLICE, floor=floor, wall_s=600)
        self.assertTrue(p["ok"], p)
        self.assertIn("price table", p["note"])


class RoutePart(unittest.TestCase):
    def test_cases_mode_needs_54_of_60_and_every_must_in_every_run(self):
        good = [{"total": 60, "model_correct": 55, "musts": 14, "musts_held": 14, "p95_ms": 900}] * 3
        self.assertTrue(smoke.route_part("cases", good, floor=None)["ok"])
        low = good[:2] + [{"total": 60, "model_correct": 53, "musts": 14, "musts_held": 14, "p95_ms": 900}]
        self.assertFalse(smoke.route_part("cases", low, floor=None)["ok"])
        must = good[:2] + [{"total": 60, "model_correct": 60, "musts": 14, "musts_held": 13, "p95_ms": 900}]
        self.assertFalse(smoke.route_part("cases", must, floor=None)["ok"])

    def test_replay_mode_compares_agreement_and_p95_with_the_floor(self):
        runs = [{"total": 40, "agree": 38, "p95_ms": 1000}] * 3
        self.assertTrue(smoke.route_part("replay", runs, floor=None)["ok"])
        floor = {"route": {"mode": "replay", "agreement": 0.95, "p95_ms": 1000}}
        self.assertTrue(smoke.route_part("replay", runs, floor=floor)["ok"])
        worse = [{"total": 40, "agree": 34, "p95_ms": 1000}] * 3
        self.assertFalse(smoke.route_part("replay", worse, floor=floor)["ok"])
        slow = [{"total": 40, "agree": 38, "p95_ms": 1400}] * 3
        self.assertFalse(smoke.route_part("replay", slow, floor=floor)["ok"])

    def test_fewer_than_three_runs_or_an_empty_run_is_a_fail(self):
        self.assertFalse(smoke.route_part("replay", [{"total": 40, "agree": 40, "p95_ms": 1}] * 2, None)["ok"])
        self.assertFalse(smoke.route_part("replay", [{"total": 0, "agree": 0, "p95_ms": 0}] * 3, None)["ok"])

    def test_a_floor_from_the_other_mode_is_not_compared(self):
        runs = [{"total": 60, "model_correct": 58, "musts": 14, "musts_held": 14, "p95_ms": 5000}] * 3
        floor = {"route": {"mode": "replay", "agreement": 0.99, "p95_ms": 100}}
        p = smoke.route_part("cases", runs, floor=floor)
        self.assertTrue(p["ok"], p)
        self.assertIn("mode", p["note"])


class OtherParts(unittest.TestCase):
    def test_recall_passes_only_when_tests_ran(self):
        self.assertTrue(smoke.recall_part(exit_code=0, seconds=9.0, tests=120, skipped=False)["ok"])
        self.assertFalse(smoke.recall_part(exit_code=0, seconds=0.1, tests=120, skipped=True)["ok"])
        self.assertFalse(smoke.recall_part(exit_code=0, seconds=0.1, tests=0, skipped=False)["ok"])
        self.assertFalse(smoke.recall_part(exit_code=1, seconds=9.0, tests=120, skipped=False)["ok"])

    def test_resume_needs_three_kills_three_resumes_and_no_repeated_effect(self):
        self.assertTrue(smoke.resume_part(kills=3, resumed=3, effects=6, expected_effects=6)["ok"])
        self.assertFalse(smoke.resume_part(kills=2, resumed=2, effects=6, expected_effects=6)["ok"])
        self.assertFalse(smoke.resume_part(kills=3, resumed=2, effects=6, expected_effects=6)["ok"])
        repeated = smoke.resume_part(kills=3, resumed=3, effects=7, expected_effects=6)
        self.assertFalse(repeated["ok"])
        self.assertIn("repeated", repeated["note"])
        self.assertFalse(smoke.resume_part(kills=3, resumed=3, effects=5, expected_effects=6)["ok"])

    def test_dm_needs_five_replies_in_time_with_footers_and_no_alarms(self):
        good = [{"turn": n, "seconds": 8.0, "footer": True} for n in range(5)]
        self.assertTrue(smoke.dm_part(good, alarms=0)["ok"])
        self.assertFalse(smoke.dm_part(good[:4], alarms=0)["ok"])
        self.assertFalse(smoke.dm_part(good[:4] + [{"turn": 4, "seconds": 61.0, "footer": True}], 0)["ok"])
        self.assertFalse(smoke.dm_part(good[:4] + [{"turn": 4, "seconds": 3.0, "footer": False}], 0)["ok"])
        self.assertFalse(smoke.dm_part(good, alarms=1)["ok"])
        self.assertFalse(smoke.dm_part([], alarms=0)["ok"])


def part(name, ok, cost=0.0, wall=1.0):
    return {"part": name, "ok": ok, "count": 1, "number": 1.0, "unit": "x", "floor": None,
            "delta": None, "note": "", "cost_usd": cost, "wall_s": wall}


class Verdict(unittest.TestCase):
    def test_pass_only_when_all_five_parts_ran_and_passed(self):
        parts = [part(n, True) for n in smoke.PARTS]
        self.assertEqual(smoke.verdict(parts), "PASS")
        self.assertEqual(smoke.verdict(parts[:4]), "FAIL (dm: did not run)")
        self.assertEqual(smoke.verdict([]), "FAIL (tb, route, recall, resume, dm: did not run)")

    def test_fail_names_the_failing_parts(self):
        parts = [part(n, n not in ("tb", "dm")) for n in smoke.PARTS]
        self.assertEqual(smoke.verdict(parts), "FAIL (tb, dm)")

    def test_the_report_starts_with_the_verdict_then_one_table_then_totals(self):
        parts = [part(n, True, cost=0.5, wall=60) for n in smoke.PARTS]
        md = smoke.report_md(parts, run={"sha": "75012080", "date": "2026-10-30", "binary": "x"})
        lines = md.splitlines()
        self.assertEqual(lines[0], "# PASS")
        self.assertIn("| part | result | number | floor | delta |", md)
        self.assertEqual(md.count("| part |"), 1)
        self.assertIn("$2.50", md)
        self.assertLess(md.index("| part |"), md.index("$2.50"))


class Floor(unittest.TestCase):
    def test_floor_from_a_run_holds_what_the_comparisons_read(self):
        tb = smoke.tb_part(tb_rows({**{t: True for t in SLICE["stable"]}, "c0": False, "c1": False}),
                           SLICE, floor=None, wall_s=600)
        route = smoke.route_part("replay", [{"total": 40, "agree": 38, "p95_ms": 1000}] * 3, None)
        f = smoke.floor_from([tb, route], sha="75012080", date="2026-10-30")
        self.assertEqual(f["sha"], "75012080")
        self.assertEqual(f["tb"]["wall_s"], 600)
        self.assertEqual(f["tb"]["price_digest"], smoke.price_digest())
        self.assertAlmostEqual(f["tb"]["cost_per_solved_usd"], 0.125)
        self.assertEqual(f["route"], {"mode": "replay", "agreement": 0.95, "p95_ms": 1000})

    def test_a_failed_run_never_writes_a_floor(self):
        bad = smoke.tb_part([], SLICE, floor=None, wall_s=0)
        with self.assertRaises(smoke.SmokeError):
            smoke.floor_from([bad], sha="x", date="d")


if __name__ == "__main__":
    unittest.main()
