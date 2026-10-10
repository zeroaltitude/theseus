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

import checks  # noqa: E402
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


def write_run(d: Path, prog: pg.Progression, label: str, compactions: list[int], failed: set[str] = frozenset(),
              replies: dict[str, str] | None = None, meta: dict | None = None):
    """A run directory as drive.py writes it, with REPLIES (and `replies`
    over them) at the probes, and `meta` in its run.json."""
    replies = {**REPLIES, **(replies or {})}
    d.mkdir(parents=True)
    pg.materialize(prog, d / "workspace")
    (d / "workspace" / "notes" / "p002.txt").write_text("fix the crash loop (ORI-5419)\n")
    (d / "workspace" / "notes" / "p008.txt").write_text("HAR-2734\n")
    (d / "progression.json").write_text(json.dumps(prog.to_json()))
    by_turn = {p.turn: p for p in prog.probes}
    with (d / "turns.jsonl").open("w") as f:
        for t in prog.turns:
            p = by_turn.get(t.index)
            reply = replies[p.id] if p else "ok"
            bad = p is not None and p.id in failed
            f.write(json.dumps({"index": t.index, "session": t.session, "reply": "" if bad else reply,
                                "exit": 1 if bad else 0, "cost_usd": 0.01, "latency_ms": 1000,
                                "tokens": {"input": 10, "output": 5, "cache_read": 0, "cache_write": 5}}) + "\n")
    delivered = {f.id: {"delivered": f.id != "f009", "marker": f.marker, "turn": f.turn} for f in prog.facts}
    (d / "delivered.json").write_text(json.dumps(delivered))
    (d / "run.json").write_text(json.dumps({"arm": label, "model": "m", "compactions": compactions,
                                            "delivered": len(prog.facts) - 1, "facts": len(prog.facts),
                                            **(meta or {})}))


