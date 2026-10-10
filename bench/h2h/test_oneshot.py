"""Tests for the one-shot runs (theseus-7gir.13): the join by time, the rows, the A/B order, and a live run.

    python3 -m unittest discover -s bench/h2h -p 'test_*.py'
"""

from __future__ import annotations

import sys
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import oneshot  # noqa: E402
import standin  # noqa: E402

B = 1_000_000_000_000
MS = 1_000_000


class Join(unittest.TestCase):
    def setUp(self):
        self.log = standin.read_log(HERE / "fixtures" / "standin.jsonl")
        # A one-shot that lived from B+1.9 s to B+3.8 s: the task's turn and its side request, nothing earlier.
        self.rec = {"spawn_ns": B + 1900 * MS, "exit_ns": B + 3800 * MS, "first_byte_ns": B + 2100 * MS,
                    "marker_ns": B + 3660 * MS, "cpu_s": 0.25, "maxrss_kb": 40000, "wall_ms": 1900.0, "exit": 0}

    def test_requests_that_arrived_while_it_lived(self):
        self.assertEqual([e["seq"] for e in oneshot.join(self.log, self.rec)], [1, 2, 3, 4, 5])

    def test_its_rows(self):
        r = oneshot.rows(self.rec, self.log, "MAAAA01")
        self.assertAlmostEqual(r["to_request_ms"], 115.0)
        self.assertAlmostEqual(r["t3_ms"], 30.0)  # the last request's first byte (E+1630) to the marker (E+1660)
        self.assertEqual((r["requests"], r["side_requests"], r["first_request_bytes"]), (4, 1, 32000))
        self.assertAlmostEqual(r["cpu_ms"], 250.0)

    def test_no_request_is_not_measured(self):
        rec = {**self.rec, "spawn_ns": 0, "exit_ns": 1}
        self.assertIsNone(oneshot.rows(rec, self.log)["to_request_ms"])


class Order(unittest.TestCase):
    def test_abba_with_a_reset_before_each(self):
        seen = []
        out = oneshot.interleave(["a", "b"], 3, lambda arm, i: f"{arm}{i}", lambda arm, i: seen.append(arm))
        self.assertEqual(seen, ["a", "b", "b", "a", "a", "b"])
        self.assertEqual(out, {"a": ["a0", "a1", "a2"], "b": ["b0", "b1", "b2"]})


class Live(unittest.TestCase):
    def test_a_run_is_stamped_and_reaped(self):
        rec = oneshot.run(["sh", "-c", "printf hello; sleep 0.05; printf ' end-Q7'; exit 3"], marker="end-Q7")
        self.assertEqual(rec["exit"], 3)
        self.assertLessEqual(rec["spawn_ns"], rec["first_byte_ns"])
        self.assertLess(rec["first_byte_ns"], rec["marker_ns"])
        self.assertLessEqual(rec["marker_ns"], rec["exit_ns"])
        self.assertGreater(rec["maxrss_kb"], 0)
        self.assertEqual(rec["out_bytes"], len("hello end-Q7"))

    def test_a_run_past_its_timeout_is_killed(self):
        rec = oneshot.run(["sleep", "30"], timeout=0.2)
        self.assertTrue(rec["timed_out"])
        self.assertLess(rec["wall_ms"], 5000)


if __name__ == "__main__":
    unittest.main()
