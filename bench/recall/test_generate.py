"""The generator's tests: determinism, stratification, the invented names,
and the budget's prices. Standard library only:

    python3 -m unittest discover -s bench/recall
"""

from __future__ import annotations

import io
import json
import math
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
import tokens as tk  # noqa: E402

# The smoke's digest for seed 7: a change to the generator, its lists, or
# SplitMix64 moves it. Pin the new one only for a change meant to make a new
# progression, and say so in its commit.
SMOKE_7 = "07e95754f01394f0"  # theseus-523y: the bulks sized by the compiler's rule


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


class Window(unittest.TestCase):
    SEEDS = (7, 11, 12)

    def test_the_request_budget_is_the_compilers(self):
        self.assertEqual(pg.request_budget(35000), 35000 - 8750 - 4096)
        self.assertEqual(pg.request_budget(124000), 124000 - 16000 - 4096)

    def test_each_mark_crosses_the_budget_and_no_read_turn_alone_passes_it(self):
        """By the compiler's rule (tokens.py), for the smoke and the full at
        three seeds: the turns before a mark fit at MARGIN over their
        estimate; each read turn alone, estimated whole beside the system
        prompt and tools, stays ALONE_MARGIN under the budget at its upper
        bound (past it, the turn fails as an overage, as the first live
        smoke's mark did); and at MARGIN under, the reads cross the budget at
        the mark or the turn after it, which holds nothing else."""
        rates = generate.PLAN_RATES
        for size in ("smoke", "full"):
            for seed in self.SEEDS:
                p = generate.build(seed, size)
                budget = pg.request_budget(p.context_window)
                bounds = generate.bounds_of(p)
                self.assertEqual([b["mark"] for b in bounds], p.marks())
                for b in bounds:
                    at = (size, seed, b["mark"])
                    m = b["mark"]
                    self.assertEqual(b["budget"], budget)
                    self.assertLessEqual(b["fit"], budget, at)
                    self.assertGreater(b["cross"], budget, at)
                    self.assertIn(b["cross_at"], (m, m + 1), at)
                    self.assertLessEqual(b["alone_limit"] * (1 + generate.ALONE_MARGIN), budget + 1, at)
                    for x in b["alone"]:
                        self.assertLessEqual(x, b["alone_limit"], at)
                    for path, nbytes, _ in b["logs"]:
                        self.assertLessEqual(nbytes, tk.RESULT_MAX_CHARS, (at, path))
                    # The mark's turn alone, worked from its bytes here.
                    log = p.workspace[generate.bulk_path(p.marks().index(m))]["content"]
                    whole = (tk.user_text(p.turns[m].text) + tk.call("fs_read", {"path": generate.bulk_path(
                        p.marks().index(m))}) + tk.result(tk.fs_read_bytes(log))).tokens(rates)
                    self.assertEqual(tk.bound(generate.OVERHEAD_TOKENS + whole), b["alone"][0], at)
                    self.assertLessEqual(tk.bound(generate.OVERHEAD_TOKENS + whole) * (1 + generate.ALONE_MARGIN),
                                         budget + 1, at)
                    # The turn after the mark holds nothing but its reads.
                    self.assertIn(p.turns[m + 1].role, ("filler", "bulk"), at)
                    self.assertFalse(any(f.turn in (m, m + 1) for f in p.facts), at)
                    self.assertFalse(any(q.turn in (m, m + 1) for q in p.probes), at)
                # A session without a mark fits whole.
                for s in range(len(p.sessions)):
                    own = p.session_turns(s)
                    if not any(t.mark for t in own):
                        total = sum(t.est_tokens for t in own)
                        self.assertLessEqual(generate.fit_bound(int(total * (1 + generate.MARGIN)),
                                                                int(own[-1].est_tokens * (1 + generate.MARGIN))),
                                             budget, (size, seed, s))

    def test_a_turns_estimate_is_its_messages_at_the_rates(self):
        """A filler that reads a file: its text, the call, the numbered
        lines, and a reply, each framed (provider.rs's Census)."""
        p = generate.build(7, "smoke")
        t = next(t for t in p.turns if t.text.startswith("How many lines are in "))
        path = t.text.removeprefix("How many lines are in ").rstrip("?")
        content = p.workspace[path]["content"]
        js, tx = generate.PLAN_RATES
        nbytes = sum(7 + len(x) + 1 for x in content.splitlines())
        want = (math.ceil((len(path) + len('fs_read{"path":""}')) / js + nbytes / js)
                + math.ceil((len(t.text) + generate.REPLY_BYTES) / tx) + 4 * 3 + 4 * 1 + 2 * 15)
        self.assertLessEqual(abs(t.est_tokens - want), 2, (t.est_tokens, want))


