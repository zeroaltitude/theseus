"""Tests for the price-table digest and the cross-basis guard (theseus-0j5k5).

    python3 -m unittest discover -s bench/harbor

A comparison column whose price table moved between the two runs is the
root cause of four corrections in one day (regress-1026 §8). These tests
pin the two halves of the repair: a record says which table billed it, and a
ratio refuses to cross two tables. The negative tests are paired with positive
ones on purpose: a guard that refuses everything also passes a negative test.
"""

from __future__ import annotations

import copy
import json
import tempfile
import unittest
from pathlib import Path

import efficiency as ef
import price_basis as pb

S55 = "claude-sonnet-5-5"


def usage(i=0, r=0, w=0, o=0):
    return {"input": i, "cache_read": r, "cache_write": w, "output": o}


def billed(table, by_model):
    """What a harness on `table` would have recorded for `by_model`'s tokens."""
    return pb.reprice(by_model, pb.TABLES[table])


def row(arm, table, r=4_000_000, o=20_000, dollars=None):
    by_model = {S55: usage(i=10_000, r=r, w=100_000, o=o)}
    d = billed(table, by_model) if dollars is None else dollars
    return {"arm": arm, "dollars": d, "by_model": by_model,
            "price_digest": pb.digest_of(table)}


class Digest(unittest.TestCase):
    def test_the_two_tables_have_different_digests(self):
        self.assertEqual(len(pb.TABLES), 2)
        self.assertEqual(len({pb.digest_of(n) for n in pb.TABLES}), 2)

    def test_the_digest_is_stable_and_order_blind(self):
        t = pb.TABLES[pb.CURRENT]
        shuffled = {k: dict(reversed(list(v.items()))) for k, v in reversed(list(t.items()))}
        self.assertEqual(pb.table_digest(t), pb.table_digest(shuffled))

    def test_the_digest_moves_with_any_price(self):
        t = copy.deepcopy(pb.TABLES[pb.CURRENT])
        t[S55]["output"] += 0.01
        self.assertNotEqual(pb.table_digest(t), pb.digest_of(pb.CURRENT))

    def test_the_current_table_is_the_efficiency_list_prices(self):
        # drift between the two would make every new record's digest a lie
        self.assertEqual(pb.TABLES[pb.CURRENT], ef.LIST_PRICES)

    def test_the_two_tables_differ_only_in_sonnet_cache_reads(self):
        a, b = pb.TABLES[pb.PRE_RKO44], pb.TABLES[pb.CURRENT]
        diff = [(m, c) for m in a for c in a[m] if a[m][c] != b[m][c]]
        self.assertEqual(diff, [(S55, "cache_read")])
        self.assertEqual(a[S55]["cache_read"], 0.10)
        self.assertEqual(b[S55]["cache_read"], 0.20)


class Measure(unittest.TestCase):
    def test_a_bill_on_each_table_is_recognised(self):
        for name in pb.TABLES:
            r = row("x", name)
            d, how = pb.measure(r["by_model"], r["dollars"])
            self.assertEqual((d, how), (pb.digest_of(name), "measured"), name)

    def test_a_bill_with_no_cache_reads_fits_both_and_is_ambiguous(self):
        by = {S55: usage(i=1000, o=500)}
        d, how = pb.measure(by, billed(pb.CURRENT, by))
        self.assertEqual((d, how), (None, "ambiguous"))

    def test_a_bill_on_neither_table_is_unmatched_not_guessed(self):
        r = row("x", pb.CURRENT)
        self.assertEqual(pb.measure(r["by_model"], r["dollars"] * 1.5), (None, "unmatched"))

    def test_no_recorded_cost_is_unpriced(self):
        self.assertEqual(pb.measure({S55: usage(r=5)}, None), (None, "unpriced"))

    def test_an_unknown_model_cannot_be_measured(self):
        by = {"some-model": usage(r=5_000_000)}
        self.assertEqual(pb.measure(by, 1.0), (None, "unmeasurable"))

    def test_the_delta_between_the_tables_is_the_cache_read_delta(self):
        r = row("x", pb.PRE_RKO44)
        hi = pb.reprice(r["by_model"], pb.TABLES[pb.CURRENT])
        self.assertAlmostEqual(hi - r["dollars"], 4_000_000 * 0.10e-6, places=6)


