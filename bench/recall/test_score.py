"""The scorer's tests, against numbers worked by hand: a known curve's
half-life, confident-wrong, stale, citation, abstention, the excluded
probes, and distance measured where the arm compacted. Standard library
only:

    python3 -m unittest discover -s bench/recall
"""

from __future__ import annotations

import io
import json
import sys
import tempfile
import unittest
from contextlib import redirect_stdout
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import generate  # noqa: E402
import progression as pg  # noqa: E402
import score  # noqa: E402


def row(bucket: str, correct: bool, turns: int, tokens: int, kind: str = "direct",
        salience: str = "incidental") -> score.Scored:
    return score.Scored(arm="a", probe="p", kind=kind, salience=salience, value_kind="port", carrier="aside",
                        planned=bucket,
                        bucket=bucket, status="scored", correct=correct, confident_wrong=False, stale=None,
                        hedged=False, cites_where=None, cites_when=None, turns_since=turns, tokens_since=tokens,
                        cost_usd=0.01, latency_ms=1000)


class HalfLife(unittest.TestCase):
    def test_a_known_curves_half_life(self):
        # near 4/4 at 2 turns; topic_shift 3/4 at 10; compaction 1/4 at 30;
        # session 0/2 at 100. Half of the nearest (100%) is 50%, crossed
        # between topic_shift (75%) and compaction (25%): 10 + (0.75 - 0.5) /
        # (0.75 - 0.25) × (30 - 10) = 20 turns; in tokens, 1000 + 0.5 × 4000.
        rows = ([row("near", True, 2, 100)] * 4
                + [row("topic_shift", True, 10, 1000)] * 3 + [row("topic_shift", False, 10, 1000)]
                + [row("compaction", True, 30, 5000)] + [row("compaction", False, 30, 5000)] * 3
                + [row("session", False, 100, 20000)] * 2
                # Abstentions and the other salience stay off this curve.
                + [row("near", False, 2, 100, kind="abstention")] * 5
                + [row("near", False, 2, 100, salience="central")] * 5)
        c = score.curve(rows, "incidental")
        self.assertEqual([b["accuracy"] for b in c], [1.0, 0.75, 0.25, 0.0, None])
        self.assertEqual([b["n"] for b in c], [4, 4, 4, 2, 0])
        self.assertEqual(score.half_lives(c), {"turns": 20.0, "tokens": 3000.0})

    def test_a_curve_that_never_halves_or_starts_at_nothing(self):
        self.assertEqual(score.half_life([(2, 1.0), (10, 0.8), (40, 0.6)]), "not reached")
        self.assertEqual(score.half_life([(2, 0.0), (10, 0.0)]), "undefined")
        self.assertEqual(score.half_life([]), "undefined")
        # Exactly half at a bucket is that bucket's distance.
        self.assertEqual(score.half_life([(2, 0.8), (12, 0.4)]), 12)
        # Points are taken nearest first, whatever order they come in.
        self.assertEqual(score.half_life([(30, 0.25), (2, 1.0), (10, 0.75)]), 20.0)


# The smoke of seed 7 (generate.py; its digest is pinned in test_generate):
#   p001 direct     supersession  port    f002 = 38013, stale 27340
#   p002 indirect   days          ticket  f003 = ORI-5419 (notes/p002.txt)
#   p003 abstention days          version (distractor f004 = 6.13.15)
#   p004 direct     days          host    f005 = shale-canary-91.lan (said, the topic)
#   p005 direct     compaction    ticket  f006 = NET-4575 (scripts/migrate-nettle-scheduler.sh), fact at 5, probe at 12
#   p006 abstention near          ticket
#   p007 direct     near          path    f008 = /srv/alder/collector/cache-53.idx
#   p008 indirect   topic_shift   ticket  f009 = HAR-2734 (notes/p008.txt)
REPLIES = {
    "p001": "It's 27340.",                                                    # stale, confident-wrong
    "p002": "Written; the ticket is ORI-0000.",                               # the file decides
    "p003": "I'm not sure, there's no record, but probably 6.13.15.",          # invented, hedged
    "p004": "You said on Monday 2026-11-09 that it's shale-canary-91.lan.",    # right, cites both
    "p005": "NET-4575, from ./scripts/migrate-nettle-scheduler.sh earlier.",   # right, cites both
    "p006": "I don't have any record of that ticket.",                        # a right abstention
    "p007": "It's /srv/alder/collector/cache-12.idx.",                        # confident-wrong
    "p008": "HAR-2734",                                                       # undelivered: excluded
}


