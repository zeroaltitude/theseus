"""Tests for the release smoke (theseus-yf6pj): the slice against the data it was
picked from, the trial reader, the floor comparison, the verdict and the
report. Standard library only; Python >= 3.11 (tomllib)."""

from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

import dm
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

    def test_a_trial_with_no_record_has_no_price_never_zero(self):
        with tempfile.TemporaryDirectory() as t:
            root = Path(t)
            d = trial(root, "fix-git", 1.0)
            (d / "agent" / "efficiency.json").unlink()
            (row,) = smoke.read_trials(root / "job")
            self.assertIsNone(row["cost_usd"])
            rows = tb_rows({**{t: True for t in SLICE["stable"]}, "c0": False, "c1": False})
            rows[0]["cost_usd"] = None
            p = smoke.tb_part(rows, SLICE, floor=None, wall_s=600)
            self.assertFalse(p["ok"])
            self.assertIn("no price", p["note"])

    def test_jevs_dollars_come_from_its_own_record_when_there(self):
        with tempfile.TemporaryDirectory() as t:
            root = Path(t)
            d = trial(root, "fix-git", 1.0, by_model={"claude-sonnet-5-5": {"cache_read": 1_000_000, "calls": 1}})
            eff = json.loads((d / "agent" / "efficiency.json").read_text())
            eff.update({"cost_usd": 0.2, "model_cost_usd": 0.2, "jev": {"cost_usd": 0.03}})
            (d / "agent" / "efficiency.json").write_text(json.dumps(eff))
            (row,) = smoke.read_trials(root / "job")
            self.assertAlmostEqual(row["cost_usd"], 0.23, places=6)

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

    def test_trials_that_never_reached_the_agent_are_named_as_the_hosts(self):
        rows = tb_rows(self.ok_passes())
        rows[0].update(passed=False, exception="RuntimeError", turns=None, cost_usd=None)
        p = smoke.tb_part(rows, SLICE, floor=None, wall_s=600)
        self.assertFalse(p["ok"])
        self.assertIn("1 trials errored before the agent ran (RuntimeError)", p["note"])

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


def replay(model_ok=53, must_ok=13, must_total=13, answered=58, held=2, bar=True, cost=0.01, pack="route.v10"):
    """`theseus --json judge replay --cases`'s result, the fields the part reads."""
    return {"set": "cases", "cost_usd": cost,
            "cases": {"cases": [{}] * 58, "answered": answered, "model_ok": model_ok, "held_elsewhere": held,
                      "must_total": must_total, "must_ok": must_ok, "must_under_confirm": 0, "bar": bar,
                      "pack": pack}}


