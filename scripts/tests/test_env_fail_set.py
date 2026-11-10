"""Tests for scripts/env-fail-set.py (theseus-4vsrz, 1.0.5 B7).

Run: python3 -m unittest -q scripts/tests/test_env_fail_set.py

The behaviour is tested on SYNTHETIC trees, and only the shape of the real one is asserted. A test that
pinned the real total would be the very defect the script exists to remove: a whole-tree number carried on
one branch, false of the merged tree the moment a sibling lands a sandbox test (theseus-4vsrz, "per-file
numeric invariants are a merge hazard as a class"). So the real-tree test asserts that each of the three
places is found and parses, never how many cases it holds.
"""

import importlib.util
import io
import os
import contextlib
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent

spec = importlib.util.spec_from_file_location("env_fail_set", os.path.join(HERE, "..", "env-fail-set.py"))
efs = importlib.util.module_from_spec(spec)
spec.loader.exec_module(efs)

HARNESS = """
mod common;
use common::Case;

/// Run only when asked.
const IGNORED: &[Case] = &[Case {
    name: "only_when_asked",
    run: live,
}];

const CASES: &[Case] = &[
    Case {
        name: "clause_01_namespaces",
        run: namespaces,
    },
    Case {
        name: "a_name_with_a_bracket_] _in_it",
        run: brackets,
    },
];
"""

INLINE = """
fn main() {
    common::main(
        &[
            Case { name: "spawn_100", run: bench },
            Case { name: "connect_first_byte_200", run: first_byte },
        ],
        &[],
        |_| {},
    );
}
"""

NEXTEST = """
fn helper() -> u8 { 1 }

#[test]
fn a_plain_test() {}

/// A doc comment between the attribute and the fn.
#[tokio::test]
#[ignore]
async fn one_that_is_ignored() {}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_tokio_test() {}

#[cfg(test)]
fn not_a_test_at_all() {}
"""


class Parsers(unittest.TestCase):
    def test_a_custom_harnesss_ignored_const_is_not_in_the_set(self):
        cases, ignored = efs.harness_cases(HARNESS)
        self.assertEqual(cases, ["clause_01_namespaces", "a_name_with_a_bracket_] _in_it"])
        self.assertEqual(ignored, ["only_when_asked"],
                         "an --ignored case does not run, so it is not an environmental failure")

    def test_a_bracket_inside_a_case_name_does_not_end_the_ignored_span(self):
        # The span is found by matching brackets, so the `]` in the second case's name cannot close
        # `const IGNORED` early and swallow the real cases into the ignored list.
        span = efs.ignored_span(HARNESS)
        self.assertIsNotNone(span)
        self.assertIn("only_when_asked", HARNESS[span[0]:span[1]])
        self.assertNotIn("clause_01_namespaces", HARNESS[span[0]:span[1]])

    def test_cases_declared_inline_in_main_are_found_and_there_is_no_ignored_const(self):
        cases, ignored = efs.harness_cases(INLINE)
        self.assertEqual(cases, ["spawn_100", "connect_first_byte_200"])
        self.assertEqual(ignored, [])
        self.assertIsNone(efs.ignored_span(INLINE), "`&[]` holds no Case, so there is no span to find")

    def test_nextest_reads_test_and_tokio_test_and_leaves_an_ignored_one_out(self):
        cases, ignored = efs.nextest_cases(NEXTEST)
        self.assertEqual(cases, ["a_plain_test", "a_tokio_test"])
        self.assertEqual(ignored, ["one_that_is_ignored"])
        self.assertNotIn("helper", cases)
        self.assertNotIn("not_a_test_at_all", cases, "a #[cfg(test)] fn is not a test")


