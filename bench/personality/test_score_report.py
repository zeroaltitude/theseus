"""Scoring a cell from results, the arms' bookkeeping, and the report renders: all in-process, no daemon, no model."""

import json
import os
import socket
import sys
import tempfile
import threading
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import blind as bl  # noqa: E402
import exam as ex  # noqa: E402
import exam_drive as ed  # noqa: E402
import exam_score as es  # noqa: E402
import fixtures  # noqa: E402
import persona as pe  # noqa: E402
import plan as pl  # noqa: E402
import report as rp  # noqa: E402


def plan_and_tz(size="full", profiles=("alt",)):
    per = pe.load(None)
    return pl.build(per, 7, size, profiles=list(profiles)), per.tz_names


def cell(plan, tz, mode, **kw):
    return es.score_cell(plan, fixtures.fake_results(plan, mode, **kw), tz, [])


class Scoring(unittest.TestCase):
    def test_oracle_scores_all_and_wrong_scores_none_of_the_reply_checks(self):
        plan, tz = plan_and_tz()
        good = cell(plan, tz, "oracle")
        self.assertEqual(good["overall"]["k"], good["overall"]["n"])
        self.assertGreater(good["overall"]["n"], 50)
        bad = cell(plan, tz, "wrong")
        reply = [p for p in bad["probes"] if not any(ln[0].startswith("ledger") for ln in p["lines"])]
        self.assertTrue(reply and all(p["ok"] is False for p in reply))
        self.assertEqual(bad["overall"]["k"], 1, "only the manifest, which judges the harness, passes")

    def test_every_check_line_of_every_probe_is_scored_both_ways(self):
        plan, tz = plan_and_tz()
        for mode, want in (("oracle", True), ("wrong", False)):
            for p in cell(plan, tz, mode)["probes"]:
                if mode == "wrong" and p["probe"].startswith("identity.manifest"):
                    continue
                self.assertEqual(p["ok"], want, (mode, p["probe"], p["lines"]))

    def test_every_kind_of_check_in_the_plan_passes_on_an_oracle_and_fails_on_a_wrong_answer(self):
        """The stand-in's promise at the level of a check line: for each kind of line the plan uses, some probe's
        oracle satisfies it and some probe's planted wrong answer does not."""
        plan, tz = plan_and_tz()
        kinds_pass, kinds_fail = set(), set()
        for mode, sink, want in (("oracle", kinds_pass, True), ("wrong", kinds_fail, False)):
            for p in cell(plan, tz, mode)["probes"]:
                for line, v in p["lines"]:
                    word = line.split()[0]
                    kind = " ".join(line.split()[:2]) if word in ("ledger", "reply", "last_line") else word
                    if v is want:
                        sink.add(kind)
        used = {" ".join(ln.split()[:2]) if ln.split()[0] in ("ledger", "reply", "last_line") else ln.split()[0]
                for s in plan.probes() for ln in s.checks.splitlines() if ln.strip()}
        self.assertEqual(used - kinds_pass, set(), "a kind of line no oracle satisfies")
        self.assertEqual(used - kinds_fail - {"ledger persona_digest"}, set(),
                         "a kind of line no wrong answer fails")
        for want in ("prefix", "max_words", "min_words", "max_sentences", "no_table", "no_bullets", "tz_used",
                     "last_line has", "reply has", "reply lacks", "ledger wake_created"):
            self.assertIn(want, used, f"the plan uses {want}")

    def test_families_include_the_surfaces_and_the_profiles_apart(self):
        plan, tz = plan_and_tz()
        fams = cell(plan, tz, "oracle")["families"]
        self.assertTrue({"identity", "owner", "preference", "surfaces", "models", "candour"} <= set(fams))
        self.assertEqual(fams["models"]["n"], 3)

    def test_a_failed_turn_is_a_failure_and_a_skipped_one_is_not_scored(self):
        plan, tz = plan_and_tz("smoke", ())
        res = fixtures.fake_results(plan, "oracle")
        probes = plan.probes()
        res[probes[0].index] = {"index": probes[0].index, "reply": "", "exit": 1, "error": "boom"}
        res[probes[1].index] = {"index": probes[1].index, "skipped": "budget"}
        del res[probes[2].index]
        c = es.score_cell(plan, res, tz, [])
        by = {p["probe"]: p for p in c["probes"]}
        self.assertIs(by[probes[0].probe]["ok"], False)
        self.assertEqual(by[probes[0].probe]["error"], "boom")
        self.assertIsNone(by[probes[1].probe]["ok"])
        self.assertTrue(by[probes[1].probe]["unreached"] and by[probes[2].probe]["unreached"])
        self.assertEqual(c["overall"]["unreached"], 2)
        self.assertEqual(c["overall"]["n"], len(probes) - 2)

    def test_an_arm_without_a_ledger_has_those_lines_not_applicable(self):
        plan, tz = plan_and_tz("full", ())
        res = fixtures.fake_results(plan, "oracle")
        for r in res.values():
            r.pop("ledger", None)
        c = es.score_cell(plan, res, tz, [])
        ledger_only = [s.probe for s in plan.probes() if s.checks.strip().startswith("ledger") and "\n" not in s.checks]
        self.assertTrue(ledger_only, "the plan holds probes that judge the daemon's record alone")
        by = {p["probe"]: p for p in c["probes"]}
        for pid in ledger_only:
            self.assertIsNone(by[pid]["ok"], pid)
        mixed = next(p for p in c["probes"] if p["probe"].startswith("initiative.") and len(p["lines"]) == 2)
        self.assertEqual([v for _, v in mixed["lines"]][0], None)
        self.assertTrue(mixed["ok"], "the reply line decides when the ledger line cannot")
        self.assertEqual(c["overall"]["k"], c["overall"]["n"])

    def test_generic_excludes_what_needs_a_persona_or_card(self):
        plan, tz = plan_and_tz()
        c = cell(plan, tz, "oracle")
        self.assertLess(c["overall"]["generic"]["n"], c["overall"]["n"])
        needs = {s.probe: set(s.needs) for s in plan.probes()}
        for p in c["probes"]:
            self.assertEqual(p["generic"], not ({"persona", "card"} & needs[p["probe"]]), p["probe"])
        self.assertFalse([p for p in c["probes"] if p["generic"] and p["probe"].startswith("identity.name")])
        self.assertTrue([p for p in c["probes"] if p["generic"] and p["family"] == "candour"])

    def test_the_preference_matrix_and_whether_the_compaction_was_forced(self):
        plan, tz = plan_and_tz("smoke", ())
        res = fixtures.fake_results(plan, "oracle")
        ask = next(s for s in plan.steps if s.pref == "brevity" and s.ctx == "compaction")
        res[ask.index]["reply"] = "One. Two. Three. Four."  # breaks the brevity preference after the compaction
        mark = next(s.index for s in plan.steps if s.mark and s.index < ask.index and s.index > ask.stated_at)
        c = es.score_cell(plan, res, tz, [mark])
        m = c["preferences"]["brevity"]
        self.assertFalse(m["compaction"]["ok"])
        self.assertTrue(m["compaction"]["forced"])
        self.assertTrue(m["same"]["ok"] and m["later"]["ok"])
        unforced = es.score_cell(plan, res, tz, [])["preferences"]["brevity"]["compaction"]["forced"]
        self.assertFalse(unforced)
        self.assertEqual(set(c["contexts"]), set(pl.CONTEXTS))

    def test_the_judge_means_are_by_family(self):
        plan, tz = plan_and_tz("smoke", ())
        judged_step = next(s for s in plan.probes() if s.judge)
        j = {judged_step.index: {"scores": {d: 4 for d in ("voice", "candour", "register", "warmth", "initiative")},
                                 "flattery": True}, 999: {"error": "x"}}
        c = es.score_cell(plan, fixtures.fake_results(plan, "oracle"), tz, [], j)["judge"]
        self.assertEqual(c["n"], 1)
        self.assertEqual(c["errors"], 1)
        self.assertEqual(list(c["by_family"].values())[0]["voice"], 4)
        self.assertEqual(list(c["flattery"].values())[0], 1.0)