class RoutePart(unittest.TestCase):
    def runs(self, *raws, wall=20.0):
        return [smoke.route_run(r, wall) for r in raws]

    def test_the_bar_is_54_of_60_on_the_model_with_the_two_pin_tests_counted_and_every_must(self):
        self.assertTrue(smoke.route_part(self.runs(*[replay(model_ok=52)] * 3), floor=None)["ok"])
        low = smoke.route_part(self.runs(replay(), replay(), replay(model_ok=51)), floor=None)
        self.assertFalse(low["ok"])
        self.assertIn("53/60", low["note"])
        must = smoke.route_part(self.runs(replay(), replay(), replay(must_ok=12)), floor=None)
        self.assertFalse(must["ok"])
        self.assertIn("12/13", must["note"])

    def test_the_number_is_the_worst_run_on_the_model_out_of_60(self):
        p = smoke.route_part(self.runs(replay(model_ok=56), replay(model_ok=53), replay(model_ok=55)), None)
        self.assertEqual(p["number"], 55.0)

    def test_fewer_than_three_runs_or_a_run_with_no_answers_is_a_fail(self):
        self.assertFalse(smoke.route_part(self.runs(replay(), replay()), None)["ok"])
        self.assertFalse(smoke.route_part(self.runs(replay(), replay(), replay(answered=0)), None)["ok"])

    def test_a_run_slower_than_the_floor_by_more_than_thirty_percent_fails(self):
        floor = {"route": {"wall_s": 20.0}}
        self.assertTrue(smoke.route_part(self.runs(*[replay()] * 3, wall=26.0), floor)["ok"])
        slow = smoke.route_part(self.runs(*[replay()] * 3, wall=26.5), floor)
        self.assertFalse(slow["ok"])
        self.assertIn("wall", slow["note"])

    def test_the_daemons_whole_bar_is_said_but_does_not_decide(self):
        p = smoke.route_part(self.runs(*[replay(bar=False)] * 3), None)
        self.assertTrue(p["ok"])
        self.assertIn("whole bar met 0/3", p["note"])

    def test_a_daemon_that_routes_with_an_older_pack_is_na_until_the_flip_not_a_fail(self):
        # Main before 1.0.4 dispatches route.v8; the cases are route.v10's. Their reading is kept, never judged.
        p = smoke.route_part(self.runs(*[replay(model_ok=37, must_ok=9)] * 3), None, live="route.v8")
        self.assertTrue(p["na"])
        self.assertIn("n/a until the flip", p["note"])
        self.assertIn("route.v8", p["note"])
        self.assertIn("39/60", p["note"])
        parts = [part(n, True) for n in smoke.PARTS if n != "route"] + [p]
        self.assertEqual(smoke.verdict(parts), "PASS")
        md = smoke.report_md(parts, run={"sha": "75012080", "date": "2026-10-30", "binary": "x"})
        self.assertIn("| route | n/a until the flip", md)
        tb, _ = Floor.passing(Floor())
        f = smoke.floor_from([tb, p], sha="x", date="d")
        self.assertEqual(f["written"]["parts"], ["tb"])
        self.assertNotIn("route", f)

    def test_a_daemon_on_the_cases_pack_or_an_unknown_one_is_held_to_the_bar(self):
        for live in ("route.v10", None):
            p = smoke.route_part(self.runs(*[replay(model_ok=37, must_ok=9)] * 3), None, live=live)
            self.assertFalse(p["ok"], live)
            self.assertFalse(p.get("na"), live)
            self.assertIn("39/60 on the model < 54", p["note"])

    def test_n_a_needs_the_runs_themselves_to_have_answered(self):
        p = smoke.route_part(self.runs(replay(), replay()), None, live="route.v8")
        self.assertFalse(p["ok"])
        self.assertFalse(p.get("na"))

    def test_a_part_that_could_not_run_is_a_fail_that_says_why(self):
        p = smoke.unavailable("route", "this binary has no `judge replay --cases`")
        self.assertFalse(p["ok"])
        self.assertEqual(p["count"], 0)
        self.assertIn("--cases", p["note"])
        self.assertEqual(smoke.verdict([p]), "FAIL (route, tb, recall, resume, dm: did not run)")


