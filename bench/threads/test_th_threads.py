"""The threads arms' unit tests (theseus-l7z1): the arm table, the simulated owner, the scratch config per arm, the
routing record, the ask loop against a stand-in `theseus`, the scorer's reading of what a daemon reported, and the
after run's attempt plan. The scratch-daemon runs are in test_th_smoke.py. Standard library only.
"""

from __future__ import annotations

import json
import stat
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
sys.path.insert(0, str(HERE))

import th_drive as td  # noqa: E402
import th_generate as gen  # noqa: E402
import th_owner as owner  # noqa: E402
import th_score as ts  # noqa: E402
import th_threads as th  # noqa: E402

# What `theseus ask` says on stderr when the router asks (crates/theseus/src/thread/ask.rs `lines`, `command`).
QUESTION = "❓ Is this about the pr round or the security fix?"
OPTIONS = "   [1] pr round   [2] security fix   [h]ome"
ANSWER = "   answer: theseus thread answer ask_7 1|2|home"
OFFER_Q = "❓ Start a new thread «ci checks» for this?"
OFFER_OPTIONS = "   [n]ew New thread «ci checks»   [h]ome"
OFFER_ANSWER = "   answer: theseus thread answer ask_8 new|home"


class Table(unittest.TestCase):
    def test_the_threads_arms_are_drivable_on_the_designs_names_and_a1p_says_why_not(self):
        names = {a: td.ARMS[a]["name"] for a in ("A1", "A1n", "A1p", "A2")}
        self.assertEqual(names, {"A1": "Theseus with threads, natural addressing",
                                 "A1n": "Theseus with threads, no prefix library",
                                 "A1p": "Theseus with threads, a prompt layer per persona",
                                 "A2": "Theseus with threads, explicit addressing"})
        for a in ("A1", "A1n", "A1p", "A2"):
            self.assertEqual(td.ARMS[a]["layout"], "thread")
        self.assertEqual(td.MEASURABLE, ("A1", "A1n", "A2", "A1o", "A3", "A4", "A5", "A6", "A7"))
        with self.assertRaises(td.NotMeasurable) as cm:
            td.main(["--arm", "A1p", "--out", "/tmp/never"])
        self.assertIn("not measurable", str(cm.exception))
        self.assertIn("no switch", str(cm.exception))

    def test_the_other_arms_are_the_tables_as_before(self):
        self.assertEqual([(a, td.ARMS[a]["layout"], td.ARMS[a]["key"]) for a in ("A3", "A4", "A5", "A6", "A7")],
                         [("A3", "one", "theseus"), ("A4", "one", "claude-code"), ("A5", "stream", "theseus-batching"),
                          ("A6", "stream", "openclaw"), ("A7", "one", "pi")])


