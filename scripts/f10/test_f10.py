"""F10's tests (theseus-qy2a): each check against the fixture screens (yes on
Claude Code's, the baseline's answer on Theseus's), the scorecard's totals,
the stand-in's rules, the scratch project, and the driver's tmux isolation.

    python3 -m unittest discover -s scripts/f10 -p 'test_*.py'
"""

import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import checks  # noqa: E402
import scorecard  # noqa: E402
import steps  # noqa: E402
import tmuxio  # noqa: E402

FIX = HERE / "fixtures"

# The Theseus baseline: each check's answer on main's screens, as the
# driver's run at 120x40 recorded them (fixtures/theseus-120x40).
THESEUS_YES = {"S1d", "S3a", "S3b", "S3d", "S6c", "S6d", "S7c", "S8a", "S8c", "S10b", "S11a",
               "B1", "B2", "B3", "B4", "B5"}


def load(name):
    return checks.Run.load(FIX / name)


class ClaudeCodeFixture(unittest.TestCase):
    """Claude Code's screens pass every match check and no beat check."""

    def setUp(self):
        self.results = checks.judge(load("claude-code-120x40"))

    def test_each_match_check_says_yes(self):
        for cid in steps.check_ids()[: steps.MATCH_CHECKS]:
            with self.subTest(cid):
                ok, evidence = self.results[cid]
                self.assertTrue(ok, f"{cid}: {evidence}")
                self.assertTrue(evidence)

    def test_each_beat_check_says_no(self):
        for cid in steps.check_ids()[steps.MATCH_CHECKS:]:
            with self.subTest(cid):
                self.assertFalse(self.results[cid][0], f"{cid}: {self.results[cid][1]}")


class TheseusFixture(unittest.TestCase):
    """Theseus's recorded screens answer as the baseline did."""

    def setUp(self):
        self.results = checks.judge(load("theseus-120x40"))

    def test_each_check_answers_as_the_baseline(self):
        for cid in steps.check_ids():
            with self.subTest(cid):
                ok, evidence = self.results[cid]
                self.assertEqual(ok, cid in THESEUS_YES, f"{cid}: {evidence}")
                self.assertNotIn("check error", evidence)

    def test_the_totals(self):
        t = scorecard.totals(self.results)
        self.assertEqual(t["match"], len([c for c in THESEUS_YES if c.startswith("S")]))
        self.assertEqual(t["beat"], len([c for c in THESEUS_YES if c.startswith("B")]))


class Scorecard(unittest.TestCase):
    def test_totals_count_each_step_and_the_beat_checks_apart(self):
        ids = steps.check_ids()
        self.assertEqual(len(ids), steps.MATCH_CHECKS + steps.BEAT_CHECKS)
        results = {c: (False, "") for c in ids}
        for c in ("S1a", "S1d", "S7c", "S12c", "B1", "B5"):
            results[c] = (True, "")
        t = scorecard.totals(results)
        self.assertEqual(t["match"], 4)
        self.assertEqual(t["beat"], 2)
        self.assertEqual(t["by_step"]["S1"], 2)
        self.assertEqual(t["by_step"]["S12"], 1)
        self.assertEqual(t["by_step"]["S2"], 0)
        self.assertEqual(sum(t["by_step"].values()), t["match"])

    def test_all_yes_is_42_and_5(self):
        t = scorecard.totals({c: (True, "") for c in steps.check_ids()})
        self.assertEqual((t["match"], t["beat"]), (42, 5))

    def test_the_markdown_names_every_check_and_both_totals(self):
        runs = [load("claude-code-120x40"), load("theseus-120x40")]
        scored = scorecard.score(runs)
        md = scorecard.markdown(runs, scored)
        for cid in steps.check_ids():
            self.assertIn(f"- {cid} ", md)
        self.assertIn("| **Match** | of 42 | **42** | **11** |", md)
        self.assertIn("| **Beat** | of 5 | **0** | **5** |", md)

    def test_a_check_that_raises_is_a_no_with_its_reason(self):
        run = load("theseus-120x40")
        saved = checks.CHECKS["S1a"]
        checks.CHECKS["S1a"] = lambda r: 1 / 0
        try:
            ok, evidence = checks.judge(run)["S1a"]
        finally:
            checks.CHECKS["S1a"] = saved
        self.assertFalse(ok)
        self.assertIn("ZeroDivisionError", evidence)


