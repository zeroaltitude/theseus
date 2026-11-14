"""Tests for scripts/status-page.py (issue 15's last line; theseus-fly32).

Run: python3 -m unittest -q scripts/tests/test_status_page.py
The gate also runs it as part of the `script tests` phase; the HARD check is the `status page` phase, which runs the
script itself on the tree.

Each case builds a throwaway repository with its own Cargo.toml, status page, front matter and release tags, so no
case depends on the real tree's version or on which tags a clone happens to hold.
"""
import contextlib
import importlib.util
import io
import os
import subprocess
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
spec = importlib.util.spec_from_file_location("status_page", os.path.join(HERE, "..", "status-page.py"))
sp = importlib.util.module_from_spec(spec)
spec.loader.exec_module(sp)

GIT = ["-c", "user.name=t", "-c", "user.email=t@example.invalid", "-c", "commit.gpgsign=false",
       "-c", "tag.gpgsign=false", "-c", "init.defaultBranch=main"]

CARGO = '[workspace]\nmembers = []\n\n[workspace.package]\nversion = "%s"\nedition = "2021"\n'
FRONT = "# The Ship of Theseus, chapter 1\n# The Ship of Theseus — v0.92\n"


def header(workspace, tag, sha, tagged="2026-11-14", name=None, updated="2026-11-14", rest=""):
    name = name or sp.release_name(tag)
    return (f"_Updated {updated} 11:5x MST. Workspace version {workspace}; newest release tag `release-{tag}` "
            f"({name}) at `{sha}`, {tagged}.{rest}_")


