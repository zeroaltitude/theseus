"""The exam against real daemons and the stand-in model: a few steps, so it takes seconds. Skipped where the debug
binaries (`cargo build -p theseusd -p theseus -p theseus-index`) are not built. The full stand-in run, all four arms
and both answer modes, is `exam.py standin` (README): it takes minutes.

Set PX_TEST_CLAUDE=1 to add the Claude Code arms (needs `claude` on PATH; the stand-in answers it too).
"""

import json
import os
import shutil
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import exam as ex  # noqa: E402
import exam_standin as xs  # noqa: E402
import persona as pe  # noqa: E402
import plan as pl  # noqa: E402

BIN = HERE.parent.parent / "target" / "debug"
HAVE = all((BIN / b).is_file() for b in ("theseus", "theseusd", "theseus-index"))


def small_plan(per):
    full = pl.build(per, 7, "full")
    want = {"identity.manifest", "initiative.promise", "initiative.nopromise"}
    have = {s.probe for s in full.probes()} & want
    return full.only(have), have


@unittest.skipUnless(HAVE, "build the debug binaries first")
class OnADaemon(unittest.TestCase):
    def run_mode(self, mode, arms):
        per = pe.load(None)
        plan, have = small_plan(per)
        self.assertEqual(have, {"identity.manifest", "initiative.promise", "initiative.nopromise"})
        si = xs.ExamStandIn(xs.rules_for(plan.steps, mode), {s.oracle for s in plan.steps if s.oracle})
        with tempfile.TemporaryDirectory(prefix="px-e2e-") as t:
            a = ex.run_namespace(["--out", t, "--bin-dir", str(BIN), "--api-base", si.base, "--budget", "1e9"])
            try:
                ex.run_arms(a, plan, per, arms, Path(t) / "run")
            finally:
                si.close()
            sc = ex.score_run(Path(t) / "run")
            out = {arm: {p["probe"]: p["ok"] for p in v["cell"]["probes"]} for arm, v in sc["arms"].items()}
            runs = {arm: json.loads(Path(t, "run", arm, "run.json").read_text()) for arm in arms}
            left = [r.get("left_running") for r in runs.values()]
        return out, runs, left

    def test_the_manifest_and_the_wakes_are_read_from_the_daemons_own_record(self):
        out, runs, left = self.run_mode("oracle", ["theseus-persona", "theseus"])
        self.assertEqual(out["theseus-persona"], {"identity.manifest": True, "initiative.promise": True,
                                                   "initiative.nopromise": True})
        self.assertEqual(out["theseus"]["identity.manifest"], False, "no persona, no digest in the manifest")
        self.assertTrue(out["theseus"]["initiative.promise"], "the wake is the daemon's, persona or not")
        self.assertEqual(left, [[], []], "nothing left running")
        self.assertTrue(runs["theseus-persona"]["has_persona"] and not runs["theseus"]["has_persona"])

    def test_a_promise_without_a_wake_fails_and_a_wake_unasked_fails(self):
        out, _, _ = self.run_mode("wrong", ["theseus-persona"])
        self.assertFalse(out["theseus-persona"]["initiative.promise"], "words with no wake behind them")
        self.assertFalse(out["theseus-persona"]["initiative.nopromise"], "a wake nobody asked for")
        self.assertTrue(out["theseus-persona"]["identity.manifest"], "the harness, not the reply")


if __name__ == "__main__":
    unittest.main()


@unittest.skipUnless(HAVE and (BIN / "theseus-sim").is_file(), "build the debug binaries first")
class OnDiscord(unittest.TestCase):
    def test_a_dm_in_the_fake_discord_is_the_same_prompt_on_another_surface(self):
        per = pe.load(None)
        full = pl.build(per, 7, "full")
        plan = full.only({"identity.name@discord", "register.terse@discord", "identity.name"})
        self.assertEqual({s.surface for s in plan.steps}, {"cli", "discord"})
        si = xs.ExamStandIn(xs.rules_for(plan.steps, "oracle"), set())
        with tempfile.TemporaryDirectory(prefix="px-e2e-") as t:
            a = ex.run_namespace(["--out", t, "--bin-dir", str(BIN), "--api-base", si.base, "--budget", "1e9"])
            try:
                ex.run_arms(a, plan, per, ["theseus-persona"], Path(t) / "run")
            finally:
                si.close()
            res = ex.ed.results_of(Path(t) / "run" / "theseus-persona" / "results.jsonl")
            sc = ex.score_run(Path(t) / "run")
        by_surface = {}
        for s in plan.steps:
            by_surface.setdefault(s.surface, []).append(res[s.index])
        for r in by_surface["discord"]:
            self.assertEqual(r["exit"], 0, r)
            self.assertNotIn("-# ", r["reply"], "the footer is the harness's, split from the reply")
            self.assertIn("-# ", r["footer"])
            self.assertGreater(r["cost_usd"], 0, "the dollars come from the footer")
        self.assertEqual(sorted(r["reply"] for r in by_surface["discord"]), ["I'm Quill.", "ls -lS"])
        self.assertTrue(all(p["ok"] for p in sc["arms"]["theseus-persona"]["cell"]["probes"]))
        self.assertEqual(sc["arms"]["theseus-persona"]["cell"]["families"]["surfaces"]["n"], 2)