class Scoring(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.prog = generate.build(7, "smoke")
        ids = {p.id: (p.kind, p.bucket, p.turn, p.fact) for p in cls.prog.probes}
        # The fixture's reading of the smoke, checked, so a new smoke fails
        # here first.
        assert ids["p001"][:2] == ("direct", "supersession") and ids["p005"][:3] == ("direct", "compaction", 12)
        assert ids["p008"][3] == "f009" and ids["p003"][0] == "abstention", ids

    def scored(self, compactions, failed=frozenset(), replies=None, meta=None, stale="strict", prog=None):
        with tempfile.TemporaryDirectory() as d:
            write_run(Path(d) / "r", prog or self.prog, "theseus", compactions, failed, replies, meta)
            run = score.load_run(Path(d) / "r")
            rows = score.score_run(run, stale)
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

    def test_an_old_runs_abstention_is_scored_by_todays_admission(self):
        """The live smoke's p003: a right abstention the first ADMIT missed
        (no `tell`; its `no` wants to come before the verb). A run keeps its
        progression's checks, so the scorer derives an abstention's again
        from its kind, and a run kept from before scores by today's rule."""
        said = ("I can't tell. A case-insensitive search for \"lanyard\" across the 20 workspace files found no "
                "matches, so nothing there pins a lanyard version for veery.")
        old = pg.Progression.from_json(json.loads(json.dumps(self.prog.to_json())))
        p3 = next(p for p in old.probes if p.id == "p003")
        p3.check = p3.check.splitlines()[0] + "\nreply has /\\bno (?:record|mention)\\b/i"
        self.assertFalse(checks.passes(p3.check, said))
        r, _ = self.scored([], replies={"p003": said}, prog=old)
        self.assertTrue(r["p003"].correct)
        self.assertFalse(r["p003"].confident_wrong)
        r, _ = self.scored([], replies={"p003": "I can't tell; it's probably 6.13.15."}, prog=old)
        self.assertFalse(r["p003"].correct)

    def test_a_retracted_old_value_is_right_only_under_the_retracted_rule(self):
        """The live smoke's p001 names 27340 only to take it back."""
        live = ("The jacana archiver is on port 38013. That's from the side note in the standup. Ignore the "
                '"27340" I mentioned in my first answer.')
        r, s = self.scored([], replies={"p001": live})
        self.assertEqual((r["p001"].correct, r["p001"].stale, r["p001"].old_named), (False, True, False))
        self.assertEqual(s["stale"], 1)
        r, s = self.scored([], replies={"p001": live}, stale="retracted")
        x = r["p001"]
        self.assertEqual((x.correct, x.stale, x.old_named, x.confident_wrong), (True, False, True, False))
        self.assertEqual((s["stale"], s["old_named"]), (0, 1))
        for reply in ("It moved from 27340 to 38013.", "38013 now; 27340 is no longer used.",
                      "It's 38013. It was 27340 before the move.", "27340 was replaced by 38013.",
                      "38013 replaced 27340.", "It's 38013, not 27340.", "It's no longer 27340; it's 38013.",
                      "It's 38013 (instead of the old port 27340).", "27340 isn't used anymore, it's 38013 now.",
                      "As I said previously, it's 38013, not 27340."):
            r, _ = self.scored([], replies={"p001": reply}, stale="retracted")
            self.assertTrue(r["p001"].correct and r["p001"].old_named, reply)
            self.assertFalse(r["p001"].stale or r["p001"].confident_wrong, reply)
        # Stale under strict, as theseus-exam's superseded items are.
        r, _ = self.scored([], replies={"p001": "It moved from 27340 to 38013."})
        self.assertEqual((r["p001"].correct, r["p001"].stale), (False, True))
        for stale in ("strict", "retracted"):
            r, _ = self.scored([], replies={"p001": "The archiver is on port 27340."}, stale=stale)
            self.assertEqual((r["p001"].correct, r["p001"].stale, r["p001"].confident_wrong), (False, True, True))
            r, _ = self.scored([], replies={"p001": "It's 27340, or maybe 38013."}, stale=stale)
            self.assertEqual((r["p001"].correct, r["p001"].stale, r["p001"].old_named), (False, True, False))
            # A retraction in another sentence does not cover the old value.
            r, _ = self.scored([], replies={"p001": "It's 38013 or 27340. Ignore my first answer."}, stale=stale)
            self.assertFalse(r["p001"].correct, stale)
        with self.assertRaises(ValueError):
            self.scored([], stale="lenient")

    def test_a_retraction_word_that_governs_the_new_value_retracts_nothing(self):
        """theseus-5dey: a retraction word in the old value's sentence that
        governs the new value gives the old one as current: wrong and stale
        under both rules."""
        for stale in ("strict", "retracted"):
            for reply in ("It's 27340, previously 38013.", "It was 38013 before, now it's 27340.",
                          "Port 27340 replaced 38013.", "38013 is no longer used; it's 27340.",
                          "It's 27340, no longer 38013.", "It's not 38013 but 27340.",
                          "Don't forget 27340; 38013 is no longer used."):
                r, _ = self.scored([], replies={"p001": reply}, stale=stale)
                x = r["p001"]
                self.assertEqual((x.correct, x.stale, x.old_named), (False, True, False), (stale, reply))

    def test_a_careful_reader_and_the_rule_agree_on_these_replies(self):
        """theseus-qryz: shapes the rule once misread, scored as a careful
        reader scores them (p001: new 38013, old 27340). Under strict none
        changes: each names the old value, so each is stale."""
        wrong = (
            # `the old` and `the former` describe; they retract nothing alone.
            "The old port 27340 is back; 38013 was rolled back.",
            "Don't use 38013; the old port 27340 still works.",
            "The former port 27340 is the one in use; 38013 is gone.",
            # A quoted move is cited, and commits to nothing.
            'You wrote "moved from 27340 to 38013"; I can\'t confirm which one is current.',
            "You said \u201cmoved from 27340 to 38013\u201d, but I can't tell.",
            # A negated retraction.
            "27340 is no longer wrong; 38013 was a typo.",
            # `new_free`: the new value stated only where a phrase governs it.
            "It's no longer 27340, and not 38013 either.",
            # The old value named again after its retraction is the current
            # one again: its last naming is what the reply leaves standing.
            "It moved from 27340 to 38013, then back to 27340.",
            "It was 27340 before, then 38013, and now it's 27340 again.",
            "38013 replaced 27340, but that was reverted, so it's 27340.",
            "I previously said 27340 was replaced by 38013, but that was wrong: it's 27340.",
            "It was 27340 before and it is still 27340; 38013 never went live.",
            # A lead word reaches "was" four words off, as every phrase does.
            "Before you change anything, the port was 27340 this morning and still is; 38013 is the target.",
        )
        right = (
            # A list under one phrase: each member is governed.
            "It's no longer 11111 or 27340; it's 38013.",
            "It used to be 11111 and later 27340; now it's 38013.",
            "It moved from 11111 to 27340, then from 27340 to 38013.",
            "It's 38013 now; 27340 and 11111 are both retired.",
            "It's 38013 now; 27340, 11111 and 99999 are all retired.",
            # A lead word ahead of "was".
            "It's 38013 now; earlier it was 27340.",
            "Before the move it was 27340; now it's 38013.",
            "It's 38013; it was 27340 earlier.",
            # `the old` beside a retraction elsewhere in its clause.
            "The old port 27340 was replaced; it's 38013 now.",
            # A move's destination, retracted later in the clause.
            "It moved to 27340 in May, then from 27340 to 38013 in June.",
            "It's 38013; originally it was 27340.",
        )
        for reply in wrong:
            for stale in ("strict", "retracted"):
                r, _ = self.scored([], replies={"p001": reply}, stale=stale)
                x = r["p001"]
                self.assertEqual((x.correct, x.stale, x.old_named), (False, True, False), (stale, reply))
        for reply in right:
            r, _ = self.scored([], replies={"p001": reply}, stale="retracted")
            x = r["p001"]
            self.assertTrue(x.correct and x.old_named, reply)
            self.assertFalse(x.stale or x.confident_wrong, reply)
            r, _ = self.scored([], replies={"p001": reply})
            self.assertEqual((r["p001"].correct, r["p001"].stale), (False, True), reply)

    def test_the_false_rights_of_the_retracted_rule_are_closed(self):
        """theseus-hau2: six replies the rule once scored right, in three
        shapes, that a careful reader scores wrong (p001: new 38013, old
        27340: each gives the old value as current), and three sentences it
        must keep right, which no test held (the `the old` arm, `after that`,
        and the other words of the then-from prefix). Strict scores every
        one stale, as before."""
        wrong = (
            # `the old X` beside a retraction that governs another value.
            "Use the old port 27340 instead; 38013 isn't up yet.",
            "It's 38013 on paper, but the old port 27340 is what answers, since 38013 was dropped.",
            # A negation ahead of a prefix.
            "It's no longer wrong to use 27340; 38013 was a mistake.",
            # A comma between two clauses read as a list.
            "Ignore 11111, 27340 is the live port, and 38013 is only planned.",
            "Ignore 11111, 27340's the live port, and 38013 is only planned.",
            "No longer 11111, 27340 was the live port, and 38013 is only planned.",
        )
        right = (
            # The `the old` arm: a retraction word about X in its clause.
            "The old port 27340, the one we no longer use, is closed; it's 38013.",
            # Each word of the then-from prefix, alone.
            "It went to 27340 first, and after that from 27340 to 38013.",
            "It went to 27340 first, and later from 27340 to 38013.",
            "It went to 27340 first, and next from 27340 to 38013.",
            # A list's later member under the phrase before it, no verb after it.
            "Ignore 11111, 27340 and 99999; it's 38013.",
            "It's 38013 now; 27340, 11111 and 99999 are all retired.",
        )
        for reply in wrong:
            for stale in ("strict", "retracted"):
                r, _ = self.scored([], replies={"p001": reply}, stale=stale)
                x = r["p001"]
                self.assertEqual((x.correct, x.stale, x.old_named), (False, True, False), (stale, reply))
        for reply in right:
            r, _ = self.scored([], replies={"p001": reply}, stale="retracted")
            x = r["p001"]
            self.assertTrue(x.correct and x.old_named, reply)
            self.assertFalse(x.stale or x.confident_wrong, reply)
            r, _ = self.scored([], replies={"p001": reply})
            self.assertEqual((r["p001"].correct, r["p001"].stale), (False, True), reply)

    def test_a_citing_prefix_in_either_quote_style_governs_nothing(self):
        """`sentences` folds typographic quotes before any phrase is read,
        so `CANCEL` needs only the ASCII ones (theseus-hau2)."""
        for q1, q2 in (("\u201c", "\u201d"), ("\u2018", "\u2019"), ('"', '"'), ("'", "'")):
            reply = f"You said {q1}moved from 27340 to 38013{q2}, but I can't tell."
            r, _ = self.scored([], replies={"p001": reply}, stale="retracted")
            x = r["p001"]
            self.assertEqual((x.correct, x.stale, x.old_named), (False, True, False), reply)
        self.assertNotIn("u201c", score.CANCEL.lower())
        self.assertNotIn("\u201c", score.CANCEL)

    def test_the_new_value_must_stand_free_for_a_retraction_to_be_right(self):
        """`retracts_only`'s `new_free`: with every old value retracted but
        the new one stated only where a phrase governs it, nothing says the
        new one is current."""
        r = score.retracts_only
        self.assertFalse(r("It's no longer 27340, and not 38013 either.", "port", "27340", "38013"))
        self.assertFalse(r("Ignore 27340 and 38013.", "port", "27340", "38013"))
        self.assertTrue(r("Ignore 27340; it's 38013.", "port", "27340", "38013"))

    def test_a_retracting_phrase_reaches_one_value_four_words_off(self):
        """`retracts_only` on its own: a phrase governs the nearest value on
        its side, within REACH words of the same clause, of any kind."""
        r = score.retracts_only
        self.assertEqual(score.REACH, 4)
        self.assertTrue(r("Instead of the old port 27340, use 38013.", "port", "27340", "38013"))
        # Six words off, the phrase is about something else.
        self.assertFalse(r("Ignore what I said about the port 27340; it's 38013.", "port", "27340", "38013"))
        # Nor across a semicolon or a dash.
        self.assertFalse(r("Ignore that; 27340 is it. Also 38013.", "port", "27340", "38013"))
        self.assertFalse(r("It moved from the rack — 27340 is on it, and 38013 too.", "port", "27340", "38013"))
        # Versions in both forms, and dates in the forms people write.
        self.assertTrue(r("It's v6.14.2 now; it was v6.13.15 before.", "version", "6.13.15", "6.14.2"))
        self.assertFalse(r("It's 6.13.15, previously 6.14.2.", "version", "6.13.15", "6.14.2"))
        self.assertTrue(r("It moved from Nov 3 to November 10.", "date", "2026-11-03", "2026-11-10"))
        self.assertFalse(r("It was November 10 before, now it's Nov 3.", "date", "2026-11-03", "2026-11-10"))

    def test_a_ring_moves_a_probe_as_a_summary_does_and_is_counted_apart(self):
        """run.json's `compaction_rows` keep each outcome: a ring at 12 cuts
        p005's fact (5) from its probe's request (12) as a summary would."""
        rows = [{"turn": 12, "outcomes": ["ring"], "cuts": [{"outcome": "ring", "why": "no summary model",
                                                             "messages": 20, "first": 3, "last": 40}]},
                {"turn": 22, "outcomes": ["compaction"], "cuts": [{"outcome": "compaction", "why": None,
                                                                   "messages": 9, "first": 41, "last": 80}]}]
        r, _ = self.scored([12, 22], meta={"compaction_rows": rows})
        self.assertEqual(r["p005"].bucket, "compaction")
        self.assertEqual(r["p007"].bucket, "compaction")
        self.assertEqual(score.compaction_turns({"compaction_rows": rows}), [12, 22])
        self.assertEqual(score.outcome_counts({"compaction_rows": rows}), {"ring": 1, "compaction": 1})
        # A run with no rows (Claude Code's, an older one) counts its list.
        self.assertEqual(score.outcome_counts({"compactions": [12]}), {"compaction": 1})
        with tempfile.TemporaryDirectory() as d:
            d = Path(d)
            write_run(d / "th", self.prog, "theseus", [12, 22], meta={"compaction_rows": rows})
            with redirect_stdout(io.StringIO()):
                self.assertEqual(score.main([str(d / "th"), "--out", str(d / "rep")]), 0)
            md = (d / "rep" / "report.md").read_text()
            self.assertIn("| theseus | m | 12 (ring), 22 (compaction) |", md)
            self.assertIn("| theseus | 1 | 1 | 0 |", md)
            j = json.loads((d / "rep" / "scores.json").read_text())
            self.assertEqual(j["arms"]["theseus"]["compactions_by_outcome"], {"ring": 1, "compaction": 1})
            self.assertEqual(j["stale_rule"], "strict")

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