class MeasureMany(unittest.TestCase):
    """A run's table, measured turn by turn: a whole-run sum does not fit either table
    (long-window tiers and other items sit in the bill), but nearly every turn does."""

    def turns(self, table, n=10):
        by = {S55: usage(i=6, r=20_000, w=500, o=150)}
        return [(by, billed(table, by)) for _ in range(n)]

    def test_a_run_on_one_table_is_that_table(self):
        for name in pb.TABLES:
            got = pb.measure_many(self.turns(name))
            self.assertEqual(got["digest"], pb.digest_of(name))
            self.assertEqual(got["how"], "measured")
            self.assertEqual(got["counts"], {pb.digest_of(name): 10})

    def test_a_few_unmatched_turns_do_not_unseat_the_measured_ones(self):
        turns = self.turns(pb.CURRENT) + [({S55: usage(r=20_000)}, 9.99)] * 2
        got = pb.measure_many(turns)
        self.assertEqual(got["digest"], pb.digest_of(pb.CURRENT))
        self.assertEqual(got["unmatched"], 2)

    def test_a_run_split_across_tables_is_mixed_and_has_no_digest(self):
        got = pb.measure_many(self.turns(pb.PRE_RKO44, 4) + self.turns(pb.CURRENT, 4))
        self.assertIsNone(got["digest"])
        self.assertEqual(got["how"], "mixed")

    def test_a_run_with_nothing_measurable_has_no_digest(self):
        got = pb.measure_many([({S55: usage(i=10)}, 0.00002)] * 3)
        self.assertEqual((got["digest"], got["how"]), (None, "ambiguous"))

    def test_an_empty_run_is_not_a_basis(self):
        self.assertEqual(pb.measure_many([])["how"], "unpriced")


class Stamp(unittest.TestCase):
    def test_the_record_carries_its_digest(self):
        by = {S55: usage(i=10, r=4_000_000, w=1000, o=100)}
        rec = {"by_model": by, "cost_usd": billed(pb.CURRENT, by)}
        s = pb.stamp(rec)
        self.assertEqual(s["price_digest"], pb.digest_of(pb.CURRENT))
        self.assertEqual(s["price_basis"], "measured")
        self.assertEqual(s["list_price_digest"], pb.digest_of(pb.CURRENT))

    def test_an_unmeasurable_record_says_so_rather_than_omitting_the_key(self):
        s = pb.stamp({"by_model": {}, "cost_usd": None})
        self.assertIn("price_digest", s)
        self.assertIsNone(s["price_digest"])
        self.assertEqual(s["price_basis"], "unpriced")

    def test_efficiency_write_puts_the_digest_in_efficiency_json(self):
        by = {S55: {**usage(i=10, r=4_000_000, w=1000, o=100), "cost_usd": 0.0, "calls": 3}}
        rec = {"arm": "theseus", "by_model": by, "cost_usd": billed(pb.PRE_RKO44, by)}
        with tempfile.TemporaryDirectory() as d:
            path = ef.write(Path(d), rec)
            got = json.loads(path.read_text())
        self.assertEqual(got["price_digest"], pb.digest_of(pb.PRE_RKO44))
        self.assertEqual(got["price_basis"], "measured")