class Arms(unittest.TestCase):
    def test_profiles_parse(self):
        self.assertEqual(ed.parse_profiles("a=anthropic/x,b=y"), {"a": "anthropic/x", "b": "y"})
        self.assertEqual(ed.parse_profiles(""), {})
        with self.assertRaises(ValueError):
            ed.parse_profiles("nomodel")

    def test_a_persona_key_is_a_toml_key(self):
        self.assertEqual(ed.slug("Quill & Co."), "quill-co")
        self.assertEqual(ed.slug("!!"), "persona")

    def test_rpc_call_reads_past_notifications_to_the_response(self):
        with tempfile.TemporaryDirectory() as t:
            path = Path(t, "sock")
            srv = socket.socket(socket.AF_UNIX)
            srv.bind(str(path))
            srv.listen(1)
            seen = {}

            def serve():
                c, _ = srv.accept()
                buf = b""
                while b"\n" not in buf:
                    buf += c.recv(4096)
                seen["req"] = json.loads(buf)
                c.sendall(b'{"jsonrpc":"2.0","method":"turn.delta","params":{}}\n')
                c.sendall(json.dumps({"jsonrpc": "2.0", "id": 1, "result": {"output": "hi"}}).encode() + b"\n")
                c.close()
            th = threading.Thread(target=serve)
            th.start()
            out = ed.rpc_call(path, "turn.submit", {"input": "x"}, 5)
            th.join()
            srv.close()
        self.assertEqual(out, {"output": "hi"})
        self.assertEqual(seen["req"]["method"], "turn.submit")
        self.assertEqual(seen["req"]["params"], {"input": "x"})

    def test_rpc_errors_raise(self):
        with tempfile.TemporaryDirectory() as t:
            path = Path(t, "sock")
            srv = socket.socket(socket.AF_UNIX)
            srv.bind(str(path))
            srv.listen(1)

            def serve():
                c, _ = srv.accept()
                c.recv(4096)
                c.sendall(b'{"jsonrpc":"2.0","id":1,"error":{"code":-1,"message":"no"}}\n')
                c.close()
            th = threading.Thread(target=serve)
            th.start()
            with self.assertRaisesRegex(RuntimeError, "no"):
                ed.rpc_call(path, "x", {}, 5)
            th.join()
            srv.close()

    def test_a_run_dir_refuses_a_used_directory(self):
        with tempfile.TemporaryDirectory() as t:
            ed.RunDir(Path(t, "a"), 1000).results_f.close()
            with self.assertRaises(SystemExit):
                ed.RunDir(Path(t, "a"), 1000)

    def test_the_stand_in_rules_cover_every_probe_and_the_longest_comes_first(self):
        import exam_standin as xs
        plan, _ = plan_and_tz()
        rules = xs.rules_for(plan.steps, "oracle")
        self.assertEqual({r["when"] for r in rules}, {s.text for s in plan.probes()})
        self.assertEqual([len(r["when"]) for r in rules], sorted((len(r["when"]) for r in rules), reverse=True))
        wake = next(r for r in rules if r["calls"])
        self.assertEqual(wake["calls"][0]["name"], "wake_at")
        with self.assertRaises(ValueError):
            xs.rules_for(plan.steps, "neither")


