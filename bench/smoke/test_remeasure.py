"""Tests for the route re-measurement (1.0.7, issue 5): which cases a change moves, the comparison of two
interleaved arms, and what it refuses. Standard library only; Python >= 3.11 (tomllib)."""

from __future__ import annotations

import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

import remeasure

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
CASES = REPO / "crates/theseus-judge/packs/route.v10.cases.toml"

BASE = """pack = "route.v10"
[defaults]
est_tokens = 40000
last_reply_model = "claude-sonnet-5-5"
switch_cost = "cheaper_to_switch"
cheaper_model = "haiku, deepseek_flash (off-peak)"
[[case]]
id = "A"
model = "haiku"
effort = "low"
[[case]]
id = "B"
est_tokens = 200000
switch_cost = "cheaper_to_switch"
cheaper_model = "deepseek_flash (off-peak)"
model = "sonnet"
effort = "low"
must = true
[[case]]
id = "C"
last_reply_model = ""
switch_cost = "about_the_same"
model = "haiku"
effort = "low"
"""
NEW = """pack = "route.v10"
[defaults]
est_tokens = 40000
last_reply_model = "claude-sonnet-5-5"
switch_cost = "about_the_same"
[[case]]
id = "A"
model = "haiku"
effort = "low"
[[case]]
id = "B"
est_tokens = 200000
switch_cost = "cheaper_to_stay"
model = "sonnet"
effort = "low"
must = true
[[case]]
id = "C"
last_reply_model = ""
switch_cost = "about_the_same"
model = "haiku"
effort = "low"
"""


def check(got, raw=None, conf=0.9, want="x", sub=False):
    return {"want": want, "got": got, "raw": raw or got, "confidence": conf, "agrees": got == want,
            "close": got == want, "sub_confirm": sub}


def replay(question, answers, ok_bar, pack="route.v10"):
    """One `judge replay --cases` result of `question`, each case answered by `answers[id]` (model, or effort)."""
    rows = [{"id": i, "must": False, "answered": True, "model": check(a if question == "model" else "", want=w),
             "effort": check(a if question == "effort" else "", want=w)} for i, (a, w) in answers.items()]
    bars = [{"question": q, "ok": ok, "of": len(rows), "need": 3, "met": True} for q, ok in ok_bar.items()]
    return {"set": "cases", "cases": {"pack": pack, "cases": rows, "answered": len(rows), "bars": bars}}


class Changed(unittest.TestCase):
    def test_a_default_that_moves_moves_every_case_that_takes_it_and_no_other(self):
        # A takes the default; B names its own and moves; C names a cold session and does not move.
        self.assertEqual(remeasure.changed(BASE, NEW), ["A", "B"])

    def test_a_name_beside_another_word_is_not_a_difference(self):
        # The parser reads `cheaper_model` only with `cheaper_to_switch`: a leftover name changes nothing.
        stale = NEW.replace('switch_cost = "about_the_same"\n[[case]]\nid = "A"',
                            'switch_cost = "about_the_same"\ncheaper_model = "haiku"\n[[case]]\nid = "A"')
        self.assertEqual(remeasure.changed(NEW, stale), [])

    def test_a_label_and_a_case_in_one_file_only_are_differences(self):
        relabelled = NEW.replace('switch_cost = "about_the_same"\nmodel = "haiku"', 'switch_cost = "about_the_same"\nmodel = "opus"')
        self.assertNotEqual(relabelled, NEW)
        self.assertEqual(remeasure.changed(NEW, relabelled), ["C"])
        self.assertEqual(remeasure.changed(NEW, NEW.split('[[case]]\nid = "C"')[0]), ["C"])

    def test_the_committed_cases_file_against_itself_changes_nothing(self):
        text = CASES.read_text()
        self.assertEqual(remeasure.changed(text, text), [])
        self.assertEqual(len(remeasure.resolved(text)), 62)


