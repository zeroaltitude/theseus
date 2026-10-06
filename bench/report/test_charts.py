"""Tests for the house charts and the statistics under every report (theseus-qla4).

    python3 -m unittest discover -s bench/report

The statistics against values worked by hand or from the standard tables; every chart form drawn in both modes and
parsed back: a valid SVG that says its title and question, paints its own surface, gives every mark its hover text,
wears text colours (never a series colour) on its words, and keeps one arm one colour.
"""

from __future__ import annotations

import importlib.util
import os
import sys
import time
import unittest
import xml.etree.ElementTree as ET
from pathlib import Path

HERE = Path(__file__).resolve().parent


def _load(name: str, file: str):
    if name in sys.modules:
        return sys.modules[name]
    spec = importlib.util.spec_from_file_location(name, HERE / file)
    mod = importlib.util.module_from_spec(spec)
    sys.modules[name] = mod
    spec.loader.exec_module(mod)
    return mod


charts = _load("bench_report_charts", "charts.py")
stats = _load("stats_shim", "stats.py")
SVG = "{http://www.w3.org/2000/svg}"
TIMES = [f"2026-10-0{2 + i // 24}T{i % 24:02d}:00:00-07:00" for i in range(48)]


def specs() -> list[dict]:
    """One figure of every form, the arms the first full run had."""
    arms = [("theseus", "A. Theseus"), ("theseus-batching", "B. + paragraph"), ("claude-code", "C. Claude Code")]
    return [
        {"name": "rates", "form": "intervals", "title": "Pass rate", "question": "Which arm passes most?",
         "x": {"label": "trials passed", "format": "pct", "min": 0, "max": 1}, "reference": [{"value": 0.5, "label": "half"}],
         "rows": [{"label": l, "arm": a, "value": v, "lo": v - 0.07, "hi": v + 0.06, "group": "by trial"}
                  for (a, l), v in zip(arms, (0.719, 0.736, 0.815))]},
        {"name": "ends", "form": "bars", "title": "Endings", "question": "How did trials end?",
         "categories": ["timeout", "refusal"], "value": {"label": "trials", "format": "int"},
         "series": [{"arm": a, "label": l, "values": [6, 0]} for a, l in arms]},
        {"name": "stack", "form": "bars", "stacked": True, "title": "Stacked", "question": "Of what?",
         "categories": ["A", "B"], "series": [{"arm": "theseus", "label": "x", "values": [3, 4]},
                                               {"arm": "claude-code", "label": "y", "values": [1, 0]}]},
        {"name": "cols", "form": "bars", "orientation": "v", "title": "Columns", "question": "How much?",
         "categories": ["one", "two"], "series": [{"arm": "theseus", "label": "z", "values": [1.5, 2]}],
         "value": {"format": "usd"}},
        {"name": "trend", "form": "lines", "title": "Cold start", "question": "Did it hold its budget?",
         "x": {"type": "time"}, "y": {"label": "ms", "format": "ms"},
         "series": [{"arm": "theseus", "label": "p95", "points": [[t, 20 + i % 7] for i, t in enumerate(TIMES)],
                     "style": "both"}],
         "limits": [{"y": 50, "label": "budget 50 ms"}], "events": [{"x": TIMES[30], "label": "a join"}],
         "marks": [{"x": TIMES[5], "y": 44, "status": "critical", "label": "miss"}]},
        {"name": "small", "form": "multiples", "title": "Phases", "question": "Each phase?", "x": {"type": "time"},
         "panels": [{"title": p, "y": {"format": "ms", "log": True},
                     "series": [{"arm": "theseus", "label": "p95", "points": [[t, 20 + i] for i, t in enumerate(TIMES)]}],
                     "limits": [{"y": 50, "label": "budget"}]} for p in ("cold", "swap", "kill")],
         "events": [{"x": TIMES[10], "label": "a join"}]},
        {"name": "front", "form": "scatter", "front": "min-x-max-y", "title": "Pareto", "question": "Most for least?",
         "x": {"format": "usd", "min": 0}, "y": {"format": "pct"},
         "points": [{"arm": a, "label": l, "x": x, "y": y, "xlo": x * 0.9, "xhi": x * 1.1}
                    for (a, l), x, y in zip(arms, (0.139, 0.136, 0.127), (0.719, 0.736, 0.815))]},
        {"name": "spread", "form": "strip", "title": "Dollars", "question": "How spread?",
         "x": {"format": "usd", "log": True},
         "groups": [{"arm": a, "label": l, "values": [0.01 * (1 + i % 9) for i in range(30)] + [None]} for a, l in arms]},
        {"name": "grid", "form": "matrix", "title": "Outcomes", "question": "Where do they differ?",
         "columns": [{"label": "A1", "arm": "theseus"}, {"label": "C1", "arm": "claude-code"}],
         "rows": [{"label": "fix-git", "cells": "PP", "note": "2/2"}, {"label": "a<b>&c", "cells": "E-"}]},
        {"name": "gain", "form": "dumbbell", "title": "Headroom", "question": "How much does memory add?",
         "arm": "theseus", "from_label": "no memory", "to_label": "oracle", "x": {"format": "pct", "min": 0, "max": 1},
         "rows": [{"label": "facts", "from": 0.1, "to": 1.0}]},
    ]


