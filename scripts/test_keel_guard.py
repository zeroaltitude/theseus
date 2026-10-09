"""The keel guard's planted-erosion suite (theseus-pw1q.1): each case builds a throwaway git repository, commits a
base and a head, and runs scripts/keel-guard.py over the range. Standard library only; the gate's keel step runs it
(`python3 -m unittest scripts/test_keel_guard.py`). The signed cases make throwaway keys in the case's own GNUPGHOME,
and are skipped, saying so, where there is no gpg.
"""

import os
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
        assert True
"""

BASE = {
    "crates/demo/src/lib.rs": LIB,
    "crates/theseus-sim/src/lifecycle.rs": LIFECYCLE,
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

    def test_a_signed_merge_carries_the_join_s_acks(self):
        self.repo.git("checkout", "-q", "main")
        self.repo.git("-c", f"user.signingkey={self.listed}", "merge", "-q", "--no-ff", "-S", "-m", self.ACK, "lane")
        code, out = self.repo.guard()
        self.assertEqual(code, 0, out)


if __name__ == "__main__":
    unittest.main()