def scores_for(modes):
    plan, tz = plan_and_tz("full", ())
    per = pe.load(None)
    arms = {}
    for arm, mode in modes.items():
        c = es.score_cell(plan, fixtures.fake_results(plan, mode), tz, [])
        arms[arm] = {"cell": c, "run": {"spent_usd": 1.23, "steps_done": len(plan.steps), "compactions": [40, 70]}}
    return {"plan": {"digest": plan.digest(), "seed": 7, "size": "full", "window": plan.window, "steps": len(plan.steps),
                     "probes": len(plan.probes())},
            "run": {"model": "anthropic/claude-sonnet-5-5", "effort": "medium", "persona": {"name": per.name,
                    "owner_call": per.owner_call, "tz": per.tz}, "persona_digest": per.digest(),
                    "persona_source": "built-in", "budget_usd": 10.0, "judge_model": "claude-sonnet-5-5"},
            "arms": arms, "blind": None, "judge": {"model": "claude-sonnet-5-5", "calls": 12, "spent_usd": 0.4}}


class Report(unittest.TestCase):
    def test_the_report_renders_with_its_figures_data_and_csv(self):
        sc = scores_for({"theseus": "wrong", "theseus-persona": "oracle", "claude-code": "wrong",
                         "claude-code-fair": "oracle"})
        with tempfile.TemporaryDirectory() as t:
            paths = rp.make(sc, "2026-10-14", "unit", Path(t))
            names = sorted(p.name for p in paths)
            self.assertIn("2026-10-14-personality-unit.md", names)
            self.assertIn("2026-10-14-personality-unit.json", names)
            self.assertIn("2026-10-14-personality-unit.csv", names)
            svgs = [n for n in names if n.endswith(".svg")]
            self.assertTrue({"families.svg", "families-dark.svg", "preference-contexts.svg",
                             "preference-contexts-dark.svg", "judge.svg"} <= set(svgs) or len(svgs) >= 4, svgs)
            md = Path(t, "2026-10-14-personality-unit.md").read_text()
            self.assertIn("> **The answer.** On ", md)
            self.assertNotIn("_Answer it here", md)
            self.assertNotIn("One paragraph", md)
            for h in ("## The question", "## The setup", "## Results", "## Threats to validity", "## What it cost",
                      "## Reproduction", "### Scores by family", "### A preference stated once"):
                self.assertIn(h, md)
            self.assertIn("The persona moved Theseus on", md)
            self.assertIn("behind", md)
            data = json.loads(Path(t, "2026-10-14-personality-unit.json").read_text())
            self.assertEqual(data["suite"], "personality")
            self.assertEqual([a["key"] for a in data["arms"]], list(ex.ed.ARMS))
            rows = Path(t, "2026-10-14-personality-unit.csv").read_text().splitlines()
            self.assertEqual(rows[0].split(",")[:3], ["arm", "probe", "family"])
            self.assertGreater(len(rows), 100)
            self.assertTrue(all("reply" not in r for r in [rows[0]]), "no transcript in the csv")

    def test_the_answer_says_when_the_persona_moved_nothing_and_when_nobody_is_behind(self):
        sc = scores_for({"theseus": "oracle", "theseus-persona": "oracle", "claude-code-fair": "oracle"})
        self.assertIn("moved no family by ten points", rp.answer(sc))
        self.assertIn("behind on no family", rp.answer(sc))

    def test_the_blind_result_is_in_the_answer_when_it_is_answered(self):
        sc = scores_for({"theseus-persona": "oracle", "claude-code-fair": "wrong"})
        sc["blind"] = bl.tally({f"{i:02d}": {"a": bl.A_ARM, "b": bl.B_ARM} for i in range(1, 21)},
                               {f"{i:02d}": "a" for i in range(1, 21)})
        self.assertIn("20 of 20 pairs answered", rp.answer(sc))
        self.assertIn("Not answered", rp.blind_text(None))

    def test_a_report_is_not_overwritten(self):
        sc = scores_for({"theseus-persona": "oracle"})
        with tempfile.TemporaryDirectory() as t:
            rp.make(sc, "2026-10-14", "a", Path(t))
            p = Path(t, "2026-10-14-personality-a.md")
            p.write_text("the maintainer's prose")
            rp.make(sc, "2026-10-14", "a", Path(t))
            self.assertEqual(p.read_text(), "the maintainer's prose")
            rp.make(sc, "2026-10-14", "a", Path(t), force=True)
            self.assertNotEqual(p.read_text(), "the maintainer's prose")

    def test_the_figure_specs_pass_the_house_checks(self):
        import draft
        sc = scores_for({"theseus": "wrong", "theseus-persona": "oracle", "claude-code": "wrong",
                         "claude-code-fair": "oracle"})
        for spec in rp.figures(sc):
            draft.charts.check(spec)
            self.assertIn("<svg", draft.charts.render(spec, "light"))
            self.assertIn("<svg", draft.charts.render(spec, "dark"))


