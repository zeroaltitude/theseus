"""The `+onto` arm and the decisions column (theseus-mqqc2): the arm differs from its control by the ontology alone,
a judgment row is read for what the column needs, the decisions are counted and priced by hand, and the report
says so. The scratch-daemon run of the arm is in test_th_smoke.py. Standard library only."""

from __future__ import annotations

import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import report as rep  # noqa: E402
import th_drive as td  # noqa: E402
import th_generate as gen  # noqa: E402
import th_score as ts  # noqa: E402
import th_threads as th  # noqa: E402

SONNET = "anthropic/claude-sonnet-5-5"


def cfg(arm: str) -> dict:
    # one directory for every call, so two arms' persona files name the same paths
    d = Path(tempfile.gettempdir()) / "th-onto-test-personas"
    return th.arm_config({"model": {"model": "m"}}, arm, gen.build(11, "smoke").personas, d)


def row(pack: str, outcome: str = "answered", micros: int = 1000, tin: int = 3000, tout: int = 30) -> dict:
    return {"pack": pack, "outcome": outcome, "cost_micros": micros, "input_tokens": tin, "output_tokens": tout}


class Arm(unittest.TestCase):
    def test_the_arm_is_a_threads_arm_the_scorer_routes_and_the_drivers_list(self):
        self.assertEqual(td.ARMS["A1o"]["layout"], "thread")
        self.assertIn("A1o", td.MEASURABLE)
        self.assertNotIn("A1o", ts.OWNER_ROUTED)  # its router is Jev's, so routing is scored
        self.assertIn("A1o", th.OVERRIDES)

    def test_it_differs_from_its_control_by_the_ontology_alone(self):
        on, off = cfg("A1o"), cfg("A1")
        self.assertEqual(on["ontology"], {"judge": True, "recall_boost": 0.25})
        self.assertEqual(off["ontology"], {"judge": False, "recall_boost": 0.0})
        self.assertEqual({k: v for k, v in on.items() if k != "ontology"}, {k: v for k, v in off.items() if k != "ontology"})

    def test_every_other_threads_arm_is_a_control_with_the_ontology_off(self):
        for arm in ("A1", "A1n", "A2"):
            self.assertEqual(cfg(arm)["ontology"], {"judge": False, "recall_boost": 0.0}, arm)
        self.assertEqual(cfg("A1n")["prefix"], {"enabled": False})

    def test_the_config_writes_as_toml_the_daemon_reads(self):
        import tomllib
        import drive as rd
        self.assertEqual(tomllib.loads(rd.toml_dumps(cfg("A1o")))["ontology"], {"judge": True, "recall_boost": 0.25})


class Rows(unittest.TestCase):
    def test_a_ledger_row_keeps_the_pack_the_outcome_the_bill_and_the_tokens(self):
        data = {"pack": "onto.v1", "outcome": {"outcome": "answered"}, "cost_micros": 420,
                "usage": {"input_tokens": 3100, "output_tokens": 41}, "answers": ["not kept"], "state": {"x": 1}}
        self.assertEqual(th.judgment_row({"position": 7, "data": data}), row("onto.v1", "answered", 420, 3100, 41))
        # a skipped judgment has no usage and no cost
        self.assertEqual(th.judgment_row({"data": {"pack": "onto.v1", "outcome": {"outcome": "skipped", "reason": "shed"}}}),
                         row("onto.v1", "skipped", 0, 0, 0))


class Decisions(unittest.TestCase):
    ROWS = [row("onto.v1"), row("onto.v1"), row("classify.v3", micros=500, tin=3000, tout=10), row("security.v3", micros=200, tin=1000, tout=5),
            row("onto.v1", "failed"), row("people.v1")]

    def test_counted_by_class_and_priced_by_hand(self):
        d = ts.decisions(self.ROWS, 2, SONNET)
        self.assertEqual(d["n"], 4)  # the failed one and the people pack are not decisions taken
        self.assertEqual((d["by_class"]["filing"]["n"], d["by_class"]["routing"]["n"], d["by_class"]["privacy"]["n"]), (2, 1, 1))
        self.assertEqual(d["per_turn"], 2.0)
        self.assertEqual(d["by_class"]["filing"]["per_turn"], 1.0)
        self.assertEqual(d["unanswered"], 1)
        # Jev billed 1000 + 1000 + 500 + 200 micro-dollars
        self.assertAlmostEqual(d["jev_usd"], 0.0027)
        # the main model: 3000 * 2 + 30 * 10 = 6300 each for the two onto rows; 3000 * 2 + 10 * 10 = 6100; 1000 * 2 + 5 * 10 = 2050
        self.assertAlmostEqual(d["main_model_usd"], (6300 * 2 + 6100 + 2050) / 1e6)
        self.assertAlmostEqual(d["saved_usd"], d["main_model_usd"] - d["jev_usd"], places=6)

    def test_an_arm_that_asks_jev_nothing_has_no_column_and_says_so(self):
        self.assertIsNone(ts.decisions([], 10, SONNET))
        self.assertIsNone(ts.decisions([row("people.v1")], 10, SONNET))
        text = rep.decisions_section([{"arm": "A3", "name": "x"}])
        self.assertIn("the arm asks Jev no decision", text)

    def test_the_report_names_each_class_and_says_the_main_model_figure_is_modelled(self):
        d = ts.decisions(self.ROWS, 2, SONNET)
        text = rep.decisions_section([{"arm": "A1o", "decisions": d}, {"arm": "A1"}])
        self.assertIn("| A1o | 2.00 (0.50 / 1.00 / 0.50) |", text)
        self.assertIn("modelled, not measured", text)
        self.assertIn("none: the arm asks Jev no decision", text)  # the control's row

    def test_the_scorer_reads_a_runs_judgments_into_its_result_only_when_it_has_them(self):
        # score_run's own wiring: a run dict with rows adds `decisions`; load_run supplies [] when the file is absent
        self.assertEqual(ts.decision_class("route.v6"), "routing")
        self.assertEqual(ts.decision_class("categorize.v1"), "filing")
        self.assertIsNone(ts.decision_class("people.v1"))
        self.assertIsNone(ts.decision_class(None))


if __name__ == "__main__":
    unittest.main()
