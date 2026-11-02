#!/usr/bin/env python3
"""Tests of the scrub gate (theseus-ev3ir): scripts/scrub-added.sh and the cloud reviewers' scrub-branch.sh.

Two sensitive names reached public refs in a `.cases.toml`, which no test read. These plant a FAKE name (never a
real one: the point is that such strings stay out of refs) in a scratch repo and ask each scrubber to find it.
A bar of "zero hits" is met by an empty set, so every pass here also asserts what was scanned and what was found.

The scrubber lives outside the repo (its patterns are the names): THESEUS_SCRUB_CHECK, default
~/reports/theseus-docs-v076/scrub_check.py; the reviewers' wrapper: THESEUS_SCRUB_BRANCH, default
~/reports/theseus-cloud/scrub-branch.sh. A missing scrubber is a FAILURE here, never a skip (the gate decides to skip before it runs this suite); the
reviewers' wrapper is no gate dependency, so its tests are skipped when it is absent.
"""
import os
import re
import subprocess
import shutil
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
DRIVER = os.path.join(HERE, "scrub-added.sh")
GATE = os.path.join(HERE, "gate.sh")
BRANCH_SH = os.environ.get("THESEUS_SCRUB_BRANCH") or os.path.expanduser("~/reports/theseus-cloud/scrub-branch.sh")
FAKE = "zzfake-widgets"  # a name no one has: the test's own configured name
PHRASE = "vault" + " note"  # a blocking family's phrase that is no one's secret; split, so this file does not trip it


def git(repo, *args):
    # No global config (a developer's `commit.gpgsign=true` hung this suite on a locked agent), and a timeout.
    env = dict(os.environ, GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_NOSYSTEM="1", GIT_AUTHOR_NAME="t",
               GIT_AUTHOR_EMAIL="t@example.invalid", GIT_COMMITTER_NAME="t", GIT_COMMITTER_EMAIL="t@example.invalid")
    return subprocess.run(["git", "-C", repo, *args], check=True, capture_output=True, text=True, env=env,
                          timeout=30).stdout.strip()


class Scratch:
    """A repo whose `main` holds one file (with the fake name already in it), and a branch `feat` cut from it."""

    def __init__(self, test, gate=False):
        self.tmp = tempfile.TemporaryDirectory()
        test.addCleanup(self.tmp.cleanup)
        self.repo = os.path.join(self.tmp.name, "repo")
        os.makedirs(self.repo)
        git(self.repo, "init", "-q", "-b", "main")
        self.write("old.txt", f"an old line naming {FAKE}\nsecond\n")
        if gate:  # main holds the driver, as it will once this lands: the gate runs main's copy
            os.makedirs(os.path.join(self.repo, "scripts"))
            shutil.copy(DRIVER, os.path.join(self.repo, "scripts", "scrub-added.sh"))
        git(self.repo, "add", "-A")
        git(self.repo, "commit", "-qm", "base")
        self.base = git(self.repo, "rev-parse", "HEAD")
        git(self.repo, "update-ref", "refs/remotes/origin/main", self.base)
        git(self.repo, "checkout", "-qb", "feat")

    def write(self, path, text):
        full = os.path.join(self.repo, path)
        os.makedirs(os.path.dirname(full), exist_ok=True)
        with open(full, "w", encoding="utf-8") as fh:
            fh.write(text)

    def commit(self, path, text):
        self.write(path, text)
        git(self.repo, "add", "-A")
        git(self.repo, "commit", "-qm", "feat")

    def env(self):
        # The scratch has no config of its own: the fake name stands in for `[privacy] sensitive_names`.
        return dict(os.environ, SCRUB_REPO=self.repo, SCRUB_EXTRA_NAMES=FAKE.replace("-", " "),
                    THESEUS_CONFIG=os.path.join(self.tmp.name, "no-such-config.toml"),
                    SCRUB_OUT_DIR=os.path.join(self.tmp.name, "out"))


def run(argv, env):
    try:
        p = subprocess.run(argv, capture_output=True, text=True, env=env, timeout=40)
    except (subprocess.TimeoutExpired, OSError) as e:
        return 124, f"did not run: {e}"
    return p.returncode, p.stdout + p.stderr


def family(out, name):
    m = re.search(rf"^{re.escape(name)}: (\d+)", out, re.M)
    return int(m.group(1)) if m else None


