"""Tests for the terminal driver (theseus-7gir.13): the ANSI stripper, marker stamps on a recorded transcript, a
live pseudo-terminal, and the /proc sampler.

    python3 -m unittest discover -s bench/h2h -p 'test_*.py'
"""

from __future__ import annotations

import importlib.util
import os
import sys
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent


def _load(name: str, path: Path):
    if name in sys.modules:
        return sys.modules[name]
    spec = importlib.util.spec_from_file_location(name, path)
    mod = importlib.util.module_from_spec(spec)
    sys.modules[name] = mod
    spec.loader.exec_module(mod)
    return mod


ptyd = _load("h2h_pty", HERE / "pty.py")
B = 1_000_000_000_000
MS = 1_000_000


class Strip(unittest.TestCase):
    def test_cursor_forward_is_spaces(self):
        self.assertEqual(ptyd.strip(b"ok\x1b[3Cthere\x1b[Cnow"), "ok   there now")

    def test_moves_to_another_row_break_the_line_and_colour_is_dropped(self):
        self.assertEqual(ptyd.strip(b"\x1b[1mtop\x1b[0m\x1b[5;10Hnext\x1b[2Bdown"), "top\nnext\ndown")

    def test_osc_and_private_modes_are_dropped(self):
        self.assertEqual(ptyd.strip(b"\x1b]0;a title\x07\x1b[?1049h\x1b[?25lA\x1b[38;5;12mB\x1b(B"), "AB")

    def test_a_column_move_is_a_gap(self):
        self.assertEqual(ptyd.strip(b"Auto\x1b[9Gmode"), "Auto mode")


class Recorded(unittest.TestCase):
    def setUp(self):
        self.t = ptyd.replay(HERE / "fixtures" / "tui.rec")

    def test_the_board_and_its_link(self):
        self.assertEqual(self.t.stamp("socket ok"), B - 400 * MS)
        self.assertEqual(self.t.stamp("h2hab12cd"), B - 380 * MS)

    def test_a_marker_cut_across_reads_is_stamped_by_the_read_that_completes_it(self):
        self.assertEqual(self.t.stamp("ok-MBBBB02"), B + 331 * MS)

    def test_an_escape_cut_across_reads_leaves_no_trace(self):
        self.assertIn("ok-MBBBB02 the   stand-in answers end-MBBBB02", self.t.text)
        self.assertNotIn("8;5;12m", self.t.text)
        self.assertEqual(self.t.stamp("end-MBBBB02"), B + 480 * MS)

    def test_after_an_offset_only_later_text_counts(self):
        at = self.t.text.index("end-")
        self.assertIsNone(self.t.stamp("socket ok", after=at))
        self.assertIsNone(self.t.stamp("never-shown"))


class Live(unittest.TestCase):
    def test_typed_text_and_enter_reach_the_child_and_its_answer_is_stamped(self):
        p = ptyd.Pty(["sh", "-c", "stty -echo; printf 'ready> '; read x; printf 'got-%s\\n' \"$x\"; sleep 5"])
        try:
            ready = p.wait_for("ready>", 10)
            self.assertIsNotNone(ready)
            p.type("abc")
            enter = p.key("enter")
            got = p.wait_for("got-abc", 10)
            self.assertIsNotNone(got)
            self.assertGreaterEqual(got, enter)
            self.assertGreaterEqual(ready, p.spawn_ns)
        finally:
            p.close()

    def test_the_terminal_is_120_by_40(self):
        p = ptyd.Pty(["sh", "-c", "stty size; sleep 5"])
        try:
            self.assertIsNotNone(p.wait_for("40 120", 10))
        finally:
            p.close()

    def test_the_sampler_reads_this_process(self):
        s = ptyd.sample_tree([os.getpid()])
        self.assertGreater(s["rss_kb"], 0)
        self.assertGreaterEqual(s["procs"], 1)
        self.assertEqual(s["clk_tck"], os.sysconf("SC_CLK_TCK"))


if __name__ == "__main__":
    unittest.main()


class Grid(unittest.TestCase):
    """The screen model: a word a TUI draws in two moves stands whole on the grid, stamped by the read that
    completed it; erases and the alternate screen clear what they clear."""

    def test_a_word_drawn_in_two_moves_is_seen_whole(self) -> None:
        g = ptyd.Screen(cols=40, rows=5)
        g.watch("end-M1")
        g.feed(b"\x1b[?1049h\x1b[3;5Hend-", 100)
        self.assertIsNone(g.stamp("end-M1"))
        g.feed(b"\x1b[1;1Hstatus line\x1b[3;9HM1", 200)
        self.assertEqual(g.stamp("end-M1"), 200)
        self.assertIn("    end-M1", g.text().splitlines()[2])
        self.assertEqual(g.text().splitlines()[0], "status line")

    def test_erases_and_moves(self) -> None:
        g = ptyd.Screen(cols=10, rows=3)
        g.feed(b"abcdefghij", 1)
        g.feed(b"\x1b[1;4H\x1b[K", 2)
        self.assertEqual(g.text().splitlines()[0], "abc")
        g.feed(b"\r\n\x1b[2Cxy\x1b[Dz", 3)
        self.assertEqual(g.text().splitlines()[1], "  xz")
        g.feed(b"\x1b[2J", 4)
        self.assertEqual(g.text().strip(), "")
        g.feed(b"1\r\n2\r\n3\r\n4", 5)  # past the last row: the grid scrolls
        self.assertEqual(g.text().splitlines(), ["2", "3", "4"])
