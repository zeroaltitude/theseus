"""Tests for the benchmark manifest (theseus-jda0z).

    python3 -m unittest discover -s bench/report

Fixtures are invented datasets in a temporary directory: two shapes of pass rate, a gate-bench-like file with none,
an omnibus summary, and a file that is not JSON. The manifest must list each run's arms sorted the same way every
time, replace a report's rows when it is appended again, skip what it cannot read with a named reason, and be
written by the drafting tool beside every run's data file.
"""

from __future__ import annotations

import csv
import importlib.util
import json
import sys
import tempfile
import unittest
from pathlib import Path
from typing import Any

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


class ListPriced(Fixture):
    def metrics(self, **entry):
        doc = flat("2026-10-04-fixturebench-a", "2026-10-04", "full", {"a": (3, 4)})
        doc["summary"]["a"].update(entry)
        self.put("2026-10-04-fixturebench-a", doc)
        rows, _ = self.build()
        return rows["runs"][0]["metrics"]

    def test_the_list_priced_total_is_published_beside_the_harnesss_and_leaves_it_alone(self):
        m = self.metrics(usd_total=6.2, list_cost_usd=6.4, list_unpriced=0)
        self.assertEqual(m["usd_total"], {"value": 6.2})
        self.assertEqual(m["list_cost_usd"], {"value": 6.4})
        self.assertAlmostEqual(m["list_cost_usd"]["value"] - m["usd_total"]["value"], 0.2)

    def test_a_floor_is_not_published_as_a_total(self):
        m = self.metrics(usd_total=6.2, list_cost_usd=3.2, list_unpriced=1)
        self.assertNotIn("list_cost_usd", m)
        self.assertEqual(m["usd_total"], {"value": 6.2})

    def test_a_dataset_that_predates_the_field_gains_none(self):
        self.assertNotIn("list_cost_usd", self.metrics())


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


def with_models(data, **models):
    """Name each arm's own model (a string, or a list for a routed arm) the way draft.py now writes it."""
    for key, m in models.items():
        data["summary"][key]["model"] = m
    return data