def families(out):
    """The family names the scrubber printed: every line after its "files read" one that ends `: <count>`."""
    after = out.split("files read:", 1)[-1].splitlines()[1:]
    return [m.group(1) for m in (re.match(r"(.+?): (?:\d+|UNAVAILABLE)", line) for line in after) if m]


class Driver(unittest.TestCase):
    def test_a_fake_name_in_a_new_cases_file_is_found(self):
        s = Scratch(self)
        s.commit("crates/x/packs/new.cases.toml", f'[[case]]\nlabel = "plans for {FAKE.title()}"\n')
        code, out = run([DRIVER, s.base], s.env())
        self.assertEqual(code, 1, out)
        self.assertEqual(family(out, "configured sensitive names"), 1, out)
        self.assertNotIn(FAKE, out, "the name must not be printed")

    def test_an_uncommitted_new_file_is_found(self):
        s = Scratch(self)
        s.commit("a.txt", "fine\n")
        s.write("untracked.cases.toml", f"label = {FAKE}\n")
        code, out = run([DRIVER, s.base], s.env())
        self.assertEqual(code, 1, out)
        self.assertEqual(family(out, "configured sensitive names"), 1, out)

    def test_a_real_family_is_found_by_the_real_scrubber(self):
        s = Scratch(self)
        s.commit("a.txt", f"see the {PHRASE}\n")
        code, out = run([DRIVER, s.base], s.env())
        self.assertEqual(code, 1, out)
        self.assertEqual(family(out, PHRASE), 1, out)

    def test_a_clean_branch_passes_and_says_what_it_scanned(self):
        s = Scratch(self)
        s.commit("a.txt", "one\ntwo\nthree\n")
        code, out = run([DRIVER, s.base], s.env())
        self.assertEqual(code, 0, out)
        self.assertRegex(out, r"scrub-added: 1 file\(s\), 3 added line\(s\)")

    def test_a_line_the_branch_did_not_add_is_not_scanned(self):
        s = Scratch(self)  # old.txt names the fake name, at the base
        s.commit("a.txt", "fine\n")
        code, out = run([DRIVER, s.base], s.env())
        self.assertEqual(code, 0, out)
        self.assertEqual(family(out, "configured sensitive names"), 0, out)

    def test_scanning_nothing_is_not_a_pass(self):
        s = Scratch(self)
        code, out = run([DRIVER, s.base], s.env())
        self.assertEqual(code, 3, out)
        self.assertIn("scanned nothing", out)

    def test_a_commit_message_is_scanned(self):
        s = Scratch(self)
        s.write("a.txt", "fine\n")
        git(s.repo, "add", "-A")
        git(s.repo, "commit", "-qm", f"fix: {PHRASE}")
        code, out = run([DRIVER, s.base], s.env())
        self.assertEqual(code, 1, out)
        self.assertEqual(family(out, PHRASE), 1, out)


    def test_no_scrubber_is_exit_4_and_says_so(self):
        s = Scratch(self)
        s.commit("a.txt", "fine\n")
        code, out = run([DRIVER, s.base], dict(s.env(), THESEUS_SCRUB_CHECK=os.path.join(s.tmp.name, "none.py")))
        self.assertEqual(code, 4, out)
        self.assertIn("no scrubber", out)

    def test_a_commit_message_at_a_staged_join_is_scanned(self):
        # At a staged `--no-ff --no-commit` merge HEAD is still main: the branch's commits are only in MERGE_HEAD.
        s = Scratch(self)
        s.write("a.txt", "fine\n")
        git(s.repo, "add", "-A")
        git(s.repo, "commit", "-qm", f"fix: {PHRASE}")
        git(s.repo, "checkout", "-q", "main")
        git(s.repo, "merge", "-q", "--no-ff", "--no-commit", "feat")
        self.assertTrue(os.path.exists(os.path.join(s.repo, ".git", "MERGE_HEAD")))
        code, out = run([DRIVER, s.base], s.env())
        self.assertEqual(code, 1, out)
        self.assertEqual(family(out, PHRASE), 1, out)

    def test_a_non_ascii_path_is_scanned(self):
        s = Scratch(self)
        s.commit("packs/r\u00e9.cases.toml", f"label = {FAKE}\n")
        code, out = run([DRIVER, s.base], s.env())
        self.assertRegex(out, r"scrub-added: 1 file\(s\), 1 added line\(s\)")
        self.assertEqual(code, 1, out)

    def test_an_untracked_non_ascii_path_is_scanned(self):
        s = Scratch(self)
        s.commit("a.txt", "fine\n")
        s.write("packs/r\u00e9.cases.toml", f"label = {FAKE}\n")
        code, out = run([DRIVER, s.base], s.env())
        self.assertEqual(code, 1, out)

    def test_a_hit_is_reported_at_its_source_line_not_its_place_in_the_extract(self):
        s = Scratch(self)
        s.commit("a.txt", "one\ntwo\nthree\nfour\n")
        s.commit("a.txt", f"one\ntwo\nthree\nfour\nfive\nsee the {PHRASE}\n")
        s.commit("old.txt", f"an old line naming {FAKE}\nsecond\nsee the {PHRASE}\n")
        code, out = run([DRIVER, s.base], s.env())
        self.assertEqual(code, 1, out)
        self.assertRegex(out, rf"\[{PHRASE}\] a\.txt:6\b", out)
        self.assertRegex(out, rf"\[{PHRASE}\] old\.txt:3\b", out)

    def test_a_commit_message_hit_names_its_commit(self):
        s = Scratch(self)
        s.write("a.txt", "fine\n")
        git(s.repo, "add", "-A")
        git(s.repo, "commit", "-qm", f"fix\n\nbody: {PHRASE}")
        head = git(s.repo, "rev-parse", "--short", "HEAD")
        code, out = run([DRIVER, s.base], s.env())
        self.assertEqual(code, 1, out)
        self.assertRegex(out, rf"commit {head} message line 3", out)

    def test_names_written_as_a_multi_line_toml_array_are_read(self):
        s = Scratch(self)
        s.commit("a.txt", f"label {FAKE}\n")
        cfg = os.path.join(s.tmp.name, "multi.toml")
        with open(cfg, "w", encoding="utf-8") as fh:
            fh.write('[privacy]\nsensitive_names = [\n  "other thing",\n  "%s",\n]\n' % FAKE.replace("-", " "))
        env = dict(s.env(), THESEUS_CONFIG=cfg)
        env.pop("SCRUB_EXTRA_NAMES")
        code, out = run([DRIVER, s.base], env)
        self.assertEqual(code, 1, out)
        self.assertEqual(family(out, "configured sensitive names"), 1, out)

    def test_an_unparseable_config_fails_naming_the_cause(self):
        s = Scratch(self)
        s.commit("a.txt", "fine\n")
        cfg = os.path.join(s.tmp.name, "bad.toml")
        with open(cfg, "w", encoding="utf-8") as fh:
            fh.write('[privacy\nsensitive_names = ["x"\n')
        env = dict(s.env(), THESEUS_CONFIG=cfg)
        env.pop("SCRUB_EXTRA_NAMES")
        code, out = run([DRIVER, s.base], env)
        self.assertEqual(code, 2, out)
        self.assertIn("not valid TOML", out)