class Checks(unittest.TestCase):
    """The rules' edges, on screens built here."""

    def run_of(self, screens, records=None, size="80x24"):
        meta = {"arm": "t", "size": size, "project_dir": "/p/project", "model": "claude-sonnet-5-5", "records": records or {}}
        return checks.Run(meta, screens)

    def test_a_word_cut_at_the_edge_is_found(self):
        cut = "x" * 70 + " harbour w"  # 80 columns, ending inside "water"
        run = self.run_of({"S2.reply": f"{steps.PROMPTS['S2']}\n{cut}\nater rises\n"})
        ok, ev = checks.s2d(run)
        self.assertFalse(ok, ev)
        wrapped = "x" * 70 + " harbour"
        run = self.run_of({"S2.reply": f"{steps.PROMPTS['S2']}\n{wrapped}\nwater rises\n"})
        self.assertTrue(checks.s2d(run)[0])

    def test_raw_markdown_fails_rendering(self):
        run = self.run_of({"S2.reply": f"{steps.PROMPTS['S2']}\n# Port Wenlow tides\nA \x1b[1msmall\x1b[22m one\n"})
        self.assertFalse(checks.s2a(run)[0])

    def test_a_tool_shown_by_name_alone_fails_s2b(self):
        run = self.run_of({"S2.reply": f"{steps.PROMPTS['S2']}\n  fs.read\n  tides.py is read\n"})
        ok, ev = checks.s2b(run)
        self.assertFalse(ok)
        self.assertIn("fs.read", ev)

    def test_s4_needs_two_lines_not_the_calls_json(self):
        js = '{"old_string": "    return round(lowest - highest, 2)", "new_string": "    return round(highest - lowest, 2)"}'
        self.assertFalse(checks.s4a(self.run_of({"S4.ask": js}))[0])

    def test_s8c_finds_a_reorder(self):
        log = steps.survey_log()
        swapped = log[:10] + [log[11], log[10]] + log[12:]
        run = self.run_of({}, {"S8": {"messages": ["\n".join(swapped)]}})
        ok, ev = checks.s8c(run)
        self.assertFalse(ok)
        self.assertIn("line 12", ev)

    def test_model_names_read_both_ways(self):
        self.assertIn("Opus 5.5", checks.model_names("claude-opus-5-5"))


class StandIn(unittest.TestCase):
    def test_each_prompt_takes_its_own_rule(self):
        rules = steps.stand_in_rules("/p")
        first = lambda text: next(r for r in rules if r["when"] in text)
        for key, text in steps.PROMPTS.items():
            with self.subTest(key):
                self.assertEqual(first(text)["when"], text, f"{key} took another step's rule")

    def test_the_reply_is_markdown_that_wraps(self):
        self.assertIn("# ", steps.S2_REPLY)
        self.assertIn("**", steps.S2_REPLY)
        self.assertTrue(any(len(l) > 120 for l in steps.S2_REPLY.splitlines()))

    def test_the_log_has_sixty_numbered_lines(self):
        log = steps.survey_log()
        self.assertEqual(len(log), 60)
        self.assertEqual([int(l[:2]) for l in log], list(range(1, 61)))


class Project(unittest.TestCase):
    """The scratch project: its tests fail on the bug and pass on S4's fix."""

    def test_the_bug_fails_and_the_fix_passes(self):
        with tempfile.TemporaryDirectory() as d:
            p = Path(d) / "project"
            shutil.copytree(HERE / "project", p, ignore=shutil.ignore_patterns("__pycache__"))
            self.assertEqual(sorted(x.name for x in p.iterdir()), ["README.md", "slow_survey.py", "test_tides.py", "tides.py"])
            run = lambda: subprocess.run([sys.executable, "-m", "unittest", "test_tides"], cwd=p, capture_output=True, text=True)
            r = run()
            self.assertNotEqual(r.returncode, 0)
            self.assertIn("FAIL: test_tidal_range", r.stderr)
            src = (p / "tides.py").read_text()
            self.assertEqual(src.splitlines()[steps.BUG_LINE - 1], steps.BUG_OLD)
            self.assertEqual(src.count(steps.S9_OLD), 1)
            (p / "tides.py").write_text(src.replace(steps.BUG_OLD, steps.BUG_NEW))
            self.assertEqual(run().returncode, 0)


@unittest.skipUnless(shutil.which("tmux"), "no tmux")
class TmuxIsolation(unittest.TestCase):
    """The driver's server is its own: never the default socket, never a
    tmux.conf."""

    def test_the_name_is_private(self):
        self.assertEqual(tmuxio.socket_name(4242), "f10-4242")
        self.assertNotEqual(tmuxio.socket_name(), "default")
        with self.assertRaises(AssertionError):
            tmuxio.Tmux.__init__(object.__new__(tmuxio.Tmux), 80, 24, "/", name="default")

    def test_every_command_names_the_private_server(self):
        uid = os.getuid()
        tmpdir = Path(os.environ.get("TMUX_TMPDIR", "/tmp")) / f"tmux-{uid}"
        default_before = (tmpdir / "default").exists()
        t = tmuxio.Tmux(80, 24, "/", name=f"f10-test-{os.getpid()}")
        try:
            t.type("echo f10-isolated")
            t.keys("Enter")
            self.assertTrue(t.wait(lambda s: "f10-isolated" in s.split("echo")[-1], 5))
            self.assertTrue((tmpdir / t.name).exists())
        finally:
            t.kill()
        self.assertTrue(t.calls)
        for argv in t.calls:
            self.assertEqual(argv[1:5], ["-L", t.name, "-f", "/dev/null"], argv)
        self.assertEqual((tmpdir / "default").exists(), default_before, "the default socket was touched")


if __name__ == "__main__":
    unittest.main()
