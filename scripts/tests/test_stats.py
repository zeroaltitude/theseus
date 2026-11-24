"""Tests for scripts/stats.py (issue 45): it counts a small fixture tree exactly, gives the same bytes for the same
tree, and reads build sizes from a build directory and an install directory, keeping them when given neither.

Run: python3 -m unittest -q scripts/tests/test_stats.py

Each case's expected numbers are counted by hand from FILES, line by line; a change of rule that moves one is a
change of what STATS.md means, so update the comment on that file with the number.
"""
import importlib.util
import os
import re
import shutil
import subprocess
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
spec = importlib.util.spec_from_file_location("stats", os.path.join(HERE, "..", "stats.py"))
stats = importlib.util.module_from_spec(spec)
spec.loader.exec_module(stats)

FILES = {
    # code 9: fn main, let s, let m and its two string lines, let c, let r, `}`, fn after.
    # comments 4: `//!`, the nested block's two lines, and the test module's comment (a comment, though in test
    # code). Test code 5: the rest of the `#[cfg(test)]` module, braces in its strings not closing it. Blank 2. Without nesting, `still comment */` would be code; without strings, the `//` line inside
    # `let m` would be a comment and `"/* not a comment"` would open a block that swallows the rest.
    "crates/demo/src/lib.rs": '''\
//! Crate doc.
fn main() {
    let s = "/* not a comment";
    let m = "first line
// inside a string, not a comment
";

    /* outer /* nested */
       still comment */
    let c = '"'; // a quote char, then a comment
    let r = r#"a "raw" */ string"#;
}

#[cfg(test)]
mod tests {
    // a test's comment, in test code
    #[test]
    fn t<'a>() { assert_eq!("}", "}"); }
}
fn after() {}
''',
    "crates/demo/src/main.rs": "fn main() {}\n",  # code 1
    # The two real layouts a basename-prefix rule missed, and the production sibling that must stay code:
    # a file in a `tests_*` module directory (test 1), a `*_tests` file (test 1), and `digest.rs` (code 1).
    "crates/demo/src/tests_m3/admission.rs": "fn a() {}\n",
    "crates/demo/src/digest_tests.rs": "fn d() {}\n",
    "crates/demo/src/digest.rs": "fn p() {}\n",
    "crates/demo/tests/it.rs": "// integration\n#[test]\nfn it() {}\n",  # test 2, comment 1
    "crates/demo/Cargo.toml": '[package]\nname = "demo"\n',
    # comments 4 (the shebang, the docstring's two lines, `# a comment`), code 3; blank 1.
    "scripts/tool.py": '#!/usr/bin/env python3\n"""A docstring\nover two lines."""\nX = "# not a comment"\n'
                       "# a comment\n\ndef f():\n    return X\n",
    # code 7, comment 3: a triple quote inside a string and inside a comment open nothing; a docstring with code
    # after it on its line (on its one line, and on its closing line) is code there, and comment above it; a
    # triple-quoted string that does not begin a statement is code on both its lines.
    "scripts/edge.py": "s = '\"\"\"'\n# comment\nx = 1\n# a \"\"\" in a comment\ny = 2\n"
                       '"""doc"""; z = 3\n"""doc\nend"""; w = 4\nv = """not\na docstring"""\n',
    # code 4, comment 1: the `#` line inside a quote opened on the line above is code; `$#` in quotes is code.
    "scripts/edge.sh": "s='first\n# literal\nlast'\n# a comment\necho \"$#\" # n\n",
    "cockpit/src/a.ts": "/* block */\nconst t = `multi\n// in a template\n`;\n",  # code 3, comment 1
    "cockpit/test/a.test.ts": "const s = '//x'; // c\n",  # test 1
    "vendor/x/lib.rs": "fn v() {}\n",  # shown, and kept out of the total
    "README.md": "# not code\n",
}
EXPECT = {
    "crates/demo": ["11", "9", "5"],
    "scripts/": ["14", "0", "8"],
    "cockpit/": ["3", "1", "1"],
    "**Total**": ["**28**", "**10**", "**14**"],
    "vendor/ (third-party, not in the total)": ["1", "0", "0"],
}
GIT_ENV = dict(os.environ, GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_NOSYSTEM="1")


