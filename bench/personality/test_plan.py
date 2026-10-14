"""The plan: probes in TOML parametrized by the persona, every fact probed after it is stated, the six contexts, the
rotation, and the validator's refusals."""

import copy
import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import fixtures  # noqa: E402
import persona as pe  # noqa: E402
import plan as pl  # noqa: E402
import xchecks as xc  # noqa: E402


def blob(plan: pl.Plan) -> str:
    return json.dumps(plan.to_json(), ensure_ascii=False)


class Probes(unittest.TestCase):
    def test_every_probe_file_loads_and_each_family_is_there(self):
        probes, rot = pl.load_probes()
        pl.validate_probes(probes, rot)
        fams = {p.family for p in probes.values()}
        self.assertTrue({"identity", "owner", "correction", "candour", "register", "initiative"} <= fams)
        self.assertGreaterEqual(len(probes), 25)

    def test_a_probe_with_a_bad_check_is_refused(self):
        with tempfile.TemporaryDirectory() as t:
            Path(t, "x.toml").write_text('[[probe]]\nid = "identity.x"\nfamily = "identity"\ndim = "P1"\n'
                                         'turns = ["hi"]\nchecks = "max_words lots"\noracle = "a"\nwrong = "b"\n')
            with self.assertRaises(ValueError):
                pl.load_probes(Path(t))

    def test_a_ledger_check_must_say_so(self):
        with tempfile.TemporaryDirectory() as t:
            Path(t, "x.toml").write_text('[[probe]]\nid = "initiative.x"\nfamily = "initiative"\ndim = "P7"\n'
                                         'turns = ["hi"]\nchecks = "ledger no_wake"\noracle = "a"\nwrong = "b"\n')
            with self.assertRaisesRegex(ValueError, "needs"):
                pl.load_probes(Path(t))

    def test_the_oracle_of_every_probe_passes_its_checks_and_the_wrong_answer_fails(self):
        plan = pl.build(pe.load(None), 7, "full", profiles=["alt"])
        led = {"ledger": {"persona_digest": True, "wakes_new": 0}}
        n = 0
        for s in plan.probes():
            facts = {"tz_names": plan.persona["tz_names"], **led}
            calls = s.oracle_calls
            facts["ledger"] = {**led["ledger"], "wakes_new": len(calls)}
            good = xc.verdict(xc.run(s.checks, s.oracle, facts))
            self.assertTrue(good, f"{s.probe}: the oracle fails {xc.run(s.checks, s.oracle, facts)}")
            wf = {**facts, "ledger": {**led["ledger"], "wakes_new": len(s.wrong_calls)}}
            bad = xc.verdict(xc.run(s.checks, s.wrong, wf))
            if "ledger persona_digest" in s.checks and s.checks.count("\n") == 0:
                continue  # the manifest probe judges the harness, not the reply
            self.assertFalse(bad, f"{s.probe}: the wrong answer passes")
            n += 1
        self.assertGreater(n, 40)


class Parametrization(unittest.TestCase):
    def test_nothing_hard_codes_a_character(self):
        with tempfile.TemporaryDirectory() as t:
            other = fixtures.other_persona(Path(t))
            plan = pl.build(other, 7, "full")
            text = blob(plan)
            for word in ("Quill", "Rowan", "Phoenix", "Biscuit", "MST"):
                self.assertNotIn(word, text, f"{word} (the default persona's) is in the other persona's plan")
            for word in ("Marlin", "Sol", "Lisbon", "tea"):
                self.assertIn(word, text)
            self.assertEqual(plan.persona["name"], "Marlin")
            self.assertNotEqual(plan.persona_digest, pe.load(None).digest())

    def test_the_default_persona_fills_every_template(self):
        plan = pl.build(pe.load(None), 7, "full")
        self.assertNotIn("<<", blob(plan))

    def test_a_persona_file_replaces_the_rendered_text(self):
        with tempfile.TemporaryDirectory() as t:
            f = Path(t, "persona.md")
            f.write_text("# Voice\nI am the owner's own words.\n")
            toml = Path(t, "p.toml")
            toml.write_text(f'[persona]\nname = "Wren"\npersona_file = "{f}"\n')
            per = pe.load(toml)
            self.assertEqual(per.persona_text(), "# Voice\nI am the owner's own words.\n")
            self.assertEqual(per.name, "Wren")
            self.assertEqual(per.owner_call, "Rowan", "a field the file does not set keeps the default")
            self.assertNotEqual(per.digest(), pe.load(None).digest())
            with self.assertRaises(ValueError):
                toml.write_text('[persona]\npersona_file = "missing.md"\n')
                pe.load(toml)

    def test_a_persona_that_cannot_be_examined_is_refused(self):
        per = pe.load(None)
        bad = copy.deepcopy(per)
        bad.tz_names = []
        with self.assertRaises(ValueError):
            pe.validate(bad)
        bad = copy.deepcopy(per)
        bad.prefs = bad.prefs[:3]
        with self.assertRaisesRegex(ValueError, "kind"):
            pe.validate(bad)
        bad = copy.deepcopy(per)
        bad.prefs[0].asks = bad.prefs[0].asks[:4]
        with self.assertRaisesRegex(ValueError, "6 asks"):
            pe.validate(bad)

    def test_the_identity_probe_about_another_persona_needs_one(self):
        without, with_other = set(), set()
        for seed in range(1, 12):
            without |= {s.probe for s in pl.build(pe.load(None), seed, "full").steps if s.probe}
            plan = pl.build(pe.load(None, ["Harrow"]), seed, "full")
            with_other |= {s.probe for s in plan.steps if s.probe}
            for s in plan.steps:
                self.assertNotIn("<<other>>", s.text)
        self.assertFalse([i for i in without if i.startswith("identity.others")])
        self.assertTrue([i for i in with_other if i.startswith("identity.others")])
        text = json.dumps([s.text for s in pl.build(pe.load(None, ["Harrow"]), 3, "full").steps])
        if "identity.others" in text or "Harrow" in text:
            self.assertIn("Harrow", text)


