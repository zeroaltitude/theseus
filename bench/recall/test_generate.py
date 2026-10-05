"""The generator's tests: determinism, stratification, the invented names,
and the budget's prices. Standard library only:

    python3 -m unittest discover -s bench/recall
"""

from __future__ import annotations

import io
import json
import re
import sys
import tempfile
import unittest
from contextlib import redirect_stdout
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
sys.path.insert(0, str(HERE))

import generate  # noqa: E402
import progression as pg  # noqa: E402

# The smoke's digest for seed 7: a change to the generator, its lists, or
# SplitMix64 moves it. Pin the new one only for a change meant to make a new
# progression, and say so in its commit.
SMOKE_7 = "a47aab4f47f4760f"


class Rng(unittest.TestCase):
    def test_the_stream_matches_the_reference(self):
        """Vigna's splitmix64.c for seed 0, as theseus-exam's rng.rs pins it."""
        r = generate.Rng(0)
        self.assertEqual(
            [r.next_u64() for _ in range(3)],
            [0xE220A8397B1DCDAF, 0x6E789E6AA1B965F4, 0x06C45D188009454F],
        )


class Determinism(unittest.TestCase):
    def test_the_smokes_digest_is_pinned(self):
        self.assertEqual(generate.build(7, "smoke").digest()[:16], SMOKE_7)

    def test_the_full_is_the_same_twice_and_another_seed_differs(self):
        a, b = generate.build(7, "full"), generate.build(7, "full")
        self.assertEqual(a.canonical(), b.canonical())
        self.assertNotEqual(generate.build(8, "full").digest(), a.digest())
        self.assertNotEqual(generate.build(8, "smoke").digest()[:16], SMOKE_7)

    def test_the_generator_draws_from_splitmix64_alone(self):
        src = (HERE / "generate.py").read_text()
        self.assertIsNone(re.search(r"^\s*(import|from)\s+random\b", src, re.M))
        self.assertIsNone(re.search(r"\b(uuid|secrets|time\.time|os\.urandom)\b", src))


