"""Tests for scripts/context-ceiling.py (theseus-4vsrz, 1.0.5 B7).

Run: python3 -m unittest -q scripts/tests/test_context_ceiling.py
The gate also runs it as part of the `script tests` phase (`scripts/gate.sh`'s `script_tests` globs
`scripts/tests/test_*.py`), which never fails the gate; the HARD guard is the `context ceiling` phase,
which runs the script itself.

Each case builds a throwaway git repository with its own `context_files.rs`, `project_context.rs` and
instruction files, so no case depends on the real tree's byte counts -- which is the point: a test that
asserted "scripts/AGENTS.md is under the ceiling" would pass today and have to be edited by the next
branch that trims it. Two cases do read the real tree, and they assert only the two things that must hold
whatever the tree's sizes are: that the ceiling is readable from the real Rust source, and that the real
scan is not empty.
"""
import contextlib
import importlib.util
import os
import subprocess
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, "..", ".."))
spec = importlib.util.spec_from_file_location("context_ceiling", os.path.join(HERE, "..", "context-ceiling.py"))
cc = importlib.util.module_from_spec(spec)
spec.loader.exec_module(cc)

GIT = ["-c", "user.name=t", "-c", "user.email=t@example.invalid", "-c", "commit.gpgsign=false",
       "-c", "init.defaultBranch=main"]

CORE_RS = "pub const MAX_BYTES: usize = %s;\n"
PROJ_RS = 'pub const NAMES: [&str; %d] = [%s];\n'


def git(cwd, *args):
    # The developer's global config is ignored on purpose: a locked signing agent hangs a commit.
    env = dict(os.environ, HOME=cwd, GIT_CONFIG_GLOBAL=os.path.join(cwd, ".gitconfig-none"),
               GIT_CONFIG_SYSTEM=os.path.join(cwd, ".gitconfig-none"))
    return subprocess.run(["git", *GIT, *args], cwd=cwd, env=env, capture_output=True, text=True, check=True).stdout


class Tree:
    """A throwaway repository: a context_files.rs with a MAX_BYTES, a project_context.rs with NAMES,
    instruction files of exact sizes, and a scripts/context-files.txt."""

    def __init__(self, stack, files, listed=(), max_bytes="64 * 1024", names=("AGENTS.md", "CLAUDE.md"),
                 core=True, proj=True, list_lines=None):
        self.dir = stack.enter_context(tempfile.TemporaryDirectory())
        core_dir = os.path.join(self.dir, "crates", "theseus-core", "src")
        os.makedirs(core_dir)
        os.makedirs(os.path.join(self.dir, "scripts"))
        if core:
            self.write("crates/theseus-core/src/context_files.rs", CORE_RS % max_bytes)
        if proj:
            quoted = ", ".join(f'"{n}"' for n in names)
            self.write("crates/theseus-core/src/project_context.rs", PROJ_RS % (len(names), quoted))
        for path, size in files.items():
            self.write(path, "x" * size)
        if list_lines is not None:
            self.write("scripts/context-files.txt", list_lines)
        elif listed:
            self.write("scripts/context-files.txt",
                       "# a throwaway list\n" + "".join(f"{p} {a} because\n" for p, a in listed))
        git(self.dir, "init", "-q")
        git(self.dir, "add", "-A")
        git(self.dir, "commit", "-qm", "t")

    def write(self, rel, text):
        full = os.path.join(self.dir, rel)
        os.makedirs(os.path.dirname(full), exist_ok=True)
        with open(full, "w", encoding="utf-8") as f:
            f.write(text)

    def run(self, *argv):
        """(exit code, stdout) of a check over this tree."""
        import io
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf), contextlib.redirect_stderr(buf):
            rc = cc.main(["--root", self.dir, *argv])
        return rc, buf.getvalue()