class Owner(unittest.TestCase):
    def offer(self, i: int = 8) -> dict:
        return owner.parse_ask(OFFER_Q, OFFER_OPTIONS, OFFER_ANSWER.replace("ask_8", f"ask_{i}"))

    def ask(self) -> dict:
        return owner.parse_ask(QUESTION, OPTIONS, ANSWER)

    def test_the_cli_lines_parse_into_the_question(self):
        a = self.ask()
        self.assertEqual((a["id"], a["keys"], a["offer"]), ("ask_7", ["1", "2", "home"], False))
        self.assertEqual(a["labels"], {"1": "pr round", "2": "security fix", "home": ""})
        o = self.offer()
        self.assertEqual((o["id"], o["keys"], o["offer"]), ("ask_8", ["new", "home"], True))
        self.assertEqual(o["labels"]["new"], "New thread «ci checks»")
        with self.assertRaises(ValueError):
            owner.parse_ask(QUESTION, OPTIONS, "   answer: nothing")

    def test_the_policy_on_a_scripted_sequence(self):
        o = owner.Owner()
        # a stream's first message: its offer is accepted
        self.assertTrue(o.first_of_stream("pr-round"))
        self.assertEqual(o.answer("pr-round", self.offer(1)), "new")
        o.title["pr-round"] = "pr round"
        o.sent("pr-round")
        # a later message of the stream: an offer is declined
        self.assertFalse(o.first_of_stream("pr-round"))
        self.assertEqual(o.answer("pr-round", self.offer(2)), "home")
        # an offer on a home message: declined, and counted
        self.assertEqual(o.answer("home", self.offer(3)), "home")
        self.assertEqual(o.over_split, 1)
        # a routing question: the message's true stream's option, whichever key it has
        self.assertEqual(o.answer("pr-round", self.ask()), "1")
        o.title["sec-fix"] = "security fix"
        self.assertEqual(o.answer("sec-fix", self.ask()), "2")
        # a home message asked about goes home; a stream with no option yet and begun goes home too
        self.assertEqual(o.answer("home", self.ask()), "home")
        o.sent("sec-fix")
        o.title.pop("sec-fix")
        self.assertEqual(o.answer("sec-fix", self.ask()), "home")
        self.assertEqual([x["choice"] for x in o.answers], ["new", "home", "home", "1", "2", "home", "home"])

    def test_a_question_that_lists_another_thread_is_answered_with_the_streams_slug(self):
        # theseus-98bz.4: over five live threads the CLI lists three and `[o]ther`, and a slug answers it.
        q = "❓ Is this about the pr round, the security fix, the ci checks or another thread?"
        opts = "   [1] pr round   [2] security fix   [3] ci checks   [o]ther (a thread's slug)   [h]ome"
        ask = owner.parse_ask(q, opts, "   answer: theseus thread answer ask_9 1|2|3|<slug>|home")
        self.assertEqual(ask["keys"], ["1", "2", "3", "<slug>", "home"])
        self.assertEqual(ask["labels"]["other"], "(a thread's slug)")
        o = owner.Owner()
        o.title["sec-fix"], o.slug["data-pipe"] = "security fix", "data-pipe"
        o.title["data-pipe"] = "data pipe"
        o.sent("sec-fix")
        o.sent("data-pipe")
        self.assertEqual(o.answer("sec-fix", ask), "2", "a listed thread by its key")
        self.assertEqual(o.answer("data-pipe", ask), "data-pipe", "an unlisted one by its slug")
        self.assertEqual(o.answer("home", ask), "home")

    def test_the_owner_starts_a_thread_by_hand_only_when_the_first_message_ran_home_and_no_offer_came(self):
        o = owner.Owner()
        self.assertTrue(o.needs_thread("sec-fix", True, None))
        self.assertFalse(o.needs_thread("sec-fix", False, None))
        self.assertFalse(o.needs_thread("sec-fix", True, "sec-fix"))
        self.assertFalse(o.needs_thread("home", True, None))
        o.title["sec-fix"] = "x"
        self.assertFalse(o.needs_thread("sec-fix", True, None))

    def test_the_policy_reads_no_label_but_the_stream(self):
        import inspect
        src = inspect.getsource(owner)
        for word in ("probe", "oracle", "fact", "persona", "ambiguous", "digest"):
            self.assertNotIn(f".{word}", src, word)


class Config(unittest.TestCase):
    BASE = {"model": {"model": "m"}, "kernel": {"spend_limit_usd": 5.0}}

    def cfg(self, arm: str) -> dict:
        with tempfile.TemporaryDirectory() as d:
            return th.arm_config(self.BASE, arm, gen.build(11, "smoke").personas, Path(d) / "personas")

    def test_a1n_turns_the_prefix_library_off_and_a1_leaves_it_at_its_default(self):
        self.assertEqual(self.cfg("A1n")["prefix"], {"enabled": False})
        self.assertNotIn("prefix", self.cfg("A1"))
        self.assertNotIn("prefix", self.cfg("A2"))

    def test_every_threads_arm_gets_the_five_persona_cards_and_threads_stay_at_the_defaults(self):
        for arm in ("A1", "A1n", "A2"):
            c = self.cfg(arm)
            self.assertEqual(sorted(c["personas"]), sorted(gen.build(11, "smoke").personas))
            self.assertTrue(all(p["card"] and len(p["files"]) == 1 for p in c["personas"].values()))
            self.assertNotIn("threads", c)  # route_wait_ms, persona_engage: the shipped defaults
        self.assertEqual(self.BASE, {"model": {"model": "m"}, "kernel": {"spend_limit_usd": 5.0}})

    def test_the_overlay_is_laid_over_table_by_table(self):
        self.assertEqual(th.merge({"a": {"x": 1, "y": 2}, "b": 3}, {"a": {"y": 5}, "c": {"z": 1}}),
                         {"a": {"x": 1, "y": 5}, "b": 3, "c": {"z": 1}})

    def test_the_config_writes_as_toml(self):
        import tomllib
        import drive as rd
        self.assertEqual(tomllib.loads(rd.toml_dumps(self.cfg("A1n")))["prefix"], {"enabled": False})


