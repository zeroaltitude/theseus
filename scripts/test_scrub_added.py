#!/usr/bin/env python3
"""Tests of the scrub gate (theseus-ev3ir): scripts/scrub-added.sh and the cloud reviewers' scrub-branch.sh.

Two sensitive names reached public refs in a `.cases.toml`, which no test read. These plant a FAKE name (never a
real one: the point is that such strings stay out of refs) in a scratch repo and ask each scrubber to find it.
A bar of "zero hits" is met by an empty set, so every pass here also asserts what was scanned and what was found.

The scrubber lives outside the repo (its patterns are the names): THESEUS_SCRUB_CHECK, default
~/reports/theseus-docs-v076/scrub_check.py; the reviewers' wrapper: THESEUS_SCRUB_BRANCH, default
~/reports/theseus-cloud/scrub-branch.sh. A missing one is a FAILURE here, never a skip.
"""
import os
import re
import subprocess
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
DRIVER = os.path.join(HERE, "scrub-added.sh")
BRANCH_SH = os.environ.get("THESEUS_SCRUB_BRANCH") or os.path.expanduser("~/reports/theseus-cloud/scrub-branch.sh")
FAKE = "zzfake-widgets"  # a name no one has: the test's own configured name
PHRASE = "vault note"  # a blocking family's phrase that is no one's secret


def git(repo, *args):
    env = dict(os.environ, GIT_AUTHOR_NAME="t", GIT_AUTHOR_EMAIL="t@example.invalid",
               GIT_COMMITTER_NAME="t", GIT_COMMITTER_EMAIL="t@example.invalid")
    return subprocess.run(["git", "-C", repo, *args], check=True, capture_output=True, text=True, env=env).stdout.strip()


class Scratch:
    """A repo whose `main` holds one file (with the fake name already in it), and a branch `feat` cut from it."""

    def __init__(self, test):
        self.tmp = tempfile.TemporaryDirectory()
        test.addCleanup(self.tmp.cleanup)
        self.repo = os.path.join(self.tmp.name, "repo")
        os.makedirs(self.repo)
        git(self.repo, "init", "-q", "-b", "main")
        self.write("old.txt", f"an old line naming {FAKE}\nsecond\n")
        git(self.repo, "add", "-A")
        git(self.repo, "commit", "-qm", "base")
        self.base = git(self.repo, "rev-parse", "HEAD")
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
                    THESEUS_CONFIG=os.path.join(self.tmp.name, "no-such-config.toml"))


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
    return re.findall(r"^([a-z0-9@'/ :\-()]+): (?:\d+|UNAVAILABLE)", out, re.M)


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
        self.assertEqual(family(out, "vault note"), 1, out)

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
        self.assertEqual(family(out, "vault note"), 1, out)


class BranchWrapper(unittest.TestCase):
    """The tool reviewers reach for must be the thorough one: it fails on what the driver fails on."""

    def test_it_fails_on_a_blocking_family_and_prints_its_line(self):
        s = Scratch(self)
        s.commit("a.txt", f"see the {PHRASE}\n")
        code, out = run([BRANCH_SH, "feat", s.base], s.env())
        self.assertEqual(family(out, "vault note"), 1, out)
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
        self.assertGreater(len(families(a)), 15, a)
        self.assertEqual(families(a), families(b))


if __name__ == "__main__":
    unittest.main()