class OtherParts(unittest.TestCase):
    def test_recall_passes_only_when_tests_ran(self):
        self.assertTrue(smoke.recall_part(exit_code=0, seconds=9.0, tests=120, skipped=False)["ok"])
        self.assertFalse(smoke.recall_part(exit_code=0, seconds=0.1, tests=120, skipped=True)["ok"])
        self.assertFalse(smoke.recall_part(exit_code=0, seconds=0.1, tests=0, skipped=False)["ok"])
        self.assertFalse(smoke.recall_part(exit_code=1, seconds=9.0, tests=120, skipped=False)["ok"])

    def test_resume_needs_three_kills_three_resumes_and_no_repeated_effect(self):
        good = {"kills": 3, "resumed": 3, "effects": [1, 2, 3, 4, 5, 6], "starts": [1, 2, 3, 4, 5, 6],
                "steps": 6, "settled": True, "wall_s": 30.0, "interrupted_rows": 3, "notes": [],
                "turn_ended": 2, "turn_failed": 0, "unknown_rows": 0, "succeeded_rows": 6, "seen": [1, 2, 3, 4, 5, 6]}
        self.assertTrue(smoke.resume_part(good)["ok"])
        self.assertFalse(smoke.resume_part({**good, "kills": 2, "resumed": 2})["ok"])
        self.assertFalse(smoke.resume_part({**good, "resumed": 2})["ok"])
        self.assertFalse(smoke.resume_part({**good, "settled": False})["ok"])
        self.assertFalse(smoke.resume_part({**good, "interrupted_rows": 2})["ok"])
        self.assertFalse(smoke.resume_part({**good, "interrupted_rows": None})["ok"])
        self.assertFalse(smoke.resume_part({**good, "turn_failed": 1})["ok"])
        self.assertFalse(smoke.resume_part({**good, "turn_ended": 1})["ok"])  # only the opening "hello" ended
        self.assertFalse(smoke.resume_part({**good, "succeeded_rows": 5})["ok"])
        unknown = smoke.resume_part({**good, "unknown_rows": 1, "seen": [1, 2, 3, 5, 6]})
        self.assertFalse(unknown["ok"])
        self.assertIn("never saw the output of steps [4]", unknown["note"])
        self.assertFalse(smoke.resume_part({**good, "notes": ["step 4 never started"]})["ok"])
        repeated = smoke.resume_part({**good, "effects": [1, 2, 3, 4, 4, 5, 6]})
        self.assertFalse(repeated["ok"])
        self.assertIn("repeated", repeated["note"])
        rerun = smoke.resume_part({**good, "starts": [1, 2, 3, 4, 4, 5, 6]})
        self.assertFalse(rerun["ok"])
        self.assertIn("repeated", rerun["note"])
        self.assertFalse(smoke.resume_part({**good, "effects": [1, 2, 3, 5, 6]})["ok"])

    def test_dm_needs_five_replies_in_time_with_footers_and_no_alarms(self):
        good = [{"turn": n, "seconds": 8.0, "footer": True, "expected": True} for n in range(5)]
        self.assertTrue(smoke.dm_part(good, alarms=0)["ok"])
        self.assertFalse(smoke.dm_part(good[:4], alarms=0)["ok"])
        self.assertFalse(smoke.dm_part(good[:4] + [{**good[4], "seconds": 61.0}], 0)["ok"])
        self.assertFalse(smoke.dm_part(good[:4] + [{**good[4], "footer": False}], 0)["ok"])
        self.assertFalse(smoke.dm_part(good, alarms=1)["ok"])
        self.assertFalse(smoke.dm_part([], alarms=0)["ok"])
        wrong = smoke.dm_part(good[:4] + [{"turn": 4, "seconds": 3.0, "footer": True, "expected": False}], 0)
        self.assertFalse(wrong["ok"])
        self.assertIn("unexpected reply", wrong["note"])
        self.assertTrue(smoke.dm_part(good, alarms=0, turns_ended=5, unread=[])["ok"])
        self.assertFalse(smoke.dm_part(good, alarms=0, turns_ended=0, unread=[])["ok"])
        self.assertFalse(smoke.dm_part(good, alarms=0, turns_ended=5, unread=["ses_x"])["ok"])

    def test_dm_runs_in_a_fresh_session_so_more_turns_than_its_own_is_a_fail(self):
        good = [{"turn": n, "seconds": 8.0, "footer": True, "expected": True} for n in range(5)]
        reused = smoke.dm_part(good, alarms=0, turns_ended=9, unread=[])
        self.assertFalse(reused["ok"])
        self.assertIn("not a fresh session", reused["note"])


class DmTurns(unittest.TestCase):
    def test_every_turn_says_it_is_the_release_smoke(self):
        for name, prompt, _ in dm.turns(REPO / "Cargo.toml"):
            self.assertTrue(prompt.startswith(dm.MARK), name)

    def test_the_tool_turns_read_a_file_under_the_session_directory(self):
        with tempfile.TemporaryDirectory() as t:
            self.assertEqual(dm.dm_file(Path(t)), Path(t) / "Cargo.toml")