class Record(unittest.TestCase):
    M = gen.build(11, "smoke").messages[0]

    def test_the_record_is_the_scorers_three_fields_then_what_the_daemon_said(self):
        tag = {"slug": "pr-x", "title": "pr round", "how": "quiet", "confidence": 0.93}
        r = th.record(self.M, tag, {"pr-x": "pr-round"}, True, None, {"timed_out": False, "jev": {"pick": "pr-x"}},
                      {"persona": "builder", "name": "Builder", "how": "jev", "p": 0.8, "x": 1},
                      [{"choice": "reuse", "fit": 0.9, "below_act": False, "header": "h"}])
        self.assertEqual((r["msg"], r["stream"], r["asked"], r["thread"], r["how"]), (0, "pr-round", True, "pr-x", "quiet"))
        self.assertEqual(r["persona"], {"persona": "builder", "name": "Builder", "how": "jev", "p": 0.8})
        self.assertEqual(r["prefix"], [{"choice": "reuse", "fit": 0.9, "below_act": False}])
        self.assertEqual(r["jev"], "pr-x")

    def test_no_tag_is_home_and_an_addressed_message_is_its_own_stream(self):
        self.assertEqual(th.record(self.M, None, {}, False, None, None, None, [])["stream"], "home")
        r = th.record(self.M, None, {}, False, "sec-fix", None, None, [])
        self.assertEqual((r["stream"], r["thread"], r["addressed"]), ("sec-fix", "sec-fix", True))

    def test_a_thread_the_owner_never_named_is_marked_as_unknown_not_guessed(self):
        r = th.record(self.M, {"slug": "odd"}, {}, False, None, None, None, [])
        self.assertEqual(r["stream"], "?odd")


FAKE_THESEUS = r'''#!/usr/bin/env python3
import sys, time
from pathlib import Path
flag = Path(sys.argv[0] + ".answered")
for line in sys.stdin: pass
sys.stderr.write("%(q)s\n%(o)s\n%(a)s\n"); sys.stderr.flush()
for _ in range(100):
    if flag.exists(): break
    time.sleep(0.05)
print('{"output": "ok", "thread": {"slug": "pr-x"}}' if flag.exists() else "{}")
'''


class Ask(unittest.TestCase):
    def fake(self, d: Path, body: str = FAKE_THESEUS) -> Path:
        f = d / "theseus"
        f.write_text(body % {"q": QUESTION, "o": OPTIONS, "a": ANSWER})
        f.chmod(f.stat().st_mode | stat.S_IEXEC)
        return f

    def test_a_question_seen_on_stderr_is_answered_while_the_turn_waits(self):
        with tempfile.TemporaryDirectory() as d:
            f = self.fake(Path(d))
            got = []

            def on_ask(a: dict) -> None:
                got.append(a)
                Path(str(f) + ".answered").write_text("1")

            out, err, code, seen = th.run_ask([str(f)], "why is the build red", 30, on_ask, {})
            self.assertEqual(code, 0)
            self.assertEqual(json.loads(out)["thread"]["slug"], "pr-x")
            self.assertEqual([a["id"] for a in got], ["ask_7"])
            self.assertEqual(seen, got)
            self.assertIn("Is this about", err)

    def test_a_turn_that_runs_past_its_limit_is_killed_and_says_so(self):
        with tempfile.TemporaryDirectory() as d:
            f = self.fake(Path(d))
            out, err, code, seen = th.run_ask([str(f)], "x", 0.5, lambda a: None, {})
            self.assertIsNone(code)
            self.assertIn("timed out", err)