class StatusPage(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = self.tmp.name
        self.env = dict(os.environ, HOME=self.root, GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_SYSTEM=os.devnull)
        self.git("init", "-q")
        os.makedirs(os.path.join(self.root, "docs", "spec"))
        self.put("docs/spec/01-front.md", FRONT)

    def git(self, *args, date="2026-11-14T10:00:00-0700"):
        env = dict(self.env, GIT_COMMITTER_DATE=date, GIT_AUTHOR_DATE=date)
        return subprocess.run(["git", *GIT, *args], cwd=self.root, env=env, capture_output=True, text=True,
                              check=True).stdout.strip()

    def put(self, path, text):
        with open(os.path.join(self.root, path), "w", encoding="utf-8") as f:
            f.write(text)

    def commit(self, version, page="_Updated nothing yet._\n", date="2026-11-14T10:00:00-0700"):
        self.put("Cargo.toml", CARGO % version)
        self.put("docs/status.md", "# Theseus: status and roadmap\n\n" + page + "\n")
        self.git("add", "-A")
        self.git("commit", "-q", "--allow-empty", "-m", f"at {version}", date=date)
        return self.git("rev-parse", "HEAD")

    def tag(self, version, date="2026-11-14T10:00:00-0700"):
        self.git("tag", "-a", f"release-{version}", "-m", f"release-{version}", date=date)

    def released(self, version, date="2026-11-14T10:00:00-0700"):
        """A commit at `version`, tagged as its release; returns its sha."""
        sha = self.commit(version, date=date)
        self.tag(version, date=date)
        return sha

    def faults(self):
        faults, _, skipped = sp.check(self.root)
        return faults, skipped

    def test_a_true_line_passes(self):
        sha = self.released("1.0.6")
        self.commit("1.0.6", header("1.0.6", "1.0.6", sha[:8], rest=" The design document is at v0.92."))
        self.assertEqual(self.faults(), ([], False))

    def test_the_line_this_page_shipped_with_is_refused(self):
        # The stale line of 2026-11-13, word for word but for its versions: it names a release by the words around
        # it, not by the shape this script reads, so nothing could check it.
        sha = self.released("1.0.6")
        old = (f"_Updated 2026-11-13 21:3x MST. Version 1.0.5 (v1 patch5), tagged `release-1.0.5` at `{sha[:8]}` on "
               "2026-11-12; the design document is at v0.92. 1.0.6 is being built now._")
        self.commit("1.0.6", old)
        faults, _ = self.faults()
        self.assertEqual(len(faults), 1, faults)
        self.assertIn("does not have the shape", faults[0])

    def test_a_workspace_version_the_page_does_not_say_is_a_fault(self):
        sha = self.released("1.0.6")
        self.commit("1.0.7", header("1.0.6", "1.0.6", sha[:8]))
        faults, _ = self.faults()
        self.assertEqual(faults, ["the page says workspace version 1.0.6, Cargo.toml says 1.0.7"])

    def test_a_page_two_releases_behind_its_newest_tag_is_a_fault(self):
        old = self.released("1.0.4", date="2026-11-06T10:00:00-0700")
        self.released("1.0.5", date="2026-11-12T10:00:00-0700")
        self.released("1.0.6")
        self.commit("1.0.6", header("1.0.6", "1.0.4", old[:8], tagged="2026-11-06"))
        faults, _ = self.faults()
        self.assertEqual(len(faults), 1, faults)
        self.assertIn("names release-1.0.4, and the newest release tag reachable from HEAD is release-1.0.6", faults[0])

    def test_a_workspace_version_behind_its_newest_tag_is_a_fault(self):
        sha = self.released("1.0.6")
        self.commit("1.0.5", header("1.0.5", "1.0.6", sha[:8]))
        faults, _ = self.faults()
        self.assertIn("the workspace version 1.0.5 is older than the newest release tag, release-1.0.6", faults)

    def test_the_tags_commit_date_and_name_are_read_back(self):
        sha = self.released("1.0.6")
        wrong = "0" * 8 if not sha.startswith("0" * 8) else "1" * 8
        self.commit("1.0.6", header("1.0.6", "1.0.6", wrong, tagged="2026-11-13", name="v1 patch5",
                                    updated="2026-11-12"))
        faults, _ = self.faults()
        self.assertEqual(len(faults), 4, faults)
        self.assertIn(f"the page puts release-1.0.6 at {wrong}, and it is at {sha[:8]}", faults)
        self.assertIn("the page dates release-1.0.6 2026-11-13, and it was tagged 2026-11-14", faults)
        self.assertIn("the page was updated 2026-11-12, before release-1.0.6 was tagged (2026-11-14)", faults)
        self.assertIn('the page calls release-1.0.6 "v1 patch5", and its name is "v1 patch6"', faults)

    def test_a_release_tagged_on_the_previous_version_is_a_fault(self):
        sha = self.commit("1.0.6")
        self.tag("1.0.7")
        self.commit("1.0.7", header("1.0.7", "1.0.7", sha[:8]))
        faults, _ = self.faults()
        self.assertEqual(faults, ["release-1.0.7 is tagged on a tree whose workspace version is 1.0.6: its binaries "
                                  "print 1.0.6"])

    def test_the_one_recorded_mistag_is_not_a_fault(self):
        self.commit("1.0.4", date="2026-11-12T10:00:00-0700")
        self.tag("1.0.5", date="2026-11-12T10:00:00-0700")
        sha = self.released("1.0.6")
        self.commit("1.0.6", header("1.0.6", "1.0.6", sha[:8]))
        self.assertEqual(self.faults(), ([], False))

    def test_the_tagged_commit_may_name_the_tag_before_it(self):
        # The page is written before its release is tagged, so the tagged tree says what was true then.
        old = self.released("1.0.5", date="2026-11-12T10:00:00-0700")
        self.commit("1.0.6", header("1.0.6", "1.0.5", old[:8], tagged="2026-11-12"))
        self.tag("1.0.6")
        self.assertEqual(self.faults(), ([], False))
        # One commit later the same line is stale.
        self.commit("1.0.6", header("1.0.6", "1.0.5", old[:8], tagged="2026-11-12"))
        faults, _ = self.faults()
        self.assertEqual(len(faults), 1, faults)
        self.assertIn("newest release tag reachable from HEAD is release-1.0.6", faults[0])

    def test_a_design_version_the_front_matter_does_not_hold_is_a_fault(self):
        sha = self.released("1.0.6")
        self.commit("1.0.6", header("1.0.6", "1.0.6", sha[:8], rest=" The design document is at v0.93."))
        faults, _ = self.faults()
        self.assertEqual(faults, [f"the page says the design document is at v0.93, {sp.FRONT} says v0.92"])

    def test_free_text_after_the_facts_is_refused(self):
        # The old line went stale in its free text ("1.0.6 is being built now"), so the shape allows none.
        sha = self.released("1.0.6")
        self.commit("1.0.6", header("1.0.6", "1.0.6", sha[:8], rest=" The next release is 1.0.7."))
        faults, _ = self.faults()
        self.assertEqual(len(faults), 1, faults)
        self.assertIn("does not have the shape", faults[0])
        self.commit("1.0.6", header("1.0.6", "1.0.6", sha[:8], rest=" The design document is at v0.92. More."))
        faults, _ = self.faults()
        self.assertEqual(len(faults), 1, faults)
        # Nor in the time's place.
        line = header("1.0.6", "1.0.6", sha[:8]).replace(" 11:5x MST.", ", while the next patch is built now.")
        self.commit("1.0.6", line)
        faults, _ = self.faults()
        self.assertEqual(len(faults), 1, faults)
        self.assertIn("does not have the shape", faults[0])
        # A date alone, or a whole time, is fine.
        for when in ("", " 9:05 MST"):
            self.commit("1.0.6", header("1.0.6", "1.0.6", sha[:8]).replace(" 11:5x MST", when))
            self.assertEqual(self.faults(), ([], False), when)

    def test_before_a_tag_its_name_is_held_to_the_workspace_version(self):
        sha = self.released("1.0.6")
        self.commit("1.0.6", header("1.0.6", "1.0.6", sha[:8]))
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            self.assertEqual(sp.main(["--tag", "release-1.0.7", self.root]), 1)
        self.assertIn("release-1.0.7 would tag a tree whose workspace version is 1.0.6", out.getvalue())
        with contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(sp.main(["--tag", "1.0.7", self.root]), 1)
            self.assertEqual(sp.main(["--tag"]), 2)
        self.commit("1.0.7", header("1.0.7", "1.0.6", sha[:8]))
        with contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(sp.main(["--tag", "release-1.0.7", self.root]), 0)

    def test_a_clone_with_no_tags_checks_the_version_and_says_it_skipped(self):
        self.commit("1.0.6", header("1.0.6", "1.0.6", "b8b403b7"))
        self.assertEqual(self.faults(), ([], True))
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            self.assertEqual(sp.main([self.root]), 0)
        self.assertTrue(out.getvalue().startswith("status page: skipped: "), out.getvalue())
        # The version half still runs without tags.
        self.commit("1.0.7", header("1.0.6", "1.0.6", "b8b403b7"))
        faults, skipped = self.faults()
        self.assertTrue(skipped)
        self.assertEqual(faults, ["the page says workspace version 1.0.6, Cargo.toml says 1.0.7"])

    def test_the_exit_status_says_fault_and_unreadable(self):
        sha = self.released("1.0.6")
        self.commit("1.0.7", header("1.0.6", "1.0.6", sha[:8]))
        with contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(sp.main([self.root]), 1)
        os.remove(os.path.join(self.root, "Cargo.toml"))
        with contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(sp.main([self.root]), 2)


if __name__ == "__main__":
    unittest.main()