class Preferences(unittest.TestCase):
    def test_each_preference_is_stated_once_then_asked_in_all_six_contexts_after(self):
        plan = pl.build(pe.load(None), 7, "full")
        stated = {s.pref: s.index for s in plan.steps if s.kind == "statement"}
        self.assertEqual(set(stated), set(pe.PREF_KINDS))
        for kind in pe.PREF_KINDS:
            asks = [s for s in plan.steps if s.kind == "probe" and s.pref == kind]
            self.assertEqual(sorted(s.ctx for s in asks), sorted(pl.CONTEXTS), kind)
            self.assertTrue(all(s.index > stated[kind] and s.stated_at == stated[kind] for s in asks))
            self.assertEqual(len({s.text for s in asks}), 6, "six different asks, none repeated")
            self.assertEqual(len([s for s in plan.steps if s.kind == "statement" and s.pref == kind]), 1)

    def test_the_contexts_are_really_different_places(self):
        plan = pl.build(pe.load(None), 7, "full")
        at = {c: next(s for s in plan.steps if s.pref == "brevity" and s.ctx == c) for c in pl.CONTEXTS}
        sess = {c: plan.sessions[s.session] for c, s in at.items()}
        self.assertEqual(at["same"].session, next(s for s in plan.steps if s.kind == "statement").session)
        self.assertEqual(at["compaction"].session, at["same"].session)
        fills = [s for s in plan.steps if s.kind == "fill" and at["same"].index < s.index < at["compaction"].index]
        self.assertTrue(fills and fills[-1].mark, "filler and a mark between the statement and the compaction ask")
        self.assertNotEqual(at["new_session"].session, at["same"].session)
        self.assertEqual(sess["project"].project, "borealis")
        self.assertEqual(sess["new_session"].project, "atlas")
        self.assertEqual(at["surface"].surface, "rpc")
        self.assertTrue(any(s.restart_before for s in plan.steps if s.session == at["later"].session))
        later_idx = at["later"].index
        self.assertTrue(any(s.kind == "distractor" and at["surface"].index < s.index < later_idx for s in plan.steps),
                        "distractor sessions between the statement and the late ask")

    def test_fills_are_sized_to_cross_the_window(self):
        plan = pl.build(pe.load(None), 7, "smoke", window=50_000)
        fills = [s for s in plan.steps if s.kind == "fill" and plan.sessions[s.session].kind == "prefs"]
        import tokens as tk
        total = sum(tk.user_text(s.text).tokens(tk.rates_of(pl.MODEL)) for s in fills)
        self.assertGreater(total, 50_000 - pl.DEFAULT_OVERHEAD)
        small = pl.build(pe.load(None), 7, "smoke", window=36_000)
        self.assertLess(len([s for s in small.steps if s.kind == "fill"]), len([s for s in plan.steps if s.kind == "fill"]))