class Statistics(unittest.TestCase):
    def test_wilson_matches_the_standard_table(self):
        for (k, n), (lo, hi) in {(0, 10): (0.0, 0.2775), (10, 10): (0.7225, 1.0), (5, 10): (0.2366, 0.7634),
                                 (128, 178): (0.6490, 0.7800)}.items():
            p, a, b = stats.wilson(k, n)
            self.assertAlmostEqual(p, k / n)
            self.assertAlmostEqual(a, lo, places=4, msg=(k, n))
            self.assertAlmostEqual(b, hi, places=4, msg=(k, n))
        p, a, b = stats.wilson(0, 0)
        self.assertTrue(p != p and (a, b) == (0.0, 1.0), "no trials: nothing known")
        with self.assertRaises(ValueError):
            stats.wilson(3, 2)

    def test_mcnemar_exact_is_the_two_sided_sign_test(self):
        self.assertAlmostEqual(stats.mcnemar_exact(1, 9), 2 * 11 / 1024)
        self.assertAlmostEqual(stats.mcnemar_exact(9, 1), 2 * 11 / 1024)
        self.assertEqual(stats.mcnemar_exact(0, 0), 1.0)
        self.assertEqual(stats.mcnemar_exact(3, 3), 1.0)

    def test_quantiles_interpolate_and_the_bootstrap_repeats_itself(self):
        self.assertEqual(stats.quantile([1, 2, 3, 4], 0.5), 2.5)
        self.assertEqual(stats.quantile([4, 1, 3, 2], 0.25), 1.75)
        self.assertEqual(stats.median([5]), 5)
        xs = [0.03, 0.04, 0.05, 0.31, 0.02, 0.06, 1.44, 0.04, 0.05, 0.03, 0.09, 0.12]
        a, b = stats.bootstrap_mean(xs), stats.bootstrap_mean(xs)
        self.assertEqual(a, b, "seeded: a report regenerates the same interval")
        self.assertLess(a[1], a[0])
        self.assertLess(a[0], a[2])
        self.assertEqual(stats.bootstrap_mean([2.0]), (2.0, 2.0, 2.0))
        d = stats.bootstrap_paired_diff([1, 2, 3], [2, 3, 4], iters=200)
        self.assertEqual(d, (1.0, 1.0, 1.0))