def sized(path, n, mtime=None):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "wb") as f:
        f.truncate(n)
    os.chmod(path, 0o755)
    if mtime:
        os.utime(path, (mtime, mtime))


class StatsTest(unittest.TestCase):
    def setUp(self):
        self.root = tempfile.mkdtemp()
        self.addCleanup(shutil.rmtree, self.root)
        # Builds and the sizes record live outside the tree, here; never the machine's own record.
        self.out = tempfile.mkdtemp()
        self.addCleanup(shutil.rmtree, self.out)
        self.record = os.path.join(self.out, "stats-sizes.md")
        env = {"THESEUS_STATS_SIZES": self.record, "CARGO_TARGET_DIR": os.path.join(self.out, "target")}
        self.addCleanup(os.environ.update, {k: os.environ[k] for k in env if k in os.environ})
        for k in env:
            self.addCleanup(os.environ.pop, k, None)
        os.environ.update(env)
        for path, text in FILES.items():
            os.makedirs(os.path.join(self.root, os.path.dirname(path)), exist_ok=True)
            with open(os.path.join(self.root, path), "w", encoding="utf-8") as f:
                f.write(text)
        for cmd in (["init", "-q"], ["add", "-A"], ["commit", "-q", "-m", "fixture"]):
            self.git(*cmd)

    def git(self, *cmd):
        who = ["-c", "user.name=t", "-c", "user.email=t@example.invalid"]
        return subprocess.run(["git", "-C", self.root, *who, *cmd], check=True, env=GIT_ENV, capture_output=True,
                              text=True).stdout.strip()

    def run_stats(self, *args):
        stats.main(["--root", self.root, *args])
        with open(os.path.join(self.root, "STATS.md"), encoding="utf-8") as f:
            return f.read()

    def test_counts_the_fixture_exactly(self):
        text = self.run_stats()
        rows = {}
        for line in text.splitlines():
            cells = [c.strip() for c in line.strip("|").split("|")]
            if cells[0] in EXPECT:
                rows[cells[0]] = cells[1:]
        self.assertEqual(rows, EXPECT)
        self.assertIn(f"Counted at commit {self.git('rev-parse', '--short=8', 'HEAD')}, over every file git tracks",
                      text)

    def test_a_merge_is_named_by_its_parents(self):
        main = self.git("rev-parse", "--short=8", "HEAD")
        self.git("checkout", "-q", "-b", "side")
        self.git("commit", "-q", "--allow-empty", "-m", "side")
        side = self.git("rev-parse", "--short=8", "HEAD")
        self.git("checkout", "-q", main)
        self.git("merge", "-q", "--no-ff", "-m", "join", "side")
        self.assertIn(f"Counted at the merge of {side} into {main},", self.run_stats())

    def test_a_merge_in_progress_is_named_by_the_parents_it_will_have(self):
        main = self.git("rev-parse", "--short=8", "HEAD")
        self.git("checkout", "-q", "-b", "side")
        self.git("commit", "-q", "--allow-empty", "-m", "side")
        side = self.git("rev-parse", "--short=8", "HEAD")
        self.git("checkout", "-q", main)
        self.git("merge", "-q", "--no-ff", "--no-commit", "side")
        text = self.run_stats()
        self.assertIn(f"Counted at the merge of {side} into {main}, over", text)  # staged merge: not "uncommitted"

    def test_a_range_prints_each_total_before_and_after(self):
        before = self.git("rev-parse", "--short=8", "HEAD")
        with open(os.path.join(self.root, "crates/demo/src/more.rs"), "w") as f:
            f.write("// one comment\nfn a() {}\nfn b() {}\n")
        os.remove(os.path.join(self.root, "crates/demo/src/digest.rs"))
        self.git("add", "-A")
        self.git("commit", "-q", "-m", "more")
        after = self.git("rev-parse", "--short=8", "HEAD")
        line = stats.range_line(self.root, f"{before}..{after}")
        code, test, comment = stats.totals(stats.count_rev(self.root, before))
        # more.rs: code 2, comment 1; digest.rs, gone: code 1. So code +1, comments +1, test code unchanged.
        self.assertEqual(
            line, f"lines {before}..{after}: code {code:,} -> {code + 1:,} (+1); test code {test:,} -> {test:,} (+0); "
                  f"comments {comment:,} -> {comment + 1:,} (+1)")

    def test_the_objects_count_is_the_working_trees(self):
        self.assertEqual(stats.count_rev(self.root, "HEAD"), stats.count_tree(self.root))

    def side_branch(self, name, base="main"):
        """A branch NAME off BASE with one new file of one code line; its short id."""
        self.git("checkout", "-q", "-b", name, base)
        with open(os.path.join(self.root, f"crates/demo/src/{name}.rs"), "w") as f:
            f.write("fn x() {}\n")
        self.git("add", "-A")
        self.git("commit", "-q", "-m", name)
        return self.git("rev-parse", "--short=8", "HEAD")

    def prepare(self):
        stats.main(["--root", self.root, "--prepare-landing"])

    def test_a_prepared_landing_is_counted_from_its_tree_and_stages_stats_alone(self):
        self.git("checkout", "-q", "-B", "main")
        main = self.git("rev-parse", "--short=8", "HEAD")
        side = self.side_branch("side")
        self.git("checkout", "-q", "main")
        self.git("merge", "-q", "--no-ff", "--no-commit", "side")
        tree = self.git("write-tree")
        # A dirty working file beside the prepared merge: not in the tree that lands, so not in the count.
        with open(os.path.join(self.root, "crates/demo/src/digest.rs"), "a") as f:
            f.write("fn dirty_one() {}\nfn dirty_two() {}\n")
        self.prepare()
        staged = self.git("show", ":STATS.md")
        self.assertIn(f"Counted at the merge of {side} into {main}, over", staged)
        self.assertNotIn("uncommitted", staged)
        code, test, comment = stats.totals(stats.count_rev(self.root, tree))
        self.assertIn(f"| **Total** | **{code:,}** | **{test:,}** | **{comment:,}** |", staged)
        self.assertNotEqual(stats.totals(stats.count_tree(self.root)), [code, test, comment], "the dirty file counts there")
        # Only STATS.md was staged; the dirty file is still only in the working tree.
        self.assertEqual(self.git("diff", "--cached", "--name-only", tree), "STATS.md")
        self.assertEqual(self.git("diff", "--name-only"), "crates/demo/src/digest.rs")
        # The landing's own commit holds it, with the parents it was named by.
        self.git("commit", "-q", "-m", "land side")
        self.assertEqual(self.git("show", "HEAD:STATS.md"), staged.rstrip("\n"))
        parents = self.git("rev-list", "--parents", "-n", "1", "HEAD").split()[1:]
        self.assertEqual([p[:8] for p in parents], [main, side])

    def test_a_landing_on_top_of_an_earlier_merge_names_the_pending_one(self):
        self.git("checkout", "-q", "-B", "main")
        self.side_branch("one")
        self.git("checkout", "-q", "main")
        self.git("merge", "-q", "--no-ff", "-m", "land one", "one")
        earlier = self.git("rev-parse", "--short=8", "HEAD")
        two = self.side_branch("two")
        self.git("checkout", "-q", "main")
        self.git("merge", "-q", "--no-ff", "--no-commit", "two")
        self.prepare()
        self.assertIn(f"Counted at the merge of {two} into {earlier}, over", self.git("show", ":STATS.md"))

    def test_the_prepare_step_refuses_off_main_and_with_no_merge(self):
        self.git("checkout", "-q", "-B", "main")
        with self.assertRaises(SystemExit) as no_merge:
            self.prepare()
        self.assertIn("no merge is in progress", str(no_merge.exception))
        self.side_branch("side")
        self.git("checkout", "-q", "-b", "other", "main")
        self.git("merge", "-q", "--no-ff", "--no-commit", "side")
        with self.assertRaises(SystemExit) as off_main:
            self.prepare()
        self.assertIn("HEAD is not main", str(off_main.exception))
        self.assertFalse(os.path.exists(os.path.join(self.root, "STATS.md")), "nothing written when refused")
        self.assertEqual(self.git("diff", "--cached", "--name-only", "--", "STATS.md"), "")

    def test_the_same_tree_gives_the_same_bytes(self):
        self.assertEqual(self.run_stats(), self.run_stats())

    def test_sizes_are_read_and_then_kept(self):
        prof, inst = os.path.join(self.root, "t", "release"), os.path.join(self.root, "i", "0123abcd")
        with open(os.path.join(os.makedirs(prof) or prof, ".theseus-build-stamp"), "w") as f:
            f.write("89abcdef0123 clean 1\n")
        sized(os.path.join(prof, "deps", "libdemo-new.rlib"), 1_230_000, mtime=2_000_000_000)
        sized(os.path.join(prof, "deps", "libdemo-old.rlib"), 9_990_000, mtime=1_000_000_000)
        sized(os.path.join(prof, "demo"), 3_000_000)
        sized(os.path.join(inst, "theseusd"), 4_560_000)
        sized(os.path.join(inst, "notes"), 1)
        measured = self.run_stats("--sizes-from", prof, "--installed", inst)
        self.assertIn("From the `release` build of 89abcdef", measured)
        self.assertIn("| crates/demo | 1.23 MB | demo 3.00 MB |", measured)
        self.assertIn("| theseusd | 4.56 MB | 0123abcd |", measured)
        self.assertIn("| **Total** | **4.56 MB** | |", measured)
        self.assertNotIn("notes", measured)
        self.assertEqual(self.run_stats(), measured)  # a recount with no sizes given leaves them as they were

    def test_sizes_never_measured_say_so(self):
        self.assertEqual(self.run_stats().count("Not yet measured"), 2)

    def build(self, commit, digest="clean", where=None):
        """An optimised build's profile directory, stamped as a build of COMMIT, outside the tree."""
        prof = where or os.path.join(self.out, f"build-{commit[:8]}-{digest}", "release")
        os.makedirs(prof, exist_ok=True)
        with open(os.path.join(prof, ".theseus-build-stamp"), "w") as f:
            f.write(f"{commit} {digest} 1\n")
        sized(os.path.join(prof, "deps", "libdemo-x.rlib"), 1_230_000)
        sized(os.path.join(prof, "demo"), 3_000_000)
        return prof

    def commit_on(self, branch, name):
        """One more commit on BRANCH (a file of one code line); its full id."""
        self.git("checkout", "-q", branch)
        with open(os.path.join(self.root, f"crates/demo/src/{name}.rs"), "w") as f:
            f.write("fn y() {}\n")
        self.git("add", "-A")
        self.git("commit", "-q", "-m", name)
        return self.git("rev-parse", "HEAD")

    def recorded(self, commit):
        """The sizes record an install build of COMMIT leaves (what `--record-sizes` writes)."""
        with open(self.record, "w", encoding="utf-8") as f:
            f.write(stats.crate_sizes(self.root, self.build(commit)))

    def main_with_sizes_of(self, commit):
        """main, its STATS.md committed with the sizes of a build of COMMIT."""
        self.git("checkout", "-q", "-B", "main")
        self.run_stats("--sizes-from", self.build(commit))
        self.git("add", "STATS.md")
        self.git("commit", "-q", "-m", "stats")

    def staged_sizes_at_a_landing(self):
        """Land a fresh side branch on main with --prepare-landing; the commit its staged sizes name."""
        self.landings = getattr(self, "landings", 0) + 1
        side = self.side_branch(f"landing{self.landings}")
        self.git("checkout", "-q", "main")
        self.git("merge", "-q", "--no-ff", "--no-commit", side)
        self.prepare()
        staged = self.git("show", ":STATS.md")
        self.git("merge", "--abort")
        return re.search(r"From the `release` build of ([0-9a-f]{8}) ", staged).group(1)

    def test_an_install_build_records_its_sizes_outside_the_tree(self):
        head = self.git("rev-parse", "HEAD")
        stats.main(["--root", self.root, "--sizes-from", self.build(head), "--record-sizes", self.record])
        with open(self.record, encoding="utf-8") as f:
            text = f.read()
        self.assertTrue(text.startswith("### What each crate builds\n"), text)
        self.assertIn(f"From the `release` build of {head[:8]} ", text)
        self.assertIn("| crates/demo | 1.23 MB | demo 3.00 MB |", text)
        self.assertEqual(self.git("status", "--porcelain", "--untracked-files=all"), "", "nothing in the tree")

    def test_a_landing_takes_the_newest_recorded_sizes_of_main(self):
        a = self.git("rev-parse", "HEAD")
        self.main_with_sizes_of(a)
        b = self.commit_on("main", "later")
        self.recorded(b)
        self.assertEqual(self.staged_sizes_at_a_landing(), b[:8])

    def test_a_landing_never_takes_older_or_foreign_sizes(self):
        a = self.git("rev-parse", "HEAD")
        self.git("checkout", "-q", "-B", "main")
        b = self.commit_on("main", "later")
        self.main_with_sizes_of(b)
        self.git("checkout", "-q", "-b", "foreign", a)
        foreign = self.commit_on("foreign", "foreign")
        self.git("checkout", "-q", "main")
        for older, why in ((a, "older than the sizes main has"), (foreign, "a build of a commit not on main")):
            with self.subTest(why):
                # Both a record and a stamped release build in the target directory, of that commit.
                self.recorded(older)
                self.build(older, where=os.path.join(os.environ["CARGO_TARGET_DIR"], "release"))
                self.assertEqual(self.staged_sizes_at_a_landing(), b[:8], why)

    def test_a_dirty_build_records_nothing(self):
        head = self.git("rev-parse", "HEAD")
        with self.assertRaises(SystemExit) as refused:
            stats.main(["--root", self.root, "--sizes-from", self.build(head, digest="0123456789abcdef"),
                        "--record-sizes", self.record])
        self.assertIn("not a clean build of one commit", str(refused.exception.code))
        self.assertFalse(os.path.exists(self.record), "no record of a build that is not of one commit")

    def test_the_count_at_head_names_a_made_merge_and_stages_nothing(self):
        self.git("checkout", "-q", "-B", "main")
        self.main_with_sizes_of(self.git("rev-parse", "HEAD"))
        main = self.git("rev-parse", "--short=8", "HEAD")
        sizes = stats.kept(self.git("show", "HEAD:STATS.md"), stats.SIZES[0])
        side = self.side_branch("side")
        # The join's tree: a made merge, HEAD detached at it.
        self.git("checkout", "-q", "--detach", "main")
        self.git("merge", "-q", "--no-ff", "-m", "join", "side")
        merge = self.git("rev-parse", "HEAD")
        stats.main(["--root", self.root, "--count-head"])
        with open(os.path.join(self.root, "STATS.md"), encoding="utf-8") as f:
            text = f.read()
        self.assertIn(f"Counted at the merge of {side} into {main}, over", text)
        code, test, comment = stats.totals(stats.count_rev(self.root, merge))
        self.assertIn(f"| **Total** | **{code:,}** | **{test:,}** | **{comment:,}** |", text)
        self.assertIn(sizes, text, "the sizes are kept")
        self.assertEqual(self.git("rev-parse", "HEAD"), merge)
        self.assertEqual(self.git("diff", "--cached", "--name-only"), "", "nothing staged")
        self.assertEqual(self.git("status", "--porcelain", "--untracked-files=all"), "M STATS.md")
        # A HEAD that is not a made merge, and a merge still in progress, are refused, and nothing is written.
        self.git("checkout", "-q", "--", "STATS.md")
        for setup, why in ((("checkout", "-q", "--detach", "side"),), "not a merge"), (
                (("checkout", "-q", "--detach", "main"), ("merge", "-q", "--no-ff", "--no-commit", "side")), "in progress"):
            with self.subTest(why):
                for cmd in setup:
                    self.git(*cmd)
                with self.assertRaises(SystemExit):
                    stats.main(["--root", self.root, "--count-head"])
                self.assertNotIn("STATS.md", self.git("status", "--porcelain", "--untracked-files=all"))


if __name__ == "__main__":
    unittest.main()