def write_run(d: Path, prog: pg.Progression, label: str, compactions: list[int], failed: set[str] = frozenset()):
    """A run directory as drive.py writes it, with REPLIES at the probes."""
    d.mkdir(parents=True)
    pg.materialize(prog, d / "workspace")
    (d / "workspace" / "notes" / "p002.txt").write_text("fix the crash loop (ORI-5419)\n")
    (d / "workspace" / "notes" / "p008.txt").write_text("HAR-2734\n")
    (d / "progression.json").write_text(json.dumps(prog.to_json()))
    by_turn = {p.turn: p for p in prog.probes}
    with (d / "turns.jsonl").open("w") as f:
        for t in prog.turns:
            p = by_turn.get(t.index)
            reply = REPLIES[p.id] if p else "ok"
            bad = p is not None and p.id in failed
            f.write(json.dumps({"index": t.index, "session": t.session, "reply": "" if bad else reply,
                                "exit": 1 if bad else 0, "cost_usd": 0.01, "latency_ms": 1000,
                                "tokens": {"input": 10, "output": 5, "cache_read": 0, "cache_write": 5}}) + "\n")
    delivered = {f.id: {"delivered": f.id != "f009", "marker": f.marker, "turn": f.turn} for f in prog.facts}
    (d / "delivered.json").write_text(json.dumps(delivered))
    (d / "run.json").write_text(json.dumps({"arm": label, "model": "m", "compactions": compactions,
                                            "delivered": len(prog.facts) - 1, "facts": len(prog.facts)}))