class TheRustRule(unittest.TestCase):
    """tokens.py's constants, each read from its Rust source, as PRICES is
    read from the catalog: a change there fails here first."""

    def src(self, rel: str) -> str:
        return (REPO / "crates" / rel).read_text()

    def test_the_rates_are_the_catalogs(self):
        src = self.src("theseus-core/src/catalog.rs")
        for name, (js, tx) in tk.RATES.items():
            m = re.search(r"pub const " + name + r": TokenRates = TokenRates \{\s*json: ([\d.]+),\s*text: ([\d.]+),",
                          src)
            self.assertIsNotNone(m, name)
            self.assertEqual((float(m.group(1)), float(m.group(2))), (js, tx), name)
        # `TokenRates::of`: Haiku 4.x the old figures, GLM its own, the rest Claude's.
        self.assertRegex(src, r'starts_with\("claude-haiku-4"\)\s*\{\s*Self::CLAUDE_OLD')
        self.assertRegex(src, r'starts_with\("glm-"\)\s*\{\s*Self::GLM')
        self.assertEqual(tk.rates_of("anthropic/claude-haiku-4-5"), tk.RATES["CLAUDE_OLD"])
        self.assertEqual(tk.rates_of("claude-sonnet-5-5"), tk.RATES["CLAUDE"])

    def test_the_framing_is_the_providers(self):
        src = self.src("theseus-core/src/provider.rs")
        for name in ("MESSAGE_TOKENS", "BLOCK_TOKENS", "ID_TOKENS"):
            m = re.search(r"pub const " + name + r": u64 = (\d+);", src)
            self.assertEqual(int(m.group(1)), getattr(tk, name), name)
        for name in ("OPAQUE_BYTES_PER_TOKEN", "DENSEST_BYTES_PER_TOKEN"):
            m = re.search(r"const " + name + r": f64 = ([\d.]+);", src)
            self.assertEqual(float(m.group(1)), getattr(tk, name), name)
        # A tool result's text, a string or text blocks, is json bytes.
        self.assertIn('Value::String(s) => self.json += s.len() as u64', src)

    def test_the_margin_the_budget_and_the_ring_are_the_compilers(self):
        src = self.src("theseus-core/src/compiler.rs")
        m = re.search(r"pub const MARGIN_PERCENT: u64 = (\d+);", src)
        self.assertEqual(int(m.group(1)), tk.MARGIN_PERCENT)
        m = re.search(r"fn request_budget\(w: u64, spec: &RequestSpec\) -> u64 \{\s*w\.saturating_sub\(spec\.max_tokens as "
                      r"u64\)\s*\.saturating_sub\(([\d_]+)\)", src)
        self.assertEqual(int(m.group(1).replace("_", "")), tk.HEADROOM)
        m = re.search(r"let target = budget \* (\d+) / (\d+);", src)
        self.assertEqual((int(m.group(1)), int(m.group(2))), tk.RING_TARGET)
        self.assertIn("upper: counted + estimated + (estimated * MARGIN_PERCENT).div_ceil(100),", src)
        self.assertIn("if est.upper > budget || input.overflowed.is_some() {", src)
        self.assertEqual(tk.upper(1000, 100), 1140)

    def test_a_summarys_room_is_the_compactions(self):
        src = self.src("theseus-core/src/turn/compaction.rs")
        m = re.search(r"pub const SUMMARY_MAX_TOKENS: u32 = (\d+);", src)
        self.assertEqual(int(m.group(1)), tk.SUMMARY_MAX_TOKENS)
        # It is written only where it fits beside the ring's kept turns.
        self.assertIn("if limit > 0 && ring.estimate.upper + u64::from(max_tokens) > limit {", src)

    def test_a_tool_results_cap_and_fs_reads_lines_are_the_tools(self):
        m = re.search(r"fn default_result_max_chars\(\) -> usize \{\s*([\d_]+)\s*\}",
                      self.src("theseus-core/src/config.rs"))
        self.assertEqual(int(m.group(1).replace("_", "")), tk.RESULT_MAX_CHARS)
        self.assertIn('let row = format!("{:>6}\\t{}\\n", i + 1, l);', self.src("theseus-tools/src/fs.rs"))
        self.assertEqual(tk.FS_READ_PREFIX, len(f"{1:>6}\t"))
        self.assertEqual(tk.fs_read_bytes("ab\nc\n"), (7 + 3) + (7 + 2))

    def test_the_census_reads_a_request_as_the_provider_does(self):
        """A request worked by hand: a system block, one tool, and a user
        text, a call and its result."""
        req = {"system": [{"type": "text", "text": "x" * 33}], "tools": [{"name": "t"}],
               "messages": [{"role": "user", "content": "y" * 66},
                            {"role": "assistant", "content": [{"type": "tool_use", "id": "i", "name": "fs_read",
                                                               "input": {"path": "a"}}]},
                            {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "i",
                                                          "content": "z" * 240}]}]}
        c = tk.census_of_request(req)
        self.assertEqual((c.json, c.text, c.messages, c.blocks, c.ids),
                         (len('{"name":"t"}') + len("fs_read") + len('{"path":"a"}') + 240, 99, 4, 3, 2))
        self.assertEqual(c.tokens(tk.RATES["CLAUDE"]), math.ceil(c.json / 2.4) + 30 + 4 * 3 + 3 + 2 * 15)


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
