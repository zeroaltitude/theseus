"""The decisions column, sharper (theseus-2e6c1): a decision is a decision question answered at the act band, by class,
counted per question and attributed to a turn; an old run's rows still score and say what is not recorded. Standard
library only."""

from __future__ import annotations

import sys
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import report as rep  # noqa: E402
import th_decisions as dec  # noqa: E402
import th_score as ts  # noqa: E402
import th_threads as th  # noqa: E402

SONNET = "anthropic/claude-sonnet-5-5"


def ans(q: str, band: str = "act", chose: bool = True) -> dict:
    return {"q": q, "band": band, "chose": chose}


def row(pack: str, answers: list[dict] | None = None, outcome: str = "answered", micros: int = 1000, tin: int = 3000,
        tout: int = 30, **kw) -> dict:
    r = {"pack": pack, "outcome": outcome, "cost_micros": micros, "input_tokens": tin, "output_tokens": tout}
    if answers is not None:
        r["answers"] = answers
    return {**r, **kw}


def turn(index: int, turn_id: str | None = None, ended: int | None = None, took: int = 1000,
         session: str = "ses_home") -> dict:
    t = {"index": index, "session_id": session, "latency_ms": took}
    if turn_id:
        t["turn_id"] = turn_id
    if ended is not None:
        t["ended_ms"] = ended
    return t


class Counting(unittest.TestCase):
    def test_one_onto_judgment_counts_a_decision_a_question_not_one_filing(self):
        j = row("onto.v1", [ans("topic"), ans("resource"), ans("personal")])
        d = ts.decisions([j], 1, SONNET)
        self.assertEqual((d["by_class"]["filing"]["n"], d["by_class"]["privacy"]["n"]), (2, 1))
        self.assertNotIn("routing", d["by_class"])
        self.assertEqual((d["n"], d["judgments"], d["per_turn"]), (3, 1, 3.0))
        self.assertEqual(d["by_class"]["filing"]["judgments"], 1)
        self.assertTrue(d["recorded"])

    def test_a_confirm_or_escalate_band_answer_counts_none(self):
        j = row("onto.v1", [ans("topic", "confirm"), ans("resource", "escalate"), ans("personal", "confirm")])
        d = ts.decisions([j], 1, SONNET)
        self.assertEqual(d["n"], 0)
        self.assertEqual(d["judgments"], 1)  # the judgment was answered; none of it was a decision
        q = d["questions"]["onto.v1/topic"]
        self.assertEqual((q["answered"], q["act"], q["chose"]), (1, 0, 0))

    def test_an_act_band_none_is_a_decision_that_chose_nothing(self):
        d = ts.decisions([row("onto.v1", [ans("topic", chose=False), ans("resource")])], 1, SONNET)
        self.assertEqual(d["n"], 2)
        self.assertEqual(d["by_class"]["filing"]["chose"], 1)
        self.assertEqual(d["questions"]["onto.v1/topic"], {"class": "filing", "answered": 1, "act": 1, "chose": 0})

    def test_the_classes_are_the_ones_the_design_names(self):
        for pack, q, want in (("onto.v1", "topic", "filing"), ("onto.v1", "capability", "filing"),
                              ("onto.v1", "personal", "privacy"), ("onto.v1", "elicits", "privacy"),
                              ("onto.v1", "shows_address", "privacy"), ("onto.v1", "topic_name", None),
                              ("onto.v1", "still_topic", None), ("categorize.v1", "new_topic", "filing"),
                              ("route.v7", "mode", "routing"), ("classify.v3", "thread", "routing"),
                              ("classify.v3", "offer_task", None), ("security.v3", "risky", "privacy"),
                              ("people.v1", "real", None), ("loop.v1", "work_state", None)):
            self.assertEqual(dec.question_class(pack, q), want, f"{pack}/{q}")

    def test_a_judgment_of_no_decision_pack_is_no_decision(self):
        self.assertIsNone(ts.decisions([row("people.v1", [ans("real")]), row("loop.v1", [ans("work_state")])], 3, SONNET))
        self.assertIsNone(ts.decisions([], 3, SONNET))

    def test_the_dollars_stay_per_judgment_and_modelled(self):
        rows = [row("onto.v1", [ans("topic"), ans("personal")], micros=1000), row("security.v3", [ans("risky")], micros=200, tin=1000, tout=5)]
        d = ts.decisions(rows, 2, SONNET)
        self.assertAlmostEqual(d["jev_usd"], 0.0012)
        self.assertAlmostEqual(d["main_model_usd"], (3000 * 2 + 30 * 10 + 1000 * 2 + 5 * 10) / 1e6)
        self.assertEqual(d["by_class"]["filing"]["judgments"], 1)


