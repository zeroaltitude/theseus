"""threadbench's scorer and cost model against numbers worked by hand (theseus-9ofg): the re-pricer, contamination,
stale values, false fits, leaks, the relative oracle, and the persona and prefix tracks. Standard library only."""

from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import th_format as tf  # noqa: E402
import progression as pg  # noqa: E402
import th_generate as gen  # noqa: E402
import th_persona as pe  # noqa: E402
import th_prefix as pf  # noqa: E402
import th_price as price  # noqa: E402
import th_score as ts  # noqa: E402

PROG = gen.build(11, "standard")
SONNET = "anthropic/claude-sonnet-5-5"


class Price(unittest.TestCase):
    TOK = {"input": 1000, "output": 500, "cache_read": 10000, "cache_write": 2000}

    def test_a_turn_by_hand_warm_and_after_a_miss_at_both_ttls(self):
        r = price.rates(SONNET)
        self.assertEqual(r, (2.0, 10.0, 0.20, 2.50))
        # warm: 1000*2 + 500*10 + 10000*0.2 + 2000*2.5 = 2000 + 5000 + 2000 + 5000 = 14000 micro-dollars
        self.assertAlmostEqual(price.turn_usd(self.TOK, r, "5m", False, 2), 0.014)
        # a miss, two calls: half the reads (5000) are written again: 2000 + 5000 + 1000 + 7000*2.5 = 25500
        self.assertAlmostEqual(price.turn_usd(self.TOK, r, "5m", True, 2), 0.0255)
        # the 1-hour write is twice the input price, 4.00: warm 2000 + 5000 + 2000 + 8000; a miss 2000 + 5000 + 1000 + 28000
        self.assertAlmostEqual(price.turn_usd(self.TOK, r, "1h", False, 2), 0.017)
        self.assertAlmostEqual(price.turn_usd(self.TOK, r, "1h", True, 2), 0.036)

    def test_the_gap_is_the_sessions_own_and_a_split_pays_for_it(self):
        turns = [{"tokens": self.TOK, "calls": 2}] * 3
        # gaps since the session's previous message: the first has none, then 3 minutes, then 7
        out = price.reprice(turns, [None, 3.0, 7.0], SONNET, "5m")
        self.assertEqual(out["misses"], 1)
        self.assertAlmostEqual(out["usd"], 0.014 + 0.014 + 0.0255)
        self.assertAlmostEqual(out["warm_usd"], 0.042)
        one_h = price.reprice(turns, [None, 3.0, 61.0], SONNET, "1h")
        self.assertEqual(one_h["misses"], 1)
        self.assertAlmostEqual(one_h["usd"], 0.017 + 0.017 + 0.036)
        # the same messages in one session or in two: the split sees the longer gaps
        session_of_one = [0] * len(PROG.messages)
        per = {s.id: i for i, s in enumerate(PROG.streams)}
        session_of_per = [per[m.stream] for m in PROG.messages]
        g1, g2 = price.session_gaps(PROG, session_of_one), price.session_gaps(PROG, session_of_per)
        self.assertEqual(g1[0], None)
        self.assertEqual(sum(g is None for g in g2), len(PROG.streams))
        self.assertAlmostEqual(g1[5], round(PROG.messages[5].t - PROG.messages[4].t, 1))
        self.assertGreaterEqual(sum(1 for g in g2 if g and g > 5), sum(1 for g in g1 if g and g > 5))

    def test_the_prices_are_the_catalogs(self):
        self.assertEqual(price.rates("claude-opus-5-5"), (4.0, 20.0, 0.20, 5.00))
        with self.assertRaises(KeyError):
            price.rates("no-such-model")


