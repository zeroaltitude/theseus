"""The rubric judge (its request, its parse, its retry, its cost, against the stand-in server) and the blind pairs."""

import json
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import blind as bl  # noqa: E402
import exam_standin as xs  # noqa: E402
import judge as jd  # noqa: E402

GOOD = json.dumps({"scores": {"voice": 4, "candour": 5, "register": 3, "warmth": 4, "initiative": 2},
                   "flattery": False, "notes": "fine"})


class Parse(unittest.TestCase):
    def test_a_fenced_and_prefixed_object_is_read(self):
        self.assertEqual(jd.parse("Here:\n```json\n" + GOOD + "\n```")["scores"]["voice"], 4)

    def test_bad_answers_are_refused(self):
        for bad in ("no json", "{}", '{"scores": {"voice": 5}}',
                    json.dumps({"scores": {d: 6 for d in jd.DIMS}}),
                    json.dumps({"scores": {d: 0 for d in jd.DIMS}}),
                    json.dumps({"scores": {d: True for d in jd.DIMS}}),
                    json.dumps({"scores": {**{d: 3 for d in jd.DIMS}, "extra": 3}})):
            with self.assertRaises((ValueError, json.JSONDecodeError), msg=bad):
                jd.parse(bad)

    def test_the_rubric_names_every_dimension_and_is_the_systems_prompt(self):
        text = jd.RUBRIC.read_text()
        for d in jd.DIMS:
            self.assertIn(f"**{d}**", text)
        self.assertIn(xs.JUDGE_MARK, jd.system_prompt())

    def test_the_prompt_hides_the_arm_and_shows_what_the_arm_was_given(self):
        given = jd.user_prompt("PERSONA TEXT", "CARD TEXT", ["earlier"], "hello", "reply!", True)
        self.assertIn("PERSONA TEXT", given)
        self.assertIn("CARD TEXT", given)
        self.assertIn("REPLY:\nreply!\nEND REPLY", given)
        none = jd.user_prompt("PERSONA TEXT", "CARD TEXT", [], "hello", "reply!", False)
        self.assertNotIn("PERSONA TEXT", none)
        self.assertIn("no persona", none)
        for arm in ("claude", "theseus", "arm"):
            self.assertNotIn(arm, given.lower().replace("persona", "").replace("assistant", ""))


class Calls(unittest.TestCase):
    def test_a_judge_that_answers_garbage_once_is_asked_again_and_a_dead_one_is_unscored(self):
        answers = iter(["not json", GOOD])
        j = jd.Judge(call=lambda *a: (next(answers), {"input_tokens": 1000, "output_tokens": 50}), key="k")
        r = j.score("p", "c", [], "t", "r")
        self.assertEqual(r["scores"]["warmth"], 4)
        self.assertEqual(j.calls, 2)
        self.assertGreater(j.spent, 0)
        dead = jd.Judge(call=lambda *a: ("nope", {}), key="k")
        self.assertIn("error", dead.score("p", "c", [], "t", "r"))

    def test_cost_uses_the_catalogs_prices(self):
        c = jd.cost("claude-sonnet-5-5", {"input_tokens": 1_000_000, "output_tokens": 1_000_000})
        self.assertAlmostEqual(c, 12.0)

    def test_against_the_stand_in_server_the_whole_request_path(self):
        si = xs.ExamStandIn([], {"A real answer."})
        try:
            j = jd.Judge(base_url=si.base, key="stand-in")
            good = j.score("p", "c", [], "hello", "A real answer.")
            bad = j.score("p", "c", [], "hello", "As an AI, I cannot say.")
        finally:
            si.close()
        self.assertEqual(set(good["scores"].values()), {5})
        self.assertEqual(set(bad["scores"].values()), {1})
        self.assertTrue(bad["flattery"])
        self.assertEqual(si.judge_calls, 2)
        self.assertGreater(good["cost_usd"], 0)


STEPS = [{"index": i, "kind": "probe", "family": f, "probe": f"{f}.{i}", "judge": j, "text": f"say {i}"}
         for i, (f, j) in enumerate([("identity", True), ("identity", False), ("candour", True), ("register", True),
                                     ("register", False), ("owner", False)])]


def res(prefix):
    return {s["index"]: {"reply": f"{prefix} {s['index']}"} for s in STEPS}


class Blind(unittest.TestCase):
    def test_pairs_are_spread_over_families_judged_first_and_differ(self):
        left, right = res("L"), res("R")
        right[5] = {"reply": "L 5"}  # identical: no pair
        left[3] = {"skipped": "budget"}
        pairs = bl.choose(STEPS, left, right, 20, 7)
        self.assertEqual(sorted(p["index"] for p in pairs), [0, 1, 2, 4])
        self.assertEqual(pairs[0]["family"], pairs[0]["family"])
        first_two = [p["family"] for p in bl.choose(STEPS, res("L"), res("R"), 3, 7)]
        self.assertEqual(len(set(first_two)), 3, "round robin over families")

    def test_the_sheet_hides_the_arms_and_the_key_has_them(self):
        sheet, key = bl.build({"steps": STEPS}, res("L"), res("R"), 4, 7)
        for arm in (bl.A_ARM, bl.B_ARM, "Theseus", "Claude"):
            self.assertNotIn(arm, sheet)
        self.assertEqual(set(key), {"01", "02", "03", "04"})
        for k, v in key.items():
            self.assertEqual({v["a"], v["b"]}, {bl.A_ARM, bl.B_ARM})
        self.assertTrue(any(v["a"] == bl.B_ARM for v in key.values()) and any(v["a"] == bl.A_ARM for v in key.values()),
                        "A and B are drawn per pair")
        left_text = sheet.split("**A**")[1]
        self.assertIn("> ", left_text)

    def test_answers_parse_and_refuse(self):
        key = {"01": {"a": bl.A_ARM, "b": bl.B_ARM}, "02": {"a": bl.B_ARM, "b": bl.A_ARM}}
        self.assertEqual(bl.parse_answers("01 a\n# c\n2 B  # why\n", key), {"01": "a", "02": "b"})
        for bad in ("01", "03 a", "01 c", "01 a\n01 b", "x a"):
            with self.assertRaises(ValueError, msg=bad):
                bl.parse_answers(bad, key)

    def test_the_tally_counts_the_arm_whose_reply_was_picked(self):
        key = {f"{i:02d}": {"a": bl.A_ARM if i % 2 else bl.B_ARM, "b": bl.B_ARM if i % 2 else bl.A_ARM} for i in range(1, 21)}
        # always pick the Theseus reply
        ans = {k: ("a" if v["a"] == bl.A_ARM else "b") for k, v in key.items()}
        t = bl.tally(key, ans)
        self.assertEqual(t["wins"], {bl.A_ARM: 20, bl.B_ARM: 0})
        self.assertLess(t["sign_p"], 0.001)
        self.assertGreater(t["lo"], 0.8)
        # a half-answered sheet with ties and passes
        part = {"01": "=", "02": "-", "03": ans["03"], "04": ans["04"]}
        t = bl.tally(key, part)
        self.assertEqual((t["ties"], t["passes"], t["decided"], t["unanswered"]), (1, 1, 2, 16))

    def test_the_sign_test(self):
        self.assertAlmostEqual(bl.binom_two_sided(10, 20), 1.0)
        self.assertAlmostEqual(bl.binom_two_sided(15, 20), 0.0414, places=3)
        self.assertAlmostEqual(bl.binom_two_sided(0, 4), 0.125)
        self.assertEqual(bl.binom_two_sided(0, 0), 1.0)


if __name__ == "__main__":
    unittest.main()