class ReadsTheRustSource(unittest.TestCase):
    """The ceiling and the names are read, not copied: that is the whole anti-drift design."""

    def test_the_real_tree_declares_a_readable_ceiling_of_sixty_four_kib(self):
        self.assertEqual(cc.ceiling(ROOT), 64 * 1024)

    def test_the_real_tree_declares_the_two_names_the_loader_reads(self):
        self.assertEqual(cc.names(ROOT), ["AGENTS.md", "CLAUDE.md"])

    def test_the_real_scan_is_not_empty(self):
        # An empty scan must not look like a clean scan. Asserts nothing about any file's size.
        found = cc.tracked(ROOT, set(cc.names(ROOT)))
        self.assertGreater(len(found), 10)
        self.assertIn("AGENTS.md", [os.path.basename(p) for p, _ in found])

    def test_it_follows_a_moved_ceiling_rather_than_a_copied_number(self):
        with contextlib.ExitStack() as s:
            # 70,000 bytes is over 64 KiB and under 128 KiB: the same file, the same list, two verdicts,
            # decided only by the constant in the Rust source.
            t = Tree(s, {"AGENTS.md": 70_000}, max_bytes="64 * 1024")
            self.assertEqual(t.run("--quiet")[0], 1)
            u = Tree(s, {"AGENTS.md": 70_000}, max_bytes="128 * 1024")
            self.assertEqual(u.run("--quiet")[0], 0)

    def test_it_follows_a_third_name_added_to_NAMES(self):
        with contextlib.ExitStack() as s:
            # An AGENTS.md in both trees so neither scan is empty: an empty scan exits 2 for its own
            # reason and would not be evidence about NAMES either way.
            t = Tree(s, {"AGENTS.md": 100, "RULES.md": 70_000}, names=("AGENTS.md", "CLAUDE.md"))
            self.assertEqual(t.run("--quiet")[0], 0, "RULES.md is not a name the loader reads")
            u = Tree(s, {"AGENTS.md": 100, "RULES.md": 70_000},
                     names=("AGENTS.md", "CLAUDE.md", "RULES.md"))
            rc, out = u.run("--quiet")
            self.assertEqual(rc, 1)
            self.assertIn("RULES.md", out)

    def test_an_unreadable_ceiling_exits_two_and_says_nothing_else(self):
        with contextlib.ExitStack() as s:
            t = Tree(s, {"AGENTS.md": 70_000}, core=False)
            rc, out = t.run()
            self.assertEqual(rc, 2)
            self.assertIn("could not read MAX_BYTES", out)
            self.assertNotIn("OVER", out)

    def test_unreadable_names_exit_two(self):
        with contextlib.ExitStack() as s:
            t = Tree(s, {"AGENTS.md": 70_000}, proj=False)
            rc, out = t.run()
            self.assertEqual(rc, 2)
            self.assertIn("could not read NAMES", out)

    def test_a_ceiling_shape_it_was_not_written_against_exits_two(self):
        with contextlib.ExitStack() as s:
            t = Tree(s, {"AGENTS.md": 70_000}, max_bytes="SOME_OTHER_CONST * 2")
            self.assertEqual(t.run()[0], 2)

    def test_an_underscored_and_plain_ceiling_are_both_read(self):
        with contextlib.ExitStack() as s:
            self.assertEqual(cc.ceiling(Tree(s, {}, max_bytes="65_536").dir), 65_536)
            self.assertEqual(cc.ceiling(Tree(s, {}, max_bytes="65536").dir), 65_536)
            self.assertEqual(cc.ceiling(Tree(s, {}, max_bytes="2 * 32 * 1024").dir), 65_536)