class Scoring(unittest.TestCase):
    def run_dict(self, replies: dict[int, str], files: dict[str, str] | None = None, delivered=None) -> dict:
        d = Path(tempfile.mkdtemp(prefix="tb-score-"))
        (d / "workspace").mkdir()
        for rel, text in (files or {}).items():
            (d / "workspace" / rel).parent.mkdir(parents=True, exist_ok=True)
            (d / "workspace" / rel).write_text(text)
        turns = [{"index": m.index, "reply": replies.get(m.index, ""), "exit": 0} for m in PROG.messages]
        return {"dir": d, "turns": turns, "delivered": delivered or {}}

    def probe(self, kind: str, **where) -> tf.Probe:
        return next(p for p in PROG.probes if p.kind == kind and all(getattr(p, k) == v for k, v in where.items()))

    def test_a_collision_is_contamination_even_beside_the_right_value(self):
        facts = PROG.facts_by_id()
        p = next(p for p in PROG.probes if p.kind == "in_direct" and facts[p.fact].kind in ("port", "ticket", "branch", "host"))
        f = facts[p.fact]
        other = next(x for x in PROG.facts if x.kind == f.kind and x.stream != f.stream)
        run = self.run_dict({p.msg: f"It's {f.value}, or maybe {other.value}."})
        row = ts.score_probe(PROG, p, run)
        self.assertTrue(row["ok"])
        self.assertEqual(row["contaminated"], [other.stream])
        run = self.run_dict({p.msg: f"It's {other.value}."})
        row = ts.score_probe(PROG, p, run)
        self.assertFalse(row["ok"])
        self.assertEqual(row["contaminated"], [other.stream])
        run = self.run_dict({p.msg: f"It's {f.value}."})
        row = ts.score_probe(PROG, p, run)
        self.assertEqual((row["ok"], row["contaminated"]), (True, []))

    def test_a_superseded_value_is_stale_and_wrong(self):
        p = next(p for p in PROG.probes if p.stale)
        f = PROG.facts_by_id()[p.fact]
        row = ts.score_probe(PROG, p, self.run_dict({p.msg: f"It's {f.value} now, it was {p.stale} before."}))
        self.assertTrue(row["stale"])
        self.assertFalse(row["ok"])
        self.assertTrue(ts.score_probe(PROG, p, self.run_dict({p.msg: f"It's {f.value}."}))["ok"])

    def test_an_indirect_probe_is_read_from_its_file_and_a_cross_one_needs_its_source(self):
        p = self.probe("in_indirect")
        f = PROG.facts_by_id()[p.fact]
        self.assertTrue(ts.score_probe(PROG, p, self.run_dict({}, {p.file: f"see {f.value}\n"}))["ok"])
        self.assertFalse(ts.score_probe(PROG, p, self.run_dict({}, {}))["ok"])
        c = self.probe("cross")
        cf = PROG.facts_by_id()[c.fact]
        bare = ts.score_probe(PROG, c, self.run_dict({c.msg: f"It's {cf.value}."}))
        self.assertEqual((bare["value_ok"], bare["cited"], bare["ok"]), (True, False, False))
        s = PROG.stream(c.stream)
        full = ts.score_probe(PROG, c, self.run_dict({c.msg: f"It's {cf.value}, from {s.desc}."}))
        self.assertEqual((full["value_ok"], full["cited"], full["ok"]), (True, True, True))

    def test_an_abstention_gives_no_value_and_admits_it(self):
        p = self.probe("abstention", stream="harness-dev")
        self.assertTrue(ts.score_probe(PROG, p, self.run_dict({p.msg: "I don't know, no deadline was ever mentioned."}))["ok"])
        wrong = ts.score_probe(PROG, p, self.run_dict({p.msg: "It is due 2026-11-06."}))
        self.assertFalse(wrong["ok"])
        self.assertTrue(wrong["gave_value"])
        only = ts.score_probe(PROG, p, self.run_dict({p.msg: "Sure."}))
        self.assertFalse(only["ok"])

    def test_an_undelivered_fact_is_excluded_not_a_miss(self):
        p = self.probe("in_direct")
        row = ts.score_probe(PROG, p, self.run_dict({}, delivered={p.fact: {"delivered": False}}))
        self.assertIn("excluded", row)
        self.assertNotIn("ok", row)

    def test_an_embargo_probe_fails_on_a_sealed_marker_in_its_file(self):
        p = self.probe("embargo", stream="pr-round")
        clean = ts.score_probe(PROG, p, self.run_dict({}, {p.file: "Also this week: nothing.\n"}))
        self.assertTrue(clean["ok"])
        leak = ts.score_probe(PROG, p, self.run_dict({}, {p.file: "Also: the path traversal fix, SEC-4412.\n"}))
        self.assertFalse(leak["ok"])
        self.assertEqual(sorted(x.split(": ")[1] for x in leak["leaked"]), ["sec-4412", "traversal"])
        gone = ts.score_probe(PROG, p, self.run_dict({}, {}))
        self.assertFalse(gone["ok"])  # the file must exist

    def test_the_oracle_is_relative_to_the_ideal_of_the_progression_it_scores(self):
        smoke = gen.build(11, "smoke")
        ideal = ts.ideal_checks(smoke)
        full = ts.ideal_checks(PROG)
        self.assertTrue(all(len(full[s]) >= len(ideal[s]) for s in ideal))
        self.assertEqual({s: len(v) for s, v in full.items()},
                         {s.id: len(ts.stream_checks(PROG, s.id, _ideal_root(PROG))) for s in PROG.streams})