class Stratification(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.full = generate.build(7, "full")
        cls.smoke = generate.build(7, "smoke")

    def test_every_cell_of_the_full_is_filled(self):
        cells = pg.cells(self.full)
        for c in pg.expected_cells():
            self.assertGreaterEqual(cells.get(c, 0), generate.FULL_PER_CELL, c)
        self.assertEqual(set(cells), set(pg.expected_cells()))

    def test_each_fact_comes_before_its_probe_and_is_probed_once(self):
        for prog in (self.full, self.smoke, generate.build(11, "full")):
            facts = prog.facts_by_id()
            seen = set()
            for p in prog.probes:
                self.assertLess(facts[p.anchor].turn, p.turn, p.id)
                if p.fact is not None:
                    self.assertNotIn(p.fact, seen, p.id)
                    seen.add(p.fact)
            pg.validate(prog)

    def test_the_full_has_three_sessions_of_two_hundred_and_a_mark_each(self):
        p = self.full
        self.assertEqual([len(p.session_turns(i)) for i in range(3)], [200, 200, 200])
        self.assertEqual(len(p.marks()), 3)
        self.assertEqual(p.sessions[0].date, p.sessions[1].date)
        self.assertNotEqual(p.sessions[1].date, p.sessions[2].date)
        self.assertEqual(len({t.block for t in p.turns}), 30)
        for fam in pg.FAMILIES[:-1]:
            self.assertTrue(any(f.family == fam for f in p.facts), fam)
        for c in pg.INCIDENTAL_CARRIERS:
            self.assertTrue(any(f.carrier == c and f.salience == "incidental" for f in p.facts), c)

    def test_the_smoke_has_two_short_sessions_a_mark_and_each_probe_kind(self):
        p = self.smoke
        self.assertEqual([len(p.session_turns(i)) for i in range(2)], [15, 15])
        self.assertEqual(len(p.marks()), 1)
        self.assertEqual({q.kind for q in p.probes}, set(pg.PROBE_KINDS))
        self.assertEqual(set(pg.cells(p)), set(generate.SMOKE_CELLS))

    def test_each_session_opens_with_its_date(self):
        for i, s in enumerate(self.full.sessions):
            first = self.full.session_turns(i)[0]
            self.assertTrue(first.text.startswith(f"It's {s.weekday}, {s.date}."), first.text)

    def test_an_incidental_tool_fact_is_gone_from_the_workspace_after_its_turn(self):
        p = self.full
        for f in p.facts:
            if f.carrier not in ("output", "error"):
                continue
            self.assertIn(f.marker, p.workspace[f.source]["content"], f.id)
            nxt = p.turns[f.turn + 1]
            edits = [e for e in nxt.before if e.path == f.source]
            self.assertEqual(len(edits), 1, f.id)
            self.assertNotIn(f.value if f.kind != "date" else f.marker, edits[0].content, f.id)

    def test_an_abstention_has_a_distractor_of_its_kind_before_it(self):
        facts = self.full.facts_by_id()
        for q in self.full.probes:
            if q.kind != "abstention":
                continue
            d = facts[q.distractors[0]]
            self.assertEqual(d.family, "distractor")
            self.assertEqual(d.kind, q.value_kind)
            self.assertNotEqual(d.subject, q.subject)

    def test_a_supersessions_stale_value_is_said_first_and_checked_absent(self):
        facts = self.full.facts_by_id()
        for q in self.full.probes:
            if q.bucket != "supersession":
                continue
            new = facts[q.fact]
            old = facts[new.supersedes]
            self.assertLess(old.turn, new.turn)
            self.assertEqual(old.family, "superseded")
            self.assertEqual(q.stale, old.value)
            self.assertIn("lacks", q.check)


class Names(unittest.TestCase):
    def test_no_generated_name_or_value_is_in_exam_v2(self):
        exam = (REPO / "crates/theseus-exam/exam/exam-v2.toml").read_text().lower()
        words = set(re.findall(r"[a-z0-9]+", exam))
        for lst, names in generate.NAME_LISTS.items():
            for n in names:
                self.assertNotIn(n, words, f"{lst}: {n}")
        for prog in (generate.build(7, "full"), generate.build(7, "smoke")):
            self.assertTrue(prog.names)
            for n in prog.names:
                self.assertNotIn(n, words, n)
            for f in prog.facts:
                if f.kind != "date":
                    self.assertNotIn(f.value.lower(), exam, f.id)
            for t in prog.turns:
                for person in generate.PEOPLE:
                    if person.capitalize() in t.text:
                        self.assertNotIn(person, words)


class Budget(unittest.TestCase):
    def test_the_prices_are_the_catalogs(self):
        """Each priced model's row in theseus-core's built-in catalog:
        input, output, cache read, cache write."""
        src = (REPO / "crates/theseus-core/src/catalog.rs").read_text()
        for model, prices in generate.PRICES.items():
            m = re.search(
                r'"' + re.escape(model) + r'"\.into\(\),\s*(?:CatalogEntry \{.*?\.\.)?claude\(\s*(?:m|\d[\d_]*),\s*'
                r"[\d_]+,\s*([\d.]+),\s*([\d.]+),\s*([\d.]+),\s*([\d.]+)",
                src,
                re.S,
            )
            self.assertIsNotNone(m, model)
            self.assertEqual(tuple(float(x) for x in m.groups()), prices, model)

    def test_the_cli_writes_the_progression_and_prints_the_budget(self):
        with tempfile.TemporaryDirectory() as d:
            out = Path(d) / "rc"
            buf = io.StringIO()
            with redirect_stdout(buf):
                rc = generate.main(["--seed", "7", "--size", "smoke", "--out", str(out)])
            self.assertEqual(rc, 0)
            text = buf.getvalue()
            self.assertIn(f"digest {SMOKE_7}", text)
            self.assertRegex(text, r"\$\d+\.\d\d \(two arms")
            self.assertIn("probes: 8", text)
            prog = pg.load(out)
            self.assertEqual(prog.digest()[:16], SMOKE_7)
            self.assertTrue((out / "workspace" / "README.md").is_file())
            self.assertEqual(json.loads((out / "progression.json").read_text())["format"], pg.FORMAT)
            # A second run refuses the full directory.
            with redirect_stdout(io.StringIO()):
                self.assertEqual(generate.main(["--seed", "7", "--size", "smoke", "--out", str(out)]), 2)

    def test_an_unpriced_model_is_refused(self):
        with self.assertRaises(SystemExit):
            generate.estimate(generate.build(7, "smoke"), "anthropic/claude-nonesuch")


if __name__ == "__main__":
    unittest.main()