class Report(unittest.TestCase):
    def out(self, tmp: Path, *, base_b="sonnet", new_b="haiku", runs=2, drop=None):
        (tmp / "base.cases.toml").write_text(BASE)
        (tmp / "new.cases.toml").write_text(NEW)
        for arm, b_ans in (("base", base_b), ("new", new_b)):
            for n in range(1, runs + 1):
                m = {"A": ("haiku", "haiku"), "B": (b_ans, "sonnet"), "C": ("haiku", "haiku")}
                e = {i: ("low", "low") for i in m}
                (tmp / f"route-{arm}-{n}.json").write_text(
                    json.dumps(replay("model", m, {"model": 3 if b_ans == "sonnet" else 2, "must": 1})))
                (tmp / f"effort-{arm}-{n}.json").write_text(json.dumps(replay("effort", e, {"effort": 3},
                                                                              "effort.v1")))
        if drop:
            (tmp / drop).unlink()

    def test_it_names_the_changed_cases_and_the_one_whose_answer_moved(self):
        with tempfile.TemporaryDirectory() as t:
            tmp = Path(t)
            self.out(tmp)
            s = remeasure.summarize(tmp)
            self.assertEqual(s["changed"], ["A", "B"])
            self.assertEqual(s["arms"]["base"]["model"], [(3, 3), (3, 3)])
            self.assertEqual(s["arms"]["new"]["model"], [(2, 3), (2, 3)])
            self.assertEqual(s["cases"]["B"]["base"]["model_right"], 2)
            self.assertEqual(s["cases"]["B"]["new"]["model_got"], ["haiku", "haiku"])
            self.assertEqual(s["cases"]["B"]["new"]["state"],
                             {"switch_cost": "cheaper_to_stay", "cheaper_model": None})
            md = remeasure.report_md(s)
            self.assertIn("The 1 cases whose answer moved", md)
            self.assertIn("| B | sonnet low **must** | cheaper_to_switch (deepseek_flash (off-peak)) -> "
                          "cheaper_to_stay | sonnet, sonnet | haiku, haiku |", md)
            self.assertIn("The 1 other changed cases", md)

    def test_an_unchanged_answer_is_listed_apart_and_not_called_moved(self):
        with tempfile.TemporaryDirectory() as t:
            tmp = Path(t)
            self.out(tmp, new_b="sonnet")
            s = remeasure.summarize(tmp)
            self.assertFalse(any(remeasure._moved(c) for c in s["cases"].values()))
            self.assertIn("The 0 cases whose answer moved", remeasure.report_md(s))

    def test_a_missing_run_or_an_unanswered_case_is_refused_not_read_as_a_miss(self):
        with tempfile.TemporaryDirectory() as t:
            tmp = Path(t)
            self.out(tmp, drop="effort-new-2.json")
            with self.assertRaises(remeasure.RemeasureError) as e:
                remeasure.summarize(tmp)
            self.assertIn("1 effort runs", str(e.exception))
        with tempfile.TemporaryDirectory() as t:
            tmp = Path(t)
            self.out(tmp)
            raw = json.loads((tmp / "route-new-1.json").read_text())
            raw["cases"]["answered"] = 2
            (tmp / "route-new-1.json").write_text(json.dumps(raw))
            with self.assertRaises(remeasure.RemeasureError) as e:
                remeasure.summarize(tmp)
            self.assertIn("2 answered of 3 graded", str(e.exception))

    def test_the_command_line_writes_the_report_and_says_it_in_one_line(self):
        with tempfile.TemporaryDirectory() as t:
            tmp = Path(t)
            self.out(tmp)
            p = subprocess.run([sys.executable, "-I", str(HERE / "remeasure.py"), "report", str(tmp)],
                               capture_output=True, text=True)
            self.assertEqual(p.returncode, 0, p.stderr)
            self.assertEqual(len(p.stdout.splitlines()), 1)
            self.assertIn("2 cases changed state, 1 answered differently; model 3 / 3 of 3 before, "
                          "2 / 2 of 3 after", p.stdout)
            self.assertTrue((tmp / "report.md").exists() and (tmp / "summary.json").exists())
            p = subprocess.run([sys.executable, "-I", str(HERE / "remeasure.py"), "report", str(tmp / "none")],
                               capture_output=True, text=True)
            self.assertEqual(p.returncode, 1)


if __name__ == "__main__":
    unittest.main()