class Scorer(unittest.TestCase):
    P = gen.build(11, "smoke")

    def test_a2_is_the_owners_own_routing_like_a5_and_a6(self):
        self.assertEqual(ts.OWNER_ROUTED, ("A2", "A5", "A6"))

    def test_the_persona_and_prefix_reports_read_only_records_that_carry_them(self):
        self.assertEqual(ts.engagement([{"msg": 0, "stream": "home", "asked": False}], self.P), {})
        m = self.P.messages
        recs = [{"msg": 0, "persona": {"persona": m[0].persona, "how": "jev"}, "prefix": [{"choice": "reuse", "below_act": False}]},
                {"msg": 1, "persona": {"persona": "nobody", "how": "prior"},
                 "prefix": [{"choice": "mint", "below_act": True}, {"choice": "reuse", "below_act": False}]}]
        e = ts.engagement(recs, self.P)
        self.assertEqual((e["persona"]["engaged"]["k"], e["persona"]["engaged"]["n"]), (1, 2))
        self.assertEqual(e["persona"]["how"], {"jev": 1, "prior": 1})
        self.assertEqual(e["prefix"], {"picks": 3, "by_choice": {"mint": 1, "reuse": 2}, "below_act": 1})


class Plan(unittest.TestCase):
    T12 = REPO / "bench" / "plans" / "threadbench-t12.json"
    T0R = REPO / "bench" / "plans" / "threadbench-t0r.json"

    def test_the_after_plan_names_the_threads_arms_and_t0rs_progressions(self):
        sys.path.insert(0, str(td.HARBOR))
        import plan as hp
        plan = json.loads(self.T12.read_text())
        hp.check(plan, "threadbench-t12")
        self.assertEqual(plan["arms"], ["A1", "A1n", "A2", "A3"])
        self.assertEqual(plan["probe_only"], ["A1p"])
        before = json.loads(self.T0R.read_text())["progressions"]
        import th_prefix
        for name, v in plan["progressions"].items():
            self.assertEqual((v["seed"], v["digest"]), (before[name]["seed"], before[name]["digest"]), name)
            built = th_prefix.build(v["seed"]) if name == "prefix" else gen.build(v["seed"], "standard")
            self.assertEqual(built.digest(), v["digest"], f"regenerate the plan's {name} digest")
        self.assertEqual((plan["budget_usd"]["expected_low"], plan["budget_usd"]["expected_high"],
                          plan["budget_usd"]["pause_with_before_run"], plan["spend_limit_usd_per_arm"]), (35, 60, 170, 25))
        self.assertEqual((plan["model"], plan["effort"], plan["size"]), ("anthropic/claude-sonnet-5-5", "medium", "standard"))

    def test_each_plan_refuses_the_arm_it_does_not_name(self):
        from unittest import mock
        sys.path.insert(0, str(td.HARBOR))
        import plan as hp
        dev = gen.build(gen.DEV_SEED, "standard")
        for path, ok_arms, bad in ((self.T12, ("A1", "A1n", "A2", "A3"), "A4"), (self.T0R, ("A3", "A5"), "A1")):
            plan = {**json.loads(path.read_text()), "file": path.name, "sha256": "x", "commit": "c", "head": "h"}
            with mock.patch.object(hp, "load", return_value=plan):
                for arm in ok_arms:
                    self.assertEqual(td.check_plan(path, arm, dev)["commit"], "c")
                with self.assertRaises(SystemExit) as cm:
                    td.check_plan(path, bad, dev)
                self.assertIn(f"does not name arm {bad}", str(cm.exception))

    def test_the_driver_refuses_an_unnamed_arm_before_it_starts_anything(self):
        from unittest import mock
        sys.path.insert(0, str(td.HARBOR))
        import plan as hp
        plan = {**json.loads(self.T12.read_text()), "file": "bench/plans/threadbench-t12.json", "sha256": "x",
                "commit": "c", "head": "h"}
        with tempfile.TemporaryDirectory() as d, mock.patch.object(hp, "load", return_value=plan):
            with self.assertRaises(SystemExit) as cm:
                td.main(["--plan", str(self.T12), "--arm", "A4", "--out", str(Path(d) / "run")])
            self.assertIn("does not name arm A4", str(cm.exception))
            self.assertFalse((Path(d) / "run").exists())


if __name__ == "__main__":
    unittest.main()
