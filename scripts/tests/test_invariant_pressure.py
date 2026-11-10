"""Tests for scripts/invariant-pressure.py (theseus-4vsrz, 1.0.5 B7).

Run: python3 -m unittest -q scripts/tests/test_invariant_pressure.py

The gate runs nothing under scripts/tests/, so run this by hand beside a change to the script. Each case
builds a throwaway git repository with its own scripts/long-files.txt and its own Rust files, so no case
depends on the real tree's line counts; the two that do read the real tree assert only that the scan is not
empty, because an empty scan must not look like no pressure.
"""
import contextlib
import importlib.util
import os
import subprocess
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, "..", ".."))
spec = importlib.util.spec_from_file_location("invariant_pressure", os.path.join(HERE, "..", "invariant-pressure.py"))
ip = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ip)

GIT = ["-c", "user.name=t", "-c", "user.email=t@example.invalid", "-c", "commit.gpgsign=false",
       "-c", "init.defaultBranch=main"]


def git(cwd, *args):
    # The developer's global config is ignored on purpose: a locked signing agent hangs a commit.
    env = dict(os.environ, HOME=cwd, GIT_CONFIG_GLOBAL=os.path.join(cwd, ".gitconfig-none"),
               GIT_CONFIG_SYSTEM=os.path.join(cwd, ".gitconfig-none"))
    return subprocess.run(["git", *GIT, *args], cwd=cwd, env=env, capture_output=True, text=True, check=True).stdout


class Tree:
    """A throwaway repository with a scripts/long-files.txt and crates/ of its own."""

    def __init__(self, stack, files, listed=()):
        self.dir = stack.enter_context(tempfile.TemporaryDirectory())
        os.makedirs(os.path.join(self.dir, "scripts"))
        for path, lines in files.items():
            os.makedirs(os.path.join(self.dir, os.path.dirname(path)), exist_ok=True)
            with open(os.path.join(self.dir, path), "w", encoding="utf-8") as f:
                f.write("".join(f"// line {i}\n" for i in range(lines)))
        with open(os.path.join(self.dir, "scripts", "long-files.txt"), "w", encoding="utf-8") as f:
            f.write("# a ceiling each\n")
            for path, c in listed:
                f.write(f"{path} {c} because\n")
        git(self.dir, "init", "-q")
        git(self.dir, "add", "-A")
        git(self.dir, "commit", "-qm", "base")

    def rows(self, margin=50):
        cwd = os.getcwd()
        os.chdir(self.dir)
        try:
            return ip.file_rows(margin)
        finally:
            os.chdir(cwd)

    def grow(self, path, lines):
        with open(os.path.join(self.dir, path), "a", encoding="utf-8") as f:
            f.write("".join(f"// more {i}\n" for i in range(lines)))

    def grown(self, rev="HEAD"):
        cwd = os.getcwd()
        os.chdir(self.dir)
        try:
            return ip.grown(rev)
        finally:
            os.chdir(cwd)