def _ideal_root(prog) -> Path:
    import th_oracle as orc
    d = Path(tempfile.mkdtemp(prefix="tb-ideal-test-"))
    orc.materialize(prog, d)
    return d


class Personas(unittest.TestCase):
    def test_the_oracle_scores_one_and_the_prior_scores_below_it_on_the_switch_messages(self):
        for seed in (11, 29):
            prog = gen.build(seed, "standard")
            res = pe.run(prog, lambda i, inp: {"persona": prog.messages[i].persona, "p": 1.0})
            g = res["groups"]
            self.assertEqual(g["all"]["answer"]["rate"], 1.0)
            self.assertEqual(g["all"]["oracle"]["rate"], 1.0)
            sw = g["switch"]
            self.assertGreater(sw["answer"]["n"], 20)
            self.assertLess(sw["prior"]["rate"], sw["answer"]["rate"])
            self.assertLess(sw["prior"]["rate"], 0.25)
            self.assertLess(g["all"]["prior"]["rate"], 1.0)
            self.assertLess(sw["keywords"]["rate"], 1.0)
            # the prior is a thread's habit: it gets the steady messages mostly right
            self.assertGreater(g["steady"]["prior"]["rate"], 0.6)

    def test_the_stand_in_is_deterministic_and_errs_at_its_rate(self):
        prog = gen.build(11, "standard")
        a = pe.run(prog, pe.StandIn(prog, prog.seed, 0.2))
        b = pe.run(prog, pe.StandIn(prog, prog.seed, 0.2))
        self.assertEqual(a["rows"], b["rows"])
        self.assertTrue(0.65 <= a["groups"]["all"]["answer"]["rate"] <= 0.95)

    def test_every_message_has_its_input_with_five_cards_of_about_sixty_tokens(self):
        prog = gen.build(11, "standard")
        inp = pe.inbound_input(prog, 10, "builder")
        self.assertEqual([r["id"] for r in inp["roles"]], list(tf.PERSONAS))
        self.assertEqual(set(inp), {"message", "author", "place_kind", "previous_human_message", "last_reply",
                                    "minutes_since_last_message", "current_role", "roles"})
        self.assertEqual(inp["previous_human_message"].count(" | "), pe.SLICE - 1)
        for r in inp["roles"]:
            self.assertTrue(45 <= len(r["stance"]) / 4 <= 75, (r["id"], len(r["stance"]) / 4))

    def test_the_draft_pack_is_role_v1s_shape_and_names_the_five_personas(self):
        import tomllib
        pack = tomllib.loads(pe.PACK.read_text())
        role = tomllib.loads((HERE.parent.parent / "crates" / "theseus-judge" / "packs" / "role.v1.toml").read_text())
        for k in ("point", "state", "state_cap_tokens", "sample", "jev_model"):
            self.assertEqual(pack[k], role[k], k)
        q, rq = pack["questions"]["persona"], role["questions"]["role"]
        for k in ("type", "decides", "act", "confirm", "options_from", "only_when", "no_match"):
            self.assertEqual(q[k], rq[k], k)
        self.assertEqual(pack["id"], "persona")
        self.assertEqual(pack["version"], 0)

    def test_a_jev_probe_answer_is_parsed_best_effort(self):
        with tempfile.TemporaryDirectory() as d:
            f = Path(d) / "jev-probe"
            f.write_text("#!/bin/sh\necho 'persona: security 0.12'\necho 'persona: ops 0.81'\necho 'persona: other 0.04'\n")
            f.chmod(0o755)
            prog = gen.build(11, "smoke")
            a = pe.JevProbe(str(f), "role.v1", Path(d) / "raw")
            out = a(3, pe.inbound_input(prog, 3, "builder"))
            self.assertEqual(out["persona"], "ops")
            self.assertAlmostEqual(out["p"], 0.81)
            self.assertTrue((Path(d) / "raw" / "persona-003.input.json").is_file())