class Attribution(unittest.TestCase):
    TURNS = [turn(0, "trn_a", 10_000), turn(1, "trn_b", 20_000), turn(2, None, 30_000)]

    def test_a_judgment_is_counted_to_the_turn_its_context_names(self):
        rows = [row("onto.v1", [ans("topic")], turn="trn_b", at_ms=99_999)]  # the time would say nothing
        d = dec.decisions(rows, self.TURNS, (3.0, 15.0))
        self.assertEqual(d["turns"]["attributed"], {"turn_id": 1, "time": 0, "between": 0})
        self.assertEqual(d["turns"]["share_with_decision"], round(1 / 3, 3))

    def test_else_by_session_and_the_turn_whose_window_holds_its_time(self):
        rows = [row("onto.v1", [ans("topic")], at_ms=29_500, session="ses_home"),   # turn 2's window [29000, 30000]
                row("onto.v1", [ans("topic")], at_ms=29_600, session="ses_other")]  # another session's: no turn
        d = dec.decisions(rows, self.TURNS, (3.0, 15.0))
        self.assertEqual(d["turns"]["attributed"], {"turn_id": 0, "time": 1, "between": 1})

    def test_else_between_turns_and_the_mean_still_counts_it(self):
        rows = [row("onto.v1", [ans("topic"), ans("resource")], at_ms=500_000)]
        d = dec.decisions(rows, self.TURNS, (3.0, 15.0))
        self.assertEqual(d["turns"]["attributed"], {"turn_id": 0, "time": 0, "between": 2})
        self.assertEqual((d["turns"]["mean"], d["turns"]["share_with_decision"]), (round(2 / 3, 3), None))

    def test_the_share_of_turns_with_a_decision_and_the_mean(self):
        rows = [row("onto.v1", [ans("topic"), ans("resource")], turn="trn_a"), row("route.v7", [ans("mode")], turn="trn_a"),
                row("onto.v1", [ans("topic", "confirm")], turn="trn_b")]
        d = dec.decisions(rows, self.TURNS, (3.0, 15.0))
        self.assertEqual(d["turns"]["mean"], 1.0)  # three decisions over three turns
        self.assertEqual(d["turns"]["share_with_decision"], round(1 / 3, 3))  # turn a alone had one
        self.assertEqual(d["n"], 3)


class Old(unittest.TestCase):
    def test_an_old_runs_rows_still_score_and_the_questions_are_not_recorded(self):
        rows = [row("onto.v1"), row("onto.v1"), row("classify.v3", micros=500), row("security.v3", micros=200),
                row("onto.v1", outcome="failed")]
        d = ts.decisions(rows, 2, SONNET)
        self.assertEqual((d["n"], d["judgments"]), (4, 4))  # by judgment, as the column was
        self.assertFalse(d["recorded"])
        self.assertEqual(d["questions"], "not recorded")
        self.assertEqual(d["turns"]["attributed"], "not recorded")
        self.assertIsNone(d["turns"]["share_with_decision"])
        self.assertEqual(d["unanswered"], 1)

    def test_a_run_whose_turn_records_have_no_ids_or_times_is_all_between(self):
        d = dec.decisions([row("onto.v1", [ans("topic")])], [turn(0), turn(1)], (3.0, 15.0))
        self.assertEqual(d["turns"]["attributed"], {"turn_id": 0, "time": 0, "between": 1})


class Rows(unittest.TestCase):
    ANSWERS = [
        {"question": "topic", "def": "topic", "about": None, "answer": {"type": "choice", "choice": "c2",
         "probabilities": [["c1", 0.1]], "confidence": 0.9}, "band": {"band": "act", "top": {"kind": "choice", "value": "c2"}, "value": 0.9}},
        {"question": "resource", "def": "resource", "about": None, "answer": {"type": "choice", "choice": "none",
         "probabilities": [], "confidence": 0.95}, "band": {"band": "act", "top": {"kind": "choice", "value": "none"}, "value": 0.95}},
        {"question": "personal", "def": "personal", "about": None, "answer": {"type": "noul", "noul": 0.97},
         "band": {"band": "act", "top": {"kind": "noul", "value": True}, "value": 0.97}},
        {"question": "elicits", "def": "elicits", "about": None, "answer": {"type": "noul", "noul": 0.1},
         "band": {"band": "confirm", "top": {"kind": "noul", "value": False}, "value": 0.1}},
    ]

    def test_a_ledger_row_keeps_each_answers_question_band_and_whether_it_chose(self):
        data = {"pack": "onto.v1", "outcome": {"outcome": "answered"}, "cost_micros": 420,
                "usage": {"input_tokens": 3100, "output_tokens": 41}, "answers": self.ANSWERS,
                "context": {"turn": "trn_1", "session": "ses_1", "purpose": "x"}, "state": {"text": "never kept"}}
        r = th.judgment_row({"position": 7, "at_unix_ms": 1_700_000_000_123, "data": data})
        self.assertEqual(r["answers"], [ans("topic"), ans("resource", chose=False), ans("personal"),
                                        ans("elicits", "confirm", False)])
        self.assertEqual((r["turn"], r["session"], r["at_ms"]), ("trn_1", "ses_1", 1_700_000_000_123))
        self.assertNotIn("state", r)
        self.assertNotIn("text", str(r))

    def test_a_row_without_them_is_the_old_row(self):
        r = th.judgment_row({"data": {"pack": "onto.v1", "outcome": {"outcome": "answered"}}})
        self.assertEqual(set(r), {"pack", "outcome", "cost_micros", "input_tokens", "output_tokens"})


class Report(unittest.TestCase):
    def test_the_section_words_per_turn_numbers_and_marks_an_old_run_not_recorded(self):
        new = ts.decisions([row("onto.v1", [ans("topic"), ans("resource"), ans("personal")], turn="trn_a")], [turn(0, "trn_a")], SONNET)
        old = ts.decisions([row("onto.v1")], 1, SONNET)
        text = rep.decisions_section([{"arm": "A1o", "decisions": new}, {"arm": "A1x", "decisions": old}, {"arm": "A1"}])
        self.assertIn("| A1o | 3.00 (0.00 / 2.00 / 1.00) |", text)  # routing / filing / privacy, per message
        self.assertIn("100.0%", text)  # the one turn had a decision
        self.assertIn("onto.v1/topic", text)
        self.assertIn("not recorded", text)  # the old run's per-question numbers
        self.assertIn("none: the arm asks Jev no decision", text)
        self.assertIn("modelled, not measured", text)


if __name__ == "__main__":
    unittest.main()
