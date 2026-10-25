"""Tests for the benchmark manifest (theseus-jda0z).

    python3 -m unittest discover -s bench/report

Fixtures are invented datasets in a temporary directory: two shapes of pass rate, a gate-bench-like file with none,
an omnibus summary, and a file that is not JSON. The manifest must list each run's arms sorted the same way every
time, replace a report's rows when it is appended again, skip what it cannot read with a named reason, and be
written by the drafting tool beside every run's data file.
"""

from __future__ import annotations

import importlib.util
import json
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent


def load(name: str, file: str):
    spec = importlib.util.spec_from_file_location(name, HERE / file)
    mod = importlib.util.module_from_spec(spec)
    sys.modules[name] = mod
    spec.loader.exec_module(mod)
    return mod


manifest = load("bench_report_manifest_t", "manifest.py")


def flat(report: str, date: str, tier: str, arms: dict[str, tuple[int, int]], commit: str = "abc12345", **extra):
    summary = {}
    for key, (k, n) in arms.items():
        summary[key] = {"label": key.title(), "trials": n, "solved": k, "rate": k / n, "wilson": [k / n - 0.05, k / n + 0.05],
                        "tasks": n // 3, "usd_total": 1.5, "usd_trial": 0.05, "usd_trial_ci": [0.04, 0.06]}
    return {"report": report, "suite": "fixturebench@1", "date": date, "tier": tier, "commit": commit, "model": "model-x",
            "arms": list(arms), "summary": summary, **extra}


def nested(report: str, date: str, arms: dict[str, tuple[int, int]]):
    summary = {key: {"label": key, "trials": n, "tasks": 2, "pass": {"k": k, "n": n, "rate": k / n, "lo": 0.1, "hi": 0.9}}
               for key, (k, n) in arms.items()}
    return {"report": report, "suite": "fixturebench@1", "date": date, "commit": "def67890", "arms": list(arms), "summary": summary}


class Fixture(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = Path(self.tmp.name)

    def tearDown(self):
        self.tmp.cleanup()

    def put(self, name: str, data) -> Path:
        p = self.dir / f"{name}.json"
        p.write_text(data if isinstance(data, str) else json.dumps(data))
        return p

    def build(self) -> tuple[dict, list[str]]:
        log: list[str] = []
        return json.loads(manifest.build(self.dir, log=log.append)), log


class Build(Fixture):
    def setUp(self):
        super().setUp()
        # Written out of date order on purpose: the manifest sorts them.
        self.put("2026-03-05-fixture-b", flat("2026-03-05-fixture-b", "2026-03-05", "high", {"alpha": (6, 9), "beta": (3, 9)}))
        self.put("2026-03-01-fixture-a", nested("2026-03-01-fixture-a", "2026-03-01", {"alpha": (4, 8)}))
        self.put("2026-03-03-fixture-c", flat("2026-03-03-fixture-c", "2026-03-03", "high", {"alpha": (5, 9)}))

    def test_rows_are_sorted_and_carry_what_the_dataset_states(self):
        doc, _ = self.build()
        self.assertEqual(doc["version"], 1)
        self.assertTrue(doc["about"])
        order = [(r["date"], r["report"], r["variant"]) for r in doc["runs"]]
        self.assertEqual(order, [("2026-03-01", "2026-03-01-fixture-a", "alpha"),
                                 ("2026-03-03", "2026-03-03-fixture-c", "alpha"),
                                 ("2026-03-05", "2026-03-05-fixture-b", "alpha"),
                                 ("2026-03-05", "2026-03-05-fixture-b", "beta")])
        row = doc["runs"][2]
        self.assertEqual((row["benchmark"], row["tier"], row["arm"], row["harness"], row["model"], row["commit"]),
                         ("fixturebench@1", "high", "alpha", "Alpha", "model-x", "abc12345"))
        self.assertEqual(row["dataset"], "docs/benchmarks/2026-03-05-fixture-b.json")
        self.assertEqual(row["metrics"]["pass_rate"]["k"], 6)
        self.assertEqual(row["metrics"]["usd_per_trial"], {"value": 0.05, "lo": 0.04, "hi": 0.06})
        nest = doc["runs"][0]["metrics"]["pass_rate"]
        self.assertEqual((nest["k"], nest["n"], nest["lo"], nest["hi"]), (4, 8, 0.1, 0.9))
        self.assertIsNone(doc["runs"][0]["tier"], "an untiered dataset says so")

    def test_a_second_build_is_byte_identical(self):
        first = manifest.build(self.dir, log=lambda m: None)
        (self.dir / manifest.INDEX).write_text(first)
        self.assertEqual(manifest.build(self.dir, log=lambda m: None), first, "the manifest itself is not read back as a dataset")
        # Another input order, the same bytes.
        rows = json.loads(first)["runs"]
        self.assertEqual(manifest.render(list(reversed(rows))), first)

    def test_a_row_repeating_an_earlier_rows_figures_is_marked_carried(self):
        self.put("2026-03-07-fixture-d", flat("2026-03-07-fixture-d", "2026-03-07", "high", {"alpha": (6, 9), "gamma": (1, 3)}))
        doc, _ = self.build()
        d = {(r["report"], r["variant"]): r for r in doc["runs"]}
        self.assertEqual(d[("2026-03-07-fixture-d", "alpha")]["carried_from"], "2026-03-05-fixture-b")
        self.assertNotIn("carried_from", d[("2026-03-05-fixture-b", "alpha")])
        self.assertNotIn("carried_from", d[("2026-03-07-fixture-d", "gamma")])

    def test_arms_a_report_compared_against_are_not_its_measurements(self):
        data = flat("2026-03-09-fixture-e", "2026-03-09", "high", {"alpha": (7, 9), "beta": (2, 3)})
        data["summary"]["alpha"]["label"] = "Alpha (this run)"
        self.put("2026-03-09-fixture-e", data)
        doc, _ = self.build()
        d = {r["variant"]: r for r in doc["runs"] if r["report"] == "2026-03-09-fixture-e"}
        self.assertNotIn("comparison", d["alpha"])
        self.assertTrue(d["beta"]["comparison"])


class Skips(Fixture):
    def test_what_cannot_be_read_is_skipped_with_a_reason_and_never_a_row(self):
        self.put("2026-03-01-fixture-a", flat("2026-03-01-fixture-a", "2026-03-01", "high", {"alpha": (6, 9)}))
        self.put("omnibus", {"report": "omnibus", "kind": "omnibus", "about": "x",
                             "summary": {"alpha": flat("x", "2026-03-02", "high", {"alpha": (1, 3)})["summary"]["alpha"]}})
        self.put("2026-03-02-gate-thing", {"report": "2026-03-02-gate-thing", "suite": "gate-bench", "date": "2026-03-02",
                                           "summary": {"cold": {"p50": 20.1, "p95": 31.0}}})
        self.put("2026-03-03-broken", "{not json")
        self.put("2026-03-04-no-date", {"suite": "fixturebench@1", "summary": {"alpha": {"trials": 3, "solved": 2, "rate": 0.67,
                                                                                     "wilson": [0.2, 0.9]}}})
        doc, log = self.build()
        self.assertEqual({r["report"] for r in doc["runs"]}, {"2026-03-01-fixture-a"})
        said = "\n".join(log)
        for name, why in (("omnibus.json", "omnibus"), ("2026-03-02-gate-thing.json", "pass rate"),
                          ("2026-03-03-broken.json", "unreadable JSON"), ("2026-03-04-no-date.json", "no `date`")):
            self.assertIn(f"skipped {name}: ", said)
            self.assertIn(why, said)
        self.assertEqual(len(log), 4, "one line per skipped file")


class Append(Fixture):
    def test_appending_a_report_twice_leaves_one_set_of_rows_with_the_second_values(self):
        index = self.dir / manifest.INDEX
        a = self.put("2026-03-01-fixture-a", flat("2026-03-01-fixture-a", "2026-03-01", "high", {"alpha": (6, 9)}))
        b = self.put("2026-03-02-fixture-b", flat("2026-03-02-fixture-b", "2026-03-02", "high", {"alpha": (5, 9)}))
        self.assertEqual(manifest.append(a, index), 1)
        self.assertEqual(manifest.append(b, index), 1)
        self.assertEqual(manifest.append(a, index), 1)
        once = index.read_text()
        a.write_text(json.dumps(flat("2026-03-01-fixture-a", "2026-03-01", "high", {"alpha": (8, 9)})))
        manifest.append(a, index)
        manifest.append(a, index)
        runs = json.loads(index.read_text())["runs"]
        self.assertEqual([(r["report"], r["metrics"]["pass_rate"]["k"]) for r in runs],
                         [("2026-03-01-fixture-a", 8), ("2026-03-02-fixture-b", 5)])
        manifest.append(a, index)
        self.assertEqual(index.read_text(), json.dumps(json.loads(index.read_text()), indent=1) + "\n")
        self.assertNotEqual(once, index.read_text())

    def test_a_dataset_with_no_pass_rate_adds_nothing_and_a_half_row_fails_loudly(self):
        index = self.dir / manifest.INDEX
        gate = self.put("2026-03-02-gate-thing", {"suite": "gate-bench", "date": "2026-03-02", "summary": {"cold": {"p50": 20.1}}})
        self.assertEqual(manifest.append(gate, index, log=lambda m: None), 0)
        self.assertFalse(index.exists())
        half = self.put("2026-03-04-half", {"report": "2026-03-04-half", "suite": "fixturebench@1",
                                            "summary": {"alpha": {"trials": 3, "solved": 2, "rate": 0.67, "wilson": [0.2, 0.9]}}})
        with self.assertRaises(manifest.ManifestError):
            manifest.append(half, index)
        self.assertFalse(index.exists(), "nothing written for a half row")


class DraftHook(unittest.TestCase):
    def test_a_drafted_run_writes_its_manifest_row(self):
        import contextlib
        import io
        spec = importlib.util.spec_from_file_location("bench_report_draft_t", HERE / "draft.py")
        draft = importlib.util.module_from_spec(spec)
        sys.modules[spec.name] = draft
        spec.loader.exec_module(draft)
        with tempfile.TemporaryDirectory() as d:
            out = Path(d) / "docs"
            data = flat("2026-03-01-fixture-a", "2026-03-01", "high", {"alpha": (6, 9)})
            data["figures"] = []
            with contextlib.redirect_stderr(io.StringIO()):
                written = draft.write_outputs(out, "2026-03-01-fixture-a", data, [], "# skeleton\n")
            self.assertIn(out / "index.json", written)
            runs = json.loads((out / "index.json").read_text())["runs"]
            self.assertEqual([(r["report"], r["arm"], r["metrics"]["pass_rate"]["k"]) for r in runs],
                             [("2026-03-01-fixture-a", "alpha", 6)])
            # A speed bench has no pass rate: it drafts, and the manifest is not touched.
            gate = {"report": "2026-03-02-gate", "suite": "gate-bench", "date": "2026-03-02", "summary": {"cold": {"p50": 1.0}},
                    "figures": []}
            before = (out / "index.json").read_text()
            with contextlib.redirect_stderr(io.StringIO()):
                draft.write_outputs(out, "2026-03-02-gate", gate, [], "# skeleton\n")
            self.assertEqual((out / "index.json").read_text(), before)


if __name__ == "__main__":
    unittest.main()
