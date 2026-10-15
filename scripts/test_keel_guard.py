"""The keel guard's planted-erosion suite (theseus-pw1q.1): each case builds a throwaway git repository, commits a
base and a head, and runs scripts/keel-guard.py over the range. Standard library only; the gate's keel step runs it
(`python3 -m unittest scripts/test_keel_guard.py`). The signed cases make throwaway keys in the case's own GNUPGHOME,
and are skipped, saying so, where there is no gpg.
"""

import os
import re
import shutil
import subprocess
import tempfile
import textwrap
import unittest
from pathlib import Path

GUARD = Path(__file__).resolve().parent / "keel-guard.py"

LIB = """\
pub fn add(a: u32, b: u32) -> u32 {
    a + b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adding_two_numbers_sums_them() {
        let n = add(2, 3);
        assert_eq!(n, 5);
        assert!(n > 4, "a sum is no smaller than a part");
    }

    #[tokio::test]
    async fn a_zero_changes_nothing() {
        assert_eq!(add(7, 0), 7);
    }

    fn helper() -> u32 {
        3
    }
}
"""

LIFECYCLE = """\
pub fn budget_ms(phase: &str, sessions: u64) -> Option<f64> {
    let cold = 50.0 + 200.0 * sessions.min(10_000) as f64 / 10_000.0;
    match phase {
        "cold" | "vault" => Some(cold),
        "shutdown" => Some(100.0),
        "cancel" => Some(100.0),
        _ => None,
    }
}

pub const CANCEL_FRAMES: usize = 2;
"""

FIRST_REQUEST = """\
/// The most tokens a cli or task place's first request may be, as sent.
pub const CLI_TASK_TOKENS: u64 = 8_000;
/// The most in the owner's DM place.
pub const DM_TOKENS: u64 = 15_000;
pub const CHANNEL_TOKENS: u64 = 7_900;
pub const HEADLESS_TOKENS: u64 = 8_600;
"""

BENCH = """\
[model]
max_loops = 200                         # set by the adapter
max_loops_mode = "end"

[kernel]
spend_limit_usd = 2.0
spend_limit_mode = "ask"
"""

LONG = """\
# Rust files over 2,500 lines.
crates/demo/src/big.rs 3000 the demo's big file
crates/demo/src/huge.rs 2600 another
"""

NEXTEST = """\
[profile.default]
test-threads = 8
slow-timeout = { period = "60s", terminate-after = 2 }
leak-timeout = "500ms"
"""

PYTEST = """\
import unittest


class Drive(unittest.TestCase):
    def test_a_trial_ends_at_its_limit(self):
        self.assertEqual(1 + 1, 2)
        self.assertLess(len("ab"), 3)
        assert True
"""

TIMING = """\
#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    #[test]
    fn a_start_is_quick() {
        let t = Instant::now();
        let took = t.elapsed();
        assert!(took < Duration::from_millis(50), "a start took {took:?}");
        assert_eq!(took.as_secs(), 0);
        assert_eq!(took.is_zero(), false);
    }
}
"""

LIMITS = """\
/// `[kernel] spend_limit_usd`'s default.
pub(super) fn default_spend_limit_usd() -> f64 {
    100.0
}

pub const CAP_USD: f64 = 5.0;
pub const LAMBDA_USD_PER_GB_SECOND: f64 = 0.000_016_666_7;
"""

COCKPIT = """\
// The ring's tests, run by `npm test`.
import { describe, test } from 'node:test'
import assert from 'node:assert/strict'

test('the ring fades within a second', () => {
  assert.ok(RING_S > 0 && RING_S <= 1, `it fades: ${RING_S} s`)
  assert.equal(depthOf(379, false), 'fleet')
})

describe('the sea', () => {
  test("it shows at fleet depth only", () => {
    assert.match(RING_VERT, /fleet/)
    assert.ok(code(RING_VERT).length > 3) // a few lines (at least)
  })
})
"""

CI = """\
name: ci
on:
  push:
    branches: [main]
jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: the cockpit
        run: cd cockpit && npm ci && npm run lint && npm test && npm run build
      - run: cargo clippy --workspace --all-targets -q -- -D warnings
      - uses: EmbarkStudios/cargo-deny-action@v2
        with:
          command: check licenses sources bans
"""

BASE = {
    "crates/demo/src/lib.rs": LIB,
    "crates/theseus-core/src/config/limits.rs": LIMITS,
    "cockpit/test/ring.test.ts": COCKPIT,
    ".github/workflows/ci.yml": CI,
    "crates/demo/src/timing.rs": TIMING,
    "crates/theseus-sim/src/lifecycle.rs": LIFECYCLE,
    "crates/theseus-core/src/first_request.rs": FIRST_REQUEST,
    "bench/theseus-bench.toml": BENCH,
    "scripts/long-files.txt": LONG,
    "scripts/shape.sh": "#!/usr/bin/env bash\nlimit=2500\n",
    ".config/nextest.toml": NEXTEST,
    "bench/recall/test_drive.py": PYTEST,
    "Cargo.toml": '[workspace.lints.clippy]\ntoo_many_lines = "warn"\n',
    "crates/theseus-core/config/theseus.example.toml":
        "[kernel]\nspend_limit_usd = 100.0\ndaily_spend_ceiling_usd = 200.0\n\n# [memory]\n"
        "# synth_limit_usd_per_day = 0.50\n",
}


def sub(text, old, new):
    assert old in text, old
    return text.replace(old, new, 1)


