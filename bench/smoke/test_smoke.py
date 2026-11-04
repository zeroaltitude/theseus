"""Tests for the release smoke (theseus-yf6pj): the slice against the data it was
picked from, the trial reader, the floor comparison, the verdict and the
report. Standard library only; Python >= 3.11 (tomllib)."""

from __future__ import annotations

import json
import tempfile
import tomllib
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


def need80(of):
    """The owner's bar, written out here a second time on purpose: 80% of `of`, rounded up."""
    return -(-of * 4 // 5)


def replay(model_ok=55, must_ok=13, must_total=13, answered=61, n=61, held=2, bar=True, cost=0.01,
           pack="route.v10", bars=True, sha=None):
    """`theseus --json judge replay --cases`'s result as Phase 2 prints it (theseus-h66v4.6): `n` cases
    graded, the pins outside every denominator, and one bar per question (`model`, then `must`),
    each `need` the ratio of its own `of`. `sha` is the pack's digest (`candidate_sha256`), which
    says which question Jev was actually asked; None is a replay that printed none."""
    c = {"cases": [{}] * n, "answered": answered, "model_ok": model_ok, "held_elsewhere": held,
         "must_total": must_total, "must_ok": must_ok, "must_under_confirm": 0, "bar": bar, "pack": pack,
         "asked": ["model"]}
    if bars:
        c["bars"] = [{"question": "model", "ok": model_ok, "of": n, "need": need80(n),
                      "met": n > 0 and model_ok >= need80(n)},
                     {"question": "must", "ok": must_ok, "of": must_total, "need": need80(must_total),
                      "met": must_total > 0 and must_ok >= need80(must_total)}]
    out = {"set": "cases", "cost_usd": cost, "cases": c}
    return out if sha is None else {**out, "candidate_sha256": sha}


def effort_replay(effort_ok=58, n=61, answered=61, cost=0.01, bars=True, sha=None):
    """`theseus --json judge replay --pack-file effort.v1.toml --cases route.v10.cases.toml` as Phase 2 prints it: the
    same cases, one bar, `effort` within one rung of the label, `need` the ratio of its own `of`."""
    c = {"cases": [{}] * n, "answered": answered, "pack": "effort.v1", "asked": ["effort"], "bar": True}
    if bars:
        c["bars"] = [{"question": "effort", "ok": effort_ok, "of": n, "need": need80(n),
                      "met": n > 0 and effort_ok >= need80(n)}]
    out = {"set": "cases", "cost_usd": cost, "cases": c}
    return out if sha is None else {**out, "candidate_sha256": sha}


class RoutePart(unittest.TestCase):
    def runs(self, *raws, wall=20.0, effort=True):
        """Each route replay with the effort replay the tag gate takes beside it (`effort=False`: none)."""
        return [smoke.route_run(r, wall, effort_replay() if effort is True else effort or None) for r in raws]

    def part(self, *raws, live=None):
        return smoke.route_part(self.runs(*raws), floor=None, live=live)

    def test_a_candidate_that_meets_the_owners_bar_passes(self):
        # The regression this row exists for: 80% of the model's 61 is 49, of the 13 musts 11. The retired
        # bar (54 of 60, every must) failed a run that read 49 and 11.
        p = self.part(*[replay(model_ok=49, must_ok=11)] * 3)
        self.assertTrue(p["ok"], p["note"])
        self.assertEqual(p["number"], 49.0)

    def test_a_candidate_that_misses_the_owners_bar_fails_and_says_by_how_much(self):
        p = self.part(*[replay(model_ok=40, must_ok=9)] * 3)
        self.assertFalse(p["ok"])
        self.assertIn("40/61 on the model < 49", p["note"])
        self.assertIn("9/13 musts < 11", p["note"])

    def test_the_release_verdict_passes_on_three_runs_that_meet_the_owners_bar_and_fails_on_three_that_miss_it(self):
        # What the tag gate reads, end to end through smoke.verdict: the READY TO TAG line. A run of 49/61 on the model
        # and 11/13 on the musts is the owner's bar to the case and was a FAIL under the retired 54 of 60 + every must.
        others = [part(n, True) for n in smoke.PARTS if n != "route"]
        meets = smoke.route_part(self.runs(*[replay(model_ok=49, must_ok=11)] * 3), None, live="route.v10")
        self.assertEqual(smoke.verdict(others + [meets]), "PASS")
        self.assertEqual((meets["count"], meets["number"]), (3, 49.0))
        misses = smoke.route_part(self.runs(*[replay(model_ok=48, must_ok=11)] * 3), None, live="route.v10")
        self.assertEqual(smoke.verdict(others + [misses]), "FAIL (route)")
        self.assertIn("48/61 on the model < 49", misses["note"])

    def test_the_bar_is_the_edge_of_80_percent_rounded_up_of_each_questions_own_count(self):
        self.assertTrue(self.part(*[replay(model_ok=49, must_ok=11)] * 3)["ok"])
        low = self.part(replay(), replay(), replay(model_ok=48))
        self.assertFalse(low["ok"])
        self.assertIn("48/61 on the model < 49", low["note"])
        must = self.part(replay(), replay(), replay(must_ok=10))
        self.assertFalse(must["ok"])
        self.assertIn("10/13 musts < 11", must["note"])

    def test_the_bar_follows_the_denominator_the_replay_reports_and_no_count_is_written_in_the_output(self):
        for n in (61, 62, 65, 70, 100):
            need = need80(n)
            ok = self.part(*[replay(n=n, answered=n, model_ok=need)] * 3)
            self.assertTrue(ok["ok"], (n, ok["note"]))
            bad = self.part(*[replay(n=n, answered=n, model_ok=need - 1)] * 3)
            self.assertFalse(bad["ok"], n)
            self.assertIn(f"{need - 1}/{n} on the model < {need}", bad["note"])
        p = self.part(*[replay(n=65, answered=65, model_ok=60)] * 3)
        for text in (p["note"], p["unit"], p["runs"] and json.dumps(p["runs"])):
            self.assertNotIn("/60", text)
            self.assertNotIn("54", text)
        self.assertIn("/65", p["note"])

    def test_the_pin_tests_get_no_credit_the_bar_is_over_the_replays_own_cases(self):
        # Before Phase 2 the 58 + 2 held read as 60; the pins are outside every denominator now.
        p = self.part(*[replay(model_ok=49, held=2)] * 3)
        self.assertEqual(p["runs"][0]["total"], 61)
        self.assertEqual(p["runs"][0]["model"], 49)
        self.assertFalse(self.part(*[replay(model_ok=48, held=2)] * 3)["ok"])

    def test_the_musts_are_the_13_the_replay_grades_not_14_with_the_daemon_pin_test(self):
        p = self.part(*[replay(model_ok=55, must_ok=11, must_total=13)] * 3)
        self.assertTrue(p["ok"], p["note"])
        self.assertIn("11/13 musts", p["note"])

    # --- the model bar as a statement about the measured distribution (theseus-h66v4.7 round 2) ------
    # The owner, 2026-11-03 09:18, on the report's §9 proposal: "Range of measurement seems good too".

    def rngfloor(self, low=48, high=51, of=62):
        return {"route": {"model_range": {"low": low, "high": high, "of": of, "runs": 3},
                          "model_worst": {"ok": low, "of": of}, "sha": "d8d8c33a", "date": "2026-11-03"}}

    def ranged(self, *raws, live=None, floor=None):
        return smoke.route_part(self.runs(*raws), floor=floor or self.rngfloor(), live=live)

    def test_a_recorded_range_is_the_models_bar_and_the_80_percent_rule_no_longer_fails_it(self):
        # 48 / 51 / 50 of 62 is what the candidate measured: under the 80% rule 48 is a FAIL (the bar is 50),
        # under the range it is the floor itself, and passes.
        runs = [replay(n=62, answered=62, model_ok=k, must_ok=11) for k in (48, 51, 50)]
        self.assertFalse(smoke.route_part(self.runs(*runs), floor=None)["ok"])
        p = self.ranged(*runs)
        self.assertTrue(p["ok"], p["note"])
        self.assertIn("the measured range 48-51 of 62", p["note"])
        self.assertEqual(p["readings"]["model"], {"low": 48, "high": 51, "of": 62, "runs": 3})

    def test_a_reading_under_the_recorded_range_fails_and_says_by_how_much(self):
        p = self.ranged(*[replay(n=62, answered=62, model_ok=k, must_ok=11) for k in (47, 51, 50)])
        self.assertFalse(p["ok"])
        self.assertIn("worst 47/62 on the model is under the floor's measured range 48-51 of 62", p["note"])

    def test_a_reading_over_the_recorded_range_passes_and_says_the_range_is_stale(self):
        p = self.ranged(*[replay(n=62, answered=62, model_ok=k, must_ok=11) for k in (52, 54, 53)])
        self.assertTrue(p["ok"], p["note"])
        self.assertIn("is over the floor's measured range 48-51", p["note"])
        self.assertIn("rewrite it with --write-floor", p["note"])

    def test_a_range_measured_over_another_count_does_not_apply_and_the_80_percent_rule_holds(self):
        # A range read over 62 cases says nothing about 61: changing the set underneath a bar never loosens it.
        p = self.ranged(*[replay(n=61, answered=61, model_ok=48, must_ok=11)] * 3)
        self.assertFalse(p["ok"])
        self.assertIn("48/61 on the model < 49", p["note"])

    def test_a_malformed_range_is_ignored_and_the_80_percent_rule_holds(self):
        for bad in ({"low": 51, "high": 48, "of": 62}, {"low": 48, "high": 51, "of": 0},
                    {"low": -1, "high": 51, "of": 62}, {"low": 48, "high": 63, "of": 62},
                    {"low": "48", "high": 51, "of": 62}, "48-51", None):
            floor = {"route": {"model_range": bad}}
            p = smoke.route_part(self.runs(*[replay(n=62, answered=62, model_ok=48, must_ok=11)] * 3), floor)
            self.assertFalse(p["ok"], bad)
            self.assertIn("48/62 on the model < 50", p["note"])

    def test_the_musts_and_effort_keep_the_80_percent_rule_under_a_model_range(self):
        # The range is the model's alone: his ruling was about the one question the instrument cannot promise.
        p = self.ranged(*[replay(n=62, answered=62, model_ok=50, must_ok=10)] * 3)
        self.assertFalse(p["ok"])
        self.assertIn("10/13 musts < 11", p["note"])
        q = smoke.route_part(self.runs(*[replay(n=62, answered=62, model_ok=50, must_ok=11)] * 3,
                                       effort=effort_replay(effort_ok=48, n=62)), self.rngfloor())
        self.assertFalse(q["ok"])
        self.assertIn("48/62 on effort < 50", q["note"])

    def test_a_range_never_excuses_a_run_that_graded_nothing(self):
        # memory absence-bar-met-by-empty-set: the empty set holds every "nothing missed" bar.
        p = self.ranged(*[replay(n=0, answered=0, model_ok=0, must_ok=0, must_total=0)] * 3)
        self.assertFalse(p["ok"])

    def test_a_replay_whose_bar_is_not_the_80_percent_rule_is_a_fail_not_a_trust(self):
        raw = replay(model_ok=40)
        raw["cases"]["bars"][0]["need"] = 40  # a replay that graded itself against a lower bar
        raw["cases"]["bars"][0]["met"] = True
        p = self.part(*[raw] * 3)
        self.assertFalse(p["ok"])
        self.assertIn("is not 80% of 61", p["note"])

    def test_a_replay_without_bars_is_read_from_its_totals_by_the_same_rule(self):
        # A binary from before Phase 2 prints no `bars`: model_ok over the cases it graded, the musts over must_total.
        ok = self.part(*[replay(model_ok=49, must_ok=11, bars=False)] * 3)
        self.assertTrue(ok["ok"], ok["note"])
        bad = self.part(*[replay(model_ok=48, must_ok=11, bars=False)] * 3)
        self.assertFalse(bad["ok"])
        self.assertIn("48/61 on the model < 49", bad["note"])

    def test_grading_no_cases_is_a_fail_not_a_pass(self):
        # An empty set meets every "nothing missed" bar; a zero denominator must not.
        empty = replay(n=0, answered=0, model_ok=0, must_ok=0, must_total=0)
        self.assertFalse(self.part(*[empty] * 3)["ok"])
        # Answered, but the replay reported a bar over zero cases.
        zero = replay(model_ok=0, must_ok=0, must_total=0)
        zero["cases"]["bars"] = [{"question": "model", "ok": 0, "of": 0, "need": 0, "met": True},
                                 {"question": "must", "ok": 0, "of": 0, "need": 0, "met": True}]
        p = self.part(*[zero] * 3)
        self.assertFalse(p["ok"])
        self.assertIn("no cases graded", p["note"])
        # Cases graded but no must in the file: the must bar is over nothing.
        nomust = replay(must_ok=0, must_total=0)
        p = self.part(*[nomust] * 3)
        self.assertFalse(p["ok"])
        self.assertIn("no must", p["note"])
        # Bars present but none of them the model's.
        nomodel = replay()
        nomodel["cases"]["bars"] = [b for b in nomodel["cases"]["bars"] if b["question"] != "model"]
        self.assertFalse(self.part(*[nomodel] * 3)["ok"])

    def test_the_command_line_exits_1_for_a_zero_case_grading_and_0_for_the_owners_bar(self):
        with tempfile.TemporaryDirectory() as t:
            def go(raw):
                r, e = Path(t) / "r.json", Path(t) / "e.json"
                r.write_text(json.dumps(raw))
                e.write_text(json.dumps(effort_replay()))
                pair = [str(r), "1", "--effort", str(e)]
                return smoke.main(["route", t, "--live", "route.v10"] + pair * 3)
            self.assertEqual(go(replay(model_ok=49, must_ok=11)), 0)
            self.assertEqual(go(replay(n=0, answered=0, model_ok=0, must_ok=0, must_total=0)), 1)
            self.assertEqual(go(replay(model_ok=40, must_ok=9)), 1)

    def test_the_number_is_the_worst_run_on_the_model(self):
        p = smoke.route_part(self.runs(replay(model_ok=56), replay(model_ok=53), replay(model_ok=55)), None)
        self.assertEqual(p["number"], 53.0)  # no pin credit: 53 of 61

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
        self.assertIn("37/61", p["note"])
        parts = [part(n, True) for n in smoke.PARTS if n != "route"] + [p]
        self.assertEqual(smoke.verdict(parts), "PASS (route n/a until the flip)")
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
            self.assertIn("37/61 on the model < 49", p["note"])

    def test_a_canary_of_the_cases_pack_or_an_unreadable_ladder_is_held_to_the_bar(self):
        for live in ("route.v8,route.v10", "None", "route.v8,", ""):
            p = smoke.route_part(self.runs(*[replay(model_ok=37, must_ok=9)] * 3), None, live=live)
            self.assertFalse(p["ok"], live)

    def test_a_replay_that_names_no_pack_is_held_to_the_bar(self):
        raw = replay(model_ok=37, must_ok=9)
        del raw["cases"]["pack"]
        self.assertFalse(smoke.route_part(self.runs(*[raw] * 3), None, live="route.v8")["ok"])

    def test_the_command_line_takes_an_empty_live_as_unknown(self):
        with tempfile.TemporaryDirectory() as t:
            r = Path(t) / "r.json"
            r.write_text(json.dumps(replay(model_ok=37, must_ok=9)))
            args = ["route", t, "--live", "", str(r), "1", str(r), "1", str(r), "1"]
            self.assertEqual(smoke.main(args), 1)
            self.assertFalse(json.loads((Path(t) / "parts" / "route.json").read_text()).get("na"))
            self.assertEqual(smoke.main(["route", t, "--live", "route.v8"] + args[4:]), 0)
            self.assertTrue(json.loads((Path(t) / "parts" / "route.json").read_text())["na"])

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


class RouteEffort(unittest.TestCase):
    """theseus-h66v4.4's fix pass: `Jev chooses the model and effort` is graded at the tag. Each run is a route.v10
    replay and an effort.v1 replay over the same cases file; the effort bar joins that run's bars."""

    def part(self, route, effort, live="route.v10", n=3):
        return smoke.route_part([smoke.route_run(route, 20.0, effort)] * n, None, live=live)

    def test_effort_within_one_rung_is_graded_at_80_percent_of_its_own_count(self):
        ok = self.part(replay(), effort_replay(effort_ok=49))
        self.assertTrue(ok["ok"], ok["note"])
        self.assertIn("49/61 on effort", ok["note"])
        bad = self.part(replay(), effort_replay(effort_ok=48))
        self.assertFalse(bad["ok"])
        self.assertIn("48/61 on effort < 49", bad["note"])
        # The case that would have passed READY TO TAG silently: effort far under the bar, model and musts fine.
        self.assertFalse(self.part(replay(), effort_replay(effort_ok=30))["ok"])

    def test_the_effort_bar_follows_its_own_denominator(self):
        for n in (61, 65, 70):
            self.assertTrue(self.part(replay(), effort_replay(effort_ok=need80(n), n=n, answered=n))["ok"], n)
            self.assertFalse(self.part(replay(), effort_replay(effort_ok=need80(n) - 1, n=n, answered=n))["ok"], n)

    def test_a_missing_effort_replay_is_a_fail_the_gate_never_stops_covering_effort_silently(self):
        p = self.part(replay(), None)
        self.assertFalse(p["ok"])
        self.assertIn("no effort bar", p["note"])
        # One run of three without it is as bad.
        runs = [smoke.route_run(replay(), 20.0, effort_replay())] * 2 + [smoke.route_run(replay(), 20.0)]
        q = smoke.route_part(runs, None, live="route.v10")
        self.assertFalse(q["ok"])
        self.assertIn("run 3: the replay graded no effort bar", q["note"])
        # An effort replay that printed no bar (an older binary), or one over no cases, is not a pass either.
        self.assertFalse(self.part(replay(), effort_replay(bars=False))["ok"])
        self.assertFalse(self.part(replay(), effort_replay(effort_ok=0, n=0, answered=0))["ok"])

    def test_only_the_effort_bar_is_taken_from_the_effort_replay_and_its_bar_must_be_80_percent(self):
        both = effort_replay()
        both["cases"]["bars"].append({"question": "model", "ok": 0, "of": 61, "need": 49, "met": False})
        p = self.part(replay(), both)
        self.assertTrue(p["ok"], p["note"])
        self.assertEqual(sorted(b["question"] for b in p["runs"][0]["bars"]), ["effort", "model", "must"])
        lax = effort_replay(effort_ok=40)
        lax["cases"]["bars"][0].update(need=40, met=True)
        q = self.part(replay(), lax)
        self.assertFalse(q["ok"])
        self.assertIn("is not 80% of 61", q["note"])

    def test_the_cases_pack_and_the_run_count_stay_the_route_replays(self):
        # The na decision reads the route pack; an effort replay beside it does not make the run `effort.v1`'s.
        run = smoke.route_run(replay(), 20.0, effort_replay())
        self.assertEqual(run["pack"], "route.v10")
        self.assertAlmostEqual(run["cost_usd"], 0.02)
        na = self.part(replay(model_ok=37, must_ok=9), effort_replay(effort_ok=30), live="route.v8")
        self.assertTrue(na["ok"] and na["na"], na)
        # n/a with no effort replay at all is still n/a: the pack does not act, nothing is judged.
        self.assertTrue(self.part(replay(model_ok=37), None, live="route.v8")["na"])
        self.assertEqual(self.part(replay(), effort_replay(), live="route.v10")["count"], 3)

    def test_the_effort_reading_is_in_the_note_and_the_part_keeps_its_worst(self):
        p = smoke.route_part([smoke.route_run(replay(), 20.0, effort_replay(effort_ok=k)) for k in (58, 50, 55)],
                             None, live="route.v10")
        self.assertTrue(p["ok"])
        self.assertIn("50/61 on effort", p["note"])
        self.assertEqual(p["worst"]["effort"], {"ok": 50, "of": 61})

    def test_the_command_line_takes_the_effort_replay_beside_each_route_replay(self):
        with tempfile.TemporaryDirectory() as t:
            r, e, gone = Path(t) / "r.json", Path(t) / "e.json", Path(t) / "gone.json"
            r.write_text(json.dumps(replay()))

            def go(effort_json, *, args=None):
                if effort_json is not None:
                    e.write_text(json.dumps(effort_json))
                pair = args if args is not None else [str(r), "1", "--effort", str(e)]
                return smoke.main(["route", t, "--live", "route.v10"] + pair * 3)
            self.assertEqual(go(effort_replay(effort_ok=49)), 0)
            self.assertEqual(go(effort_replay(effort_ok=48)), 1)
            self.assertEqual(go(None, args=[str(r), "1"]), 1)  # no --effort
            self.assertEqual(go(None, args=[str(r), "1", "--effort", str(gone)]), 1)  # an effort replay never written
            self.assertIn("no effort bar", json.loads((Path(t) / "parts" / "route.json").read_text())["note"])
            e.write_text("not json")
            self.assertEqual(go(None), 1)


class RouteNotes(unittest.TestCase):
    def test_a_fail_note_numbers_every_run_by_its_place_among_all_runs(self):
        # theseus-h66v4.4 finding 2: [no cases, 40/61, ok, ok] named the 40/61 run "run 1" (numbered over the runs
        # that answered) beside the zero-case run's "run 1".
        runs = [smoke.route_run(replay(n=0, answered=0, model_ok=0, must_ok=0, must_total=0), 1.0, effort_replay()),
                smoke.route_run(replay(model_ok=40), 1.0, effort_replay()),
                smoke.route_run(replay(), 1.0, effort_replay()), smoke.route_run(replay(), 1.0, effort_replay())]
        p = smoke.route_part(runs, None, live="route.v10")
        self.assertFalse(p["ok"])
        self.assertIn("run 2: 40/61 on the model < 49", p["note"])
        self.assertIn("run 1: no cases graded", p["note"])
        self.assertNotIn("run 1: 40/61", p["note"])
        low_effort = [smoke.route_run(replay(), 1.0, effort_replay()), smoke.route_run(replay(), 1.0, effort_replay(effort_ok=40)),
                      smoke.route_run(replay(), 1.0, effort_replay())]
        self.assertIn("run 2: 40/61 on effort", smoke.route_part(low_effort, None, live="route.v10")["note"])

    def test_the_passing_note_does_not_say_the_daemons_bar_asks_more_than_the_part_grades(self):
        # Under Phase 2 route.v10's whole bar is exactly the model's, and the part grades model, must and effort.
        p = smoke.route_part([smoke.route_run(replay(), 1.0, effort_replay())] * 3, None, live="route.v10")
        self.assertTrue(p["ok"])
        self.assertNotIn("effort and confidence too", p["note"])
        self.assertIn("whole bar met 3/3", p["note"])


class RouteFloor(unittest.TestCase):
    def test_the_floor_keeps_each_worst_reading_with_its_own_count(self):
        # theseus-h66v4.4 finding 3: a bare 49 beside a later 52/65 is the defect this row exists to remove.
        runs = [smoke.route_run(replay(model_ok=k), 20.0, effort_replay(effort_ok=k + 2)) for k in (55, 49, 52)]
        route = smoke.route_part(runs, None, live="route.v10")
        tb, _ = Floor().passing()
        f = smoke.floor_from([tb, route], sha="x", date="d")
        self.assertEqual(f["route"]["model_worst"], {"ok": 49, "of": 61})
        self.assertEqual(f["route"]["effort_worst"], {"ok": 51, "of": 61})

    def test_the_next_release_prints_the_floor_with_its_count_and_a_legacy_bare_count_as_it_is(self):
        run = [smoke.route_run(replay(model_ok=55, n=65, answered=65), 20.0, effort_replay(n=65, answered=65))] * 3
        floor = {"route": {"wall_s": 20.0, "model_worst": {"ok": 49, "of": 61}}}
        p = smoke.route_part(run, floor, live="route.v10")
        self.assertEqual(p["floor"], "49/61")
        self.assertTrue(p["ok"], p["note"])
        legacy = smoke.route_part(run, {"route": {"wall_s": 20.0, "model_worst": 49.0}}, live="route.v10")
        self.assertEqual(legacy["floor"], 49.0)


class Floor(unittest.TestCase):
    def passing(self):
        tb = smoke.tb_part(tb_rows({**{t: True for t in SLICE["stable"]}, "c0": False, "c1": False}),
                           SLICE, floor=None, wall_s=600)
        route = smoke.route_part([smoke.route_run(replay(), 20.0, effort_replay())] * 3, None)
        return tb, route

    def test_floor_from_a_run_holds_what_the_comparisons_read(self):
        tb, route = self.passing()
        f = smoke.floor_from([tb, route], sha="75012080", date="2026-10-30")
        self.assertEqual(f["written"]["parts"], ["tb", "route"])
        self.assertEqual(f["tb"]["wall_s"], 600)
        self.assertEqual(f["tb"]["price_digest"], smoke.price_digest())
        self.assertAlmostEqual(f["tb"]["cost_per_solved_usd"], 0.125)
        self.assertEqual((f["route"]["wall_s"], f["route"]["model_worst"]), (20.0, {"ok": 55, "of": 61}))
        # What the next run compares with is what this run wrote.
        self.assertTrue(smoke.tb_part(tb_rows({**{t: True for t in SLICE["stable"]}, "c0": False, "c1": False}),
                                      SLICE, floor=f, wall_s=600)["ok"])

    def test_a_failed_part_keeps_the_previous_floors_entry(self):
        tb, _ = self.passing()
        bad_route = smoke.route_part([smoke.route_run(replay(model_ok=30), 99.0, effort_replay())] * 3, None)
        previous = {"route": {"wall_s": 20.0, "model_worst": {"ok": 56, "of": 61}, "sha": "old", "date": "d0"}}
        f = smoke.floor_from([tb, bad_route], sha="new", date="d1", previous=previous)
        self.assertEqual(f["written"]["parts"], ["tb"])
        self.assertEqual(f["route"], previous["route"])
        self.assertEqual(f["tb"]["sha"], "new")
        self.assertIn("FAIL", f["written"]["verdict"])

    def test_the_floor_is_this_releases_passing_reading(self):
        tb, route = self.passing()
        previous = {"tb": {"cost_per_solved_usd": 0.10, "wall_s": 500, "price_digest": smoke.price_digest()},
                    "route": {"wall_s": 10.0, "model_worst": {"ok": 57, "of": 61}}}
        f = smoke.floor_from([tb, route], sha="new", date="d1", previous=previous)
        self.assertEqual((f["tb"]["cost_per_solved_usd"], f["tb"]["wall_s"]), (0.125, 600))
        self.assertEqual((f["route"]["wall_s"], f["route"]["model_worst"]), (20.0, {"ok": 55, "of": 61}))

    def test_the_floor_records_the_runs_measured_range_for_the_model(self):
        tb = smoke.tb_part(tb_rows({**{t: True for t in SLICE["stable"]}, "c0": False, "c1": False}),
                           SLICE, floor=None, wall_s=600)
        raws = [replay(n=62, answered=62, model_ok=k, must_ok=11) for k in (48, 51, 50)]
        route = smoke.route_part([smoke.route_run(r, 20.0, effort_replay(n=62)) for r in raws],
                                 {"route": {"model_range": {"low": 48, "high": 51, "of": 62, "runs": 3}}})
        f = smoke.floor_from([tb, route], sha="new", date="2026-11-03")
        self.assertEqual(f["route"]["model_range"], {"low": 48, "high": 51, "of": 62, "runs": 3})
        self.assertEqual(f["route"]["model_worst"], {"ok": 48, "of": 62})
        self.assertEqual(f["route"]["must_worst"], {"ok": 11, "of": 13})
        # What this run wrote is what the next run is held to.
        self.assertTrue(smoke.route_part([smoke.route_run(r, 20.0, effort_replay(n=62)) for r in raws], f)["ok"])

    def test_no_floor_when_nothing_a_floor_reads_passed(self):
        bad = smoke.tb_part([], SLICE, floor=None, wall_s=0)
        with self.assertRaises(smoke.SmokeError):
            smoke.floor_from([bad], sha="x", date="d")


class RouteOnAWiredPack(unittest.TestCase):
    """theseus-8ouns: `judge replay --cases` replays a wired pack, so with route.v10 live the part reaches its bar."""

    def test_a_live_cases_pack_that_meets_the_bar_passes_and_is_not_na(self):
        # Each run carries its effort.v1 replay: a run without one is a FAIL (the effort bar, within one rung).
        runs = [smoke.route_run(replay(), 20.0, effort_replay()) for _ in range(3)]
        p = smoke.route_part(runs, None, live="route.v10")
        self.assertTrue(p["ok"], p)
        self.assertFalse(p.get("na"))
        self.assertEqual(p["count"], 3)

    def test_runs_that_graded_no_case_never_read_as_a_pass(self):
        # A replay of a file with no cases (the command refuses it now; an older binary did not): zero graded
        # cases, bar "not judged". Three of them are three runs that answered nothing.
        empty = {"set": "cases", "cost_usd": 0.0, "cases": {"cases": [], "answered": 0, "model_ok": 0,
                 "held_elsewhere": 2, "must_total": 0, "must_ok": 0, "bar": None, "pack": "route.v10"}}
        for live in ("route.v10", "route.v8", None):
            p = smoke.route_part([smoke.route_run(empty, 1.0)] * 3, None, live=live)
            self.assertFalse(p["ok"], live)
            self.assertFalse(p.get("na"), live)
            self.assertIn("0 of 3 runs answered any case", p["note"])

    def test_a_replay_that_exits_clean_with_no_answer_is_a_fail_not_a_pass(self):
        # The CLI exits 0 on "bar: not judged" (a breaker tripped, the limit cut every case): the 58 cases
        # are there and Jev answered none. Three of those must not read as a pass, nor as "n/a until the flip".
        mute = replay(answered=0, model_ok=0, must_ok=0, bar=None)
        for live in ("route.v10", "route.v8", None):
            p = smoke.route_part([smoke.route_run(mute, 1.0)] * 3, None, live=live)
            self.assertFalse(p["ok"], live)
            self.assertFalse(p.get("na"), live)
            self.assertIn("0 of 3 runs answered any case", p["note"])


class ThreeSamples(unittest.TestCase):
    """theseus-h420q: three runs must be three Jev samples. The 1.0.4 release smoke's three route
    runs each cost $0.004718 and each scored 51 of 62 to six decimal places -- one cached answer
    served three times -- and would have written a floor of 51 from it. The driver gives each run
    its own pack version (`smoke.bust`), and the part fails when two runs share a pack digest."""

    PACK = REPO / "crates" / "theseus-judge" / "packs" / "route.v10.toml"
    CASES = REPO / "crates" / "theseus-judge" / "packs" / "route.v10.cases.toml"
    EFFORT = REPO / "crates" / "theseus-judge" / "packs" / "effort.v1.toml"

    def runs(self, shas, effort_shas=None):
        effort_shas = effort_shas or [None] * len(shas)
        return [smoke.route_run(replay(sha=s), 20.0, effort_replay(sha=e))
                for s, e in zip(shas, effort_shas)]

    def test_three_distinct_digests_pass_and_two_equal_ones_fail(self):
        ok = smoke.route_part(self.runs(["a" * 64, "b" * 64, "c" * 64]), None, live="route.v10")
        self.assertTrue(ok["ok"], ok["note"])
        same = smoke.route_part(self.runs(["a" * 64] * 3), None, live="route.v10")
        self.assertFalse(same["ok"], same["note"])
        self.assertIn("one cached sample", same["note"])
        self.assertIn("runs 1 and 2", same["note"])
        # Two of three is enough to fail it: the worst of three is then the worst of two.
        two = smoke.route_part(self.runs(["a" * 64, "b" * 64, "a" * 64]), None, live="route.v10")
        self.assertFalse(two["ok"], two["note"])

    def test_the_effort_replays_count_too_and_a_replay_with_no_digest_is_not_judged(self):
        shared = smoke.route_part(self.runs(["a" * 64, "b" * 64, "c" * 64], ["e" * 64] * 3),
                                  None, live="route.v10")
        self.assertFalse(shared["ok"], shared["note"])
        # A binary that prints no digest: nothing to compare, and the rest of the part still holds.
        quiet = smoke.route_part([smoke.route_run(replay(), 20.0, effort_replay())] * 3,
                                 None, live="route.v10")
        self.assertTrue(quiet["ok"], quiet["note"])

    def test_bumping_the_version_renames_the_real_pack_and_changes_nothing_else(self):
        text = self.PACK.read_text()
        out, name = smoke.bump_version(text, smoke.BUMP_BASE + 7)
        self.assertEqual(name, f"route.v{smoke.BUMP_BASE + 7}")
        before, after = tomllib.loads(text), tomllib.loads(out)
        self.assertNotEqual(before["version"], after["version"])
        before.pop("version"), after.pop("version")
        self.assertEqual(before, after, "only the version moves")
        with self.assertRaises(smoke.SmokeError):
            smoke.bump_version("id = 'route'\n", 5)

    def test_the_bumped_packs_are_named_in_the_real_cases_files_also_and_its_pack_is_left_alone(self):
        text = self.CASES.read_text()
        out = smoke.also_grades(text, ["route.v1000007", "effort.v1000007"])
        before, after = tomllib.loads(text), tomllib.loads(out)
        self.assertEqual(after["pack"], before["pack"], "the set's own pack is what route_part reads")
        self.assertEqual(after["also"], before["also"] + ["route.v1000007", "effort.v1000007"])
        self.assertEqual(after["case"], before["case"], "the cases are untouched")
        self.assertEqual(after["defaults"], before["defaults"])
        # Idempotent, and a name the file already carries is not added twice.
        self.assertEqual(smoke.also_grades(out, ["route.v1000007", before["pack"]]), out)

    def test_the_part_reads_the_cases_files_own_pack_not_the_bumped_candidates(self):
        # The regression this guard exists for: the replay names the candidate, which is a bumped
        # copy (`route.v938510651`), so the acting check read "the cases are route.v938510651's"
        # and turned the whole part into "n/a until the flip" -- a silent PASS.
        bumped = [replay(pack="route.v938510651", sha=c * 64) for c in "abc"]
        runs = [smoke.route_run(r, 20.0, effort_replay(sha=e * 64))
                for r, e in zip(bumped, "xyz")]
        na = smoke.route_part(runs, None, live="route.v10")
        self.assertTrue(na.get("na"), na["note"])
        read = smoke.route_part(runs, None, live="route.v10", cases_pack="route.v10")
        self.assertFalse(read.get("na"), read["note"])
        self.assertTrue(read["ok"], read["note"])
        self.assertEqual(smoke.cases_pack_of(self.CASES), "route.v10")
        self.assertIsNone(smoke.cases_pack_of(self.CASES.parent / "no-such-file.toml"))
        # A candidate that is no version of the cases' pack is a FAIL, not an n/a and not a pass.
        other = [smoke.route_run(replay(pack="loop.v2", sha=c * 64), 20.0,
                                 effort_replay(sha=e * 64)) for c, e in zip("abc", "xyz")]
        wrong = smoke.route_part(other, None, live="route.v10", cases_pack="route.v10")
        self.assertFalse(wrong["ok"])
        self.assertIn("is no version of route.v10", wrong["note"])

    def test_bust_writes_a_runs_copies_and_every_run_gets_its_own_version(self):
        with tempfile.TemporaryDirectory() as d:
            got = [smoke.bust(Path(d), i, [self.PACK, self.EFFORT], self.CASES, at=1_762_000_000)
                   for i in (1, 2, 3)]
            versions = [tomllib.loads(run[0].read_text())["version"] for run in got]
            self.assertEqual(len(set(versions)), 3, versions)
            for run, version in zip(got, versions):
                self.assertEqual(len(run), 3)
                route, effort, cases = (tomllib.loads(p.read_text()) for p in run)
                # One version for the run, and both names in the cases file the run replays with.
                self.assertEqual((route["version"], effort["version"]), (version, version))
                self.assertIn(f"route.v{version}", cases["also"])
                self.assertIn(f"effort.v{version}", cases["also"])
                self.assertEqual(cases["pack"], "route.v10")
            # Above every version anybody writes by hand, so a copy is never read as the pack's own,
            # and inside a u32, which is what a pack's version is: a bare epoch times ten is not,
            # and the daemon refused the file with "expected u32" on the first live run.
            self.assertTrue(all(smoke.BUMP_BASE < v < 2 ** 32 for v in versions), versions)
            for at in (0, 1_762_000_000, 2 ** 31, 2 ** 32 + 7, 4_000_000_000):
                for i in (1, 2, 3):
                    run = smoke.bust(Path(d), i, [self.PACK], self.CASES, at=at)
                    v = tomllib.loads(run[0].read_text())["version"]
                    self.assertTrue(smoke.BUMP_BASE < v < 2 ** 32, (at, i, v))


if __name__ == "__main__":
    unittest.main()
