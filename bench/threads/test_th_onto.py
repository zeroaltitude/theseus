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


def run_of(arm: str, key: str, **kw) -> dict:
    """The fields the report's figures and its +onto section read, from a few numbers."""
    sids = [x.id for x in gen.build(11, "smoke").streams]
    return {"arm": arm, "key": key, "name": f"arm {arm}", "layout": "thread", "oracle_mean": kw.get("oracle", 1.0),
            "streams": {s: {"oracle": kw.get("oracle", 1.0), "ideal_passed": 1, "ideal_checks": 1,
                            "recall": {"k": 1, "n": 1, "rate": 1.0}} for s in sids},
            "contamination": kw.get("cont", {"k": 0, "n": 0, "rate": None, "lo": 0, "hi": 0}),
            "routing": {"top1": kw["top1"]} if "top1" in kw else {"owner_routed": True},
            "tokens": {"request_mean": kw.get("tokens", 1000.0), "request_curve": [1, 2]},
            "usd": {"per_turn": kw.get("per_turn", 0.01), "measured": 1.0,
                    "modeled": {"5m": {"usd": 1.0, "misses": 0}, "1h": {"usd": 1.0, "misses": 0}}},
            "latency": {"wall_ms_mean": kw.get("wall", 1000.0), "wall_ms_p50": 1.0, "wall_ms_p90": 2.0},
            "decisions": kw.get("decisions")}


def rate(k: int, n: int) -> dict:
    return {"k": k, "n": n, "rate": k / n, "lo": 0.0, "hi": 1.0}


class Palette(unittest.TestCase):
    def test_a_figure_that_holds_a_1o_and_pi_is_refused_naming_both(self):
        prog = gen.build(11, "smoke")
        with self.assertRaises(ValueError) as cm:
            rep.figures([run_of("A1o", "theseus-threads-onto"), run_of("A7", "pi")], prog)
        self.assertIn("A1o", str(cm.exception))
        self.assertIn("A7", str(cm.exception))
        self.assertIn("slot 5", str(cm.exception))

    def test_a_1o_with_a_1_to_a_6_draws(self):
        prog = gen.build(11, "smoke")
        keys = {"A1": "theseus-threads", "A1n": "theseus-threads-noprefix", "A2": "theseus-threads-addressed",
                "A3": "theseus", "A4": "claude-code", "A5": "theseus-batching", "A6": "openclaw",
                "A1o": "theseus-threads-onto"}
        figs = rep.figures([run_of(a, k) for a, k in keys.items()], prog)
        self.assertEqual(figs[0]["name"], "stream-correctness")
        self.assertEqual(len(figs[0]["columns"]), len(keys))
        # many runs of one arm are not a collision
        rep.figures([run_of("A1o", "theseus-threads-onto"), run_of("A1o", "theseus-threads-onto")], prog)


