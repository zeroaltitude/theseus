"""Tests for the quiet-host guard (theseus-w052 3): a fake /proc/pressure file
and a held lock. Standard library only."""

from __future__ import annotations

import fcntl
import os
import tempfile
import unittest
from pathlib import Path

import quiet

PRESSURE = ("some avg10={a} avg60=0.50 avg300=0.40 total=123456\n"
            "full avg10=0.00 avg60=0.00 avg300=0.00 total=0\n")


class Quiet(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = Path(self.tmp.name)

    def tearDown(self):
        self.tmp.cleanup()

    def pressure(self, avg10: str) -> str:
        p = self.dir / "cpu"
        p.write_text(PRESSURE.format(a=avg10))
        return str(p)

    def test_the_some_line_avg10_is_the_reading(self):
        self.assertEqual(quiet.cpu_pressure(self.pressure("12.34")), 12.34)
        self.assertIsNone(quiet.cpu_pressure(self.dir / "absent"))
        (self.dir / "odd").write_text("nothing here\n")
        self.assertIsNone(quiet.cpu_pressure(self.dir / "odd"))

    def test_pressure_above_the_line_refuses_with_its_reason_and_below_it_starts(self):
        lock = self.dir / "lock"
        busy = quiet.refusals(["-n", "4"], lock=lock, pressure_file=self.pressure("5.01"))
        self.assertEqual(len(busy), 1)
        self.assertIn("CPU pressure is 5.01", busy[0])
        self.assertEqual(quiet.refusals(["-n", "4"], lock=lock, pressure_file=self.pressure("0.40")), [])
        self.assertEqual(quiet.refusals([], lock=lock, pressure_file=self.pressure("5.0")), [], "at the line is not above")

    def test_a_host_without_pressure_files_is_not_refused_and_the_record_says_so(self):
        self.assertEqual(quiet.refusals([], lock=self.dir / "lock", pressure_file=str(self.dir / "none")), [])
        self.assertFalse(quiet.reading(str(self.dir / "none"))["pressure_available"])

    def test_a_held_gate_lock_refuses_and_a_free_one_does_not(self):
        lock = self.dir / "gate.lock"
        self.assertFalse(quiet.lock_held(lock), "no file: not held")
        lock.write_text("")
        self.assertFalse(quiet.lock_held(lock))
        fd = os.open(lock, os.O_RDONLY)
        try:
            fcntl.flock(fd, fcntl.LOCK_EX)
            self.assertTrue(quiet.lock_held(lock))
            why = quiet.refusals([], lock=lock, pressure_file=self.pressure("0.0"))
            self.assertEqual(len(why), 1)
            self.assertIn("gate's lock is held", why[0])
        finally:
            fcntl.flock(fd, fcntl.LOCK_UN)
            os.close(fd)
        self.assertFalse(quiet.lock_held(lock), "released: free again")

    def test_the_lock_is_where_the_gate_keeps_it(self):
        old = os.environ.get("THESEUS_GATE_LOCK_FILE")
        os.environ["THESEUS_GATE_LOCK_FILE"] = "/x/y.lock"
        try:
            self.assertEqual(quiet.lock_path(), Path("/x/y.lock"))
        finally:
            os.environ.pop("THESEUS_GATE_LOCK_FILE")
            if old is not None:
                os.environ["THESEUS_GATE_LOCK_FILE"] = old
        self.assertTrue(str(quiet.lock_path()).endswith("theseus-gate.lock"))
        self.assertIn("theseus-gate.lock", (Path(__file__).resolve().parents[2] / "scripts" / "gate.sh").read_text())

    def test_at_most_four_trials_at_once(self):
        self.assertEqual(quiet.MAX_PARALLEL, 4)
        for forms in (["-n", "5"], ["-n5"], ["--n-concurrent", "8"], ["--n-concurrent=6"]):
            why = quiet.refusals(forms, lock=self.dir / "l", pressure_file=self.pressure("0"))
            self.assertEqual(len(why), 1, forms)
            self.assertIn("more than 4 trials", why[0])
        self.assertEqual(quiet.refusals(["-n", "4"], lock=self.dir / "l", pressure_file=self.pressure("0")), [])
        self.assertEqual(quiet.with_parallel(["run", "-d", "x"]), ["run", "-d", "x", "-n", "4"])
        self.assertEqual(quiet.with_parallel(["run", "-n", "2"]), ["run", "-n", "2"])

    def test_every_reason_is_given_at_once(self):
        lock = self.dir / "gate.lock"
        lock.write_text("")
        fd = os.open(lock, os.O_RDONLY)
        try:
            fcntl.flock(fd, fcntl.LOCK_EX)
            why = quiet.refusals(["-n", "9"], lock=lock, pressure_file=self.pressure("30"))
        finally:
            os.close(fd)
        self.assertEqual(len(why), 3)


if __name__ == "__main__":
    unittest.main()
