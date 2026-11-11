"""Tests for scripts/test-code-map.py (theseus-4vsrz, 1.0.5 B7).

Run: python3 -m unittest -q scripts/tests/test_test_code_map.py

Each case is one of the resolution rules the walk got wrong first, and the
figure it cost on the real tree is in the case's own comment. They are here
because every one of them made the walk under-report, and an under-reporting
completeness walk passes on an empty set.
"""

import importlib.util
import os
import subprocess
import sys
import tempfile
import textwrap
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
SCRIPT = os.path.join(HERE, "..", "test-code-map.py")

spec = importlib.util.spec_from_file_location("test_code_map", SCRIPT)
tcm = importlib.util.module_from_spec(spec)
spec.loader.exec_module(tcm)


def write(root, rel, body):
    path = os.path.join(root, rel)
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", encoding="utf-8") as handle:
        handle.write(textwrap.dedent(body).lstrip("\n"))
    return path


def crate(root, name, manifest_extra=""):
    write(root, f"crates/{name}/Cargo.toml", f"""
        [package]
        name = "{name}"
        version = "0.1.0"
        {manifest_extra}
    """)


def run(root, *args):
    out = subprocess.run(
        [sys.executable, "-I", SCRIPT, root, *args],
        capture_output=True, text=True, check=False)
    return out.returncode, out.stdout + out.stderr


class CodeOnly(unittest.TestCase):
    def test_a_raw_string_holding_braces_does_not_close_the_block(self):
        # The real cost: a `r#"{ .. }"#` fixture at
        # crates/theseus-aws-catalog/src/lib.rs:361 closed its `mod tests`
        # block 190 lines early and hid 32 tests.
        lines = [
            "mod tests {",
            '    let j = r#"{"a": {"b": 1}}"#;',
            "}",
        ]
        self.assertEqual(tcm.block_end(tcm.code_only(lines), 0), 2)

    def test_a_brace_in_a_char_literal_is_not_a_brace(self):
        lines = ["mod tests {", "    let c = '{';", "}"]
        self.assertEqual(tcm.block_end(tcm.code_only(lines), 0), 2)

    def test_a_brace_in_a_comment_is_not_a_brace(self):
        lines = ["mod tests {", "    // }", "    /* } */", "}"]
        self.assertEqual(tcm.block_end(tcm.code_only(lines), 0), 3)

    def test_a_raw_string_spanning_lines_is_blanked_throughout(self):
        lines = ["mod tests {", '    let j = r#"{', "      }", '    "#;', "}"]
        self.assertEqual(tcm.block_end(tcm.code_only(lines), 0), 4)


class ModuleDir(unittest.TestCase):
    def test_a_mod_rs_resolves_against_its_own_directory(self):
        self.assertEqual(tcm.module_dir(os.path.join("a", "b", "mod.rs")),
                         os.path.join("a", "b"))

    def test_any_other_file_resolves_against_its_stem(self):
        self.assertEqual(tcm.module_dir(os.path.join("a", "turn.rs")),
                         os.path.join("a", "turn"))


class Walk(unittest.TestCase):
    def regions(self, root):
        walk = tcm.Walk(root)
        walk.crates(os.path.join(root, "crates"))
        return {path: kind for path, _, _, kind in walk.regions}

    def test_a_path_attribute_is_followed(self):
        # The real cost: crates/theseus-core/src/turn/admission.rs:1059
        # declares turn/tests_admission.rs this way; missing it hid 376
        # test functions.
        with tempfile.TemporaryDirectory() as root:
            crate(root, "k")
            write(root, "crates/k/src/lib.rs", "pub mod admission;\n")
            write(root, "crates/k/src/admission.rs", """
                #[cfg(test)]
                #[path = "tests_admission.rs"]
                mod tests;
            """)
            write(root, "crates/k/src/tests_admission.rs", "#[test]\nfn t() {}\n")
            self.assertIn("crates/k/src/tests_admission.rs", self.regions(root))

    def test_a_mod_inside_an_inline_test_mod_resolves_under_it_and_is_test_code(self):
        # The real cost: push.rs:622-625 declares push/tests/{seed,short,
        # why,work}.rs from inside `mod tests {`, and tests_m3.rs:6134
        # declares tests_m3/parallel/{order,runs}.rs the same way.
        with tempfile.TemporaryDirectory() as root:
            crate(root, "k")
            write(root, "crates/k/src/lib.rs", "pub mod push;\n")
            write(root, "crates/k/src/push.rs", """
                #[cfg(test)]
                mod tests {
                    mod seed;
                }
            """)
            write(root, "crates/k/src/push/tests/seed.rs", "#[test]\nfn t() {}\n")
            # a same-named production module beside it must not be taken instead
            write(root, "crates/k/src/push/seed.rs", "pub fn p() {}\n")
            found = self.regions(root)
            self.assertEqual(found.get("crates/k/src/push/tests/seed.rs"),
                             "cfg(test) file module")
            self.assertNotIn("crates/k/src/push/seed.rs", found)

    def test_an_integration_target_is_test_code_and_autotests_false_is_honoured(self):
        with tempfile.TemporaryDirectory() as root:
            crate(root, "k")
            write(root, "crates/k/src/lib.rs", "pub fn p() {}\n")
            write(root, "crates/k/tests/live.rs", "#[test]\nfn t() {}\n")
            self.assertEqual(self.regions(root).get("crates/k/tests/live.rs"),
                             "[[test]] target")
        with tempfile.TemporaryDirectory() as root:
            crate(root, "k", 'autotests = false')
            write(root, "crates/k/src/lib.rs", "pub fn p() {}\n")
            write(root, "crates/k/tests/live.rs", "#[test]\nfn t() {}\n")
            self.assertNotIn("crates/k/tests/live.rs", self.regions(root))