class Charts(unittest.TestCase):
    def test_every_form_draws_both_modes_as_valid_svg(self):
        text_colours = {m: {charts.CHROME[m][k] for k in ("ink", "ink2")} for m in charts.MODES}
        for spec in specs():
            for mode in charts.MODES:
                with self.subTest(form=spec["form"], mode=mode):
                    root = ET.fromstring(charts.render(spec, mode))
                    self.assertEqual(root.tag, SVG + "svg")
                    self.assertEqual(root.get("role"), "img")
                    self.assertEqual(root.find(SVG + "title").text, spec["title"])
                    self.assertEqual(root.find(SVG + "desc").text, spec["question"])
                    self.assertEqual(root.find(SVG + "rect").get("fill"), charts.CHROME[mode]["surface"],
                                     "each file paints its own surface, so it reads on either page")
                    for t in root.iter(SVG + "text"):
                        self.assertIn(t.get("fill"), text_colours[mode], "words wear text colours")
                    hovers = [g for g in root.iter(SVG + "g") if g.find(SVG + "title") is not None]
                    self.assertTrue(hovers, "marks carry their hover text")

    def test_every_mark_says_its_value_on_hover(self):
        by = {s["name"]: s for s in specs()}
        root = ET.fromstring(charts.render(by["rates"]))
        hovers = [g.find(SVG + "title").text for g in root.iter(SVG + "g") if g.find(SVG + "title") is not None]
        self.assertIn("A. Theseus: 71.9%  [64.9%, 77.9%]", hovers)
        root = ET.fromstring(charts.render(by["grid"]))
        hovers = [g.find(SVG + "title").text for g in root.iter(SVG + "g") if g.find(SVG + "title") is not None]
        self.assertEqual(len(hovers), 4, "one per cell")
        self.assertIn("a<b>&c · A1: ended in an error class", hovers, "names are escaped, and parse back")

    def test_one_arm_is_one_colour_and_the_order_is_fixed(self):
        self.assertEqual([charts.arm_color(a, "light") for a in ("theseus", "claude-code", "theseus-batching")],
                         ["#2a78d6", "#eb6834", "#1baf7a"])
        self.assertEqual(charts.arm_color("theseus", "dark"), "#3987e5")
        slots = sorted(s for s, _ in charts.ARMS.values())
        self.assertEqual(slots, list(range(1, len(charts.ARMS) + 1)), "each arm its own slot, none skipped")
        fills = {c.get("fill") for c in ET.fromstring(charts.render(specs()[0])).iter(SVG + "circle")}
        self.assertTrue({"#2a78d6", "#1baf7a", "#eb6834"} <= fills)

    def test_a_spec_the_renderer_cannot_draw_honestly_is_refused(self):
        good = specs()[0]
        for bad, why in (({**good, "rows": [{**good["rows"][0], "arm": "gpt"}]}, "unknown arm"),
                         ({**good, "name": "Rates"}, "upper case"), ({**good, "name": "rates-dark"}, "a -dark name"),
                         ({**good, "question": ""}, "no question"), ({**good, "form": "pie"}, "no pies")):
            with self.subTest(why), self.assertRaises(ValueError):
                charts.render(bad)

    def test_time_ticks_are_the_datas_own_zone_on_any_machine(self):
        spec = specs()[4]
        old = os.environ.get("TZ")
        out = []
        try:
            for tz in ("UTC", "Asia/Tokyo"):
                os.environ["TZ"] = tz
                time.tzset()
                out.append(charts.render(spec))
        finally:
            if old is None:
                os.environ.pop("TZ", None)
            else:
                os.environ["TZ"] = old
            time.tzset()
        self.assertEqual(out[0], out[1])

    def test_the_picture_shows_the_dark_file_to_a_dark_reader(self):
        md = charts.picture("img/2026-10-04-x", specs()[0])
        self.assertIn('<source media="(prefers-color-scheme: dark)" srcset="img/2026-10-04-x/rates-dark.svg">', md)
        self.assertIn('src="img/2026-10-04-x/rates.svg"', md)
        self.assertIn('alt="Pass rate. Which arm passes most?"', md)

    def test_write_gives_a_light_and_a_dark_file(self):
        import tempfile
        with tempfile.TemporaryDirectory() as d:
            paths = charts.write(specs()[1], Path(d))
            self.assertEqual([p.name for p in paths], ["ends.svg", "ends-dark.svg"])
            self.assertNotEqual(paths[0].read_text(), paths[1].read_text())

    def test_a_value_past_the_axis_is_an_arrowhead_that_says_its_value(self):
        spec = {**specs()[5], "panels": [{"title": "swap", "y": {"format": "ms", "min": 0, "max": 300},
                                          "series": [{"arm": "theseus", "label": "p95",
                                                      "points": [[TIMES[0], 100], [TIMES[1], 900], [TIMES[2], 110]]},
                                                     {"arm": "context", "label": "p50",
                                                      "points": [[TIMES[0], 60], [TIMES[2], 70]]}],
                                          "events": [{"x": TIMES[2], "label": "its own"}]}]}
        root = ET.fromstring(charts.render(spec))
        hovers = [g.find(SVG + "title").text for g in root.iter(SVG + "g") if g.find(SVG + "title") is not None]
        self.assertIn("p95: 900 ms (off the scale, up)", hovers)
        self.assertTrue(root.findall(f".//{SVG}clipPath"), "the series are clipped to the plot")
        self.assertIn("2: its own", hovers, "a panel's own event, numbered after the shared one")
        self.assertIn("p50", {t.text for t in root.iter(SVG + "text")}, "the legend names the context series")

    def test_a_spec_may_name_its_arms_in_the_legend(self):
        spec = {**specs()[0], "legend_labels": {"theseus": "the plain build"}}
        texts = {t.text for t in ET.fromstring(charts.render(spec)).iter(SVG + "text")}
        self.assertIn("the plain build", texts)
        self.assertIn("Claude Code", texts)

    def test_a_log_axis_places_its_ticks_by_their_logarithm(self):
        spec = {**specs()[0], "x": {"label": "ms", "format": "ms", "log": True, "min": 50, "max": 2000},
                "rows": [{"label": "a", "arm": "theseus", "value": 100, "lo": 60, "hi": 900}]}
        root = ET.fromstring(charts.render(spec))
        at = {t.text: float(t.get("x")) for t in root.iter(SVG + "text") if t.text in ("50", "100", "200", "500")}
        self.assertAlmostEqual(at["100"] - at["50"], at["200"] - at["100"], delta=0.2, msg="a doubling, one width")
        self.assertGreater(at["500"] - at["200"], at["200"] - at["100"])

    def test_a_matrix_slants_column_labels_wider_than_a_column(self):
        cols = [{"label": l, "arm": "bm25" if i < 2 else "fused"} for i, l in enumerate(("BM25 + entities", "vectors",
                                                                                         "fused, w = 1"))]
        spec = {**specs()[8], "columns": cols, "rows": [{"label": "item", "cells": "PFE"}]}
        root = ET.fromstring(charts.render(spec))
        slanted = [t for t in root.iter(SVG + "text") if t.text in {c["label"] for c in cols}]
        self.assertEqual(len(slanted), 3)
        self.assertTrue(all("rotate(-45" in (t.get("transform") or "") for t in slanted))
        upright = [t for t in ET.fromstring(charts.render(specs()[8])).iter(SVG + "text") if t.text in ("A1", "C1")]
        self.assertTrue(upright and not any(t.get("transform") for t in upright), "short labels stay upright")

    def test_the_palette_figure_lists_every_arm_in_both_modes(self):
        for mode in charts.MODES:
            root = ET.fromstring(charts.palette_svg(mode))
            fills = {r.get("fill") for r in root.iter(SVG + "rect")}
            self.assertTrue(set(charts.SLOTS[mode]) <= fills, mode)
            texts = " ".join(t.text or "" for t in root.iter(SVG + "text"))
            for key in list(charts.ARMS) + list(charts.NEUTRALS):
                self.assertIn(key, texts)

    def test_numbers_read_as_people_write_them(self):
        f = charts.fmt
        self.assertEqual((f(0.719, "pct"), f(24.72, "usd"), f(0.0123, "usd"), f(0.004, "usd"), f(1615.4, "ms"),
                          f(2.0, "x"), f(None)), ("71.9%", "$24.72", "$0.012", "$0.0040", "1,615 ms", "2.00×", "–"))
        self.assertEqual([f(v, "ms", None, tick=True) for v in (0.1, 1, 1000)], ["0.1", "1", "1,000"])
        self.assertEqual(charts.nice_ticks(0, 57)[0], [0, 20, 40, 60], "about five steps of 1, 2 or 5 times ten")
        self.assertEqual([f(v, "x", 0.25, tick=True) for v in (0.75, 1.25)] + [f(0.025, "pct", 0.025, tick=True),
                                                                              f(0.075, "usd", 0.025, tick=True)],
                         ["0.75×", "1.25×", "2.5%", "$0.075"], "a tick prints the places its step needs")
        self.assertEqual([f(-0.25, "pct", 0.25, tick=True), f(-0.5, "usd"), f(-19.4, "ms"), f(-0.0001, "num", 1, tick=True)],
                         ["−25%", "−$0.50", "−19.4 ms", "0"], "a true minus, ahead of the unit; no minus on a zero")

    def test_a_limits_label_sits_over_the_series_on_a_halo(self):
        root = ET.fromstring(charts.render(specs()[4]))
        order = [e.tag.replace(SVG, "") + ":" + (e.text or "") for e in root.iter() if e.tag in (SVG + "polyline", SVG + "text")]
        label = order.index("text:budget 50 ms")
        self.assertGreater(label, max(i for i, t in enumerate(order) if t.startswith("polyline")), "drawn after the series")
        halo = [t for t in root.iter(SVG + "text") if t.text == "budget 50 ms"][0]
        self.assertEqual(halo.get("paint-order"), "stroke")


if __name__ == "__main__":
    unittest.main()
