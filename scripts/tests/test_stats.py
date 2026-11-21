"""Tests for scripts/stats.py (issue 45): it counts a small fixture tree exactly, gives the same bytes for the same
tree, and reads build sizes from a build directory and an install directory, keeping them when given neither.

Run: python3 -m unittest -q scripts/tests/test_stats.py

Each case's expected numbers are counted by hand from FILES, line by line; a change of rule that moves one is a
change of what STATS.md means, so update the comment on that file with the number.
"""
import importlib.util
import os
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


if __name__ == "__main__":
    unittest.main()