class Repo:
    def __init__(self, root, gnupg=None):
        self.root = Path(root)
        self.env = {
            **os.environ,
            "GIT_CONFIG_GLOBAL": os.devnull,
            "GIT_CONFIG_NOSYSTEM": "1",
            "GIT_AUTHOR_NAME": "Wren Fixture",
            "GIT_AUTHOR_EMAIL": "wren@example.invalid",
            "GIT_COMMITTER_NAME": "Wren Fixture",
            "GIT_COMMITTER_EMAIL": "wren@example.invalid",
        }
        self.env.pop("THESEUS_KEEL_BASE", None)
        if gnupg:
            self.env["GNUPGHOME"] = gnupg
        self.git("init", "-q", "-b", "main")

    def git(self, *args):
        return subprocess.run(["git", *args], cwd=self.root, env=self.env, check=True, capture_output=True,
                              text=True).stdout

    def write(self, files):
        for path, text in files.items():
            p = self.root / path
            if text is None:
                p.unlink()
                continue
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_text(text)

    def commit(self, files, message="change", sign=None):
        self.write(files)
        self.git("add", "-A")
        args = ["commit", "-q", "--allow-empty", "-m", message]
        if sign:
            args = ["-c", f"user.signingkey={sign}", *args, "-S"]
        self.git(*args)

    def guard(self, *args):
        p = subprocess.run(["python3", str(GUARD), *args], cwd=self.root, env=self.env, capture_output=True,
                           text=True)
        return p.returncode, p.stdout + p.stderr