class Shape(unittest.TestCase):
    def test_the_plan_is_deterministic_and_the_seed_rotates_it(self):
        a = pl.build(pe.load(None), 7, "full")
        b = pl.build(pe.load(None), 7, "full")
        c = pl.build(pe.load(None), 8, "full")
        self.assertEqual(a.digest(), b.digest())
        self.assertNotEqual(a.digest(), c.digest())
        self.assertNotEqual({s.probe for s in a.probes() if s.family != "preference"},
                            {s.probe for s in c.probes() if s.family != "preference"})

    def test_smoke_is_smaller_and_still_covers_every_family(self):
        smoke, full = pl.build(pe.load(None), 7, "smoke"), pl.build(pe.load(None), 7, "full")
        self.assertLess(len(smoke.probes()), len(full.probes()))
        self.assertEqual({s.family for s in smoke.probes()},
                         {"identity", "owner", "correction", "candour", "register", "initiative", "preference"})

    def test_the_surfaces_and_the_profiles_repeat_probes(self):
        plan = pl.build(pe.load(None), 7, "full", profiles=["alt", "big"])
        ids = {s.probe for s in plan.probes()}
        self.assertTrue(any(i.endswith("@rpc") for i in ids))
        self.assertTrue({"register.tired@alt", "register.tired@big"} <= ids)
        prof = [s for s in plan.probes() if s.profile == "big"]
        self.assertTrue(prof and all(s.surface == "cli" for s in prof))

    def test_a_families_filter(self):
        plan = pl.build(pe.load(None), 7, "full", families={"candour"})
        self.assertEqual({s.family for s in plan.probes()}, {"candour"})

    def test_the_ledger_probes_need_the_ledger(self):
        plan = pl.build(pe.load(None), 7, "full")
        for s in plan.probes():
            self.assertEqual(xc.uses_ledger(s.checks), "ledger" in s.needs, s.probe)
        self.assertTrue(any("ledger" in s.needs for s in plan.probes()))

    def test_only_keeps_a_probe_and_its_setup(self):
        plan = pl.build(pe.load(None), 7, "full")
        one = plan.only({"correction.pm"} if any(s.probe == "correction.pm" for s in plan.steps)
                        else {next(s.probe for s in plan.steps if s.family == "correction")})
        self.assertEqual([s.kind for s in one.steps], ["setup", "setup", "probe"])
        self.assertEqual([s.index for s in one.steps], [0, 1, 2])
        pl.validate(one, pe.load(None))


class Cost(unittest.TestCase):
    def test_the_estimate_grows_with_the_plan_and_is_inside_the_default_budget(self):
        per = pe.load(None)
        smoke = pl.estimate_cost(pl.build(per, 7, "smoke"), arms=4)
        full = pl.estimate_cost(pl.build(per, 7, "full", profiles=["alt"]), arms=4)
        self.assertLess(smoke["per_arm_usd"], full["per_arm_usd"])
        self.assertAlmostEqual(full["arms_usd"], 4 * full["per_arm_usd"], delta=0.05)
        self.assertLess(full["arms_usd"], 10.0, "four arms of the full exam fit the default budget")
        self.assertGreaterEqual(full["compactions"], 1, "the fills cross the window")
        pricey = pl.estimate_cost(pl.build(per, 7, "full"), model="claude-opus-5", arms=1)
        self.assertGreater(pricey["per_arm_usd"], pl.estimate_cost(pl.build(per, 7, "full"), arms=1)["per_arm_usd"])


class Refusals(unittest.TestCase):
    def setUp(self):
        self.per = pe.load(None)
        self.plan = pl.build(self.per, 7, "smoke")

    def test_a_preference_probed_before_it_is_stated(self):
        p = copy.deepcopy(self.plan)
        st = next(s for s in p.steps if s.kind == "statement" and s.pref == "brevity")
        ask = next(s for s in p.steps if s.kind == "probe" and s.pref == "brevity")
        i, j = p.steps.index(st), p.steps.index(ask)
        p.steps[i], p.steps[j] = ask, st
        for k, s in enumerate(p.steps):
            s.index = k
        for s in p.steps:
            if s.pref == "brevity" and s.kind == "probe":
                s.stated_at = st.index
        with self.assertRaisesRegex(ValueError, "before it is stated"):
            pl.validate(p, self.per)

    def test_a_preference_that_points_at_the_wrong_step(self):
        p = copy.deepcopy(self.plan)
        ask = next(s for s in p.steps if s.kind == "probe" and s.pref)
        ask.stated_at = next(s.index for s in p.steps if s.kind == "fill")
        with self.assertRaisesRegex(ValueError, "not stated"):
            pl.validate(p, self.per)

    def test_a_context_missing(self):
        p = copy.deepcopy(self.plan)
        p.steps = [s for s in p.steps if not (s.pref == "brevity" and s.ctx == "later")]
        for i, s in enumerate(p.steps):
            s.index = i
            if s.stated_at is not None:
                s.stated_at = next(x.index for x in p.steps if x.kind == "statement" and x.pref == s.pref)
        with self.assertRaisesRegex(ValueError, "contexts"):
            pl.validate(p, self.per)

    def test_a_checked_text_inside_another_message_and_a_text_asked_twice_differently(self):
        p = copy.deepcopy(self.plan)
        probe = next(s for s in p.steps if s.kind == "probe" and not s.pref)
        fill = next(s for s in p.steps if s.kind == "fill")
        fill.text = fill.text + "\n" + probe.text
        with self.assertRaisesRegex(ValueError, "inside another"):
            pl.validate(p, self.per)
        p = copy.deepcopy(self.plan)
        probes = [s for s in p.steps if s.kind == "probe"]
        probes[1].text = probes[0].text
        with self.assertRaisesRegex(ValueError, "twice"):
            pl.validate(p, self.per)

    def test_an_unfilled_template(self):
        p = copy.deepcopy(self.plan)
        p.steps[0].text = "Hello <<persona>>"
        with self.assertRaisesRegex(ValueError, "unfilled"):
            pl.validate(p, self.per)


if __name__ == "__main__":
    unittest.main()