def extract(src, name):
    """A function of gate.sh, verbatim: the text the gate runs, not a copy of it."""
    m = re.search(rf"^{name}\(\) \{{\n.*?^\}}\n", src, re.M | re.S)
    assert m, f"gate.sh has no function {name}"
    return m.group(0)


class GatePhase(unittest.TestCase):
    """gate.sh's own `scrub_added`, run under the gate's `set -euo pipefail` in a scratch repo. Its first version failed
    every gate of a tree that added nothing, because errexit fired before the exit code was read."""

    def gate(self, s, **extra):
        with open(GATE, encoding="utf-8") as fh:
            src = fh.read()
        funcs = "\n".join(extract(src, n) for n in ("phase", "scrub_added"))
        script = (f'set -euo pipefail\n{funcs}\nphase_names=(); phase_secs=(); phase_label=""; '
                  'gate_tmp=$(mktemp -d); trap \'rm -rf "$gate_tmp"\' EXIT\n'
                  'phase scrub scrub_added\necho "TABLE: ${phase_names[*]}"\n')
        env = dict(s.env(), **extra)
        for k in ("THESEUS_GATE_NO_SCRUB",):
            if k not in extra:
                env.pop(k, None)
        try:
            p = subprocess.run(["bash", "-c", script], capture_output=True, text=True, env=env, cwd=s.repo, timeout=60)
        except subprocess.TimeoutExpired as e:
            return 124, f"timed out: {e}"
        return p.returncode, p.stdout + p.stderr

    def test_a_tree_that_adds_nothing_passes_and_says_so(self):
        s = Scratch(self, gate=True)  # no commit on feat: main's own tip
        code, out = self.gate(s)
        self.assertEqual(code, 0, out)
        self.assertIn("gate: scrub: nothing added", out)
        self.assertIn("TABLE: scrub empty", out)

    def test_a_clean_branch_is_scrub_ok(self):
        s = Scratch(self, gate=True)
        s.commit("a.txt", "fine\n")
        code, out = self.gate(s)
        self.assertEqual(code, 0, out)
        self.assertIn("TABLE: scrub ok", out)

    def test_a_hit_fails_the_phase(self):
        s = Scratch(self, gate=True)
        s.commit("a.txt", f"see the {PHRASE}\n")
        code, out = self.gate(s)
        self.assertNotEqual(code, 0, out)
        self.assertNotIn("TABLE:", out)

    def test_no_scrubber_skips_loudly_and_the_table_says_skipped(self):
        s = Scratch(self, gate=True)
        s.commit("a.txt", f"see the {PHRASE}\n")
        code, out = self.gate(s, THESEUS_SCRUB_CHECK=os.path.join(s.tmp.name, "none.py"))
        self.assertEqual(code, 0, out)
        self.assertRegex(out, r"(?m)^gate: scrub SKIPPED .*no scrubber on this host.*WSL host")
        self.assertIn("TABLE: scrub skipped", out)

    def test_the_explicit_skip_is_skipped_too(self):
        s = Scratch(self, gate=True)
        s.commit("a.txt", f"see the {PHRASE}\n")
        code, out = self.gate(s, THESEUS_GATE_NO_SCRUB="1")
        self.assertEqual(code, 0, out)
        self.assertRegex(out, r"(?m)^gate: scrub SKIPPED \(THESEUS_GATE_NO_SCRUB\)")
        self.assertIn("TABLE: scrub skipped", out)

    def test_a_scrubber_that_is_present_but_unusable_is_not_a_skip(self):
        s = Scratch(self, gate=True)
        s.commit("a.txt", "fine\n")
        cfg = os.path.join(s.tmp.name, "empty.toml")
        open(cfg, "w").close()
        env = dict(THESEUS_CONFIG=cfg, SCRUB_EXTRA_NAMES="")
        code, out = self.gate(s, **env)
        self.assertNotEqual(code, 0, out)

    def test_a_branch_is_judged_by_mains_scrubber_not_its_own(self):
        s = Scratch(self, gate=True)
        s.commit("scripts/scrub-added.sh", "#!/usr/bin/env bash\nexit 0\n")
        s.commit("a.txt", f"see the {PHRASE}\n")
        code, out = self.gate(s)
        self.assertNotEqual(code, 0, f"a weakened copy of the driver judged its own branch:\n{out}")