class Framing(Fixture):
    """theseus-uhc2d: the manifest says which model each arm ran, how it was routed, how often it tried each task, and
    which tasks it ran. Every key is always present; unknown is null. `harness`/`model`/`routed` are benchframing.ts's A/B/T."""

    def rows(self, name, data):
        self.put(name, data)
        doc, _ = self.build()
        return {r["variant"]: r for r in doc["runs"] if r["report"] == name}

    def test_arms_that_ran_different_models_get_different_models_and_a_model_framing(self):
        d = with_models(flat("2026-03-01-a", "2026-03-01", "high", {"alpha": (6, 9), "beta": (3, 9)}),
                        alpha="anthropic/claude-sonnet-5-5", beta="openai/gpt-x")
        r = self.rows("2026-03-01-a", d)
        self.assertEqual((r["alpha"]["arm_model"], r["beta"]["arm_model"]), ("anthropic/claude-sonnet-5-5", "openai/gpt-x"))
        self.assertEqual((r["alpha"]["model"], r["beta"]["model"]), ("model-x", "model-x"),
                         "`model` stays the dataset's prose line: the cockpit reads it as the reference (join fix 1)")
        self.assertEqual((r["alpha"]["provider"], r["beta"]["provider"]), ("anthropic", "openai"))
        self.assertEqual((r["alpha"]["framing"], r["beta"]["framing"]), ("model", "model"))
        self.assertEqual((r["alpha"]["routing"], r["beta"]["routing"]), ("pinned", "pinned"))

    def test_one_model_across_the_arms_is_a_harness_framing_and_a_routed_arm_is_its_own(self):
        d = with_models(flat("2026-03-01-a", "2026-03-01", "high", {"alpha": (6, 9), "beta": (3, 9), "theseus_routed": (4, 9)}),
                        alpha="claude-sonnet-5-5", beta="anthropic/claude-sonnet-5-5",
                        theseus_routed=["claude-haiku-5-5", "claude-sonnet-5-5"])
        r = self.rows("2026-03-01-a", d)
        self.assertEqual((r["alpha"]["framing"], r["beta"]["framing"]), ("harness", "harness"))
        self.assertEqual(r["theseus_routed"]["framing"], "routed")
        self.assertEqual(r["theseus_routed"]["routing"], "routed")
        self.assertEqual(r["theseus_routed"]["arm_model"], ["claude-haiku-5-5", "claude-sonnet-5-5"])

    def test_the_newest_dataset_has_no_kind_and_no_per_arm_model_and_still_makes_rows(self):
        data = flat("2026-03-01-a", "2026-03-01", "high", {"alpha": (3, 3)})
        del data["model"]
        self.assertNotIn("kind", data)
        r = self.rows("2026-03-01-a", data)["alpha"]
        self.assertEqual((r["framing"], r["model"], r["arm_model"], r["provider"], r["routing"], r["effort"]),
                         ("harness", None, None, None, None, None))

    def test_an_arm_whose_label_names_a_model_nothing_else_names_is_a_model_framing(self):
        """join fix 2: the 10-25 codex dataset names GPT-6-Astra only in a label, and no model for its peers."""
        data = flat("2026-03-01-a", "2026-03-01", "high", {"alpha": (6, 9), "beta": (3, 9)})
        del data["model"]
        data["summary"]["alpha"]["label"] = "Alpha CLI 1.0 (GPT-6-Astra, high)"
        data["summary"]["beta"]["label"] = "Beta 2.0"
        r = self.rows("2026-03-01-a", data)
        self.assertEqual((r["alpha"]["framing"], r["beta"]["framing"]), ("model", "model"))
        self.assertEqual((r["alpha"]["arm_model"], r["alpha"]["provider"], r["alpha"]["model"]), ("GPT-6-Astra", "openai", None))
        self.assertEqual((r["beta"]["arm_model"], r["beta"]["provider"]), (None, None))
        data["model"] = "openai/gpt-6-astra"   # a reference the label agrees with: only the harness varies
        r = self.rows("2026-03-01-a", data)
        self.assertEqual((r["alpha"]["framing"], r["beta"]["framing"]), ("harness", "harness"))

    def test_the_published_codex_comparison_is_a_model_framing(self):
        name = "2026-10-25-terminal-bench-codex-high"
        data = json.loads((DOCS / f"{name}.json").read_text())
        rows, _ = manifest.rows_of(data, name)
        by = {r["variant"]: r for r in rows}
        self.assertEqual((by["codex"]["framing"], by["codex"]["provider"], by["codex"]["k"]), ("model", "openai", 1))
        self.assertEqual(by["claude-code"]["k"], 3)
        self.assertEqual(by["theseus-routed"]["framing"], "routed")

    def test_a_gateway_prefix_is_not_a_provider(self):
        self.assertEqual(manifest.provider_of("openrouter/deepseek/deepseek-v4-pro"), None)
        self.assertEqual(manifest.provider_of("zai/glm-5.3"), None)
        self.assertEqual(manifest.provider_of("openrouter/anthropic/claude-sonnet-5-5"), "anthropic")
        self.assertEqual(manifest.provider_of("openai/gpt-x"), "openai")

    def test_an_arm_that_ran_no_tasks_does_not_break_the_build(self):
        data = flat("2026-03-01-a", "2026-03-01", "high", {"alpha": (0, 3)})
        data["summary"]["alpha"]["tasks"] = 0
        self.assertEqual(self.rows("2026-03-01-a", data)["alpha"]["task_set"], "full")

    def test_k_is_never_trials_over_tasks_for_an_arm_that_ran_a_subset(self):
        data = flat("2026-03-01-a", "2026-03-01", "high", {"alpha": (6, 9), "beta": (6, 9)})  # 3 tasks each, k=3
        data["summary"]["beta"]["tasks"] = 2          # beta ran 2 of the 3: 9 trials over 2 tasks is no k
        r = self.rows("2026-03-01-a", data)
        self.assertEqual((r["alpha"]["task_set"], r["alpha"]["k"]), ("full", 3))
        self.assertEqual((r["beta"]["task_set"], r["beta"]["k"]), ("subset:2", None))
        data["summary"]["alpha"]["k"] = 3             # a stated k is kept; an uneven division is not guessed at
        data["summary"]["beta"]["tasks"] = 3
        data["summary"]["beta"]["trials"] = 8
        r = self.rows("2026-03-01-a", data)
        self.assertEqual((r["alpha"]["k"], r["beta"]["k"]), (3, None))

    def test_a_retest_with_a_scope_is_a_subset_and_not_the_full_run(self):
        full = flat("2026-03-01-full", "2026-03-01", "high", {"alpha": (6, 9)})
        cut = flat("2026-03-02-cut", "2026-03-02", "high", {"alpha": (2, 3)}, subset_of="2026-03-01-full",
                   scope={"planned": 1, "finished": 1, "tasks": ["t1"], "left_out": {}})
        cut["summary"]["alpha"]["tasks"] = 1
        self.put("2026-03-01-full", full)
        self.put("2026-03-02-cut", cut)
        doc, _ = self.build()
        d = {r["report"]: r for r in doc["runs"]}
        self.assertEqual((d["2026-03-01-full"]["task_set"], d["2026-03-01-full"]["subset_of"]), ("full", None))
        self.assertEqual((d["2026-03-02-cut"]["task_set"], d["2026-03-02-cut"]["subset_of"], d["2026-03-02-cut"]["k"]),
                         ("subset:1", "2026-03-01-full", None))

    def test_the_omnibus_is_still_skipped(self):
        self.put("omnibus", {"report": "omnibus", "kind": "omnibus", "summary": flat("x", "2026-03-02", "high", {"a": (1, 3)})["summary"]})
        doc, log = self.build()
        self.assertEqual(doc["runs"], [])
        self.assertIn("omnibus", "\n".join(log))

    def test_the_vocabulary_maps_onto_the_cockpits_framings(self):
        """benchframing.ts: A = same model, harness varies; B = model varies; T = Theseus routes over every model."""
        self.assertEqual(dict(zip(manifest.FRAMINGS, "ABT")), {"harness": "A", "model": "B", "routed": "T"})