class Orphans(unittest.TestCase):
    def test_a_test_file_no_declaration_reaches_is_reported(self):
        # The real finding: crates/theseus-core/src/tests_fan_out_cue.rs, four
        # tokio tests, declaration removed at 0925819c and never re-added,
        # file edited twice since.
        with tempfile.TemporaryDirectory() as root:
            crate(root, "k")
            write(root, "crates/k/src/lib.rs", """
                #[cfg(test)]
                mod tests;
            """)
            write(root, "crates/k/src/tests.rs", "#[test]\nfn reached() {}\n")
            write(root, "crates/k/src/tests_orphan.rs", """
                #[tokio::test]
                async fn never_run() {}
            """)
            code, out = run(root)
            self.assertEqual(code, 0, out)
            self.assertIn("ORPHANED test code: 1 test functions in 1 files", out)
            self.assertIn("crates/k/src/tests_orphan.rs: 1 at line 1", out)
            self.assertNotIn("tests.rs: 1", out)

    def test_cfg_test_is_not_mistaken_for_a_test_attribute(self):
        # A looser attribute pattern matched `#[cfg(test)]` itself, and every
        # production file in the tree then read as an orphan.
        with tempfile.TemporaryDirectory() as root:
            crate(root, "k")
            write(root, "crates/k/src/lib.rs", """
                pub fn p() {}

                #[cfg(test)]
                mod tests {
                    #[test]
                    fn t() {}
                }
            """)
            code, out = run(root)
            self.assertEqual(code, 0, out)
            self.assertIn("ORPHANED test code: none.", out)

    def test_a_tests_shaped_path_is_not_itself_evidence(self):
        # Membership comes from the declaration, so a file merely NAMED like
        # test code is an orphan, and a declared file not named like one is
        # test code.
        with tempfile.TemporaryDirectory() as root:
            crate(root, "k")
            write(root, "crates/k/src/lib.rs", """
                #[cfg(test)]
                #[path = "checks.rs"]
                mod tests;
            """)
            write(root, "crates/k/src/checks.rs", "#[test]\nfn t() {}\n")
            write(root, "crates/k/src/tests_looks_like_it.rs", "#[test]\nfn t() {}\n")
            code, out = run(root)
            self.assertEqual(code, 0, out)
            self.assertIn("crates/k/src/tests_looks_like_it.rs: 1", out)
            self.assertNotIn("crates/k/src/checks.rs", out.split("ORPHANED")[1])


class Sweep(unittest.TestCase):
    def test_a_tell_with_a_source_beside_it_is_cleared_and_one_without_is_not(self):
        with tempfile.TemporaryDirectory() as root:
            crate(root, "k")
            write(root, "crates/k/src/lib.rs", """
                #[cfg(test)]
                mod tests {
                    /// The pick has to be a switch, and nothing here says why.
                    #[test]
                    fn unsourced() {}

                    /// The pick has to be a switch (theseus-a2kh6).
                    #[test]
                    fn sourced() {}
                }
            """)
            code, out = run(root, "--sweep")
            self.assertEqual(code, 0, out)
            self.assertIn("with a source beside them: 1", out)
            self.assertIn("and NO source, to read by hand: 1", out)
            self.assertIn("[has-to-be]", out)
            self.assertIn("nothing here says why", out)

    def test_a_blank_comment_line_bounds_the_paragraph(self):
        with tempfile.TemporaryDirectory() as root:
            crate(root, "k")
            write(root, "crates/k/src/lib.rs", """
                #[cfg(test)]
                mod tests {
                    /// theseus-a2kh6 decided this.
                    ///
                    /// The pick has to be a switch.
                    #[test]
                    fn t() {}
                }
            """)
            code, out = run(root, "--sweep")
            self.assertEqual(code, 0, out)
            # The row id is in a DIFFERENT paragraph, so it does not clear it.
            self.assertIn("and NO source, to read by hand: 1", out)


class ScannedNothing(unittest.TestCase):
    def test_no_crates_directory_exits_2(self):
        with tempfile.TemporaryDirectory() as root:
            code, out = run(root)
            self.assertEqual(code, 2, out)
            self.assertIn("no crates/", out)

    def test_a_tree_with_crates_but_no_test_code_exits_2(self):
        with tempfile.TemporaryDirectory() as root:
            crate(root, "k")
            write(root, "crates/k/src/lib.rs", "pub fn p() {}\n")
            code, out = run(root)
            self.assertEqual(code, 2, out)
            self.assertIn("a report over nothing is a lie", out)


if __name__ == "__main__":
    unittest.main()