class Prefix(unittest.TestCase):
    def test_the_track_has_two_reopened_streams_a_sibling_and_header_probes(self):
        p = pf.build(11)
        tf.validate(p)
        self.assertEqual([s.id for s in p.streams], ["rail-a", "rail-b", "rail-sib"])
        reopen = [m for m in p.messages if m.reopen]
        self.assertEqual(sorted(m.stream for m in reopen), ["rail-a", "rail-b"])
        self.assertGreaterEqual(min(m.gap for m in reopen if m.stream == "rail-a"), 1000)  # a cold gap, over any TTL
        hdr = [q for q in p.probes if q.sibling]
        self.assertTrue(len(hdr) >= 3)
        for q in hdr:
            self.assertNotEqual(q.family, q.sibling)
            self.assertTrue(p.messages[q.msg].day == 1)  # asked after the cold gap
        # the sibling has the same layout and different values of the same kinds
        fam = {f.kind: f.value for f in p.facts if f.id.startswith("family.")}
        sib = {f.kind: f.value for f in p.facts if f.id.startswith("sibling.")}
        self.assertEqual(set(fam), set(sib))
        self.assertTrue(all(fam[k] != sib[k] for k in fam))

    def test_the_sibling_value_is_a_false_fit_counted_apart(self):
        p = pf.build(11)
        facts = p.facts_by_id()
        q = next(x for x in p.probes if x.sibling and x.kind == "in_direct")
        d = Path(tempfile.mkdtemp(prefix="tb-prefix-"))
        (d / "workspace").mkdir()
        turns = [{"index": m.index, "reply": "", "exit": 0} for m in p.messages]

        def row(reply):
            t = [dict(x) for x in turns]
            t[q.msg]["reply"] = reply
            return ts.score_probe(p, q, {"dir": d, "turns": t, "delivered": {}})

        right = row(f"It's {facts[q.fact].value}.")
        fit = row(f"It's {q.sibling}.")
        other = row("It's something else.")
        self.assertEqual((right["ok"], right["false_fit"]), (True, False))
        self.assertEqual((fit["ok"], fit["false_fit"]), (False, True))
        self.assertEqual((other["ok"], other["false_fit"]), (False, False))
        self.assertEqual(fit["contaminated"], ["rail-sib"])

    def test_the_ideal_effect_scores_the_streams(self):
        p = pf.build(11)
        import th_oracle as orc
        d = Path(tempfile.mkdtemp(prefix="tb-prefix-ideal-"))
        orc.materialize(p, d)
        for s in p.streams:
            self.assertTrue(all(ok for _, ok in pf.oracle(s.id, d)), s.id)
        bare = Path(tempfile.mkdtemp(prefix="tb-prefix-bare-"))
        for rel, f in p.workspace.items():
            pg.write_file(bare, pg.Edit(rel, f["content"], False))
        for s in p.streams:
            self.assertFalse(all(ok for _, ok in pf.oracle(s.id, bare)), s.id)


if __name__ == "__main__":
    unittest.main()