class Case(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.mkdtemp(prefix="keel-")
        self.repo = Repo(self.dir)
        self.repo.commit(dict(BASE), "base")
        self.repo.git("checkout", "-q", "-b", "lane")

    def tearDown(self):
        shutil.rmtree(self.dir, ignore_errors=True)

    def head(self, files, message="the lane's change"):
        base = dict(BASE)
        self.repo.commit({k: v for k, v in files.items()}, message)
        return base

    def findings(self, *args):
        code, out = self.repo.guard(*args)
        self.assertIn(code, (0, 1), out)
        return code, out

    def assertCaught(self, rule, path, files):
        self.head(files)
        code, out = self.findings()
        self.assertEqual(code, 1, out)
        self.assertRegex(out, rf"{path}:\d+: {rule}: ", out)
        self.assertIn(f"Keel: {rule} {path}", out)

    def assertClean(self, files):
        self.head(files)
        code, out = self.findings()
        self.assertEqual(code, 0, out)
        self.assertIn("keel: ok", out)


class Erosions(Case):
    """The eight the brief names, and the rest of the rules, each caught by name."""

    def test_a_deleted_test(self):
        lib = sub(LIB, "    #[tokio::test]\n    async fn a_zero_changes_nothing() {\n"
                       "        assert_eq!(add(7, 0), 7);\n    }\n\n", "")
        self.assertCaught("test-removed", "crates/demo/src/lib.rs", {"crates/demo/src/lib.rs": lib})

    def test_an_ignore_added(self):
        lib = sub(LIB, "    #[test]\n    fn adding", "    #[test]\n    #[ignore = \"flaky\"]\n    fn adding")
        self.assertCaught("test-ignored", "crates/demo/src/lib.rs", {"crates/demo/src/lib.rs": lib})

    def test_an_assertion_deleted(self):
        lib = sub(LIB, '        assert!(n > 4, "a sum is no smaller than a part");\n', "")
        self.assertCaught("assert-removed", "crates/demo/src/lib.rs", {"crates/demo/src/lib.rs": lib})

    def test_an_allow_added(self):
        lib = sub(LIB, "pub fn add(", "#[allow(clippy::too_many_lines)]\npub fn add(")
        self.assertCaught("allow-added", "crates/demo/src/lib.rs", {"crates/demo/src/lib.rs": lib})

    def test_a_lifecycle_budget_raised(self):
        lc = sub(LIFECYCLE, "let cold = 50.0", "let cold = 60.0")
        self.assertCaught("budget-raised", "crates/theseus-sim/src/lifecycle.rs",
                          {"crates/theseus-sim/src/lifecycle.rs": lc})

    def test_a_first_request_token_budget_raised(self):
        fr = sub(FIRST_REQUEST, "DM_TOKENS: u64 = 15_000;", "DM_TOKENS: u64 = 16_000;")
        self.assertCaught("budget-raised", "crates/theseus-core/src/first_request.rs",
                          {"crates/theseus-core/src/first_request.rs": fr})

    def test_a_first_request_token_budget_dropped(self):
        fr = sub(FIRST_REQUEST, "pub const CLI_TASK_TOKENS: u64 = 8_000;\n", "")
        self.assertCaught("budget-raised", "crates/theseus-core/src/first_request.rs",
                          {"crates/theseus-core/src/first_request.rs": fr})

    def test_a_bench_max_loops_raised(self):
        self.assertCaught("cap-raised", "bench/theseus-bench.toml",
                          {"bench/theseus-bench.toml": sub(BENCH, "max_loops = 200", "max_loops = 400")})

    def test_a_long_files_ceiling_raised(self):
        self.assertCaught("ceiling-raised", "scripts/long-files.txt",
                          {"scripts/long-files.txt": sub(LONG, "big.rs 3000", "big.rs 3100")})

    def test_the_flaky_list_given_a_test(self):
        nx = NEXTEST + ("\n[[profile.default.overrides]]\n"
                        "filter = 'package(demo) & test(=tests::adding_two_numbers_sums_them)'\nretries = 2\n")
        self.assertCaught("ceiling-raised", ".config/nextest.toml", {".config/nextest.toml": nx})

    def test_a_test_deleted_beside_an_unlike_one_added(self):
        lib = sub(LIB, "    #[tokio::test]\n    async fn a_zero_changes_nothing() {\n"
                       "        assert_eq!(add(7, 0), 7);\n    }\n\n", "")
        lib = sub(lib, "    fn helper()", "    #[test]\n    fn the_helper_is_three() {\n"
                                         "        let h = helper();\n        assert!(h == 3 && h % 3 == 0);\n"
                                         "    }\n\n    fn helper()")
        self.assertCaught("test-removed", "crates/demo/src/lib.rs", {"crates/demo/src/lib.rs": lib})

    def test_a_python_test_deleted(self):
        self.assertCaught("test-removed", "bench/recall/test_drive.py", {"bench/recall/test_drive.py": sub(
            PYTEST, "    def test_a_trial_ends_at_its_limit(self):\n", "    def helper(self):\n")})

    def test_a_python_skip_added(self):
        self.assertCaught("test-ignored", "bench/recall/test_drive.py", {"bench/recall/test_drive.py": sub(
            PYTEST, "    def test_a", '    @unittest.skip("later")\n    def test_a')})

    def test_a_python_assertion_deleted(self):
        self.assertCaught("assert-removed", "bench/recall/test_drive.py",
                          {"bench/recall/test_drive.py": sub(PYTEST, "        assert True\n", "")})

    def test_a_whole_test_file_deleted(self):
        self.assertCaught("test-removed", "bench/recall/test_drive.py", {"bench/recall/test_drive.py": None})

    def test_a_cancel_budget_dropped(self):
        lc = sub(LIFECYCLE, '        "cancel" => Some(100.0),\n', "")
        self.assertCaught("budget-raised", "crates/theseus-sim/src/lifecycle.rs",
                          {"crates/theseus-sim/src/lifecycle.rs": lc})

    def test_a_spend_mode_loosened(self):
        self.assertCaught("cap-raised", "bench/theseus-bench.toml", {"bench/theseus-bench.toml": sub(
            BENCH, 'spend_limit_mode = "ask"', 'spend_limit_mode = "notify"')})

    def test_a_caps_mode_first_named_as_notify(self):
        tpl = BASE["crates/theseus-core/config/theseus.example.toml"]
        self.assertCaught("cap-raised", "crates/theseus-core/config/theseus.example.toml",
                          {"crates/theseus-core/config/theseus.example.toml":
                               sub(tpl, "spend_limit_usd = 100.0\n",
                                   'spend_limit_usd = 100.0\nspend_limit_mode = "notify"\n')})

    def test_a_template_spend_line_raised_even_commented(self):
        tpl = BASE["crates/theseus-core/config/theseus.example.toml"]
        self.assertCaught("cap-raised", "crates/theseus-core/config/theseus.example.toml",
                          {"crates/theseus-core/config/theseus.example.toml":
                               sub(tpl, "synth_limit_usd_per_day = 0.50", "synth_limit_usd_per_day = 5.0")})

    def test_a_lint_level_lowered(self):
        self.assertCaught("allow-added", "Cargo.toml",
                          {"Cargo.toml": '[workspace.lints.clippy]\ntoo_many_lines = "allow"\n'})

    def test_the_shape_limit_raised(self):
        self.assertCaught("ceiling-raised", "scripts/shape.sh",
                          {"scripts/shape.sh": "#!/usr/bin/env bash\nlimit=3000\n"})

    def test_a_slow_timeout_widened(self):
        self.assertCaught("ceiling-raised", ".config/nextest.toml", {".config/nextest.toml": sub(
            NEXTEST, 'period = "60s"', 'period = "120s"')})

    def weaken(self, old, new, why):
        self.head({"crates/demo/src/timing.rs": sub(TIMING, old, new)})
        code, out = self.findings()
        self.assertEqual(code, 1, out)
        self.assertRegex(out, r"crates/demo/src/timing.rs:\d+: assert-weakened: .*" + re.escape(why), out)

    def test_an_assert_eq_made_an_assert(self):
        self.assertCaught("assert-weakened", "crates/demo/src/lib.rs", {"crates/demo/src/lib.rs": sub(
            LIB, "assert_eq!(n, 5);", "assert!(n > 0);")})

    def test_a_lower_bound_lowered(self):
        self.assertCaught("assert-weakened", "crates/demo/src/lib.rs", {"crates/demo/src/lib.rs": sub(
            LIB, "assert!(n > 4,", "assert!(n > 1,")})

    def test_a_timing_bound_raised_in_a_test(self):
        self.weaken("Duration::from_millis(50)", "Duration::from_millis(500)", "(an upper bound)")

    def test_an_assertion_made_vacuous(self):
        self.weaken("assert_eq!(took.as_secs(), 0);", "assert!(true);", "(made vacuous)")

    def test_an_assertion_commented_out_beside_a_new_one(self):
        self.weaken("        assert_eq!(took.as_secs(), 0);\n",
                    "        // assert_eq!(took.as_secs(), 0);\n        assert_eq!(took.as_millis(), 0);\n",
                    "(commented out)")

    def test_an_assertion_made_a_discard(self):
        self.weaken("assert_eq!(took.as_secs(), 0);", "let _ = took.as_secs();", "(discarded)")

    def test_a_python_equality_made_a_truth_test(self):
        self.assertCaught("assert-weakened", "bench/recall/test_drive.py", {"bench/recall/test_drive.py": sub(
            PYTEST, "self.assertEqual(1 + 1, 2)", "self.assertTrue(1 + 1)")})

    def test_a_python_upper_bound_raised(self):
        self.assertCaught("assert-weakened", "bench/recall/test_drive.py", {"bench/recall/test_drive.py": sub(
            PYTEST, 'self.assertLess(len("ab"), 3)', 'self.assertLess(len("ab"), 30)')})

    def test_a_cap_s_code_default_raised(self):
        self.assertCaught("cap-raised", "crates/theseus-core/src/config/limits.rs", {
            "crates/theseus-core/src/config/limits.rs": sub(LIMITS, "    100.0\n", "    150.0\n")})

    def test_a_cap_constant_raised(self):
        self.assertCaught("cap-raised", "crates/theseus-core/src/config/limits.rs", {
            "crates/theseus-core/src/config/limits.rs": sub(LIMITS, "CAP_USD: f64 = 5.0", "CAP_USD: f64 = 10.0")})

    def test_a_cap_s_code_default_gone(self):
        self.assertCaught("cap-raised", "crates/theseus-core/src/config/limits.rs", {
            "crates/theseus-core/src/config/limits.rs": sub(LIMITS, "    100.0\n", "    f64::INFINITY\n")})

    def test_a_cockpit_test_deleted(self):
        self.assertCaught("test-removed", "cockpit/test/ring.test.ts", {"cockpit/test/ring.test.ts": sub(
            COCKPIT, "test('the ring fades within a second', () => {\n", "const unused = (() => {\n")})

    def test_a_cockpit_test_skipped(self):
        self.assertCaught("test-ignored", "cockpit/test/ring.test.ts", {"cockpit/test/ring.test.ts": sub(
            COCKPIT, 'test("it shows', 'test.skip("it shows')})

    def test_a_cockpit_test_given_a_skip_option(self):
        self.assertCaught("test-ignored", "cockpit/test/ring.test.ts", {"cockpit/test/ring.test.ts": sub(
            COCKPIT, "test('the ring fades within a second', () => {", "test('the ring fades within a second', "
                                                                       "{ skip: 'later' }, () => {")})

    def test_a_cockpit_assertion_deleted(self):
        self.assertCaught("assert-removed", "cockpit/test/ring.test.ts", {"cockpit/test/ring.test.ts": sub(
            COCKPIT, "    assert.match(RING_VERT, /fleet/)\n", "")})

    def test_a_cockpit_equality_made_a_truth_test(self):
        self.assertCaught("assert-weakened", "cockpit/test/ring.test.ts", {"cockpit/test/ring.test.ts": sub(
            COCKPIT, "assert.equal(depthOf(379, false), 'fleet')", "assert.ok(depthOf(379, false))")})

    def test_a_cockpit_bound_loosened(self):
        self.assertCaught("assert-weakened", "cockpit/test/ring.test.ts", {"cockpit/test/ring.test.ts": sub(
            COCKPIT, "RING_S <= 1,", "RING_S <= 5,")})

    def test_a_cockpit_lower_bound_lowered_beside_an_upper_one(self):
        self.assertCaught("assert-weakened", "cockpit/test/ring.test.ts", {"cockpit/test/ring.test.ts": sub(
            COCKPIT, "RING_S > 0 &&", "RING_S > -1 &&")})

    def test_a_ci_command_dropped(self):
        self.assertCaught("ci-loosened", ".github/workflows/ci.yml", {".github/workflows/ci.yml": sub(
            CI, "npm run lint && npm test && npm run build", "npm run lint && npm run build")})

    def test_a_ci_command_made_to_pass(self):
        self.assertCaught("ci-loosened", ".github/workflows/ci.yml", {".github/workflows/ci.yml": sub(
            CI, "-D warnings\n", "-D warnings || true\n")})

    def test_a_ci_step_allowed_to_fail(self):
        self.assertCaught("ci-loosened", ".github/workflows/ci.yml", {".github/workflows/ci.yml": sub(
            CI, "      - run: cargo clippy", "      - continue-on-error: true\n        run: cargo clippy")})

    def test_a_ci_action_removed(self):
        self.assertCaught("ci-loosened", ".github/workflows/ci.yml", {".github/workflows/ci.yml": sub(
            CI, "      - uses: EmbarkStudios/cargo-deny-action@v2\n        with:\n"
                "          command: check licenses sources bans\n", "")})

    def test_an_uncommitted_erosion_in_the_working_tree(self):
        self.repo.write({"scripts/long-files.txt": LONG + "crates/demo/src/vast.rs 2700 new\n"})
        code, out = self.findings()
        self.assertEqual(code, 1, out)
        self.assertIn("scripts/long-files.txt:4: ceiling-raised: (no entry) -> crates/demo/src/vast.rs 2700", out)
        code, out = self.findings("--head", "HEAD")
        self.assertEqual(code, 0, "a named head is judged as committed: " + out)


class NonFindings(Case):
    def test_a_test_moved_to_another_file(self):
        lib = sub(LIB, "    #[tokio::test]\n    async fn a_zero_changes_nothing() {\n"
                       "        assert_eq!(add(7, 0), 7);\n    }\n\n", "")
        moved = ("#[tokio::test]\nasync fn a_zero_changes_nothing() {\n"
                 "    assert_eq!(demo::add(7, 0), 7);\n}\n")
        self.assertClean({"crates/demo/src/lib.rs": lib, "crates/demo/tests/zero.rs": moved})

    def test_a_test_renamed_with_the_same_body(self):
        self.assertClean({"crates/demo/src/lib.rs": sub(LIB, "fn adding_two_numbers_sums_them",
                                                        "fn two_numbers_add_up")})

    def test_a_test_renamed_and_rewritten_with_its_code(self):
        self.assertClean({"crates/demo/src/lib.rs": sub(sub(LIB, "fn adding_two_numbers_sums_them", "fn two_sums"),
                                                        "let n = add(2, 3);", "let n = add(2, 3) + add(0, 0);")})

    def test_an_assertion_rewritten(self):
        self.assertClean({"crates/demo/src/lib.rs": sub(LIB, '        assert!(n > 4, "a sum is no smaller than a part");',
                                                        "        assert_ne!(n, 4);")})

    def test_an_assertion_made_a_wait(self):
        self.assertClean({"crates/demo/src/lib.rs": sub(LIB, '        assert!(n > 4, "a sum is no smaller than a part");',
                                                        '        wait_for("the sum", || (n > 4).then_some(()));')})

    def test_a_budget_lowered(self):
        self.assertClean({"crates/theseus-sim/src/lifecycle.rs": sub(LIFECYCLE, '"shutdown" => Some(100.0)',
                                                                     '"shutdown" => Some(90.0)')})

    def test_a_first_request_token_budget_lowered(self):
        self.assertClean({"crates/theseus-core/src/first_request.rs": sub(FIRST_REQUEST, "CLI_TASK_TOKENS: u64 = 8_000;",
                                                                         "CLI_TASK_TOKENS: u64 = 7_500;")})

    def test_a_ceiling_lowered(self):
        self.assertClean({"scripts/long-files.txt": sub(LONG, "big.rs 3000", "big.rs 2900")})

    def test_an_entry_removed_when_its_file_falls_under_2500(self):
        self.assertClean({"scripts/long-files.txt": sub(LONG, "crates/demo/src/huge.rs 2600 another\n", "")})

    def test_a_cap_lowered_and_a_test_added(self):
        lib = sub(LIB, "    fn helper()", "    #[test]\n    fn more() {\n        assert!(true);\n    }\n\n    fn helper()")
        self.assertClean({"bench/theseus-bench.toml": sub(BENCH, "spend_limit_usd = 2.0", "spend_limit_usd = 1.5"),
                          "crates/demo/src/lib.rs": lib})

    def test_the_guards_own_files_added(self):
        self.assertClean({"scripts/keel-guard.py": "# the guard\n", "scripts/keel-signers": "# no key\n"})

    def test_an_equality_with_a_bool_made_an_assert(self):
        self.assertClean({"crates/demo/src/timing.rs": sub(TIMING, "assert_eq!(took.is_zero(), false);",
                                                           "assert!(!took.is_zero());")})

    def test_an_assert_eq_made_an_assert_of_its_equality(self):
        self.assertClean({"crates/demo/src/lib.rs": sub(LIB, "assert_eq!(n, 5);", "assert!(n == 5, \"the sum\");")})

    def test_a_timing_bound_tightened(self):
        self.assertClean({"crates/demo/src/timing.rs": sub(TIMING, "from_millis(50)", "from_millis(40)")})

    def test_a_measured_side_rewritten(self):
        self.assertClean({"crates/demo/src/timing.rs": sub(TIMING, "assert!(took < Duration::from_millis(50),",
                                                           "assert!(t.elapsed() < Duration::from_millis(50),")})

    def test_an_assertion_added_among_changed_ones(self):
        # The work-types join's shape (4ffcc282): an assert! added among assertions whose arguments all changed.
        self.assertClean({"crates/demo/src/timing.rs": sub(
            TIMING, '        assert!(took < Duration::from_millis(50), "a start took {took:?}");\n'
                    '        assert_eq!(took.as_secs(), 0);\n',
            '        assert!(took < Duration::from_millis(50), "a start took {took:?}, cold");\n'
            '        assert!(took.subsec_nanos() < 1_000_000_000);\n'
            '        assert_eq!(took.as_secs() + 0, 0);\n')})

    def test_a_truth_test_made_an_equality(self):
        self.assertClean({"crates/demo/src/timing.rs": sub(TIMING, "assert!(took < Duration::from_millis(50), "
                                                                   "\"a start took {took:?}\");",
                                                           "assert_eq!(took < Duration::from_millis(50), true);")})

    def test_a_cap_s_code_default_lowered_moved_or_new(self):
        self.assertClean({"crates/theseus-core/src/config/limits.rs": sub(sub(
            LIMITS, "    100.0\n", "    90.0\n"), "pub const CAP_USD: f64 = 5.0;\n", "")
            + "\npub(super) fn default_sweep_usd_per_day() -> f64 {\n    2.0\n}\n",
            "crates/theseus-core/src/judge/backfill.rs": "pub const CAP_USD: f64 = 5.0;\n"})

    def test_a_price_constant_raised(self):
        self.assertClean({"crates/theseus-core/src/config/limits.rs": sub(
            LIMITS, "0.000_016_666_7", "0.000_017")})

    def test_a_cockpit_test_renamed_and_one_added(self):
        self.assertClean({"cockpit/test/ring.test.ts": sub(COCKPIT, "the ring fades within a second",
                                                           "the ring fades out within a second")
                          + "\ntest('a third', () => {\n  assert.equal(1, 1)\n})\n"})

    def test_a_ci_step_added_and_an_action_bumped(self):
        self.assertClean({".github/workflows/ci.yml": sub(CI, "actions/checkout@v4", "actions/checkout@v5")
                          + "      - run: cargo doc --no-deps\n"})

    def test_a_comment_in_a_budget(self):
        self.assertClean({"crates/theseus-sim/src/lifecycle.rs": sub(
            LIFECYCLE, '        "shutdown" => Some(100.0),', '        // 120 once, now 100.\n        "shutdown" => Some(100.0),')})


class Ranges(Case):
    def test_a_merge_is_judged_whole_against_its_first_parent(self):
        self.repo.commit({"scripts/long-files.txt": sub(LONG, "big.rs 3000", "big.rs 3200")}, "raise")
        self.repo.git("checkout", "-q", "main")
        self.repo.git("merge", "-q", "--no-ff", "-m", "Merge lane", "lane")
        code, out = self.findings()
        self.assertEqual(code, 1, out)
        self.assertIn("a merge: HEAD^1..HEAD", out)
        self.assertIn("ceiling-raised", out)

    def test_main_itself_has_an_empty_range(self):
        self.repo.git("checkout", "-q", "main")
        code, out = self.findings()
        self.assertEqual(code, 0, out)

    def test_the_base_can_be_named(self):
        self.repo.commit({"scripts/shape.sh": "#!/usr/bin/env bash\nlimit=2600\n"}, "first")
        first = self.repo.git("rev-parse", "HEAD").strip()
        self.repo.commit({"scripts/shape.sh": "#!/usr/bin/env bash\nlimit=2500\n"}, "back")
        code, out = self.findings("--base", first)
        self.assertEqual(code, 0, out)
        self.repo.env["THESEUS_KEEL_BASE"] = "main"
        code, out = self.findings()
        self.assertEqual(code, 0, "the net change is nothing: " + out)

    def test_no_main_is_an_error_that_names_the_override(self):
        self.repo.git("branch", "-m", "main", "trunk")
        code, out = self.repo.guard()
        self.assertEqual(code, 2, out)
        self.assertIn("THESEUS_KEEL_BASE", out)

    def join_with_a_fix_after_its_merge(self):
        """main at the base is origin/main; the lane raises a ceiling; the join merges it on main and commits a fix
        after the merge, so HEAD is the fix (theseus-pw1q.18: the people join's gate judged HEAD..HEAD here)."""
        self.repo.git("update-ref", "refs/remotes/origin/main", "main")
        self.repo.commit({"scripts/long-files.txt": sub(LONG, "big.rs 3000", "big.rs 3200")}, "raise")
        self.repo.git("checkout", "-q", "main")
        self.repo.git("merge", "-q", "--no-ff", "-m", "Merge lane", "lane")
        self.repo.commit({"scripts/shape.sh": "#!/usr/bin/env bash\nlimit=2500\n# the join's fix\n"}, "join fix")

    def test_a_join_whose_fix_follows_its_merge_is_judged_whole(self):
        self.join_with_a_fix_after_its_merge()
        code, out = self.findings()
        self.assertEqual(code, 1, out)
        self.assertIn("merge-base(HEAD, origin/main)", out)
        self.assertIn("scripts/long-files.txt:2: ceiling-raised", out)

    def test_a_branch_cut_at_a_landed_join_is_judged_from_it(self):
        self.repo.git("update-ref", "refs/remotes/origin/main", "main")
        self.repo.commit({"scripts/long-files.txt": sub(LONG, "big.rs 3000", "big.rs 3200")}, "raise")
        self.repo.git("checkout", "-q", "main")
        self.repo.git("merge", "-q", "--no-ff", "-m", "Merge lane", "lane")
        self.repo.git("update-ref", "refs/remotes/origin/main", "main")
        self.repo.git("checkout", "-q", "-b", "next")
        self.repo.write({"scripts/shape.sh": "#!/usr/bin/env bash\nlimit=2400\n"})
        code, out = self.findings()
        self.assertEqual(code, 0, "the landed join is not judged again: " + out)
        self.assertIn("a branch: merge-base(HEAD, origin/main)", out)


GATE = Path(__file__).resolve().parent / "gate.sh"


class GateCase(Case):
    """The real scripts/gate.sh, run in a throwaway repository: its door and keel phases run, and its next phase (fmt)
    fails there, since the repository has no Cargo.toml; the door's and the keel's lines say what was judged."""

    def setUp(self):
        super().setUp()
        self.repo.git("checkout", "-q", "main")
        self.repo.commit({"scripts/gate.sh": GATE.read_text(), "scripts/keel-guard.py": GUARD.read_text()}, "gate")
        os.chmod(self.repo.root / "scripts/gate.sh", 0o755)
        self.repo.git("checkout", "-q", "-B", "lane")
        self.logs = tempfile.mkdtemp(prefix="keel-logs-")

    def tearDown(self):
        super().tearDown()
        shutil.rmtree(self.logs, ignore_errors=True)

    def gate(self, script="scripts/gate.sh", **env):
        e = {k: v for k, v in self.repo.env.items()
             if k not in ("THESEUS_GATE_ROOT", "THESEUS_GATE_DOOR", "THESEUS_GATE_NO_BENCH")}
        e.update({"THESEUS_GATE_TIMES": f"{self.logs}/times.csv", "THESEUS_BENCH_HISTORY": f"{self.logs}/bench.csv",
                  "THESEUS_FLAKY_LOG": f"{self.logs}/flaky.csv", "THESEUS_GATE_LOCK_FILE": f"{self.logs}/lock",
                  "TMPDIR": self.logs, "PYTHONDONTWRITEBYTECODE": "1", **env})
        p = subprocess.run(["bash", script], cwd=self.repo.root, env=e, capture_output=True, text=True, timeout=60)
        return p.returncode, p.stdout + p.stderr


class Gate(GateCase):
    def test_the_gate_judges_a_join_whose_fix_follows_its_merge(self):
        Ranges.join_with_a_fix_after_its_merge(self)
        code, out = self.gate()
        self.assertNotEqual(code, 0, out)
        self.assertIn("scripts/long-files.txt:2: ceiling-raised", out)
        self.assertIn("gate: FAILED in keel", out)

    def test_the_gate_judges_a_branch_cut_at_a_landed_join_from_it(self):
        self.repo.git("checkout", "-q", "main")
        self.repo.git("update-ref", "refs/remotes/origin/main", "main")
        self.repo.git("checkout", "-q", "lane")
        self.repo.commit({"scripts/long-files.txt": sub(LONG, "big.rs 3000", "big.rs 3200")}, "raise")
        self.repo.git("checkout", "-q", "main")
        self.repo.git("merge", "-q", "--no-ff", "-m", "Merge lane", "lane")
        self.repo.git("update-ref", "refs/remotes/origin/main", "main")
        self.repo.git("checkout", "-q", "-b", "next")
        self.repo.write({"scripts/shape.sh": "#!/usr/bin/env bash\nlimit=2400\n"})
        code, out = self.gate()
        self.assertIn("keel: ok (", out)
        self.assertIn("1 files changed", out)
        self.assertIn("gate: FAILED in fmt", out)


class KeelFiles(Case):
    def setUp(self):
        super().setUp()
        self.repo.git("checkout", "-q", "main")
        self.repo.commit({"scripts/keel-guard.py": "# the guard\n", "scripts/keel-signers": "# no key\n",
                          "scripts/gate.sh": "phase keel keel_guard\nphase fmt cargo fmt\n"}, "the guard")
        self.repo.git("checkout", "-q", "-B", "lane")

    def test_a_change_to_the_guard_is_a_finding(self):
        self.assertCaught("keel-file", "scripts/keel-guard.py", {"scripts/keel-guard.py": "# the guard, relaxed\n"})

    def test_a_key_added_to_the_signers_is_a_finding(self):
        self.assertCaught("keel-file", "scripts/keel-signers", {"scripts/keel-signers": "# no key\nABCDEF\n"})

    def test_the_gate_step_removed_is_a_finding(self):
        self.assertCaught("keel-file", "scripts/gate.sh", {"scripts/gate.sh": "phase fmt cargo fmt\n"})

    def test_another_gate_change_is_not(self):
        self.assertClean({"scripts/gate.sh": "phase keel keel_guard\nphase fmt cargo fmt --all\n"})

    def test_a_gate_phase_gone_is_a_finding(self):
        self.assertCaught("keel-file", "scripts/gate.sh", {"scripts/gate.sh": "phase keel keel_guard\n"})

    def test_a_gate_phase_added_is_not(self):
        self.assertClean({"scripts/gate.sh": "phase keel keel_guard\nphase fmt cargo fmt\nphase \"a new one\" true\n"})


class DoorLines(Case):
    def setUp(self):
        super().setUp()
        self.repo.git("checkout", "-q", "main")
        self.repo.commit({"scripts/gate.sh": "phase door keel_door\nTHESEUS_GATE_DOOR=judge \"$copy\"\n"}, "the door")
        self.repo.git("checkout", "-q", "-B", "lane")

    def test_a_door_line_changed_is_a_finding(self):
        self.assertCaught("keel-file", "scripts/gate.sh", {"scripts/gate.sh": "phase door keel_door\n\"$copy\"\n"})


# A stand-in for main's gate.sh that has the door (its word, THESEUS_GATE_DOOR, is all the door looks for): it says
# what it was asked to judge, and passes or fails as STUB_EXIT says.
STUB_GATE = """#!/usr/bin/env bash
# main's gate, standing in: THESEUS_GATE_DOOR
echo "stub main gate judged ${THESEUS_GATE_ROOT:-?} as ${THESEUS_GATE_DOOR:-?}"
exit "${STUB_EXIT:-0}"
"""


class Door(GateCase):
    """The gate that judges is main's (theseus-pw1q.18): a branch's own gate.sh is never its only judge."""

    def main_gate(self, text):
        """main's (and origin/main's) gate.sh becomes `text`; the lane is cut before it."""
        self.repo.git("checkout", "-q", "main")
        self.repo.commit({"scripts/gate.sh": text}, "main's gate moves on")
        self.repo.git("update-ref", "refs/remotes/origin/main", "main")
        self.repo.git("checkout", "-q", "lane")

    def weakened(self):
        """The real gate.sh without its keel phase."""
        return sub(GATE.read_text(), "phase keel keel_guard\n", "")

    def test_a_lane_behind_main_s_gate_is_judged_by_main_s(self):
        self.main_gate(STUB_GATE)
        code, out = self.gate()
        self.assertEqual(code, 0, out)
        self.assertIn(f"stub main gate judged {os.path.realpath(self.dir)} as judge", out)
        self.assertIn("gate: ok (main's gate judged this tree)", out)
        self.assertNotIn("keel:", out, "the lane does not change gate.sh, so its own copy does not run")

    def test_a_long_main_gate_is_still_seen_to_have_the_door(self):
        # Main's copy names the door early and runs on for 400 KB: a `cat-file | grep -q` under pipefail failed here
        # (grep's early exit SIGPIPEs cat-file), and the door took main for one without it.
        self.main_gate(STUB_GATE + "# padding\n" * 40_000)
        for _ in range(3):
            code, out = self.gate()
            self.assertIn("as judge", out)

    def test_main_s_gate_failing_fails_the_door(self):
        self.main_gate(STUB_GATE)
        code, out = self.gate(STUB_EXIT="1")
        self.assertNotEqual(code, 0, out)
        self.assertIn("door: main's gate failed on this tree", out)
        self.assertIn("gate: FAILED in door", out)

    def test_a_branch_that_changes_gate_sh_runs_its_own_after_main_s(self):
        self.repo.git("checkout", "-q", "main")
        self.repo.commit({"scripts/gate.sh": STUB_GATE}, "main's gate is the stand-in")
        self.repo.git("update-ref", "refs/remotes/origin/main", "main")
        self.repo.git("checkout", "-q", "-B", "lane")
        self.repo.commit({"scripts/gate.sh": GATE.read_text()}, "the lane's gate.sh")
        code, out = self.gate()
        self.assertIn("as judge", out)
        self.assertIn("door: main's gate passed; this branch changes scripts/gate.sh, so its own copy runs now", out)
        self.assertRegex(out, r"(?s)as judge.*\nkeel: ", "the lane's own keel phase ran after main's gate")

    def test_a_weakened_gate_sh_is_judged_by_main_s(self):
        self.repo.git("update-ref", "refs/remotes/origin/main", "main")
        self.repo.commit({"scripts/long-files.txt": sub(LONG, "big.rs 3000", "big.rs 3200"),
                          "scripts/gate.sh": self.weakened()}, "raise, and drop the keel phase")
        code, out = self.gate()
        self.assertNotEqual(code, 0, out)
        self.assertIn("door: this gate.sh is not main's", out)
        self.assertRegex(out, r"main's judges this tree first \(\S*/theseus-gate-door-[0-9a-f]+/scripts/gate\.sh\)",
                         "main's copy runs as scripts/gate.sh, as review-step.sh knows a gate")
        self.assertIn("scripts/long-files.txt:2: ceiling-raised", out)
        self.assertRegex(out, r"scripts/gate.sh:\d+: keel-file")
        self.assertIn("gate: FAILED in door", out)

    def test_main_s_copy_from_outside_judges_a_tree_without_the_door(self):
        self.repo.git("update-ref", "refs/remotes/origin/main", "main")
        gate = sub(sub(self.weakened(), "phase door keel_door\n", ""), "THESEUS_GATE_DOOR", "THESEUS_GATE_GONE")
        self.repo.commit({"scripts/long-files.txt": sub(LONG, "big.rs 3000", "big.rs 3200"), "scripts/gate.sh": gate},
                         "raise, and drop the keel phase and the door")
        code, out = self.gate()
        self.assertNotIn("ceiling-raised", out, "the tree's own copy judges nothing of the keel: the hole")
        copy = Path(self.logs) / "main-gate.sh"
        copy.write_text(self.repo.git("show", "origin/main:scripts/gate.sh"))
        code, out = self.gate(str(copy), THESEUS_GATE_ROOT=str(self.repo.root))
        self.assertNotEqual(code, 0, out)
        self.assertIn("door: main's copy judges; this tree changes scripts/gate.sh", out)
        self.assertIn("scripts/long-files.txt:2: ceiling-raised", out)
        self.assertIn("gate: FAILED in keel", out)

def gpg_key(home, name):
    subprocess.run(["gpg", "--batch", "--passphrase", "", "--quick-gen-key", f"{name} <{name}@example.invalid>",
                    "ed25519", "sign", "never"], env={**os.environ, "GNUPGHOME": home}, check=True,
                   capture_output=True)
    out = subprocess.run(["gpg", "--batch", "--with-colons", "--list-secret-keys", f"{name}@example.invalid"],
                         env={**os.environ, "GNUPGHOME": home}, check=True, capture_output=True, text=True).stdout
    return next(line.split(":")[9] for line in out.split("\n") if line.startswith("fpr:"))


@unittest.skipIf(shutil.which("gpg") is None, "no gpg: the signed acks are not tested here")
class Acks(unittest.TestCase):
    """An ack passes only on a commit a key listed in the base's keel-signers signed."""

    @classmethod
    def setUpClass(cls):
        cls.gnupg = tempfile.mkdtemp(prefix="keel-gnupg-")
        os.chmod(cls.gnupg, 0o700)
        cls.listed = gpg_key(cls.gnupg, "harbor-keeper")
        cls.stranger = gpg_key(cls.gnupg, "drifter")

    @classmethod
    def tearDownClass(cls):
        subprocess.run(["gpgconf", "--kill", "all"], env={**os.environ, "GNUPGHOME": cls.gnupg}, capture_output=True)
        shutil.rmtree(cls.gnupg, ignore_errors=True)

    def setUp(self):
        self.dir = tempfile.mkdtemp(prefix="keel-")
        self.repo = Repo(self.dir, self.gnupg)
        self.repo.commit({**BASE, "scripts/keel-signers": f"# the keel's signers\n{self.listed}\n"}, "base")
        self.repo.git("checkout", "-q", "-b", "lane")
        self.repo.commit({"scripts/long-files.txt": sub(LONG, "big.rs 3000", "big.rs 3050")}, "raise a ceiling")

    def tearDown(self):
        shutil.rmtree(self.dir, ignore_errors=True)

    ACK = "ack\n\nKeel: ceiling-raised scripts/long-files.txt — big.rs gains the turn's new field\n"

    def test_an_unsigned_ack_is_refused(self):
        self.repo.commit({}, self.ACK)
        code, out = self.repo.guard()
        self.assertEqual(code, 1, out)
        self.assertIn("refused the ack", out)
        self.assertIn("unsigned", out)

    def test_an_ack_signed_by_a_key_not_listed_is_refused(self):
        self.repo.commit({}, self.ACK, sign=self.stranger)
        code, out = self.repo.guard()
        self.assertEqual(code, 1, out)
        self.assertIn(f"signed by {self.stranger}", out)

    def test_an_ack_signed_by_a_listed_key_passes(self):
        self.repo.commit({}, self.ACK, sign=self.listed)
        code, out = self.repo.guard()
        self.assertEqual(code, 0, out)
        self.assertIn("keel: acked: scripts/long-files.txt:2: ceiling-raised", out)

    def test_an_ack_for_another_rule_or_path_passes_nothing(self):
        self.repo.commit({}, "ack\n\nKeel: budget-raised scripts/long-files.txt — wrong rule\n"
                             "Keel: ceiling-raised scripts/shape.sh — wrong path\n", sign=self.listed)
        code, out = self.repo.guard()
        self.assertEqual(code, 1, out)

    def test_a_branch_cannot_list_its_own_key(self):
        self.repo.commit({"scripts/keel-signers": f"# the keel's signers\n{self.listed}\n{self.stranger}\n"},
                         self.ACK + "Keel: keel-file scripts/keel-signers — mine\n", sign=self.stranger)
        code, out = self.repo.guard()
        self.assertEqual(code, 1, out)
        self.assertIn("keel-file", out)
        self.assertIn(f"signed by {self.stranger}", out)

    @unittest.skipIf(shutil.which("ssh-keygen") is None, "no ssh-keygen: the ssh-signed ack is not tested here")
    def test_an_ssh_signed_ack_is_refused_even_where_it_verifies(self):
        keys = tempfile.mkdtemp(prefix="keel-ssh-")
        self.addCleanup(shutil.rmtree, keys, True)
        key = f"{keys}/key"
        subprocess.run(["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-C", "wren@example.invalid", "-f", key],
                       check=True, capture_output=True)
        with open(f"{keys}/allowed", "w") as f:
            f.write("wren@example.invalid " + Path(key + ".pub").read_text())
        fpr = subprocess.run(["ssh-keygen", "-lf", key + ".pub"], check=True, capture_output=True,
                             text=True).stdout.split()[1]
        # A base that lists the ssh key's fingerprint, so only the kind of the signature can refuse it.
        self.repo.git("checkout", "-q", "main")
        self.repo.commit({"scripts/keel-signers": f"# the keel's signers\n{self.listed}\n{fpr}\n"}, "list it")
        self.repo.git("checkout", "-q", "-B", "lane")
        self.repo.commit({"scripts/long-files.txt": sub(LONG, "big.rs 3000", "big.rs 3050")}, "raise a ceiling")
        self.repo.git("config", "gpg.ssh.allowedSignersFile", f"{keys}/allowed")
        self.repo.git("-c", "gpg.format=ssh", "-c", f"user.signingkey={key}", "commit", "-q", "--allow-empty", "-S",
                      "-m", self.ACK)
        self.assertEqual(self.repo.git("log", "-1", "--format=%G?|%GF").strip(), f"G|{fpr}", "it verifies")
        code, out = self.repo.guard()
        self.assertEqual(code, 1, out)
        self.assertIn("an ssh signature: the keel takes a GPG key's only", out)

    def test_a_signed_merge_carries_the_join_s_acks(self):
        self.repo.git("checkout", "-q", "main")
        self.repo.git("-c", f"user.signingkey={self.listed}", "merge", "-q", "--no-ff", "-S", "-m", self.ACK, "lane")
        code, out = self.repo.guard()
        self.assertEqual(code, 0, out)


if __name__ == "__main__":
    unittest.main()
