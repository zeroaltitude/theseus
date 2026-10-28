"""The recall cell report refuses a cost ratio across two price tables (theseus-0j5k5).

    python3 -m unittest discover -s bench/gauntlet

The published seed-7 run (2026-10-18) was billed before `theseus-rko44` moved the Sonnet 5.5
cache-read price; seeds 11 and 13 (2026-10-26) after it. Their whole-run costs are not a
ratio of behaviour. These tests build a tiny cell from synthetic turns and scores and run
the report's own `build`, so the refusal, its exit code and the same-table positive case are
the program's output.
"""

from __future__ import annotations

import contextlib
import io
import json
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent / "harbor"))

import price_basis as pb  # noqa: E402
import recall_report as rr  # noqa: E402

S55 = "claude-sonnet-5-5"


def write_turns(path: Path, table: str, n: int = 30, cache_read: int = 40_000):
    tok = {"input": 6, "cache_read": cache_read, "cache_write": 600, "output": 150}
    cost = pb.reprice({S55: tok}, pb.TABLES[table])
    path.write_text("".join(json.dumps({"index": i, "tokens": tok, "cost_usd": cost}) + "\n"
                            for i in range(n)))


def write_scores(path: Path):
    probes = [{"arm": "theseus/baseline", "kind": "direct", "bucket": "near" if i % 3 else "session", "correct": i % 2 == 0,
               "cost_usd": 0.02, "latency_ms": 2000, "salience": "central", "planned": "near",
               "status": "scored", "turns_since": 3, "tokens_since": 900, "value_kind": "path"}
              for i in range(10)]
    arm = {"scored": 10, "failed": 0, "undelivered": 0, "recall_accuracy": 0.5,
           "abstention_accuracy": 0.2, "confident_wrong": 1, "confident_wrong_rate": 0.1,
           "cites_when": 0.5, "cites_where": 0.5, "cost_usd_per_probe": 0.02}
    path.write_text(json.dumps({"arms": {"theseus/baseline": arm}, "digest": "d" * 64,
                                "stale_rule": "strict", "probes": probes}))


def build(runs: Path, seed7_table: str, seed_table: str, *extra: str):
    write_turns(runs / "seed11-turns.jsonl", seed_table)
    write_scores(runs / "seed11-scores.json")
    seed7 = runs / "seed7-turns.jsonl"
    write_turns(seed7, seed7_table, cache_read=60_000)
    keep = (rr.RUNS, rr.SEED7_TURNS, dict(rr.SEED7))
    rr.RUNS, rr.SEED7_TURNS = runs, seed7
    rr.SEED7["run_cost_usd"] = round(sum(json.loads(l)["cost_usd"] for l in seed7.read_text().splitlines()), 4)
    out = io.StringIO()
    try:
        with contextlib.redirect_stdout(out):
            code = rr.build([11], *extra)
    finally:
        rr.RUNS, rr.SEED7_TURNS = keep[:2]
        rr.SEED7.clear()
        rr.SEED7.update(keep[2])
    return code, out.getvalue(), json.loads((runs / f"{rr.NAME}.json").read_text())


class RunBasis(unittest.TestCase):
    def test_a_run_reads_its_table_from_its_own_turns(self):
        for name in pb.TABLES:
            with tempfile.TemporaryDirectory() as t:
                p = Path(t) / "turns.jsonl"
                write_turns(p, name)
                b = rr.run_basis(p)
            self.assertEqual(b["price_digest"], pb.digest_of(name))
            self.assertEqual(b["price_counts"], {pb.digest_of(name): 30})

    def test_the_two_published_runs_are_on_different_tables(self):
        # the regress run's turns, when this machine has them: seed 7 before the move, 11 after
        if not rr.SEED7_TURNS.exists() or not rr.turns_path(11).exists():
            self.skipTest("the recall turns are not on this machine")
        a, b = rr.run_basis(rr.SEED7_TURNS), rr.run_basis(rr.turns_path(11))
        self.assertEqual(a["price_digest"], pb.digest_of(pb.PRE_RKO44))
        self.assertEqual(b["price_digest"], pb.digest_of(pb.CURRENT))


class Build(unittest.TestCase):
    def test_a_new_seed_on_the_other_table_is_refused_and_exits_nonzero(self):
        with tempfile.TemporaryDirectory() as t:
            code, out, data = build(Path(t), pb.PRE_RKO44, pb.CURRENT)
        self.assertEqual(code, 3, out)
        self.assertIn("COST RATIO REFUSED", out)
        self.assertIn(pb.digest_of(pb.PRE_RKO44), out)
        self.assertIn(pb.digest_of(pb.CURRENT), out)
        self.assertIn("refused", data["cost_comparison"])

    def test_every_dataset_row_carries_the_digest(self):
        with tempfile.TemporaryDirectory() as t:
            _, _, data = build(Path(t), pb.PRE_RKO44, pb.CURRENT)
            csv_lines = (Path(t) / f"{rr.NAME}.csv").read_text().splitlines()
        header = csv_lines[0].split(",")
        self.assertIn("price_digest", header)
        i = header.index("price_digest")
        digests = {ln.split(",")[i] for ln in csv_lines[1:]}
        self.assertEqual(digests, {pb.digest_of(pb.CURRENT)})
        self.assertEqual(len(csv_lines) - 1, 10)  # a positive row count, not just no mismatch
        self.assertEqual(data["summary"]["theseus/baseline@seed11"]["price_digest"],
                         pb.digest_of(pb.CURRENT))
        self.assertEqual(data["compare"]["2026-10-18 seed 7, Theseus"]["price_digest"],
                         pb.digest_of(pb.PRE_RKO44))

    def test_a_seed_on_the_published_runs_table_prints_the_ratio_and_exits_zero(self):
        with tempfile.TemporaryDirectory() as t:
            code, out, data = build(Path(t), pb.CURRENT, pb.CURRENT)
        self.assertEqual(code, 0, out)
        self.assertNotIn("REFUSED", out)
        self.assertIn("whole-run cost ratio", out)
        self.assertGreater(data["cost_comparison"]["n"]["new"], 0)
        self.assertEqual(data["cost_comparison"]["basis"], pb.digest_of(pb.CURRENT))

    def test_reprice_moves_both_onto_one_table_and_says_so(self):
        with tempfile.TemporaryDirectory() as t:
            code, out, data = build(Path(t), pb.PRE_RKO44, pb.CURRENT, pb.CURRENT)
        self.assertEqual(code, 0, out)
        self.assertIn("REPRICED", out)
        self.assertTrue(data["cost_comparison"]["repriced"])
        self.assertEqual(data["cost_comparison"]["basis"], pb.digest_of(pb.CURRENT))
        self.assertEqual(data["cost_comparison"]["seen"]["seed7"], [pb.digest_of(pb.PRE_RKO44)])


if __name__ == "__main__":
    unittest.main()