class Scoring(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.prog = generate.build(7, "smoke")
        ids = {p.id: (p.kind, p.bucket, p.turn, p.fact) for p in cls.prog.probes}
        # The fixture's reading of the smoke, checked, so a new smoke fails
        # here first.
        assert ids["p001"][:2] == ("direct", "supersession") and ids["p005"][:3] == ("direct", "compaction", 12)
        assert ids["p008"][3] == "f009" and ids["p003"][0] == "abstention", ids

    def scored(self, compactions, failed=frozenset()):
        with tempfile.TemporaryDirectory() as d:
            write_run(Path(d) / "r", self.prog, "theseus", compactions, failed)
            run = score.load_run(Path(d) / "r")
            rows = score.score_run(run)
        return {r.probe: r for r in rows}, score.summarize(rows)

    def test_each_probe_is_scored_as_worked_by_hand(self):
        r, s = self.scored([])
        self.assertEqual(r["p008"].status, "undelivered")
        self.assertEqual({k: v.correct for k, v in r.items()},
                         {"p001": False, "p002": True, "p003": False, "p004": True, "p005": True,
                          "p006": True, "p007": False, "p008": None})
        self.assertEqual({k for k, v in r.items() if v.confident_wrong}, {"p001", "p007"})
        self.assertTrue(r["p001"].stale)
        self.assertTrue(r["p003"].hedged)
        self.assertEqual((r["p004"].cites_where, r["p004"].cites_when), (True, True))
        self.assertEqual((r["p005"].cites_where, r["p005"].cites_when), (True, True))
        # Recall: p001 ✗, p002 ✓, p004 ✓, p005 ✓, p007 ✗ = 3 of 5; abstention:
        # p003 ✗, p006 ✓ = 1 of 2.
        self.assertEqual((s["scored"], s["undelivered"], s["failed"]), (7, 1, 0))
        self.assertAlmostEqual(s["recall_accuracy"], 0.6)
        self.assertAlmostEqual(s["abstention_accuracy"], 0.5)
        self.assertEqual((s["confident_wrong"], s["stale"], s["stale_of"]), (2, 1, 1))
        self.assertEqual((s["cites_where"], s["cites_when"]), (1.0, 1.0))
        self.assertAlmostEqual(s["cost_usd_per_probe"], 0.01)
        # By carrier: p001's fact an aside (✗), p002's an error (✓), p004's the
        # topic (✓), p005's an error (✓), p007's a remark (✗).
        self.assertEqual({c: v for c, v in s["by_carrier"].items() if v is not None},
                         {"aside": 0.0, "error": 1.0, "topic": 1.0, "remark": 0.0})
        # Turns and tokens since: p005's fact at turn 5, its probe at 12, and
        # 6 turns between of 20 new tokens each.
        self.assertEqual((r["p005"].turns_since, r["p005"].tokens_since), (7, 120))

    def test_an_invented_value_is_not_an_abstention(self):
        r, _ = self.scored([])
        self.assertFalse(r["p003"].correct)
        self.assertFalse(score.checks.passes(pg.kind_check("version"), REPLIES["p003"]))

    def test_a_failed_turn_is_excluded_and_counted(self):
        r, s = self.scored([], failed={"p006"})
        self.assertEqual(r["p006"].status, "failed")
        self.assertEqual((s["scored"], s["failed"]), (6, 1))
        self.assertAlmostEqual(s["abstention_accuracy"], 0.0)

    def test_distance_is_where_the_arm_compacted_not_the_marks(self):
        # The mark is at turn 10, between p005's fact (5) and its probe (12).
        self.assertEqual(self.prog.marks(), [10])
        r, s = self.scored([])
        self.assertEqual((r["p005"].planned, r["p005"].bucket), ("compaction", "topic_shift"))
        self.assertEqual(s["moved"], 1)
        r, _ = self.scored([12])
        self.assertEqual(r["p005"].bucket, "compaction")
        r, _ = self.scored([13])
        self.assertEqual(r["p005"].bucket, "topic_shift")
        # p007, planned near (fact 20, probe 24), lands past a compaction at 22.
        r, _ = self.scored([22])
        self.assertEqual((r["p007"].planned, r["p007"].bucket), ("near", "compaction"))

    def test_the_cli_writes_the_report_the_svg_and_the_scores(self):
        with tempfile.TemporaryDirectory() as d:
            d = Path(d)
            write_run(d / "th", self.prog, "theseus", [12])
            write_run(d / "cc", self.prog, "claude-code", [])
            with redirect_stdout(io.StringIO()) as out:
                rc = score.main([str(d / "th"), str(d / "cc"), "--out", str(d / "rep")])
            self.assertEqual(rc, 0)
            self.assertIn("theseus: recall 60%", out.getvalue())
            md = (d / "rep" / "report.md").read_text()
            self.assertIn("| theseus | m | 12 |", md)
            self.assertIn("| claude-code | m | never |", md)
            svg = (d / "rep" / "curve.svg").read_text()
            self.assertTrue(svg.startswith("<svg") and svg.rstrip().endswith("</svg>"))
            self.assertIn("polyline", svg)
            j = json.loads((d / "rep" / "scores.json").read_text())
            self.assertEqual(set(j["arms"]), {"theseus", "claude-code"})
            self.assertEqual(len(j["probes"]), 2 * len(self.prog.probes))
            # Runs of different progressions are refused.
            other = generate.build(8, "smoke")
            write_run(d / "x", other, "claude-code", [])
            with redirect_stdout(io.StringIO()):
                self.assertEqual(score.main([str(d / "th"), str(d / "x"), "--out", str(d / "rep2")]), 2)


if __name__ == "__main__":
    unittest.main()
