"""Tests for scripts/flake-hunt.sh (theseus-20, "No test may fail by chance"): the hunt counts a failure and never reads
a silence as a pass.

Run: python3 -m unittest -q scripts/tests/test_flake_hunt.py

The hunt is run in a scratch tree that holds a copy of the script and of the load rig, with a fake `cargo` first on
PATH. The fake reads three variables: FAKE_FAIL_RUNS (the numbers of the suite runs, counted from 1, that fail, as
nextest's final recap shows a failed test), FAKE_NO_BUILD (the test build fails) and FAKE_LOG (where it notes each call,
with the test threads it was given).
"""
import os
import shutil
import subprocess
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
SCRIPTS = os.path.join(HERE, "..")

FAKE_CARGO = """#!/usr/bin/env bash
echo "cargo $* threads=${NEXTEST_TEST_THREADS:-unset}" >> "$FAKE_LOG"
case " $* " in
  *" --test self_build "*)
    # The view's own test: it runs the command a hunt handed it, the way the view would (here, plainly).
    sh -c "$THESEUS_SELF_BUILD_CMD" > "${FAKE_VIEW_LOG:-/dev/null}" 2>&1
    exit $? ;;
  *" --no-run "*)
    [ -z "${FAKE_NO_BUILD:-}" ] || { echo "error: could not compile"; exit 101; }
    exit 0 ;;
esac
n=$(grep -c ' --no-fail-fast ' "$FAKE_LOG")
case " ${FAKE_FAIL_RUNS:-} " in
  *" $n "*)
    echo "------------"
    echo "     Summary [   1.0s] 5 tests run: 4 passed, 1 failed, 0 skipped"
    echo "        FAIL [   0.1s] ($n/$((n + 4000))) theseus-core term::tests::a_flaky_one"
    echo "--- STDERR: theseus-core term::tests::a_flaky_one ---"
    echo "assertion failed: it ended by the other wording"
    exit 100 ;;
esac
echo "------------"
echo "     Summary [   1.0s] 5 tests run: 5 passed, 0 skipped"
"""