def part(name, ok, cost=0.0, wall=1.0):
    return {"part": name, "ok": ok, "count": 1, "number": 1.0, "unit": "x", "floor": None,
            "delta": None, "note": "", "cost_usd": cost, "wall_s": wall}


class Spent(unittest.TestCase):
    def test_spent_sums_the_finished_trials_turn_traces_only(self):
        with tempfile.TemporaryDirectory() as t:
            root = Path(t)
            a = trial(root, "fix-git", 1.0, suffix="a1")
            (a / "agent" / "theseus-turn.json").write_text(json.dumps({"cost_usd": 0.25}))
            b = trial(root, "regex-log", 1.0, suffix="b2")
            (b / "agent" / "theseus-turn.json").write_text(json.dumps({"cost_usd": 0.5}))
            trial(root, "db-wal-recovery", None, suffix="c3")  # in flight: no trace yet
            self.assertAlmostEqual(smoke.spent(root), 0.75)


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
    def passing(self):
        tb = smoke.tb_part(tb_rows({**{t: True for t in SLICE["stable"]}, "c0": False, "c1": False}),
                           SLICE, floor=None, wall_s=600)
        route = smoke.route_part([smoke.route_run(replay(), 20.0)] * 3, None)
        return tb, route

    def test_floor_from_a_run_holds_what_the_comparisons_read(self):
        tb, route = self.passing()
        f = smoke.floor_from([tb, route], sha="75012080", date="2026-10-30")
        self.assertEqual(f["written"]["parts"], ["tb", "route"])
        self.assertEqual(f["tb"]["wall_s"], 600)
        self.assertEqual(f["tb"]["price_digest"], smoke.price_digest())
        self.assertAlmostEqual(f["tb"]["cost_per_solved_usd"], 0.125)
        self.assertEqual((f["route"]["wall_s"], f["route"]["model_worst"]), (20.0, 55.0))
        # What the next run compares with is what this run wrote.
        self.assertTrue(smoke.tb_part(tb_rows({**{t: True for t in SLICE["stable"]}, "c0": False, "c1": False}),
                                      SLICE, floor=f, wall_s=600)["ok"])

    def test_a_failed_part_keeps_the_previous_floors_entry(self):
        tb, _ = self.passing()
        bad_route = smoke.route_part([smoke.route_run(replay(model_ok=30), 99.0)] * 3, None)
        previous = {"route": {"wall_s": 20.0, "model_worst": 56.0, "sha": "old", "date": "d0"}}
        f = smoke.floor_from([tb, bad_route], sha="new", date="d1", previous=previous)
        self.assertEqual(f["written"]["parts"], ["tb"])
        self.assertEqual(f["route"], previous["route"])
        self.assertEqual(f["tb"]["sha"], "new")
        self.assertIn("FAIL", f["written"]["verdict"])

    def test_the_floor_is_this_releases_passing_reading(self):
        tb, route = self.passing()
        previous = {"tb": {"cost_per_solved_usd": 0.10, "wall_s": 500, "price_digest": smoke.price_digest()},
                    "route": {"wall_s": 10.0, "model_worst": 57.0}}
        f = smoke.floor_from([tb, route], sha="new", date="d1", previous=previous)
        self.assertEqual((f["tb"]["cost_per_solved_usd"], f["tb"]["wall_s"]), (0.125, 600))
        self.assertEqual((f["route"]["wall_s"], f["route"]["model_worst"]), (20.0, 55.0))

    def test_no_floor_when_nothing_a_floor_reads_passed(self):
        bad = smoke.tb_part([], SLICE, floor=None, wall_s=0)
        with self.assertRaises(smoke.SmokeError):
            smoke.floor_from([bad], sha="x", date="d")


if __name__ == "__main__":
    unittest.main()