DOCS = HERE.parent.parent / "docs" / "benchmarks"
OLD = ("2026-10-13-terminal-bench-harness-arms", "2026-10-21-terminal-bench-rerun-high", "2026-10-23-terminal-bench-retest-max")


class Published(unittest.TestCase):
    def test_a_published_datasets_rows_keep_every_key_they_had_and_gain_only_new_ones(self):
        """The regression guard: the keys rows_of wrote before theseus-uhc2d, with their values."""
        before = {"report", "benchmark", "tier", "arm", "variant", "harness", "model", "date", "commit", "dataset", "trials",
                  "tasks", "metrics"}
        added = {"arm_model", "provider", "routing", "effort", "k", "task_set", "subset_of", "framing"}
        for name in OLD:
            data = json.loads((DOCS / f"{name}.json").read_text())
            rows, _ = manifest.rows_of(data, name)
            self.assertTrue(rows, name)
            for r in rows:
                self.assertEqual(set(r) - {"comparison"}, before | added, name)
                self.assertEqual(r["model"], data["model"], "no per-arm model in a published dataset: the prose stays")
                self.assertEqual(r["trials"], data["summary"][r["variant"]]["trials"])

    def test_every_published_rows_arm_key_names_the_harness_its_label_names(self):
        """theseus-sjf5q: the Bug Hunt's OpenCode rows were keyed `openclaw` and its Codex CLI rows `pi`, so any
        chart over them drew the wrong harness in the wrong colour. An arm key is the only thing the cockpit and
        `charts.py` read a harness's name and colour from, so it has to agree with the label beside it."""
        checked, wrong = 0, []

        def hold(where: str, arm: Any, label: Any) -> None:
            nonlocal checked
            if not isinstance(arm, str) or not isinstance(label, str):
                return
            fam, names = manifest.family(arm), manifest.harnesses_named(label)
            if fam in manifest.NEUTRAL_ARMS or not names:
                return                                   # a role, or a label that names no harness
            checked += 1
            if fam not in names:
                wrong.append(f"{where}: arm {arm!r} against label {label!r} ({'/'.join(sorted(names))})")

        def walk(o: Any, where: str) -> None:
            if isinstance(o, dict):
                hold(where, o.get("arm"), o.get("label"))
                for k, v in o.items():
                    walk(v, f"{where}/{k}")
            elif isinstance(o, list):
                for v in o:
                    walk(v, f"{where}[]")

        for p in sorted(DOCS.glob("*.json")):
            if p.name == manifest.INDEX:
                continue
            walk(json.loads(p.read_text()), p.name)
        for i, r in enumerate(json.loads((DOCS / manifest.INDEX).read_text())["runs"]):
            hold(f"{manifest.INDEX} run {i}", r.get("arm"), r.get("harness"))   # the manifest says it as `harness`
        for p in sorted(DOCS.glob("*.csv")):
            rows = list(csv.DictReader(p.read_text().splitlines()))
            for i, r in enumerate(rows):
                hold(f"{p.name} row {i + 1}", r.get("arm"), r.get("label"))
        self.assertEqual(wrong, [], "\n".join(wrong))
        self.assertGreater(checked, 200, "the published datasets stopped reaching this check")


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