class FileCeilings(unittest.TestCase):
    def test_a_listed_file_is_named_with_both_numbers_and_its_headroom(self):
        with contextlib.ExitStack() as stack:
            t = Tree(stack, {"crates/a/src/big.rs": 3021}, listed=[("crates/a/src/big.rs", 3030)])
            rows = t.rows()
            self.assertEqual(rows, [(9, "crates/a/src/big.rs", 3021, 3030, "")])

    def test_two_growths_that_each_fit_alone_are_over_together(self):
        """The class itself: 9 lines of headroom, two branches of +5 and +6."""
        with contextlib.ExitStack() as stack:
            t = Tree(stack, {"crates/a/src/big.rs": 3021}, listed=[("crates/a/src/big.rs", 3030)])
            t.grow("crates/a/src/big.rs", 5)
            self.assertEqual(t.rows()[0][0], 4, "a +5 branch alone still fits")
            t.grow("crates/a/src/big.rs", 6)
            left, path, n, limit, _ = t.rows()[0]
            self.assertEqual((left, n, limit), (-2, 3032, 3030))
            self.assertEqual(path, "crates/a/src/big.rs")

    def test_an_unlisted_file_under_the_default_says_no_entry_is_possible(self):
        with contextlib.ExitStack() as stack:
            t = Tree(stack, {"crates/a/src/mid.rs": 2497})
            left, path, n, limit, note = t.rows()[0]
            self.assertEqual((left, n, limit), (3, 2497, 2500))
            self.assertIn("refuses one at or under 2500", note)

    def test_a_listed_file_that_fell_to_the_default_says_its_entry_is_stale(self):
        """shape.sh's other direction: an entry left standing for a shrunken file fails too."""
        with contextlib.ExitStack() as stack:
            t = Tree(stack, {"crates/a/src/was_big.rs": 2480}, listed=[("crates/a/src/was_big.rs", 3000)])
            note = t.rows(margin=600)[0][4]
            self.assertIn("deleted", note)

    def test_a_file_with_room_to_spare_is_not_reported(self):
        with contextlib.ExitStack() as stack:
            t = Tree(stack, {"crates/a/src/small.rs": 100}, listed=[])
            self.assertEqual(t.rows(), [])

    def test_the_range_column_counts_what_the_range_added(self):
        with contextlib.ExitStack() as stack:
            t = Tree(stack, {"crates/a/src/big.rs": 3021}, listed=[("crates/a/src/big.rs", 3030)])
            t.grow("crates/a/src/big.rs", 7)
            self.assertEqual(t.grown("HEAD").get("crates/a/src/big.rs"), 7)


class CensusRoster(unittest.TestCase):
    def test_a_spec_is_located_with_its_item_least_and_stale_fails(self):
        src = '''
fn spec() -> MustBeComplete<'static> {
    MustBeComplete {
        item: "a denied system call",
        list: "denied()",
        world: "MUST_DENY",
        add: "add it",
        drop: "drop it",
        least: 45,
        stale_fails: true,
    }
}
'''
        with tempfile.TemporaryDirectory() as d:
            p = os.path.join(d, "x.rs")
            with open(p, "w", encoding="utf-8") as f:
                f.write(src)
            got = list(ip.CENSUS.finditer(src))
            self.assertEqual(len(got), 1)
            fields = dict(ip.FIELD.findall(got[0].group(1)))
            self.assertEqual(fields["least"], "45")
            self.assertEqual(fields["stale_fails"], "true")
            self.assertIn("denied system call", fields["item"])

    def test_a_derived_least_is_called_derived_and_not_a_number(self):
        """`least,` in Rust's field shorthand is a value computed at test time: there is no number to go stale."""
        body = 'item: "a field",\n        least,\n        stale_fails: true,\n'
        self.assertEqual(set(ip.SHORTHAND.findall(body)), {"least"})
        self.assertNotIn("least", dict(ip.FIELD.findall(body)))


class RealTree(unittest.TestCase):
    def test_the_repository_scan_finds_ceilings_functions_and_censuses(self):
        cwd = os.getcwd()
        os.chdir(ROOT)
        try:
            self.assertTrue(ip.ceilings(), "scripts/long-files.txt holds no entry")
            self.assertGreater(len(ip.rust_files()), 500, "it read a small tree")
            cen = ip.censuses()
            self.assertGreater(len(cen), 5, f"only {len(cen)} censuses found: the finder stopped matching")
            self.assertTrue(any(c[4] == "true" for c in cen), "no census with stale_fails was found")
        finally:
            os.chdir(cwd)

    def test_a_report_of_pressure_still_exits_zero(self):
        """It is a warning instrument: making it a gate would be a second ceiling (line-pressure.py's reason)."""
        r = subprocess.run(["python3", os.path.join(ROOT, "scripts", "invariant-pressure.py")],
                           capture_output=True, text=True)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("invariant-pressure:", r.stdout)
        self.assertIn("censuses:", r.stdout)


if __name__ == "__main__":
    unittest.main()