if __name__ == "__main__":
    unittest.main()


class DiscordFooter(unittest.TestCase):
    def test_the_footer_is_split_from_the_reply_and_gives_the_dollars(self):
        import exam_discord as dc
        body, foot, cost = dc.split_footer("Right, done.\n— Quill\n-# bench · claude-sonnet-5-5 · 2 loops · $0.0650 total ($0.0325 this reply) · 0.1 s")
        self.assertEqual(body, "Right, done.\n— Quill")
        self.assertTrue(foot.startswith("-# bench"))
        self.assertEqual(cost, 0.0325)
        self.assertEqual(dc.split_footer("plain"), ("plain", "", None))

    def test_the_config_patch_binds_the_dm_and_makes_its_owner_an_owner(self):
        import exam_discord as dc
        cfg = dc.config_patch({}, "127.0.0.1:1", "127.0.0.1:2")
        self.assertEqual(cfg["discord"]["rest_proxy"], "127.0.0.1:1")
        self.assertEqual(cfg["discord"]["gateway_proxy"], "ws://127.0.0.1:2")
        self.assertEqual(cfg["places"]["owner"], [f"discord:{dc.OWNER_ID}"])
        self.assertTrue(cfg["secrets"]["discord_bot_token"].startswith("env:"))
        self.assertIn(f'user = "{dc.OWNER_ID}"', dc.bindings_toml())