class TheCeilingIsHeld(unittest.TestCase):
    """The four verdicts, each the way scripts/shape.sh words its own."""

    def test_an_unlisted_file_over_the_ceiling_fails_and_names_the_overage(self):
        with contextlib.ExitStack() as s:
            t = Tree(s, {"scripts/AGENTS.md": 71_910})
            rc, out = t.run("--quiet")
            self.assertEqual(rc, 1)
            self.assertIn("scripts/AGENTS.md is 71,910 bytes", out)
            self.assertIn("over the 65,536 ceiling by 6,374", out)
            self.assertIn("no entry in scripts/context-files.txt", out)

    def test_a_file_at_or_under_the_ceiling_passes(self):
        with contextlib.ExitStack() as s:
            # Exactly at the ceiling is NOT a cut: context_files::section() cuts when len > MAX_BYTES.
            t = Tree(s, {"AGENTS.md": 65_536, "scripts/AGENTS.md": 64_000, "scripts/CLAUDE.md": 11})
            rc, out = t.run("--quiet")
            self.assertEqual(rc, 0)
            self.assertIn("none over 65,536 bytes", out)

    def test_one_byte_over_the_ceiling_fails(self):
        with contextlib.ExitStack() as s:
            self.assertEqual(Tree(s, {"AGENTS.md": 65_537}).run("--quiet")[0], 1)

    def test_a_listed_file_within_its_allowance_passes_and_is_marked_listed(self):
        with contextlib.ExitStack() as s:
            t = Tree(s, {"crates/c/AGENTS.md": 93_334}, listed=[("crates/c/AGENTS.md", 93_334)])
            rc, out = t.run()
            self.assertEqual(rc, 0)
            self.assertIn("LISTED, allowance 93,334", out)
            self.assertIn("but the 1 listed", out)

    def test_a_listed_file_past_its_allowance_fails(self):
        with contextlib.ExitStack() as s:
            t = Tree(s, {"crates/c/AGENTS.md": 93_400}, listed=[("crates/c/AGENTS.md", 93_334)])
            rc, out = t.run("--quiet")
            self.assertEqual(rc, 1)
            self.assertIn("over its allowance of 93,334", out)
            self.assertIn("by 66", out)

    def test_a_listed_file_that_fell_under_the_ceiling_fails_as_a_stale_entry(self):
        with contextlib.ExitStack() as s:
            t = Tree(s, {"crates/c/AGENTS.md": 60_000}, listed=[("crates/c/AGENTS.md", 93_334)])
            rc, out = t.run("--quiet")
            self.assertEqual(rc, 1)
            self.assertIn("at or under the 65,536 ceiling: delete its entry", out)

    def test_an_entry_for_a_file_that_is_gone_fails(self):
        with contextlib.ExitStack() as s:
            t = Tree(s, {"AGENTS.md": 100}, listed=[("crates/gone/AGENTS.md", 93_334)])
            rc, out = t.run("--quiet")
            self.assertEqual(rc, 1)
            self.assertIn("not a tracked context file any more", out)

    def test_an_untracked_file_is_not_judged(self):
        # git ls-files is the roster: a file nobody committed is not in any session's tree.
        with contextlib.ExitStack() as s:
            t = Tree(s, {"AGENTS.md": 100})
            t.write("crates/x/AGENTS.md", "x" * 90_000)
            self.assertEqual(t.run("--quiet")[0], 0)

    def test_a_malformed_list_line_fails_rather_than_being_skipped(self):
        # A typo that silently dropped an entry would un-hold a file nobody meant to un-hold.
        with contextlib.ExitStack() as s:
            t = Tree(s, {"crates/c/AGENTS.md": 93_334},
                     list_lines="crates/c/AGENTS.md 93334\n")  # no reason
            rc, out = t.run("--quiet")
            self.assertEqual(rc, 1)
            self.assertIn("is not `<path> <allowance> <reason>`", out)

    def test_comments_and_blank_lines_in_the_list_are_free(self):
        with contextlib.ExitStack() as s:
            t = Tree(s, {"crates/c/AGENTS.md": 93_334},
                     list_lines="# a comment\n\n   \n  # an indented comment\ncrates/c/AGENTS.md 93334 why\n")
            self.assertEqual(t.run("--quiet")[0], 0)

    def test_an_empty_tree_exits_two_rather_than_reporting_clean(self):
        with contextlib.ExitStack() as s:
            rc, out = Tree(s, {}).run()
            self.assertEqual(rc, 2)
            self.assertIn("read nothing", out)


class TheReport(unittest.TestCase):
    def test_headroom_is_printed_for_every_file_largest_first(self):
        with contextlib.ExitStack() as s:
            t = Tree(s, {"AGENTS.md": 1_000, "crates/c/AGENTS.md": 60_000, "crates/c/CLAUDE.md": 11})
            rc, out = t.run()
            self.assertEqual(rc, 0)
            lines = [l for l in out.splitlines() if "/65,536" in l]
            self.assertEqual([l.split()[0] for l in lines],
                             ["crates/c/AGENTS.md", "AGENTS.md", "crates/c/CLAUDE.md"])
            self.assertIn("crates/c/AGENTS.md 60,000/65,536  5,536 left", out)

    def test_quiet_prints_no_table_and_still_prints_the_summary(self):
        with contextlib.ExitStack() as s:
            rc, out = Tree(s, {"AGENTS.md": 1_000}).run("--quiet")
            self.assertEqual(rc, 0)
            self.assertNotIn("/65,536", out)
            self.assertIn("context-ceiling: ok", out)

    def test_show_truncates_the_table_and_says_how_many_more(self):
        with contextlib.ExitStack() as s:
            t = Tree(s, {f"crates/c{i}/AGENTS.md": 1_000 + i for i in range(5)})
            rc, out = t.run("--show", "2")
            self.assertEqual(rc, 0)
            self.assertEqual(len([l for l in out.splitlines() if "/65,536" in l]), 2)
            self.assertIn("3 more, largest first", out)

    def test_the_summary_names_the_closest_unlisted_file(self):
        with contextlib.ExitStack() as s:
            t = Tree(s, {"AGENTS.md": 65_000, "crates/c/AGENTS.md": 93_334},
                     listed=[("crates/c/AGENTS.md", 93_334)])
            rc, out = t.run("--quiet")
            self.assertEqual(rc, 0)
            # The listed breach is excluded from "closest": it is already over, and reporting -27,798
            # as the headroom would make the warning useless.
            self.assertIn("closest unlisted has 536 bytes left", out)


if __name__ == "__main__":
    unittest.main()
