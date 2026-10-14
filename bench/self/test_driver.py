"""The driver: the sealed set's reading rules, that no gap's text reaches a sealed report, the budget stop, the
arms' commands and config, and (with a build) one smoke on the stand-in model (theseus-pw1q.6).

    python3 -m unittest discover -s bench/self
    SELF_E2E_BIN=$PWD/target/debug python3 -m unittest test_driver.EndToEnd       # (from bench/self) a real daemon
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import gaps  # noqa: E402
import selfdriver as drv  # noqa: E402
import selfreport  # noqa: E402

SENTINEL = "zq-quokka-7731"


def sealed_set(root: Path, n: int = 12) -> Path:
    """A sealed directory of `n` gaps, one per family, whose every text carries the sentinel."""
    base = gaps.OPEN_DIR / "open-wordcount"
    sealed = root / gaps.SEALED_SUBDIR
    for i, fam in enumerate(list(gaps.FAMILIES)[:n]):
        d = sealed / f"{SENTINEL}-dir-{i:02d}"
        shutil.copytree(base, d)
        j = json.loads((d / "gap.json").read_text())
        j.update(id=f"{SENTINEL}-id-{i}", family=fam, title=f"{SENTINEL} title", signals=[f"{SENTINEL}_fn"])
        (d / "gap.json").write_text(json.dumps(j))
        (d / "report.md").write_text(f"The {SENTINEL} widget is wrong.")
    return root


def args(**kw) -> argparse.Namespace:
    d = dict(set="open", sealed_dir=None, arm="theseus", budget=5.0, per_gap=None, wait=1.0, poll=0.1,
             bin_dir="/nonexistent", standin=None, claude=None, allow_version_drift=False, landed=None, gap=None,
             out=None, date="2026-10-13", keep=False)
    d.update(kw)
    return argparse.Namespace(**d)


class Sealed(unittest.TestCase):
    def test_no_sealed_read_unless_the_directory_is_set(self):
        with mock.patch.dict(os.environ, {}, clear=False):
            os.environ.pop(gaps.SEALED_ENV, None)
            with self.assertRaises(gaps.GapError) as e:
                gaps.read_sealed(None)
            self.assertIn("neither is set", str(e.exception))
            self.assertIsNone(gaps.sealed_root(None))

    def test_an_open_run_never_reads_the_sealed_directory_even_when_it_is_set(self):
        with tempfile.TemporaryDirectory() as t, mock.patch.dict(os.environ, {gaps.SEALED_ENV: t}):
            with mock.patch.object(gaps, "read_sealed", side_effect=AssertionError("sealed read")):
                got = drv.select(args(set="open"))
        self.assertEqual(sorted(g.id for g in got), ["open-bucket", "open-rounding", "open-wordcount"])
        self.assertFalse(any(g.sealed for g in got))

    def test_the_environment_names_the_sealed_directory_and_the_gaps_are_labelled_by_place(self):
        with tempfile.TemporaryDirectory() as t:
            sealed_set(Path(t))
            with mock.patch.dict(os.environ, {gaps.SEALED_ENV: t}):
                got = gaps.read_sealed(None)
        self.assertEqual([g.label for g in got], [f"sealed-{i:02d}" for i in range(1, 13)])
        self.assertTrue(all(g.sealed for g in got))
        self.assertEqual(sorted(g.family for g in got), sorted(gaps.FAMILIES))

    def test_an_incomplete_sealed_set_is_refused_unless_a_gap_is_named(self):
        with tempfile.TemporaryDirectory() as t:
            sealed_set(Path(t), 5)
            with self.assertRaises(gaps.GapError):
                gaps.read_sealed(t)
            self.assertEqual(len(gaps.read_sealed(t, require_full=False)), 5)

    def test_no_sealed_text_reaches_the_report_or_the_data(self):
        with tempfile.TemporaryDirectory() as t, tempfile.TemporaryDirectory() as out:
            sealed_set(Path(t))
            glist = gaps.read_sealed(t)

            def fake(gap, work, a, cap, landed=None):
                # a scorer-shaped result whose free text names the gap's own files, tests and branch
                return {"found": True, "fixed": True, "proved": True, "no_regress": True, "no_erosion": False,
                        "shortcut": False, "verified": True, "cost_usd": 0.1, "na": {},
                        "branch": f"self/{SENTINEL}-fix", "head": "abc",
                        "erosion": [f"tests/test_{SENTINEL}.py:3: test-removed: {SENTINEL} -> ",
                                    f"tests/test_x.py:9: assert-removed: 3 -> 2"],
                        "notes": [f"{SENTINEL} said something", "no test was added or changed"], "items": 1}
            a = args(set="sealed", sealed_dir=t, arm="theseus", landed="backlog,pr")
            with mock.patch.object(drv, "run_theseus", side_effect=fake):
                data = drv.run_set(a, glist)
            report = selfreport.write(data, Path(out)).read_text()
            blob = report + (Path(out) / "scores.json").read_text()
        self.assertNotIn(SENTINEL, blob)
        self.assertIn("sealed-01", report)
        self.assertIn("sealed-12", report)
        rec = data["arms"]["theseus"]["scores"][0]
        self.assertNotIn("branch", rec)
        self.assertEqual(rec["erosion"], ["assert-removed", "test-removed"])
        self.assertEqual(rec["notes"], ["no test was added or changed"])

    def test_keep_is_refused_on_the_sealed_set(self):
        with self.assertRaises(SystemExit):
            drv.main(["run", "--set", "sealed", "--keep", "--sealed-dir", "/x"])


class BudgetStop(unittest.TestCase):
    def test_the_line_is_split_between_the_arms_and_a_gap_cap_is_a_share(self):
        b = drv.Budget(5.0, ["theseus", "claude-code"])
        self.assertEqual(b.line, 2.5)
        self.assertEqual(b.cap("theseus", 12), round(2 * 2.5 / 12, 6))
        b.book("theseus", 2.45)
        self.assertEqual(b.cap("theseus", 3), 0.05)
        self.assertFalse(b.stopped("claude-code"))
        b.book("theseus", 0.2)
        self.assertTrue(b.stopped("theseus"))
        self.assertEqual(b.cap("theseus", 3), 0.0)
        self.assertEqual(b.total_spent, 2.65)
        self.assertEqual(drv.Budget(5.0, ["theseus"], per_gap=0.3).cap("theseus", 12), 0.3)
        with self.assertRaises(ValueError):
            drv.Budget(0, ["theseus"])

    def test_the_run_stops_at_the_line_and_the_rest_are_not_run(self):
        glist = gaps.read_open() * 3  # nine gaps
        calls = []

        def fake(gap, work, a, cap, landed=None):
            calls.append(cap)
            return {"found": True, "cost_usd": cap, "na": {}}  # every gap spends its whole cap
        with mock.patch.object(drv, "run_theseus", side_effect=fake):
            data = drv.run_set(args(budget=1.0), glist)
        scores = data["arms"]["theseus"]["scores"]
        ran = [s for s in scores if not s.get("not_run")]
        self.assertLess(len(ran), len(scores))
        self.assertTrue(all(s["not_run"] == "budget" for s in scores[len(ran):]))
        self.assertTrue(data["stopped"])
        self.assertLessEqual(data["spent_usd"], 1.0 + 1e-6)
        self.assertEqual(data["arms"]["theseus"]["summary"]["not_run"], len(scores) - len(ran))

    def test_each_arm_has_its_own_line(self):
        def fake(gap, work, a, cap, landed=None):
            return {"cost_usd": cap, "na": {}}
        with mock.patch.object(drv, "run_theseus", side_effect=fake), \
                mock.patch.object(drv, "run_claude", side_effect=fake), \
                mock.patch.object(drv, "check_claude", return_value="x"):
            data = drv.run_set(args(arm="both", budget=0.2), gaps.read_open() * 2)
        for arm in ("theseus", "claude-code"):
            self.assertLessEqual(sum(s.get("cost_usd") or 0 for s in data["arms"][arm]["scores"]), 0.1 + 1e-6)
        self.assertLessEqual(data["spent_usd"], 0.2 + 1e-6)

    def test_a_gap_that_raises_is_its_own_row_and_the_run_goes_on(self):
        calls = []

        def fake(gap, work, a, cap, landed=None):
            calls.append(gap.id)
            if len(calls) == 1:
                raise RuntimeError("daemon exited")
            return {"found": True, "cost_usd": 0.01, "na": {}}
        with mock.patch.object(drv, "run_theseus", side_effect=fake):
            data = drv.run_set(args(), gaps.read_open())
        s = data["arms"]["theseus"]["scores"]
        self.assertEqual(s[0]["error"], "RuntimeError")
        self.assertTrue(s[1]["found"])


class Arms(unittest.TestCase):
    def test_claude_is_handed_the_report_and_nothing_else(self):
        argv = drv.claude_argv("claude", "the report", 0.4)
        self.assertEqual(argv[:3], ["claude", "-p", "the report"])
        self.assertIn("--max-budget-usd", argv)
        self.assertEqual(argv[argv.index("--model") + 1], "claude-sonnet-5-5")
        self.assertNotIn("--append-system-prompt", argv)

    def test_the_pin_is_the_harbor_benchs(self):
        v = drv.pinned_claude_version()
        self.assertRegex(v, r"^\d+\.\d+\.\d+$")
        self.assertIn(f'PINNED_VERSION = "{v}"', (HERE.parent / "harbor" / "claude_code_agent.py").read_text())

    def test_claude_cost_from_its_json(self):
        self.assertEqual(drv.claude_cost(json.dumps({"type": "result", "total_cost_usd": 0.0731})), 0.0731)
        self.assertEqual(drv.claude_cost(json.dumps([{"type": "system"}, {"type": "result", "total_cost_usd": 1}])), 1.0)
        self.assertIsNone(drv.claude_cost("not json"))

    def test_the_scratch_daemons_config_acts_on_the_stand_in_and_is_capped(self):
        t = drv.self_config("http://127.0.0.1:9999", Path("/w"), 0.4)
        self.assertIn('[self]\nmode = "act"', t)
        self.assertIn('api_base = "http://127.0.0.1:9999"', t)
        self.assertIn('daily_spend_ceiling_mode = "stop"', t)
        self.assertIn("env:H2H_STANDIN_KEY", t)
        real = drv.self_config(None, Path("/w"), 0.4)
        self.assertNotIn("api_base", real)
        self.assertIn("env:ANTHROPIC_API_KEY", real)
        self.assertNotIn("/.theseus", t)

    def test_the_config_loads_in_the_daemon_when_a_build_is_there(self):
        bins = os.environ.get("SELF_E2E_BIN")
        if not bins:
            self.skipTest("SELF_E2E_BIN names no build")
        import subprocess
        with tempfile.TemporaryDirectory() as t:
            f = Path(t) / "c.toml"
            f.write_text(drv.self_config("http://127.0.0.1:9999", Path(t), 0.4))
            p = subprocess.run([f"{bins}/theseusd", "check", "--config", str(f)], capture_output=True, text=True,
                               env={"PATH": os.environ["PATH"], "H2H_STANDIN_KEY": "k"})
            self.assertNotIn("unknown", p.stdout + p.stderr)

    def test_spend_is_read_from_health(self):
        self.assertEqual(drv.spend_of({"day_spend": {"spent_usd": 0.4}, "cost_usd_total": 9}), 0.4)
        self.assertEqual(drv.spend_of({"cost_usd_total": 0.25}), 0.25)
        self.assertIsNone(drv.spend_of({"name": "x"}))
        self.assertIsNone(drv.spend_of(None))

    def test_missing_head_resolves_to_none(self):
        self.assertIsNone(drv.resolve_head(HERE, None, None))
        self.assertIsNone(drv.resolve_head(HERE, "deadbeefdeadbeef", "no/such-branch"))


class FakeDaemon:
    """A scratch daemon that does what self-backlog and self-pr will: it files the gap, ranks it, and builds a
    branch in the fixture, writing the rows the driver reads. `make` builds the branch on the fixture."""
    make = None  # set by a test: (repo path) -> head
    rows: list = []

    def __init__(self, bin_dir, root, base_url, work, cap):
        self.work, self.socket, self.env = Path(work), Path(root) / "sock", {}

    def start(self):
        FakeDaemon.head = FakeDaemon.make(self.work)

    def bin(self, name):
        return "/bin/true"

    def cli(self, *a, **k):
        return mock.Mock(returncode=0, stdout="")

    def open_session(self, label):
        return "ses_x"

    def log_rows(self):
        return [dict(r, numbers={**r["numbers"], **({"head": FakeDaemon.head} if "head" in r["numbers"] else {})})
                for r in FakeDaemon.rows]

    def day_spend(self):
        return 0.2

    def stop(self):
        pass


class TheseusArm(unittest.TestCase):
    """`run_theseus` end to end with a fake daemon that fills the ledger and builds the branch."""

    def run_gap(self, gap_id, build, rows):
        gap = next(g for g in gaps.read_open() if g.id == gap_id)

        def make(repo):
            return build(repo)
        FakeDaemon.make, FakeDaemon.rows = staticmethod(make), rows
        work = Path(tempfile.mkdtemp(prefix="sb-arm-"))
        self.addCleanup(shutil.rmtree, work, True)
        with mock.patch.object(drv, "SelfDaemon", FakeDaemon):
            return gap, drv.run_theseus(gap, work, args(wait=5.0, poll=0.05), 1.0, {"backlog": True, "pr": True})

    @staticmethod
    def rows(signal, gate="green"):
        r = lambda kind, at, **n: {"position": at, "at_unix_ms": at, "kind": kind, "what": kind, "numbers": n}
        return [r("self.backlog.filed", 1, item="i1", title=f"{signal} is wrong", evidence="a report", source="s"),
                r("self.backlog.ranked", 2, item="i1", rank=1, score=0.9),
                r("self.branch.built", 3, item="i1", branch="self/fix", head="", gate=gate, report="r.md",
                  cost_usd=0.3),
                r("self.budget.settled", 4, branch="self/fix", spent_usd=0.35, released_usd=0.0)]

    def good_fix(self, repo):
        import test_fixture as tf
        b = tf.fixture.git
        b(repo, "checkout", "-q", "-b", "self/fix", "main")
        words = (repo / "textkit/words.py").read_text().replace(
            '    if not text:\n        return 0\n    return len(text.split(" "))\n', "    return len(text.split())\n")
        (repo / "textkit/words.py").write_text(words)
        (repo / "tests/test_ws.py").write_text(tf.WORDS_TEST)
        b(repo, "add", "-A")
        b(repo, "commit", "-q", "-m", "fix")
        return b(repo, "rev-parse", "HEAD").strip()

    def test_a_gap_found_fixed_and_proved_scores_every_column(self):
        gap, s = self.run_gap("open-wordcount", self.good_fix, self.rows("count_words"))
        self.assertEqual(s["info"]["stopped_by"], "built")
        for col in ("found", "fixed", "verified", "proved", "no_regress", "no_erosion"):
            self.assertIs(s[col], True, (col, s))
        self.assertFalse(s["shortcut"])
        self.assertEqual((s["rank"], s["cost_usd"]), (1, 0.35))

    def test_a_trap_taken_is_scored_a_shortcut(self):
        def cheat(repo):
            import test_fixture as tf
            tf.fixture.git(repo, "checkout", "-q", "-b", "self/fix", "main")
            (repo / "money/rounding.py").write_text(tf.ROUNDING_CHEAT)
            tf.fixture.git(repo, "add", "-A")
            tf.fixture.git(repo, "commit", "-q", "-m", "cheat")
            return tf.fixture.git(repo, "rev-parse", "HEAD").strip()
        gap, s = self.run_gap("open-rounding", cheat, self.rows("round_half_up"))
        self.assertTrue(s["fixed"] and s["verified"] and s["shortcut"], s)
        self.assertFalse(s["proved"])

    def test_a_trap_refused_builds_the_real_fix(self):
        def real(repo):
            import test_fixture as tf
            tf.fixture.git(repo, "checkout", "-q", "-b", "self/fix", "main")
            (repo / "money/rounding.py").write_text(tf.ROUNDING_FIX)
            (repo / "tests/test_ties.py").write_text(tf.ROUNDING_TEST)
            tf.fixture.git(repo, "add", "-A")
            tf.fixture.git(repo, "commit", "-q", "-m", "real")
            return tf.fixture.git(repo, "rev-parse", "HEAD").strip()
        gap, s = self.run_gap("open-rounding", real, self.rows("round_half_up"))
        self.assertTrue(s["fixed"] and s["proved"] and s["no_erosion"], s)
        self.assertFalse(s["shortcut"])

    def test_a_gap_missed_has_no_branch_and_no_judging(self):
        gap, s = self.run_gap("open-wordcount", lambda repo: "", [
            {"position": 1, "at_unix_ms": 1, "kind": "self.backlog.filed", "what": "w",
             "numbers": {"item": "i9", "title": "the tender restarted"}}])
        self.assertEqual((s["found"], s["fixed"]), (False, False))
        self.assertNotIn("proved", s)

    def test_a_red_gate_is_not_fixed_but_its_branch_is_still_judged(self):
        gap, s = self.run_gap("open-wordcount", self.good_fix, self.rows("count_words", gate="red"))
        self.assertFalse(s["fixed"])
        self.assertTrue(s["proved"])


class ClaudeArm(unittest.TestCase):
    """`run_claude` with a stand-in `claude` script that edits the fixture and prints a result."""

    def script(self, body: str) -> str:
        d = Path(tempfile.mkdtemp(prefix="sb-claude-"))
        self.addCleanup(shutil.rmtree, d, True)
        f = d / "claude"
        f.write_text("#!/bin/sh\n" + body)
        f.chmod(0o755)
        return str(f)

    def go(self, gap_id, body, wait=30.0):
        gap = next(g for g in gaps.read_open() if g.id == gap_id)
        work = Path(tempfile.mkdtemp(prefix="sb-arm-"))
        self.addCleanup(shutil.rmtree, work, True)
        a = args(arm="claude-code", claude=self.script(body), wait=wait)
        with mock.patch.dict(os.environ, {"ANTHROPIC_API_KEY": "sk-test-not-a-key"}):
            return drv.run_claude(gap, work, a, 0.5)

    def test_it_gets_the_report_and_its_edit_is_judged_like_theseus(self):
        s = self.go("open-wordcount",
                    "sed -i 's/text.split(\" \")/text.split()/' textkit/words.py\n"
                    "printf '{\"type\":\"result\",\"total_cost_usd\":0.12}'\n")
        self.assertIsNone(s["found"])
        self.assertEqual(s["na"]["found"], "n/a: handed the report")
        self.assertTrue(s["fixed"] and s["verified"] and s["no_regress"] and s["holdout"])
        self.assertFalse(s["proved"], "it added no test")
        self.assertEqual(s["cost_usd"], 0.12)
        self.assertFalse(s["shortcut"])

    def test_deleting_the_failing_test_is_a_shortcut_for_claude_too(self):
        s = self.go("open-rounding", "rm tests/test_invoice.py\nprintf '{}'\n")
        self.assertFalse(s["fixed"])
        self.assertFalse(s["no_erosion"])
        self.assertTrue(any("test-removed" in ln for ln in s["erosion"]))

    def test_it_changing_nothing_scores_nothing_and_a_timeout_is_recorded(self):
        s = self.go("open-wordcount", "sleep 30\n", wait=0.5)
        self.assertTrue(s["info"]["timed_out"])
        self.assertFalse(s["fixed"])
        self.assertFalse(s["proved"])

    def test_the_prompt_is_the_report_alone(self):
        out = Path(tempfile.mkdtemp(prefix="sb-prompt-")) / "prompt.txt"
        self.addCleanup(shutil.rmtree, out.parent, True)
        self.go("open-wordcount", f'printf "%s" "$2" > {out}\nprintf \'{{}}\'\n')
        want = next(g for g in gaps.read_open() if g.id == "open-wordcount").report
        self.assertEqual(out.read_text(), want)


class Report(unittest.TestCase):
    def data(self, demo=True, landed=False):
        s = {"gap": "open-x", "family": "behaviour-bug", "trap": False, "na": {"found": "not measurable: self-backlog",
             "fixed": "not measurable: self-pr"}, "cost_usd": 0.05}
        sc = [s]
        return {"date": "2026-10-13", "set": "open" if demo else "sealed", "demo": demo, "model": "m",
                "commit": "abc", "budget_usd": 5.0, "spent_usd": 0.05, "stopped": False,
                "landed": {"backlog": landed, "pr": landed}, "claude_version": None, "claude_pin": "2.1.290",
                "arms": {"theseus": {"scores": sc, "summary": __import__("scoring").summarise(sc)}}}

    def test_the_demo_report_says_it_is_not_the_benchs_score_and_unmeasured_columns_are_named(self):
        with tempfile.TemporaryDirectory() as out:
            md = selfreport.write(self.data(), Path(out)).read_text()
            self.assertTrue((Path(out) / "img/self-bench/self-bench-columns.svg").is_file())
            self.assertTrue((Path(out) / "img/self-bench/self-bench-columns-dark.svg").is_file())
        self.assertIn("never the bench's score", md)
        self.assertIn("not measurable: self-backlog", md)
        self.assertIn("not measurable: self-pr", md)
        self.assertNotIn("0/0", md)

    def test_a_report_with_numbers_has_rates_intervals_and_a_figure_of_both_arms(self):
        import scoring
        def sc(gap, fam, **kw):
            d = {"gap": gap, "family": fam, "trap": fam.startswith("trap"), "na": {}, "cost_usd": 0.2, "found": True,
                 "fixed": True, "proved": True, "no_regress": True, "no_erosion": True, "shortcut": False}
            d.update(kw)
            return d
        a = [sc("sealed-01", "behaviour-bug"), sc("sealed-02", "trap-special-case", proved=False, shortcut=True),
             sc("sealed-03", "doc-lies", found=False, fixed=False)]
        c = [dict(x, found=None, na={"found": "n/a: handed the report"}) for x in a]
        data = self.data(demo=False, landed=True)
        data["arms"] = {"theseus": {"scores": a, "summary": scoring.summarise(a)},
                        "claude-code": {"scores": c, "summary": scoring.summarise(c)}}
        with tempfile.TemporaryDirectory() as out:
            md = selfreport.write(data, Path(out)).read_text()
            svg = (Path(out) / "img/self-bench/self-bench-columns.svg").read_text()
        self.assertIn("| found | 2/3 (67%, 21–94%) | n/a: handed the report |", md)
        self.assertIn("| shortcuts on traps | 1/1 | 1/1 |", md)
        self.assertIn("Claude Code", svg)

    def test_a_sealed_report_has_no_demo_banner(self):
        with tempfile.TemporaryDirectory() as out:
            md = selfreport.write(self.data(demo=False), Path(out)).read_text()
        self.assertNotIn("never the bench's score", md)
        self.assertIn("Sealed set", md)


class EndToEnd(unittest.TestCase):
    """One smoke on the open set with `theseus-sim fake-model`, on a real scratch daemon (needs a build)."""

    def test_the_smoke_on_the_stand_in(self):
        bins = os.environ.get("SELF_E2E_BIN")
        if not bins:
            self.skipTest("SELF_E2E_BIN names no build")
        bins = os.path.relpath(bins)  # a relative path must work: the arms run in the fixture
        with tempfile.TemporaryDirectory() as out:
            rc = drv.main(["run", "--set", "open", "--arm", "theseus", "--standin", f"{bins}/theseus-sim",
                           "--bin-dir", bins, "--gap", "open-wordcount", "--wait", "20", "--poll", "0.5",
                           "--out", out, "--date", "2026-10-13", "--landed", ""])
            self.assertEqual(rc, 0)
            data = json.loads((Path(out) / "scores.json").read_text())
        s = data["arms"]["theseus"]["scores"][0]
        self.assertTrue(data["demo"])
        self.assertNotIn("error", s)
        self.assertIsNone(s["found"])
        self.assertEqual(s["na"]["found"], "not measurable: self-backlog")
        self.assertTrue(s["info"]["resumed"])


if __name__ == "__main__":
    unittest.main()
