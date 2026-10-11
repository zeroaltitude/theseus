"""Tests for the report generator (theseus-7gir.13) on a recorded run: the answer first, every row, "not measured"
where an arm has no value, and every figure in a light and a dark file.

    python3 -m unittest discover -s bench/h2h -p 'test_*.py'
"""

from __future__ import annotations

import json
import sys
import tempfile
import unittest
import xml.etree.ElementTree as ET
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import report  # noqa: E402

RUN = HERE / "fixtures" / "run.json"


class Report(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tmp = tempfile.TemporaryDirectory()
        cls.out = Path(cls.tmp.name)
        cls.data = json.loads(RUN.read_text())
        cls.written = report.build(cls.data, cls.out)
        cls.name = "2026-10-09-head-to-head-speed"  # the run's date, not today's
        cls.md = (cls.out / f"{cls.name}.md").read_text()

    @classmethod
    def tearDownClass(cls):
        cls.tmp.cleanup()

    def test_the_answer_comes_first(self):
        body = [ln for ln in self.md.splitlines() if ln.strip()]
        self.assertTrue(body[0].startswith("# "))
        self.assertTrue(body[1].startswith("> **The answer.**"))
        self.assertIn("Theseus is faster by", body[1])
        self.assertIn("95% CI", self.md)
        self.assertIn("T6 (CPU per turn, interactive): Theseus is lighter by", body[1])

    def test_every_row_is_in_the_table(self):
        table = self.md.split("## Results", 1)[1].split("## Figures", 1)[0]
        for row in report.ROWS:
            self.assertIn(f"| {row} |", table, row)

    def test_an_arm_without_values_is_not_measured(self):
        t5 = next(ln for ln in self.md.splitlines() if ln.startswith("| T5 |"))
        self.assertIn("not measured", t5)
        self.assertIn("T5 (resume of a long session, interactive): Claude Code not measured.", self.md)

    def test_the_sections_in_order(self):
        heads = [ln for ln in self.md.splitlines() if ln.startswith("## ")]
        self.assertEqual(heads, ["## Results", "## Figures", "## Method", "## Threats to validity", "## Reproduction",
                                 "## Data"])
        self.assertIn("python3 bench/h2h/run.py", self.md)
        self.assertIn("load averages 0.50 / 0.60 / 0.70 before", self.md)
        self.assertIn("the scratch daemon's own start to its first health answer, not counted in T1", self.md)

    def test_every_figure_in_light_and_dark(self):
        data = json.loads((self.out / f"{self.name}.json").read_text())
        names = [f["name"] for f in data["figures"]]
        self.assertEqual(names, ["latency", "cpu-per-turn", "first-request-bytes", "idle-cpu", "idle-memory"])
        for n in names:
            for stem in (n, f"{n}-dark"):
                p = self.out / "img" / self.name / f"{stem}.svg"
                self.assertTrue(p.exists(), p)
                ET.parse(p)  # a valid SVG
            self.assertIn(f"img/{self.name}/{n}-dark.svg", self.md)

    def test_medians_and_intervals(self):
        s = report.median_interval([5, 1, 3])
        self.assertEqual((s["median"], s["lo"], s["hi"], s["method"]), (3, 1, 5, "range"))
        s = report.median_interval([10, 11, 12, 13, 14, 15])
        self.assertEqual(s["method"], "bootstrap")
        self.assertLessEqual(s["lo"], s["median"])
        self.assertLessEqual(s["median"], s["hi"])
        self.assertEqual(report.median_interval([])["median"], None)

    def test_an_existing_report_is_kept_and_the_date_can_be_given(self):
        md = self.out / f"{self.name}.md"
        md.write_text("hand-written")
        report.build(self.data, self.out)
        self.assertEqual(md.read_text(), "hand-written")
        report.build(self.data, self.out, date="2026-10-01")
        self.assertTrue((self.out / "2026-10-01-head-to-head-speed.md").exists())

    def test_a_theseus_only_run_says_so(self):
        run = {**self.data, "theseus_only": True,
               "arms": {"claude-code": {"available": False, "why": "claude was not found"}},
               "samples": [s for s in self.data["samples"] if s["arm"] == "theseus"]}
        with tempfile.TemporaryDirectory() as d:
            report.build(run, Path(d))
            md = (Path(d) / f"{self.name}.md").read_text()
        self.assertIn("Claude Code was not run (claude was not found)", md)
        self.assertNotIn("is faster by", md)


if __name__ == "__main__":
    unittest.main()