class Against(unittest.TestCase):
    def runs(self):
        return [run_of("A1o", "theseus-threads-onto", oracle=0.8, cont=rate(1, 10), top1=rate(9, 10), tokens=1000.0,
                       per_turn=0.03, wall=2000.0, decisions={"per_turn": 2.0}),
                run_of("A1o", "theseus-threads-onto", oracle=1.0, cont=rate(2, 10), top1=rate(7, 10), tokens=1200.0,
                       per_turn=0.05, wall=2400.0, decisions={"per_turn": 3.0}),
                run_of("A1", "theseus-threads", oracle=0.5, cont=rate(4, 10), top1=rate(8, 10), tokens=1500.0,
                       per_turn=0.02, wall=1500.0),
                run_of("A1", "theseus-threads", oracle=0.7, cont=rate(6, 10), top1=rate(6, 10), tokens=1700.0,
                       per_turn=0.02, wall=1700.0)]

    def test_each_difference_is_a_1o_minus_a_1_over_the_arms_runs(self):
        text = rep.onto_section(self.runs())
        row = lambda label: next(x for x in text.splitlines() if x.startswith(f"| {label}"))
        # oracle mean: (0.8 + 1.0) / 2 = 90.0%, (0.5 + 0.7) / 2 = 60.0%, +30.0%
        self.assertEqual(row("**oracle mean**"), "| **oracle mean** | 90.0% | 60.0% | +30.0% |")
        # contamination pooled: 3 of 20 against 10 of 20: 15.0% - 50.0% = -35.0 points
        self.assertEqual(row("contamination"), "| contamination (in-stream answers with a colliding value) | 3/20 (15.0%) | 10/20 (50.0%) | -35.0 points |")
        # routing: 16 of 20 (80.0%) against 14 of 20 (70.0%): +10.0 points
        self.assertEqual(row("routing top-1"), "| routing top-1 (true stream) | 16/20 (80.0%) | 14/20 (70.0%) | +10.0 points |")
        # tokens: 1100 against 1600
        self.assertEqual(row("context tokens"), "| context tokens per model call, mean | 1,100 | 1,600 | -500 |")
        # dollars: 0.04 against 0.02
        self.assertEqual(row("dollars per message"), "| dollars per message, measured | $0.0400 | $0.0200 | +$0.0200 |")
        self.assertEqual(row("wall per message"), "| wall per message, mean (ms) | 2,200 | 1,600 | +600 |")
        # decisions: 2.5 for A1o, none for A1
        self.assertEqual(row("decisions per message"), "| decisions per message taken by Jev | 2.50 | 0.00 | +2.50 |")
        self.assertIn("2 run(s) of A1o against 2 of A1", text)

    def test_it_needs_both_arms(self):
        self.assertIsNone(rep.onto_section([run_of("A1o", "theseus-threads-onto")]))
        self.assertIsNone(rep.onto_section([run_of("A1", "theseus-threads")]))

    def test_the_report_has_the_section_only_with_both_arms_and_the_accuracy_under_its_heading(self):
        md = "# onto.v1 against the import's labels\n\n3 sessions replayed.\n"
        both = "\n".join(rep.onto_sections(self.runs(), md))
        self.assertIn("## The ontology arm against its control", both)
        self.assertIn("## Accuracy against the import's labels\n\n# onto.v1 against the import's labels", both)
        plain = "\n".join(rep.onto_sections(self.runs(), None))
        self.assertIn("## Accuracy against the import's labels\n\nNot run in this report", plain)
        one = "\n".join(rep.onto_sections(self.runs()[:2], None))
        self.assertNotIn("The ontology arm against its control", one)
        self.assertIn("Accuracy against the import's labels", one)
        self.assertEqual(rep.onto_sections(self.runs()[2:], None), [])

    def test_the_run_text_names_the_plan_the_overlay_and_each_command(self):
        text = rep.onto_run_text(11, "standard")
        for needle in ("bench/plans/threadbench-t13-onto.json", "jev-overlay.example.toml", "--config-overlay /tmp/jev.toml",
                       "th_generate.py --seed 11", "--arm $arm", "th_score.py", "judge replay --onto --limit 50",
                       "report.py", "--onto-replay", f"--seed {gen.HELD_OUT_SEED}"):
            self.assertIn(needle, text)
        doc = rep.improvement([], gen.build(11, "smoke"), 11, "standard")
        self.assertIn("What T13 (the first +onto run) needs", doc)
        self.assertIn("<A1|A1n|A1o|A2|A3|A4|A5|A6|A7>", rep.reproduction("onto", 11, "standard"))


class Overlay(unittest.TestCase):
    def test_the_overlay_parses_names_the_judge_keys_and_holds_no_value(self):
        import tomllib
        path = HERE / "jev-overlay.example.toml"
        t = tomllib.loads(path.read_text())
        self.assertEqual(set(t), {"judge", "secrets"})
        self.assertEqual((t["judge"]["enabled"], t["judge"]["key_secret"]), (True, "jev_api_key"))
        self.assertTrue(t["secrets"]["jev_api_key"].startswith(("op://", "env:", "file:")))
        self.assertIn("<your vault>", t["secrets"]["jev_api_key"])


if __name__ == "__main__":
    unittest.main()