@unittest.skipUnless(os.path.isfile(BRANCH_SH), f"{BRANCH_SH}: the reviewers' tool, not a gate dependency")
class BranchWrapper(unittest.TestCase):
    """The tool reviewers reach for must be the thorough one: it fails on what the driver fails on."""

    def test_it_fails_on_a_blocking_family_and_prints_its_line(self):
        s = Scratch(self)
        s.commit("a.txt", f"see the {PHRASE}\n")
        code, out = run([BRANCH_SH, "feat", s.base], s.env())
        self.assertEqual(family(out, PHRASE), 1, out)
        self.assertEqual(code, 1, f"a hit in a blocking family exited {code}, a confident pass:\n{out}")

    def test_it_finds_the_configured_name(self):
        s = Scratch(self)
        s.commit("p.cases.toml", f"label = {FAKE}\n")
        code, out = run([BRANCH_SH, "feat", s.base], s.env())
        self.assertEqual(family(out, "configured sensitive names"), 1, out)
        self.assertEqual(code, 1, out)

    def test_inverted_arguments_are_refused_not_passed(self):
        s = Scratch(self)
        s.commit("a.txt", f"see the {PHRASE}\n")
        code, out = run([BRANCH_SH, s.base, "feat"], s.env())  # <base> <head>: the order that reads naturally
        self.assertNotEqual(code, 0, f"inverted arguments passed:\n{out}")

    def test_it_prints_the_families_the_driver_does(self):
        s = Scratch(self)
        s.commit("a.txt", "fine\nmore\n")
        _, a = run([DRIVER, s.base], s.env())
        _, b = run([BRANCH_SH, "feat", s.base], s.env())
        self.assertGreaterEqual(len(families(a)), 18, a)
        self.assertEqual(families(a), families(b))


if __name__ == "__main__":
    unittest.main()
