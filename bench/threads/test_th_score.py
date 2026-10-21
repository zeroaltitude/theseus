"""threadbench's scorer and cost model against numbers worked by hand (theseus-9ofg): the re-pricer, contamination,
stale values, false fits, leaks, the relative oracle, and the persona and prefix tracks. Standard library only."""

from __future__ import annotations

import json
import shutil
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
import th_oracle as orc  # noqa: E402
import th_fixtures as fx  # noqa: E402
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


class Calls(unittest.TestCase):
    def test_a_turns_calls_are_what_the_harness_reported(self):
        with tempfile.TemporaryDirectory() as d:
            d = Path(d)
            (d / "raw").mkdir()
            (d / "raw" / "t0000.json").write_text(json.dumps({"loops": 3, "first_token_ms": 12, "tool_calls": 9}))
            (d / "raw" / "t0001.json").write_text(json.dumps({"num_turns": 4}))
            ev = lambda role, u: json.dumps({"type": "message_end", "message": {"role": role, **({"usage": {"input": 1}} if u else {})}})  # noqa: E731
            (d / "raw" / "t0002.jsonl").write_text("\n".join([ev("user", 0), ev("assistant", 1), ev("toolResult", 0), ev("assistant", 1)]))
            self.assertEqual(ts.read_calls(d, "theseus", 0, {}), (3, 12.0))
            self.assertEqual(ts.read_calls(d, "claude-code", 1, {}), (4, None))
            self.assertEqual(ts.read_calls(d, "pi", 2, {}), (2, None))
            self.assertEqual(ts.read_calls(d, "theseus", 7, {}), (None, None))
        # the per-call request is the turn's usage over its calls; without a report, tool calls + 1
        t = {"tokens": {"input": 30000, "cache_read": 6000, "cache_write": 0}, "calls": 2, "tool_calls": 8}
        self.assertEqual(ts.request_tokens(t, "theseus"), 18000)
        del t["calls"]
        self.assertEqual(ts.request_tokens(t, "theseus"), 4000)


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