class Totals(unittest.TestCase):
    def tree(self, **files):
        d = tempfile.mkdtemp()
        self.addCleanup(lambda: None)
        for path, text in files.items():
            p = Path(d) / path
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_text(text)
        return d

    def whole(self):
        return self.tree(**{
            "crates/theseus-sandbox/tests/contract.rs": HARNESS,
            "crates/theseus-sandbox/tests/bench.rs": INLINE,
            "crates/theseusd/tests/sandbox.rs": NEXTEST,
        })

    def run_main(self, *args):
        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            rc = efs.main(list(args))
        return rc, out.getvalue(), err.getvalue()

    def test_the_total_is_the_three_places_cases_and_never_their_ignored_ones(self):
        rc, out, _ = self.run_main("--root", self.whole())
        self.assertEqual(rc, 0, out)
        # 2 + 2 + 2 cases, and the three ignored ones (one per place at most) are out.
        self.assertIn("6 cases in 3 places", out)
        self.assertIn("(not in the set) only_when_asked", out)

    def test_expect_fails_on_drift_and_says_to_regenerate_rather_than_trust_the_number(self):
        root = self.whole()
        rc, out, err = self.run_main("--root", root, "--expect", "6")
        self.assertEqual(rc, 0, err)
        rc, out, err = self.run_main("--root", root, "--expect", "36")
        self.assertEqual(rc, 1, "a stale brief is a failure, which is the whole point")
        self.assertIn("the tree totals 6, not the 36 asked for", err)
        self.assertIn("regenerate", err)

    def test_a_tree_with_none_of_the_three_places_says_so_rather_than_reporting_zero(self):
        rc, out, err = self.run_main("--root", self.tree(**{"README.md": "nothing here\n"}))
        self.assertEqual(rc, 2, "an empty scan must not read as `no environmental failures`")
        self.assertIn("it read nothing, so it says nothing", err)

    def test_a_missing_place_is_named_and_the_others_still_counted(self):
        root = self.tree(**{"crates/theseusd/tests/sandbox.rs": NEXTEST})
        rc, out, err = self.run_main("--root", root)
        self.assertEqual(rc, 0, err)
        self.assertIn("crates/theseus-sandbox/tests/contract.rs is not in this tree", err)
        self.assertIn("2 cases in 1 places", out)

    def test_markdown_names_the_count_the_places_and_every_case(self):
        rc, out, _ = self.run_main("--root", self.whole(), "--markdown")
        self.assertEqual(rc, 0, out)
        self.assertIn("**6 tests fail on this VM", out)
        self.assertIn("`clause_01_namespaces`", out)
        self.assertIn("scripts/env-fail-set.py", out, "the block says how it was made, so it is regenerated")

    def test_names_prints_one_labelled_name_a_line(self):
        rc, out, _ = self.run_main("--root", self.whole(), "--names")
        self.assertEqual(rc, 0, out)
        lines = out.strip().split("\n")
        self.assertEqual(len(lines), 6, lines)
        self.assertIn("theseus-sandbox::bench spawn_100", lines)


class RealTree(unittest.TestCase):
    def test_each_of_the_three_places_is_in_the_tree_and_parses(self):
        found, missing = efs.collect(str(REPO))
        self.assertEqual(missing, [], "a renamed or moved place must be noticed here, not by a cloud session")
        self.assertEqual(len(found), 3, [f[0] for f in found])
        for label, path, cases, _ in found:
            self.assertTrue(cases, f"{label} ({path}) parsed to no cases at all")

    def test_the_contract_binarys_twelve_clauses_are_found_by_name(self):
        # The clauses are what a fleet gate reports green, so they are the names a reader cross-checks.
        found, _ = efs.collect(str(REPO))
        cases = next(c for label, _, c, _ in found if label == "theseus-sandbox::contract")
        for n in range(1, 13):
            self.assertTrue(any(c.startswith(f"clause_{n:02d}_") for c in cases),
                            f"no clause_{n:02d}_* among {cases}")

    def test_the_live_egress_case_is_not_counted_as_an_environmental_failure(self):
        # It reaches a public host and runs only under THESEUS_SANDBOX_LIVE_EGRESS, so no VM gate runs it.
        found, _ = efs.collect(str(REPO))
        label, _, cases, ignored = next(f for f in found if f[0] == "theseus-sandbox::contract")
        self.assertTrue(any("live" in i for i in ignored), ignored)
        self.assertFalse(any("live" in c for c in cases), cases)


if __name__ == "__main__":
    unittest.main()