class FlakeHunt(unittest.TestCase):
    def setUp(self):
        self.tree = tempfile.mkdtemp()
        self.addCleanup(shutil.rmtree, self.tree, True)
        os.makedirs(os.path.join(self.tree, "scripts"))
        for f in ("flake-hunt.sh", "load-rig.py"):
            shutil.copy(os.path.join(SCRIPTS, f), os.path.join(self.tree, "scripts", f))
        self.bin = os.path.join(self.tree, "bin")
        os.makedirs(self.bin)
        fake = os.path.join(self.bin, "cargo")
        with open(fake, "w") as f:
            f.write(FAKE_CARGO)
        os.chmod(fake, 0o755)
        self.log = os.path.join(self.tree, "calls.log")
        open(self.log, "w").close()

    def hunt(self, *args, **env):
        e = dict(os.environ, PATH=f"{self.bin}:{os.environ['PATH']}", FAKE_LOG=self.log, FLAKE_HUNT_SETTLE="0")
        e.pop("NEXTEST_TEST_THREADS", None)
        e.update(env)
        return subprocess.run(
            ["bash", os.path.join(self.tree, "scripts", "flake-hunt.sh"), *args],
            capture_output=True, text=True, env=e, timeout=120, cwd="/",
        )

    def calls(self):
        with open(self.log) as f:
            return f.read().splitlines()

    def test_clean_runs_exit_zero_and_say_how_many(self):
        r = self.hunt("3")
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        self.assertIn("no test failed in 3 run(s)", r.stdout)
        self.assertEqual(len([c for c in self.calls() if "--no-fail-fast" in c]), 3)

    def test_a_failure_in_one_run_is_counted_named_and_shown(self):
        r = self.hunt("3", FAKE_FAIL_RUNS="2")
        self.assertEqual(r.returncode, 1, r.stdout + r.stderr)
        self.assertIn("1 of 3 whole run(s) had a failure", r.stdout)
        self.assertIn("1 of 3 runs failed: theseus-core term::tests::a_flaky_one", r.stdout)
        # The failure's own words come with it.
        self.assertIn("assertion failed: it ended by the other wording", r.stdout)
        with open(os.path.join(self.tree, "target", "flake-hunt", "failed-tests.tsv")) as f:
            self.assertEqual(f.read(), "2\tno-load\ttheseus-core term::tests::a_flaky_one\n")

    def test_the_same_test_failing_twice_is_one_row_of_two(self):
        r = self.hunt("3", FAKE_FAIL_RUNS="1 3")
        self.assertEqual(r.returncode, 1)
        self.assertIn("2 of 3 runs failed: theseus-core term::tests::a_flaky_one", r.stdout)

    def test_threads_reach_the_suite_and_not_the_build(self):
        r = self.hunt("1", "--threads", "96")
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        build = [c for c in self.calls() if "--no-run" in c]
        suite = [c for c in self.calls() if "--no-fail-fast" in c]
        self.assertEqual(len(build), 1)
        self.assertTrue(build[0].endswith("threads=unset"), build)
        self.assertTrue(suite[0].endswith("threads=96"), suite)
        self.assertIn("threads-96", r.stdout)

    def test_the_suite_command_is_the_gates_and_runs_in_the_default_profile(self):
        self.hunt("1")
        suite = [c for c in self.calls() if "--no-fail-fast" in c][0]
        self.assertIn("nextest run --workspace --no-fail-fast", suite)
        self.assertNotIn("--profile", suite)
        self.assertNotIn("--retries", suite)

    def test_load_runs_the_suite_beside_burners_and_keeps_its_exit_status(self):
        r = self.hunt("2", "--load", "--spinners", "2", FAKE_FAIL_RUNS="2")
        self.assertEqual(r.returncode, 1, r.stdout + r.stderr)
        self.assertIn("load-2-burners-nice-0", r.stdout)
        self.assertIn("1 of 2 runs failed", r.stdout)

    def test_a_filter_narrows_the_suite_and_says_so(self):
        r = self.hunt("1", "--filter", "test(=one_of_them)")
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        suite = [c for c in self.calls() if "--no-fail-fast" in c][0]
        self.assertIn("-E test(=one_of_them)", suite)
        self.assertIn("no-load+filtered", r.stdout)

    def test_the_views_withheld_set_is_left_out_by_name_and_said(self):
        r = self.hunt("1", THESEUS_VIEW_WITHHELD="theseus-sandbox::contract clause_04\ntheseusd::x a_test",
                      THESEUS_VIEW_GRANTS="a self build's, granting pty")
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        suite = [c for c in self.calls() if "--no-fail-fast" in c][0]
        self.assertIn("-E not ((binary_id(=theseus-sandbox::contract) & test(=clause_04)) | "
                      "(binary_id(=theseusd::x) & test(=a_test)))", suite)
        self.assertIn("withheld set is EXCLUDED from every run, by name", r.stdout)
        self.assertIn("clause_04", r.stdout)
        self.assertIn("self-build-view", r.stdout)

    def test_a_malformed_withheld_entry_is_refused_not_guessed(self):
        r = self.hunt("1", THESEUS_VIEW_WITHHELD="no-test-name")
        self.assertEqual(r.returncode, 2, r.stdout + r.stderr)
        self.assertIn("is not '<binary id> <test name>'", r.stdout + r.stderr)
        self.assertEqual([c for c in self.calls() if "--no-fail-fast" in c], [])

    def test_view_hands_the_same_hunt_to_the_views_test_and_reads_its_verdict_back(self):
        r = self.hunt("2", "--view", "--threads", "7", FAKE_FAIL_RUNS="2")
        self.assertEqual(r.returncode, 1, r.stdout + r.stderr)
        self.assertIn("1 of 2 runs failed: theseus-core term::tests::a_flaky_one", r.stdout)
        view = [c for c in self.calls() if "--test self_build" in c]
        self.assertEqual(len(view), 1, self.calls())
        self.assertIn("--run-ignored only", view[0])
        suite = [c for c in self.calls() if "--no-fail-fast" in c]
        self.assertEqual(len(suite), 2)
        self.assertTrue(suite[0].endswith("threads=7"), suite)

    def test_a_clean_view_hunt_exits_zero(self):
        r = self.hunt("2", "--view")
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        self.assertIn("no test failed in 2 run(s)", r.stdout)

    def test_a_view_that_ran_no_hunt_is_not_a_pass(self):
        with open(os.path.join(self.bin, "cargo"), "w") as f:
            f.write("#!/usr/bin/env bash\nexit 0\n")
        r = self.hunt("1", "--view")
        self.assertEqual(r.returncode, 2, r.stdout + r.stderr)
        self.assertIn("the view ran no hunt", r.stderr)

    def test_a_build_that_fails_hunts_nothing(self):
        r = self.hunt("2", FAKE_NO_BUILD="1")
        self.assertEqual(r.returncode, 2, r.stdout + r.stderr)
        self.assertIn("did not build", r.stdout + r.stderr)
        self.assertEqual([c for c in self.calls() if "--no-fail-fast" in c], [])

    def test_a_run_that_exits_badly_with_no_test_named_is_not_a_pass(self):
        # A runner error: exit 1, no recap. The hunt must not call it clean.
        with open(os.path.join(self.bin, "cargo"), "w") as f:
            f.write("#!/usr/bin/env bash\ncase \" $* \" in *' --no-run '*) exit 0;; esac\necho 'error: runner died'; exit 1\n")
        r = self.hunt("1")
        self.assertEqual(r.returncode, 1, r.stdout + r.stderr)
        self.assertIn("no test named: exit 1", r.stdout)
        self.assertNotIn("no test failed", r.stdout)

    def test_bad_arguments_are_refused(self):
        for args in (["0"], ["x"], ["2", "--threads", "0"], ["2", "--spinners", "3"], ["2", "--what"], ["2", "--filter"], []):
            r = self.hunt(*args)
            self.assertEqual(r.returncode, 2, (args, r.stdout, r.stderr))
        self.assertEqual(self.calls(), [])


if __name__ == "__main__":
    unittest.main()