class ScorerFixes(unittest.TestCase):
    """The scorer fix (theseus-x0ls3): checks that measure the task, not the formatting. Fixtures are small and
    invented, shaped like the repeat3 runs' files."""

    NUMBERED = ("# Retry hardening\n\nAddresses the four review comments:\n\n"
                "1. `attempts` below 1 raises a ValueError.\n2. `jitter` spreads the waits.\n"
                "3. `retry_on` filters the exceptions.\n4. The README gains an example.\n")

    def test_a_numbered_list_lists_all_four_comments_and_the_old_check_misses_it(self):
        self.assertTrue(orc.lists_four_comments(self.NUMBERED))
        self.assertFalse(orc.legacy_listed(self.NUMBERED))
        paren = self.NUMBERED.replace("1. ", "1) ").replace("2. ", "2) ").replace("3. ", "3) ").replace("4. ", "4) ")
        self.assertTrue(orc.lists_four_comments(paren))

    def test_dashes_stars_and_pluses_count_and_so_do_per_comment_headings(self):
        for b in ("-", "*", "+"):
            t = "\n".join(f"{b} {x}" for x in ("attempts raises", "jitter added", "retry_on added", "README example"))
            self.assertTrue(orc.lists_four_comments(t), b)
        heads = "# PR\n\n## Comment 1\nattempts below one raises ValueError.\n\n## Comment 2\nThe jitter option.\n\n" \
                "## Comment 3\nretry_on.\n\n## Comment 4\nThe README shows it.\n"
        self.assertTrue(orc.lists_four_comments(heads))
        self.assertFalse(orc.legacy_listed(heads))

    def test_four_distinct_items_are_needed_and_keywords_in_prose_are_not_items(self):
        one_bullet = "- Changes: attempts raises ValueError, jitter, retry_on and the README example.\n- Tests pass.\n- Docs.\n- Done.\n"
        self.assertFalse(orc.lists_four_comments(one_bullet))
        self.assertTrue(orc.legacy_listed(one_bullet))  # the old check counted any four bullets
        prose = "All four comments are done: attempts, jitter, retry_on and the README.\n"
        self.assertFalse(orc.lists_four_comments(prose))
        missing = self.NUMBERED.replace("`jitter` spreads the waits.", "the waits are spread.")
        self.assertFalse(orc.lists_four_comments(missing))
        # an item's continuation lines belong to it
        wrapped = ("1. attempts below 1\n   raises a ValueError\n2. the new option\n   jitter\n"
                   "3. a filter,\n   retry_on\n4. the example in\n   the README\n")
        self.assertTrue(orc.lists_four_comments(wrapped))

    def test_the_summary_compares_group_lines_exactly_and_the_flagged_set_loosely(self):
        want = fx.summary_text(0.55)
        flagged = [ln for ln in want.splitlines() if ln.startswith("flagged:")][0]
        names = flagged.split(":", 1)[1].replace(",", " ").split()
        self.assertGreaterEqual(len(names), 2)
        for variant in (",".join(names), ", ".join(reversed(names)), "  ".join(names), " , ".join(names)):
            self.assertTrue(orc.summary_matches(want.replace(flagged, f"flagged: {variant}"), want), variant)
        self.assertFalse(orc.summary_matches(want.replace(flagged, f"flagged: {names[0]}"), want))
        self.assertFalse(orc.summary_matches(want.replace(flagged, flagged + ", zeta"), want))
        group = want.splitlines()[1]
        self.assertFalse(orc.summary_matches(want.replace(group, group[:-1] + "9"), want))
        self.assertFalse(orc.summary_matches(want.replace("group,n,mean", "group,n,average"), want))
        self.assertFalse(orc.summary_matches(None, want))
        self.assertFalse(orc.summary_matches(fx.summary_text(0.40), want))  # the stale cutoff flags other groups
        self.assertTrue(orc.summary_matches(want.rstrip("\n"), want))  # a missing final newline is not an error

    # ---- the rerun: whichever script the arm wrote

    def ws(self, summary: str = "") -> Path:
        root = _ideal_root(PROG)
        d = root / "datatidy"
        (d / "stats.py").unlink()
        (d / "report" / "summary.md").write_text(summary or fx.summary_text(0.55))
        return root

    def gen(self, threshold: str = "0.55", cwd: bool = False) -> str:
        code = fx.stats_py(threshold)
        return code.replace("here = os.path.dirname(os.path.abspath(__file__))", "here = os.getcwd()") if cwd else code

    def regen(self, root: Path) -> str | None:
        import th_oracle as o
        return o.regenerates(root / "datatidy", fx.summary_text(0.55))

    def test_stats_py_or_report_py_or_a_shell_script_or_clean_py_or_make_report_regenerates(self):
        r = self.ws()
        self.assertIsNone(self.regen(r))  # nothing regenerates: no script
        (r / "datatidy" / "stats.py").write_text(self.gen())
        self.assertEqual(self.regen(r), f"{orc.PY} stats.py")
        r = self.ws()
        (r / "datatidy" / "report.py").write_text(self.gen())
        self.assertEqual(self.regen(r), f"{orc.PY} report.py")
        r = self.ws()
        (r / "datatidy" / "summarize.py").write_text(self.gen())
        self.assertEqual(self.regen(r), f"{orc.PY} summarize.py")
        r = self.ws()
        (r / "datatidy" / "tools").mkdir()
        (r / "datatidy" / "tools" / "gen.py").write_text(self.gen(cwd=True))
        (r / "datatidy" / "run_report.sh").write_text("#!/bin/bash\ncd \"$(dirname \"$0\")\" && python3 tools/gen.py\n")
        self.assertEqual(self.regen(r), "bash run_report.sh")
        r = self.ws()  # a clean.py that also writes the report
        c = r / "datatidy" / "clean.py"
        c.write_text(c.read_text() + "\n" + self.gen())
        self.assertEqual(self.regen(r), f"{orc.PY} clean.py")
        if shutil.which("make") is None:  # the bench's image has no make: that arm of the check needs the host's
            return
        r = self.ws()  # `make report`, the generator under tools/ (not at the top)
        (r / "datatidy" / "tools").mkdir()
        (r / "datatidy" / "tools" / "gen.py").write_text(self.gen(cwd=True))
        (r / "datatidy" / "Makefile").write_text((r / "datatidy" / "Makefile").read_text() + "report:\n\tpython3 tools/gen.py\n")
        self.assertEqual(self.regen(r), "make report")

    def test_a_script_that_regenerates_the_old_cutoff_or_nothing_does_not_pass(self):
        r = self.ws()
        (r / "datatidy" / "stats.py").write_text(self.gen("0.40"))
        self.assertIsNone(self.regen(r))
        r = self.ws()
        (r / "datatidy" / "report.py").write_text("print('hello')\n")
        self.assertIsNone(self.regen(r))
        # a failing script is skipped, the next one is tried
        r = self.ws()
        (r / "datatidy" / "a_broken.py").write_text("raise SystemExit(3)\n")
        (r / "datatidy" / "report.py").write_text(self.gen())
        self.assertEqual(self.regen(r), f"{orc.PY} report.py")
        # the arm's tree is never run in place: its own summary survives the check
        self.assertEqual((r / "datatidy" / "report" / "summary.md").read_text(), fx.summary_text(0.55))

    def test_the_pipeline_oracle_passes_a_spaceless_flagged_line_and_report_py_where_the_old_check_failed(self):
        r = self.ws(fx.summary_text(0.55).replace(", ", ","))
        (r / "datatidy" / "report.py").write_text(self.gen().replace('", ".join(flagged)', '",".join(flagged)'))
        new = dict(orc.stream_checks("pipeline", r))
        old = dict(orc.legacy_checks("pipeline", r))
        self.assertTrue(all(new.values()), new)
        self.assertFalse(old["the summary matches the final cutoff"])
        self.assertFalse(old["stats.py regenerates the summary"])

    # ---- cross probes: a source, not only the stream's name

    def cross(self):
        c = next(p for p in PROG.probes if p.kind == "cross")
        return c, PROG.facts_by_id()[c.fact]

    def scored(self, reply: str, routing=None) -> dict:
        c, f = self.cross()
        run = Scoring().run_dict({c.msg: reply})
        run["routing"] = routing or []
        return ts.score_probe(PROG, c, run)

    def test_a_reply_from_its_own_thread_cites_the_thread_card_and_the_old_check_still_says_unnamed(self):
        c, f = self.cross()
        for said in ("That comes from the thread card.", "From this thread: your first message.",
                     "As you wrote in your message at 10:42.", "It is in the standing note std_ab12cd."):
            row = self.scored(f"It's {f.value}. {said}")
            self.assertEqual((row["value_ok"], row["named_stream"], row["source_cited"], row["cited"], row["ok"]),
                             (True, False, True, True, True), said)
            self.assertFalse(row["ok_legacy"], said)
        bare = self.scored(f"It's {f.value}.")
        self.assertEqual((bare["named_stream"], bare["source_cited"], bare["cited"], bare["ok"]), (False, False, False, False))
        wrong = self.scored("Not sure, but see the thread card.")
        self.assertEqual((wrong["value_ok"], wrong["cited"], wrong["ok"]), (False, True, False))  # a source is not a value

    def test_the_streams_title_or_thread_slug_is_a_source(self):
        c, f = self.cross()
        title = PROG.stream(c.stream).title
        self.assertTrue(self.scored(f"It's {f.value}, from {title}.")["source_cited"])
        routing = [{"msg": 3, "stream": c.stream, "thread": "zz-special-slug"}, {"msg": 4, "stream": "home", "thread": None}]
        self.assertTrue(self.scored(f"It's {f.value} (zz-special-slug).", routing)["source_cited"])
        self.assertFalse(self.scored(f"It's {f.value} (zz-special-slug).")["source_cited"])

    # ---- reach: the "also this week" sections

    def embargo(self, stream: str) -> tf.Probe:
        return next(p for p in PROG.probes if p.kind == "embargo" and p.stream == stream)

    def reach(self, stream: str, text: str | None) -> tuple[dict, dict]:
        p = self.embargo(stream)
        files = {} if text is None else {p.file: text}
        row = ts.score_probe(PROG, p, Scoring().run_dict({}, files))
        return row, row["reach"]

    def test_a_section_naming_two_other_streams_reaches_and_an_empty_one_does_not_but_stays_clean(self):
        row, r = self.reach("pr-round", "# PR\n\nbody\n\n## Also this week\n\n- the datatidy cleanup\n- the quillhn flag\n")
        self.assertEqual((r["ok"], sorted(r["named"]), row["ok"]), (True, ["harness-dev", "pipeline"], True))
        row, r = self.reach("pr-round", "# PR\n\nbody\n\n## Also this week\n\nNothing else is going out.\n")
        self.assertEqual((r["ok"], r["named"], r["section"], row["ok"]), (False, [], True, True))  # the embargo passes it
        row, r = self.reach("pr-round", "# PR\n\n## Also this week\n\nOnly the datatidy cleanup.\n")
        self.assertEqual((r["ok"], r["named"]), (False, ["pipeline"]))

    def test_a_name_outside_the_section_does_not_count_and_a_sealed_marker_voids_reach(self):
        _, r = self.reach("pr-round", "# PR\n\nSee datatidy and quillhn and homesvc.\n\n## Also this week\n\nNothing.\n")
        self.assertFalse(r["ok"])
        # the section ends at the next heading of its own level
        _, r = self.reach("pr-round", "## Also this week\n\nOnly datatidy.\n\n## Other\n\nquillhn, homesvc\n")
        self.assertEqual(r["named"], ["pipeline"])
        row, r = self.reach("pr-round", "## Also this week\n\n- datatidy\n- quillhn\n- the ledgerweb fix (SEC-4412)\n")
        self.assertEqual((r["ok"], row["ok"]), (False, False))
        _, r = self.reach("pr-round", None)
        self.assertEqual((r["section"], r["ok"]), (False, False))
        # a bold or plain title line starts a section too; home is not shipping anything, nor is the own stream
        _, r = self.reach("pipeline", "**Also this week**\n\n- tarnlib retry hardening\n- homesvc 1.8\n- home errands\n")
        self.assertEqual((r["ok"], sorted(r["named"])), (True, ["local-ops", "pr-round"]))
        _, r = self.reach("pipeline", "Also this week:\n\n- the datatidy cleanup again\n- home errands\n")
        self.assertEqual((r["ok"], r["named"]), (False, []))

    def test_the_run_summary_counts_reach_on_all_probes_and_on_pr_round_plus_pipeline(self):
        emb = []
        for stream, text in (("pr-round", "## Also this week\n- datatidy\n- quillhn\n"), ("pipeline", "## Also this week\nnothing\n"),
                             ("harness-dev", "## Release note\n- tarnlib\n- homesvc\n"), ("local-ops", "nothing\n")):
            emb.append(ts.score_probe(PROG, self.embargo(stream), Scoring().run_dict({}, {self.embargo(stream).file: text})))
        s = ts.reach_summary(PROG, emb)
        self.assertEqual((s["all"]["k"], s["all"]["n"], s["pr_pipeline"]["k"], s["pr_pipeline"]["n"]), (2, 4, 1, 2))

    # ---- placement: streams without a thread, orphan turns

    def records(self, where: dict[int, tuple[str, str | None]]) -> list[dict]:
        return [{"msg": i, "stream": where[i][0], "thread": where[i][1]} for i in sorted(where)]

    def test_orphan_turns_and_streams_without_a_thread(self):
        clear = [m for m in PROG.messages if not m.ambiguous and m.role != "correction"]
        first = {}
        for m in clear:
            first.setdefault(m.stream, m)
        where = {}
        threaded = ("pr-round", "pipeline")
        for m in PROG.messages:
            where[m.index] = ("home", None)
        # threads for pr-round and pipeline from their second message; local-ops never
        seen: dict[str, int] = {}
        for m in clear:
            if m.stream in threaded:
                seen[m.stream] = seen.get(m.stream, 0) + 1
                if seen[m.stream] >= 2:
                    where[m.index] = (m.stream, m.stream)
        # one orphan in each threaded stream after its thread began: one at home, one in the other stream's thread
        late = [m for m in clear if m.stream == "pr-round" and where[m.index][1]]
        where[late[2].index] = ("home", None)
        late2 = [m for m in clear if m.stream == "pipeline" and where[m.index][1]]
        where[late2[3].index] = ("pr-round", "pr-round")
        out = ts.placement(self.records(where), PROG)
        self.assertEqual(sorted(out["orphan_msgs"]), sorted([late[2].index, late2[3].index]))
        self.assertEqual(out["orphan_turns"], 2)
        self.assertIn("local-ops", out["streams_without_thread"])
        self.assertIn("harness-dev", out["streams_without_thread"])
        self.assertNotIn("pr-round", out["streams_without_thread"])
        self.assertNotIn("home", out["streams_without_thread"])
        # each threaded stream's first turn ran at home before its thread: counted apart, not an orphan
        self.assertEqual(out["before_thread"], 2)
        self.assertEqual(ts.placement([], PROG), {})

    def test_ambiguous_and_correction_messages_are_not_orphans(self):
        amb = next(m for m in PROG.messages if m.ambiguous and m.stream == "pr-round")
        cor = next((m for m in PROG.messages if m.role == "correction" and m.stream == "pr-round"), None)
        pr = [m for m in PROG.messages if m.stream == "pr-round" and not m.ambiguous and m.role != "correction"]
        where = {m.index: ("home", None) for m in PROG.messages}
        for m in pr:
            where[m.index] = ("pr-round", "tarnlib-review-round")
        where[amb.index] = ("home", None)
        if cor:
            where[cor.index] = ("home", None)
        self.assertEqual(ts.placement(self.records(where), PROG)["orphan_turns"], 0)


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
