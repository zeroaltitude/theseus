"""Tests for the joins (theseus-7gir.13): screen stamps against the stand-in's recorded log.

    python3 -m unittest discover -s bench/h2h -p 'test_*.py'
"""

from __future__ import annotations

import sys
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import joins  # noqa: E402
import standin  # noqa: E402

B = 1_000_000_000_000
MS = 1_000_000
E = B + 2000 * MS  # the task's Enter


class Joins(unittest.TestCase):
    def setUp(self):
        self.log = standin.read_log(HERE / "fixtures" / "standin.jsonl")

    def test_t2_is_enter_to_the_first_request_of_the_turn(self):
        self.assertAlmostEqual(joins.t2(B, self.log, "MBBBB02"), 12.0)
        # The side request at E+5 ms is not the turn's: its first is at E+15.
        self.assertAlmostEqual(joins.t2(E, self.log, "MAAAA01"), 15.0)
        self.assertAlmostEqual(joins.t2(E, self.log), 15.0)  # without a marker, side requests still don't count

    def test_t3_is_the_first_byte_to_the_first_marker_on_screen(self):
        req = joins.reply_request(self.log, "MBBBB02")
        self.assertAlmostEqual(joins.t3(req, B + 330 * MS), 18.0)
        self.assertIsNone(joins.t3(req, None))

    def test_t4_is_each_responses_last_byte_to_the_next_request_labelled_by_its_tool(self):
        calls = joins.t4(self.log, ["read", "edit", "shell"], "MAAAA01")
        self.assertEqual([(c["kind"], round(c["ms"], 3)) for c in calls], [("read", 30.0), ("edit", 20.0),
                                                                          ("shell", 80.0)])
        self.assertEqual(joins.t4_by_kind(calls)["shell"], [80.0])

    def test_the_task_turn_whole(self):
        stamps = {"task-MAAAA01": E + 1650 * MS, "end-MAAAA01": E + 1720 * MS}
        t = joins.turn(E, self.log, "MAAAA01", "task", stamps, ["read", "edit", "shell"])
        self.assertEqual(t["requests"], 4)
        self.assertAlmostEqual(t["t3_ms"], 20.0)
        self.assertAlmostEqual(t["turn_ms"], 1720.0)
        self.assertEqual(t["first_request_bytes"], 32000)
        self.assertEqual(t["keepalive"], [True, True, True, True])

    def test_a_log_that_never_names_the_marker_joins_by_the_window(self):
        # Claude Code's opening can start with a system reminder, pushing the prompt past the logged opening.
        log = [{**e, "opening": "<system-reminder> ..."} for e in self.log]
        self.assertAlmostEqual(joins.t2(E, log, "MAAAA01", E + 1720 * MS), 15.0)
        calls = joins.t4(log, ["read", "edit", "shell"], "MAAAA01", E, E + 1720 * MS)
        self.assertEqual([c["kind"] for c in calls], ["read", "edit", "shell"])
        # The window ends at the reply's end on screen: nothing later joins the turn.
        self.assertEqual(len(joins.turn_requests(log, "MAAAA01", B, B + 600 * MS)), 1)

    def test_a_logged_marker_field_names_the_turn(self):
        log = [{**e, "opening": "", "marker": "MAAAA01" if e.get("rule") == "h2h-task3" else None}
               for e in self.log]
        self.assertEqual(len(joins.turn_requests(log, "MAAAA01")), 4)

    def test_cpu_per_turn_and_idle(self):
        a = {"ns": 0, "cpu_ticks": 100, "rss_kb": 1000, "clk_tck": 100}
        b = {"ns": 2_000_000_000, "cpu_ticks": 103, "rss_kb": 3000, "clk_tck": 100}
        self.assertAlmostEqual(joins.t6([(a, b)])[0], 30.0)
        idle = joins.idle([a, b])
        self.assertAlmostEqual(idle["cpu_pct"], 1.5)
        self.assertEqual((idle["rss_kb_mean"], idle["rss_kb_max"]), (2000, 3000))
        self.assertIsNone(joins.idle([a])["cpu_pct"])


if __name__ == "__main__":
    unittest.main()