class Guard(unittest.TestCase):
    def test_a_cross_digest_comparison_fails_loudly(self):
        a = [row("round1", pb.PRE_RKO44)]
        b = [row("round2", pb.CURRENT)]
        with self.assertRaises(pb.CrossBasisError) as cm:
            pb.compare({"round1": a, "round2": b}, value=lambda r: r["dollars"])
        msg = str(cm.exception)
        # it names both digests it saw, and which arm carried which
        self.assertIn(pb.digest_of(pb.PRE_RKO44), msg)
        self.assertIn(pb.digest_of(pb.CURRENT), msg)
        self.assertIn("round1", msg)
        self.assertIn("round2", msg)

    def test_a_same_digest_comparison_still_prints(self):
        a = [row("round1", pb.CURRENT, r=8_000_000), row("round1", pb.CURRENT, r=6_000_000)]
        b = [row("round2", pb.CURRENT, r=2_000_000)]
        out = pb.compare({"round1": a, "round2": b}, value=lambda r: r["dollars"])
        self.assertEqual(out["basis"], pb.digest_of(pb.CURRENT))
        self.assertEqual(out["repriced"], False)
        # the second arm over the first: round2's one row against round1's mean
        self.assertAlmostEqual(out["ratio"], b[0]["dollars"] / ((a[0]["dollars"] + a[1]["dollars"]) / 2))
        self.assertLess(out["ratio"], 1.0)
        self.assertEqual(out["n"], {"round1": 2, "round2": 1})

    def test_the_positive_case_has_a_positive_count(self):
        out = pb.compare({"a": [row("a", pb.CURRENT)], "b": [row("b", pb.CURRENT)]},
                         value=lambda r: r["dollars"])
        self.assertGreater(sum(out["n"].values()), 0)
        self.assertAlmostEqual(out["ratio"], 1.0)

    def test_an_arm_with_no_digest_cannot_be_compared(self):
        bare = {"arm": "old", "dollars": 1.0, "by_model": {}}
        with self.assertRaises(pb.CrossBasisError) as cm:
            pb.compare({"old": [bare], "new": [row("new", pb.CURRENT)]}, value=lambda r: r["dollars"])
        self.assertIn("no price digest", str(cm.exception))

    def test_an_arm_that_is_itself_mixed_is_refused(self):
        mixed = [row("m", pb.PRE_RKO44), row("m", pb.CURRENT)]
        with self.assertRaises(pb.CrossBasisError) as cm:
            pb.compare({"m": mixed, "n": [row("n", pb.CURRENT)]}, value=lambda r: r["dollars"])
        self.assertIn("m", str(cm.exception))

    def test_an_empty_arm_is_refused_not_passed(self):
        # a "zero X" bar is met by an empty set
        with self.assertRaises(pb.CrossBasisError) as cm:
            pb.compare({"a": [], "b": [row("b", pb.CURRENT)]}, value=lambda r: r["dollars"])
        self.assertIn("no rows", str(cm.exception))

    def test_rows_that_could_not_be_measured_do_not_veto_a_stamped_arm(self):
        # a haiku-only trial has no cache read to measure; its arm is still one basis
        haiku = {"arm": "a", "dollars": 0.1, "by_model": {}, "price_digest": None}
        out = pb.compare({"a": [row("a", pb.CURRENT), haiku], "b": [row("b", pb.CURRENT)]},
                         value=lambda r: r["dollars"])
        self.assertEqual(out["basis"], pb.digest_of(pb.CURRENT))

    def test_repricing_both_arms_onto_one_table_prints_and_says_so(self):
        a = [row("round1", pb.PRE_RKO44)]
        b = [row("round2", pb.CURRENT)]
        out = pb.compare({"round1": a, "round2": b}, value=lambda r: r["dollars"],
                         reprice_to=pb.CURRENT)
        self.assertEqual(out["repriced"], True)
        self.assertEqual(out["basis"], pb.digest_of(pb.CURRENT))
        self.assertEqual(out["seen"], {"round1": [pb.digest_of(pb.PRE_RKO44)],
                                       "round2": [pb.digest_of(pb.CURRENT)]})
        # same tokens in both arms: priced alike, the ratio is 1, not the price change
        self.assertAlmostEqual(out["ratio"], 1.0)

    def test_the_cross_table_ratio_is_the_price_change_and_repricing_removes_it(self):
        a = [row("round1", pb.PRE_RKO44)]
        b = [row("round2", pb.CURRENT)]
        raw = b[0]["dollars"] / a[0]["dollars"]
        self.assertGreater(raw, 1.05)  # the mixed ratio a naive report would have printed
        out = pb.compare({"round1": a, "round2": b}, value=lambda r: r["dollars"],
                         reprice_to=pb.CURRENT)
        self.assertLess(abs(out["ratio"] - 1.0), 1e-6)

    def test_repricing_a_row_with_no_per_model_usage_refuses(self):
        bare = {"arm": "old", "dollars": 1.0, "by_model": {}, "price_digest": pb.digest_of(pb.PRE_RKO44)}
        with self.assertRaises(pb.CrossBasisError) as cm:
            pb.compare({"old": [bare], "new": [row("new", pb.CURRENT)]},
                       value=lambda r: r["dollars"], reprice_to=pb.CURRENT)
        self.assertIn("cannot reprice", str(cm.exception))


if __name__ == "__main__":
    unittest.main()
