"""The gate judges the tree it built, or says it could not (theseus-4vsrz).

A gate compiles one tree and then tests whatever tree is there when the tests run. On 2026-11-07 a builder detached
the join's worktree and restored it inside a join gate's window, and the gate reported four red tests that passed 15
of 15 on the identical tree: a false result that looked exactly like a real one. So the gate takes the tree's
identity before its first compile and checks it again before the lock, when it takes the lock, before the suite, and
at the end; a gate that fails is checked too, since a failure on a moved tree is not a verdict. A tree that moved
ABORTS the gate with its own exit status (75) and its own message, never a test failure.

These run the REAL scripts/gate.sh in a throwaway repository whose cargo, node and npm are stand-ins (the gate puts
$HOME/.cargo/bin first on PATH, so a HOME of the test's own is where they go). The stand-in cargo is where the tree
moves: after the test build (the compile phases are done), or inside the suite.

The gate also builds with CARGO_INCREMENTAL=0 from its first cargo (a lane is built once and gated once: 47 GB of a
68 GB target was `debug/incremental`), and the stand-in cargo records what each call saw.

It refuses to start below a disk floor (exit status 73): on 2026-11-07 `/` was 631 GB of 639 GB used and a 40-minute
build began on 2.1 GB free. A stand-in df answers for the filesystem here, so no test depends on the host's own disk;
one test lets the real df answer.
"""

import os
import re
import shutil
import subprocess
import tempfile
import time
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent
MOVED = 75
FLOOR = 73
GIB = 1048576  # a GB of free space, in the KiB df reports

# A stand-in for cargo: every subcommand passes and prints nothing unless a phase reads its output. It writes one line
# per call to $STUB_LOG ("<what> inc=<CARGO_INCREMENTAL as it saw it>"), and moves the tree where $STUB_MOVE_AT says
# ("build": the test build, the last compile; "suite": the suite), the way $STUB_MOVE says (an edit, or a checkout
# away and back, which leaves the same commit and the same files and a longer reflog).
CARGO = r"""#!/usr/bin/env bash
case " $* " in
  *" --no-run "*) what=build ;;
  *tests_registry*) what=registry ;;
  *" -E "*) what=protocol ;;
  *" nextest "*) what=suite ;;
  *) what=$1 ;;
esac
echo "$what inc=${CARGO_INCREMENTAL-unset}" >>"$STUB_LOG"
move() {
  [ "${STUB_MOVE_AT:-}" = "$1" ] || return 0
  case "$STUB_MOVE" in
    edit) echo "an edit made while the gate ran" >>notes.txt ;;
    smoke-run) mkdir -p bench/smoke/runs/2026-01-01-abcd1234 && echo report >bench/smoke/runs/2026-01-01-abcd1234/report.md ;;
    lock) echo "# rewritten by the first cargo" >>Cargo.lock ;;
    away-and-back) git checkout -q --detach && git checkout -q main ;;
  esac
}
[ "$what" != deny ] || echo "$*" >>"${STUB_DENY_LOG:-/dev/null}"
case "$what" in
  fmt)
    # On request fmt reds, as it does in a throwaway repository with no workspace.
    if [ "${STUB_FMT_FAIL:-}" = 1 ]; then echo "error: a stub fmt failure" >&2; exit 1; fi
    ;;
  deny)
    # On request only advisories fail: the bare `check` reds, and `check bans licenses sources` passes.
    if [ "${STUB_DENY:-}" = advisories ] && [ "${*: -1}" = check ]; then echo "error[vulnerability]: a stub advisory" >&2; exit 1; fi
    ;;
  build)
    for b in theseusd theseus-sim theseus-index; do
      echo "{\"reason\":\"compiler-artifact\",\"executable\":\"$PWD/target/debug/$b\"}"
    done
    move build
    ;;
  registry) echo "     Summary [   0.010s] 3 tests run: 3 passed, 0 skipped" ;;
  protocol)
    [ "${STUB_PROTOCOL:-}" != regenerate ] || echo "// regenerated" >>cockpit/src/protocol.gen/types.ts
    echo "     Summary [   0.010s] 1 test run: 1 passed, 0 skipped"
    ;;
  suite)
    move suite
    if [ "${STUB_SUITE_RC:-0}" != 0 ]; then
      echo "        FAIL [   0.100s] stub::a_test_that_fails_on_the_moved_tree"
      exit "$STUB_SUITE_RC"
    fi
    if [ -n "${STUB_APPLIED:-}" ]; then
      echo "────────────"
      echo "     Starting 9 tests across 1 binaries ($STUB_APPLIED tests skipped, including $STUB_APPLIED tests via profile.default.default-filter)"
    fi
    echo "     Summary [   1.000s] 9 tests run: 9 passed, 0 skipped"
    ;;
esac
exit 0
"""

NODE = "#!/bin/sh\necho v1.2.3\n"
NPM = "#!/bin/sh\n[ \"$2\" != test ] || echo 'ℹ tests 1'\nexit 0\n"
PASSES = "#!/bin/sh\nexit 0\n"

# A stand-in for `df -Pk -- PATH`: the POSIX report for a filesystem with $STUB_DF_AVAIL_KB available (any text, to
# be unreadable), the real df's when $STUB_DF_REAL is set, a failure when $STUB_DF_FAIL is. Like the real one it fails
# on a path that is not there, and it logs the path it was asked about, one line per call, to $STUB_DF_LOG.
DF = r"""#!/usr/bin/env bash
path=${*: -1}
echo "$path" >>"$STUB_DF_LOG"
if [ -n "${STUB_DF_REAL:-}" ]; then
  real=$(PATH=$(getconf PATH) command -v df)
  exec "$real" "$@"
fi
[ -z "${STUB_DF_FAIL:-}" ] || { echo "df: stub failure" >&2; exit 1; }
[ -e "$path" ] || { echo "df: $path: No such file or directory" >&2; exit 1; }
echo "Filesystem     1024-blocks      Used Available Capacity Mounted on"
echo "stub           1000000000 100000000 $STUB_DF_AVAIL_KB      10% /stub"
"""

# The lane's nextest config: what the ARM runner edits (its sed puts a default-filter after test-threads), with a
# commented default-filter as the real file has in its prose, which is no filter.
NEXTEST = """[profile.default]
test-threads = 8
# default-filter = "not (test(=a_comment_is_no_filter))"
slow-timeout = { period = "60s", terminate-after = 2 }
"""
FILTER = (
    'default-filter = "not (test(=the_owner_starts_asks_answers_joins_pauses_ends_and_finds_two_tasks)'
    ' | test(=rendering_costs_little_per_hundred_lines))"'
)

COMMITTED = {
    ".config/nextest.toml": NEXTEST,
    ".gitignore": "target\ncockpit/node_modules\ncrates/theseusd/cockpit/dist\n",
    "notes.txt": "the file a mover edits\n",
    "Cargo.lock": "# a lock\n",
    "cockpit/package.json": "{}\n",
    "cockpit/src/protocol.gen/types.ts": "// generated\n",
    "crates/theseus-core/src/sandbox/self_build.rs": 'pub const NODE_PIN: &str = "1.2.3";\n',
    "scripts/shape.sh": PASSES,
    "scripts/build.sh": PASSES,
    "scripts/recall-tests.sh": PASSES,
    "scripts/size-step.sh": PASSES,
    "scripts/prompt-step.sh": PASSES,
}
REAL = ("gate.sh", "keel-guard.py")


def run(cmd, cwd, env=None, timeout=300):
    return subprocess.run(cmd, cwd=cwd, env=env, capture_output=True, text=True, timeout=timeout)


class GateTree(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        tmp = Path(self.tmp.name)
        self.home = tmp / "home"
        self.root = tmp / "tree"
        self.log = tmp / "cargo.log"
        self.df_log = tmp / "df.log"
        self.git_env = {**os.environ, "HOME": str(self.home), "GIT_CONFIG_GLOBAL": "/dev/null",
                        "GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "",
                        "GIT_COMMITTER_NAME": "t", "GIT_COMMITTER_EMAIL": ""}
        bin_dir = self.home / ".cargo" / "bin"
        bin_dir.mkdir(parents=True)
        (self.home / ".cache").mkdir()
        for name, text in (("cargo", CARGO), ("node", NODE), ("npm", NPM), ("df", DF)):
            self.put(bin_dir / name, text)
        self.root.mkdir()
        files = dict(COMMITTED)
        for name in REAL:
            files[f"scripts/{name}"] = (HERE / name).read_text()
        for name, text in files.items():
            self.put(self.root / name, text)
        for sh in ("gate.sh", "keel-guard.py", "shape.sh", "build.sh", "recall-tests.sh", "size-step.sh",
                   "prompt-step.sh"):
            (self.root / "scripts" / sh).chmod(0o755)
        self.git("init", "-q", "-b", "main")
        self.git("add", "-A")
        self.git("commit", "-q", "-m", "the tree")
        # What the compile phases and the benches need to find, none of it committed.
        (self.root / "cockpit" / "node_modules").mkdir()
        dist = self.root / "crates" / "theseusd" / "cockpit" / "dist"
        dist.mkdir(parents=True)
        (dist / "index.html").write_text("<html></html>\n")
        for b in ("theseusd", "theseus-sim", "theseus-index"):
            self.put(self.root / "target" / "debug" / b, PASSES, 0o755)

    def put(self, path, text, mode=0o755):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)
        path.chmod(mode)

    def git(self, *args):
        p = run(["git", *args], self.root, self.git_env)
        self.assertEqual(p.returncode, 0, p.stderr)
        return p.stdout

    def gate(self, **env):
        """The real gate.sh, with the stand-ins; (exit status, its output, the stand-in cargo's log lines)."""
        # What a door, or a caller judging a moved base, leaves in the environment is the tree it judges and not this one's.
        outer = ("CARGO_INCREMENTAL", "THESEUS_GATE_ROOT", "THESEUS_GATE_DOOR", "THESEUS_KEEL_BASE",
                 "THESEUS_GATE_MIN_FREE_GB")
        base = {k: v for k, v in self.git_env.items() if k not in outer}
        # The stand-in df has 500 GB free unless a test says otherwise, so the floor is no part of a test about the tree.
        e = {**base, "THESEUS_GATE_NO_BENCH": "1", "THESEUS_GATE_NO_SCRUB": "1", "STUB_LOG": str(self.log),
             "STUB_DF_LOG": str(self.df_log), "STUB_DF_AVAIL_KB": str(500 * GIB), "STUB_MOVE": "edit", **env}
        self.log.unlink(missing_ok=True)  # the calls of this run, whatever an earlier run of the same test made
        p = run(["bash", "scripts/gate.sh"], self.root, e)
        calls = self.log.read_text().splitlines() if self.log.exists() else []
        return p.returncode, p.stdout + p.stderr, calls

    # ---- the tree moves

    def test_a_tree_that_moved_after_the_test_build_aborts_the_gate_before_its_suite(self):
        rc, out, calls = self.gate(STUB_MOVE_AT="build")
        self.assertEqual(rc, MOVED, out)
        self.assertIn("gate: ABORTED", out)
        self.assertIn("NOT a test failure", out)
        self.assertNotIn("gate: FAILED", out, "an aborted gate is not a failed one")
        self.assertIn("built:", out)
        self.assertIn("found:", out)
        self.assertIn("notes.txt", out, "it names the path whose state changed")
        self.assertNotIn("suite inc=", "\n".join(calls), "no test ran on the other tree")
        self.assertNotIn("registry inc=", "\n".join(calls), "the reader rule is a test too")

    def test_a_tree_detached_and_restored_still_aborts_the_gate(self):
        # The incident itself: the same commit and the same files at the end, but for the window in between.
        rc, out, calls = self.gate(STUB_MOVE_AT="build", STUB_MOVE="away-and-back")
        self.assertEqual(rc, MOVED, out)
        self.assertIn("reflog", out, "the reflog is what saw the detach and the restore")
        self.assertNotIn("suite inc=", "\n".join(calls))

    def test_a_gate_that_failed_on_a_tree_that_moved_is_not_a_failure(self):
        # The move comes inside the suite, which then reds: the failure is the one the incident looked like.
        rc, out, calls = self.gate(STUB_MOVE_AT="suite", STUB_SUITE_RC="100")
        self.assertEqual(rc, MOVED, out)
        self.assertIn("gate: ABORTED", out)
        self.assertIn("a_test_that_fails_on_the_moved_tree", out, "the red is still in the log, for whoever reads it")
        self.assertNotIn("gate: FAILED", out)

    def test_a_tree_that_moved_inside_the_suite_does_not_pass_the_gate(self):
        rc, out, _ = self.gate(STUB_MOVE_AT="suite")
        self.assertEqual(rc, MOVED, out)
        self.assertNotIn("gate: ok", out)

    def test_a_phase_that_exits_75_on_an_unmoved_tree_is_a_failure_and_not_an_abort(self):
        # 75 is also EX_TEMPFAIL: only a move the gate recorded (or re-found) is its abort.
        rc, out, _ = self.gate(STUB_SUITE_RC="75")
        self.assertEqual(rc, 1, out)
        self.assertIn("gate: FAILED in suite", out)
        self.assertIn("a_test_that_fails_on_the_moved_tree", out)
        self.assertIn("on a tree that had not moved", out)
        self.assertNotIn("ABORTED", out)
        self.assertNotIn("THE TREE MOVED", out)

    def test_a_smoke_run_written_beside_the_gate_is_not_a_move(self):
        # scripts/release-smoke.sh writes bench/smoke/runs/<run>/report.md, tracked and not ignored.
        rc, out, calls = self.gate(STUB_MOVE_AT="suite", STUB_MOVE="smoke-run")
        self.assertEqual(rc, 0, out)
        self.assertIn("gate: ok", out)
        self.assertTrue((self.root / "bench/smoke/runs/2026-01-01-abcd1234/report.md").exists(), "the move did happen")
        self.assertTrue(any(c.startswith("suite ") for c in calls), calls)

    def test_a_move_that_is_only_a_rewritten_cargo_lock_says_to_commit_it(self):
        rc, out, _ = self.gate(STUB_MOVE_AT="build", STUB_MOVE="lock")
        self.assertEqual(rc, MOVED, out)
        self.assertIn("gate: ABORTED", out)
        self.assertIn("Cargo.lock", out)
        self.assertIn("the first cargo rewrote a stale Cargo.lock: commit the regenerated Cargo.lock, then rerun", out)
        self.assertNotIn("do not check out, reset or edit a tree", out)

    def test_a_move_of_another_file_keeps_the_old_advice(self):
        rc, out, _ = self.gate(STUB_MOVE_AT="build", STUB_MOVE="edit")
        self.assertEqual(rc, MOVED, out)
        self.assertIn("do not check out, reset or edit a tree while a gate runs in it; rerun the gate", out)
        self.assertNotIn("rewrote a stale Cargo.lock", out)

    # ---- the door's environment

    def test_the_doors_environment_does_not_reach_a_gate_these_tests_run(self):
        # Main's gate judges a branch that changes gate.sh or this file with THESEUS_GATE_ROOT (the tree it judges) and
        # THESEUS_GATE_DOOR=judge in its environment, the door's word to the copy it starts, and its `gate tests` phase
        # runs these: a gate here that took them went to the tree the door judges and ran there, and 7 of the 8 tests of
        # main's own failed on the ARM host that way (2026-11-08, theseus-4vsrz). Seeded here the way a door passes them.
        elsewhere = Path(self.tmp.name) / "elsewhere"
        elsewhere.mkdir()
        self.git_env.update({"THESEUS_GATE_ROOT": str(elsewhere), "THESEUS_GATE_DOOR": "judge", "THESEUS_KEEL_BASE": "no-such-rev"})
        rc, out, calls = self.gate()
        self.assertEqual(rc, 0, out)
        self.assertIn("gate: ok", out)
        self.assertTrue(any(c.startswith("suite ") for c in calls), f"the suite did run, in this tree: {calls}")

    def test_the_identity_comment_names_as_many_unseen_paths_as_tree_unseen_holds(self):
        # The comment once named one path over a two-entry list; a count keeps it from drifting again.
        lines = (HERE / "gate.sh").read_text().splitlines()
        arrays = [ln for ln in lines if ln.startswith("tree_unseen=(")]
        self.assertEqual(len(arrays), 1, arrays)
        self.assertEqual(arrays[0].count(":(exclude)"), 2, arrays[0])
        sentence = [ln for ln in lines if "minus " in ln and "tree-stamp" not in ln and "protocol.gen and" in ln]
        self.assertEqual(len(sentence), 1, sentence)
        self.assertIn("minus two paths", sentence[0])
        for path in ("cockpit/src/protocol.gen", "bench/smoke/runs"):
            self.assertIn(path, arrays[0])
            self.assertIn(path, sentence[0])

    # ---- what the suite does not run

    def stale_filter(self, committed=False):
        """A default-filter after test-threads in the lane's config, as the ARM runner's sed puts one."""
        path = self.root / ".config" / "nextest.toml"
        path.write_text(path.read_text().replace("test-threads = 8\n", "test-threads = 8\n" + FILTER + "\n"))
        if committed:
            self.git("add", "-A")
            self.git("commit", "-q", "-m", "a committed default-filter")

    def test_a_default_filter_left_in_the_lane_is_named_by_the_gate(self):
        # 2026-11-07: an uncommitted line a lane kept from an earlier run skipped four tests from every run, nextest
        # counted them as "skipped" with the ignored ones, and the gate said ok: nothing in the result told.
        self.stale_filter()
        rc, out, calls = self.gate()
        self.assertEqual(rc, 0, "a gate never refuses one, the ARM runner sets its known skips this way: " + out)
        self.assertIn("gate: suite filters: ACTIVE", out)
        self.assertIn("test(=the_owner_starts_asks_answers_joins_pauses_ends_and_finds_two_tasks)", out)
        self.assertIn("test(=rendering_costs_little_per_hundred_lines)", out, "the whole expression, not its start")
        self.assertIn("profile default, .config/nextest.toml:3", out, "which profile, and the line to delete")
        self.assertIn("only in the working tree, not in HEAD", out)
        self.assertIn("ran under a default-filter", out, "said again beside the verdict")
        self.assertRegex(out, r"\n  suite filtered +\d+\n", "the table does not call a filtered suite `suite`")
        self.assertTrue(any(c.startswith("suite ") for c in calls), f"the suite did run: {calls}")

    def test_a_default_filter_parked_in_a_profile_the_run_does_not_use_is_named_and_no_mismatch(self):
        # nextest ran the default profile and rightly applied nothing: a filter under [profile.arm] is not this run's.
        path = self.root / ".config" / "nextest.toml"
        path.write_text(path.read_text() + "\n[profile.arm]\n" + FILTER + "\n")
        rc, out, calls = self.gate()
        self.assertEqual(rc, 0, out)
        self.assertNotIn("MISMATCH", out)
        self.assertIn("profile arm", out, "still named, losing it would be the worse bug")
        self.assertIn(".config/nextest.toml:", out)
        self.assertRegex(out, r"\n  suite +\d+\n", "not this run's filter: the phase is plain")
        self.assertTrue(any(c.startswith("suite ") for c in calls), f"the suite did run: {calls}")

    def test_a_committed_default_filter_is_named_as_committed(self):
        self.stale_filter(committed=True)
        rc, out, _ = self.gate()
        self.assertEqual(rc, 0, out)
        self.assertIn("gate: suite filters: ACTIVE", out)
        self.assertIn("committed in HEAD", out)
        self.assertNotIn("not in HEAD", out)

    def test_a_gate_with_no_default_filter_says_none(self):
        # The comment in the fixture's config names a default-filter and is none.
        rc, out, calls = self.gate()
        self.assertEqual(rc, 0, out)
        self.assertIn("gate: suite filters: none", out)
        self.assertNotIn("ACTIVE", out)
        self.assertNotIn("ran under a default-filter", out)
        self.assertRegex(out, r"\n  suite +\d+\n")
        self.assertTrue(any(c.startswith("suite ") for c in calls), f"the suite did run: {calls}")

    def test_a_filter_nextest_applied_that_the_banner_missed_is_a_mismatch(self):
        # The banner re-derives the filter with awk; nextest's own line says what it applied (one set through
        # NEXTEST_PROFILE, a config file passed in, or an overrides table never shows in the awk's reading).
        rc, out, _ = self.gate(STUB_APPLIED="4")
        self.assertEqual(rc, 0, "the exit status never changes: " + out)
        self.assertIn("gate: suite filters: none", out)
        self.assertIn("MISMATCH", out)
        self.assertIn("4 test(s) excluded", out)
        self.assertRegex(out, r"\n  suite filtered +\d+\n")

    def test_a_filter_the_banner_named_and_nextest_applied_prints_its_count_and_no_mismatch(self):
        self.stale_filter()
        rc, out, _ = self.gate(STUB_APPLIED="2")
        self.assertEqual(rc, 0, out)
        self.assertIn("gate: suite filters: ACTIVE", out)
        self.assertIn("2 test(s) excluded", out)
        self.assertNotIn("MISMATCH", out)
        self.assertRegex(out, r"\n  suite filtered +\d+\n")

    # ---- cargo deny: the gate's database is a pin, and the gate never fetches into it

    def deny_gate(self, age_days=None, no_fetch_head=False, **env):
        """deny_check through the real gate, with a CARGO_HOME of the test's own; (rc, output, deny argv lines)."""
        cargo_home = self.home / ".cargo"
        if age_days is not None:
            db = cargo_home / "advisory-dbs" / "advisory-db-abcd1234"
            (db / ".git").mkdir(parents=True, exist_ok=True)
            fetch_head = db / ".git" / "FETCH_HEAD"
            fetch_head.write_text("x\n")
            then = time.time() - age_days * 86400
            os.utime(fetch_head, (then, then))
        if no_fetch_head:
            (cargo_home / "advisory-dbs" / "advisory-db-abcd1234").mkdir(parents=True)
        deny_log = Path(self.tmp.name) / "deny.log"
        rc, out, _ = self.gate(CARGO_HOME=str(cargo_home), STUB_DENY_LOG=str(deny_log), **env)
        calls = deny_log.read_text().splitlines() if deny_log.exists() else []
        self.assertFalse([c for c in calls if "fetch" in c.split()], f"the gate fetched: {calls}")
        return rc, out, calls

    BANS = "deny --offline --log-level error check bans licenses sources"
    FULL = "deny --offline --log-level error check"

    def test_deny_without_a_database_says_advisories_were_not_checked_and_runs_no_command_from_its_echo(self):
        rc, out, calls = self.deny_gate()
        self.assertEqual(rc, 0, out)
        self.assertIn("advisories NOT checked (`cargo deny fetch db` makes the gate's pin)", out)
        self.assertEqual(calls, [self.BANS], calls)

    def test_deny_with_an_old_database_names_its_age_and_runs_no_command_from_its_echo(self):
        rc, out, calls = self.deny_gate(age_days=15)
        self.assertEqual(rc, 0, out)
        self.assertIn("is 15 days old", out)
        self.assertIn("`cargo deny fetch db` moves it", out)
        self.assertEqual(calls, [self.FULL], calls)

    def test_deny_with_a_fresh_database_prints_no_age_line(self):
        rc, out, calls = self.deny_gate(age_days=1)
        self.assertEqual(rc, 0, out)
        self.assertNotIn("days old", out)
        self.assertEqual(calls, [self.FULL], calls)

    def test_deny_with_only_advisories_failing_says_so_and_fails_the_gate_in_deny(self):
        rc, out, calls = self.deny_gate(age_days=15, STUB_DENY="advisories")
        self.assertEqual(rc, 1, out)
        self.assertIn("ONLY advisories failed", out)
        self.assertIn("`cargo deny fetch db`", out)
        self.assertIn("gate: FAILED in deny", out)
        self.assertEqual(calls, [self.FULL, self.BANS], calls)

    def test_deny_with_a_database_of_unknown_age_never_says_unknown_days_old(self):
        # One age, through deny_db_age: no FETCH_HEAD reads as a sentence, not "is unknown days old".
        rc, out, calls = self.deny_gate(no_fetch_head=True, STUB_DENY="advisories")
        self.assertEqual(rc, 1, out)
        self.assertIn("ONLY advisories failed", out)
        self.assertNotIn("unknown days old", out)
        self.assertIn("The advisory database's age is unknown (no FETCH_HEAD); it is the gate's pin", out)
        self.assertNotIn("days old; it is the gate's pin, and", out, "no old-database line without an age")
        self.assertEqual(calls, [self.FULL, self.BANS], calls)

    # ---- and what is not a move

    def test_an_unmoved_tree_passes_and_every_cargo_ran_without_incremental(self):
        rc, out, calls = self.gate()
        self.assertEqual(rc, 0, out)
        self.assertIn("gate: ok", out)
        self.assertNotIn("ABORTED", out)
        # A zero is met by an empty set: the calls must be there, and the phases that read them.
        self.assertGreaterEqual(len(calls), 9, calls)
        for phase in ("fmt", "tree", "clippy", "build", "registry", "protocol", "suite", "deny"):
            self.assertTrue(any(c.startswith(phase + " ") for c in calls), f"no `{phase}` call in {calls}")
        self.assertEqual({c.split(" ", 1)[1] for c in calls}, {"inc=0"}, calls)
        # The deny phase ran (a positive), and the stand-in cargo wrote no redirect error into the output.
        self.assertIn("deny: no advisory database", out)
        self.assertNotIn("No such file or directory", out)

    def test_an_incremental_build_the_caller_asked_for_holds_from_the_first_cargo_to_the_last(self):
        # Flipping it half way builds a second complete set of artifacts, so the gate sets one value for all of it.
        rc, out, calls = self.gate(CARGO_INCREMENTAL="1")
        self.assertEqual(rc, 0, out)
        self.assertGreaterEqual(len(calls), 9, calls)
        self.assertEqual({c.split(" ", 1)[1] for c in calls}, {"inc=1"}, calls)

    def test_the_cockpits_generated_types_failing_the_gate_is_still_a_failure_and_not_a_move(self):
        # The protocol phase writes cockpit/src/protocol.gen itself, and fails saying `git add it`: its own write.
        rc, out, _ = self.gate(STUB_PROTOCOL="regenerate")
        self.assertEqual(rc, 1, out)
        self.assertIn("cockpit/src/protocol.gen changed", out)
        self.assertNotIn("ABORTED", out)
        self.assertIn("gate: FAILED in protocol types", out)

    def test_the_tree_is_read_before_the_first_compile_and_checked_at_each_stage(self):
        text = (HERE / "gate.sh").read_text()
        begin = text.index("\ntree_begin\n")
        self.assertLess(begin, text.index("\nphase fmt "), "the identity is read before the first compile")
        self.assertRegex(text, r'\n\s+tree_check "before the lock')
        self.assertRegex(text, r'\n\s+tree_check "when it took the lock')
        before_suite = text.index('tree_check "before the suite"')
        self.assertLess(before_suite, text.index("phase suite suite"))
        self.assertRegex(text, r'\ntree_check "at the end"')

    # ---- the disk floor (a gate that starts on a nearly full disk dies part way and leaves a target nobody can read)

    def df_calls(self):
        return self.df_log.read_text().splitlines() if self.df_log.exists() else []

    def ran_the_suite(self, calls):
        return any(c.startswith("suite ") for c in calls)

    def test_a_disk_below_the_floor_refuses_the_gate_before_any_build(self):
        # 2026-11-07: `/` was 631 GB of 639 GB used, 2.1 GB free, and a 40-minute build began on it.
        rc, out, calls = self.gate(STUB_DF_AVAIL_KB=str(2 * GIB))
        self.assertEqual(rc, FLOOR, out)
        self.assertNotIn(rc, (0, 1, 2, MOVED), "a status of its own: not a failure, not a usage error, not a moved tree")
        self.assertIn("gate: REFUSED: only 2.0 GB free", out, "the number it read")
        self.assertIn("below the disk floor of 10 GB", out, "the floor it compared against")
        self.assertIn(str((self.root / "target").resolve()), out, "which filesystem it read")
        self.assertIn("THESEUS_GATE_MIN_FREE_GB", out, "how to override it, by name")
        self.assertIn("gate: FAILED in disk floor", out, "the phase table names it")
        self.assertNotIn("gate: ok", out)
        self.assertEqual(calls, ["fmt inc=0"], "cargo ran fmt and nothing after it: it refused before the first resolve or compile")
        self.assertEqual(self.df_calls(), ["target"], "and it asked once, about the target")
        # The one call above means something only if this harness records the calls of a gate that runs.
        rc, out, calls = self.gate(STUB_DF_AVAIL_KB=str(40 * GIB))
        self.assertEqual(rc, 0, out)
        self.assertTrue(self.ran_the_suite(calls), f"the same tree, on a roomy disk, ran its suite: {calls}")

    def test_a_disk_exactly_at_the_floor_passes_and_the_gate_says_what_it_read(self):
        rc, out, calls = self.gate(STUB_DF_AVAIL_KB=str(10 * GIB))
        self.assertEqual(rc, 0, out)
        self.assertIn("gate: ok", out)
        self.assertIn(f"gate: disk floor: 10.0 GB free on {(self.root / 'target').resolve()} (floor 10 GB)", out)
        self.assertIn("  disk floor", out, "a phase of the table")
        self.assertTrue(self.ran_the_suite(calls), f"the suite did run: {calls}")
        # One KiB less is under it, and shown as under it (9.9, never a rounded 10.0 below 10).
        rc, out, calls = self.gate(STUB_DF_AVAIL_KB=str(10 * GIB - 1))
        self.assertEqual(rc, FLOOR, out)
        self.assertIn("gate: REFUSED: only 9.9 GB free", out)
        self.assertFalse(self.ran_the_suite(calls), calls)

    def test_the_floor_can_be_overridden_for_a_deliberate_small_disk_run(self):
        low = str(2 * GIB)
        rc, out, calls = self.gate(STUB_DF_AVAIL_KB=low, THESEUS_GATE_MIN_FREE_GB="1")
        self.assertEqual(rc, 0, out)
        self.assertIn("(floor 1 GB)", out, "the gate says which floor it held the disk to")
        self.assertIn("gate: disk floor: 2.0 GB free on ", out)
        self.assertTrue(self.ran_the_suite(calls), f"the suite did run: {calls}")
        self.assertEqual(self.df_calls(), ["target"])
        # An override above the disk refuses, so the number is the one in force and not decoration.
        rc, out, calls = self.gate(STUB_DF_AVAIL_KB=low, THESEUS_GATE_MIN_FREE_GB="3")
        self.assertEqual(rc, FLOOR, out)
        self.assertIn("below the disk floor of 3 GB", out)
        self.assertFalse(self.ran_the_suite(calls), calls)
        # A leading zero is a decimal (bash would read 08 as a bad octal number, and 010 as 8).
        rc, out, calls = self.gate(STUB_DF_AVAIL_KB=str(9 * GIB), THESEUS_GATE_MIN_FREE_GB="010")
        self.assertEqual(rc, FLOOR, out)
        self.assertIn("below the disk floor of 10 GB", out)
        self.assertFalse(self.ran_the_suite(calls), calls)
        # 0 is no floor at all, and then df is not asked.
        asked = len(self.df_calls())
        rc, out, calls = self.gate(STUB_DF_AVAIL_KB=low, THESEUS_GATE_MIN_FREE_GB="0")
        self.assertEqual(rc, 0, out)
        self.assertIn("gate: disk floor: off (THESEUS_GATE_MIN_FREE_GB=0)", out)
        self.assertTrue(self.ran_the_suite(calls), f"the suite did run: {calls}")
        self.assertEqual(len(self.df_calls()), asked, "no floor, no df")

    def test_a_floor_that_is_not_a_whole_number_of_gb_is_a_usage_error(self):
        for bad in ("plenty", "-3", "1.5", "10GB"):
            with self.subTest(bad=bad):
                rc, out, calls = self.gate(THESEUS_GATE_MIN_FREE_GB=bad)
                self.assertEqual(rc, 2, out)
                self.assertIn(f"THESEUS_GATE_MIN_FREE_GB is '{bad}'", out)
                self.assertEqual(calls, [], "it refused before the first compile")
                self.assertNotIn("gate: ok", out)

    def test_the_floor_reads_the_filesystem_that_holds_the_target(self):
        self.gate(STUB_DF_AVAIL_KB=str(2 * GIB))
        self.assertEqual(self.df_calls(), ["target"], "a lane's target may be a symlink to another disk: df follows it")
        # A lane that has not built has no target yet: the nearest directory there is is on the disk it will be made on,
        # and a df of the missing path would fail.
        shutil.rmtree(self.root / "target")
        rc, out, calls = self.gate(STUB_DF_AVAIL_KB=str(2 * GIB))
        self.assertEqual(self.df_calls(), ["target", "."])
        self.assertEqual(rc, FLOOR, out)
        self.assertIn(f"only 2.0 GB free on {self.root.resolve()},", out)
        self.assertEqual(calls, ["fmt inc=0"])

    def test_a_gate_that_fails_at_fmt_on_a_small_disk_fails_there_and_does_not_read_the_disk(self):
        # The guard's own tests run this gate in throwaway repositories under the temp directory and expect it to fail at
        # fmt there. That directory can be a small tmpfs (16 GB with 9.5 GB free on the ARM host, 2026-11-09): the first
        # version of the floor stood at the head of the flow, and eight of those tests were refused by it ("only 9.4 GB
        # free on /tmp/keel-...", in the first ARM gate to carry it). So the floor comes after fmt, and a gate that fails
        # at fmt says so whatever the disk.
        rc, out, calls = self.gate(STUB_DF_AVAIL_KB=str(2 * GIB), STUB_FMT_FAIL="1")
        self.assertEqual(rc, 1, out)
        self.assertIn("gate: FAILED in fmt", out)
        self.assertNotIn("REFUSED", out)
        self.assertEqual(self.df_calls(), [], "df was not asked")
        self.assertEqual(calls, ["fmt inc=0"], "fmt ran, and nothing after it")
        # The same tree, fmt passing, is refused by the floor on that disk: the empty df log above is not a gate that never got here.
        rc, out, calls = self.gate(STUB_DF_AVAIL_KB=str(2 * GIB))
        self.assertEqual(rc, FLOOR, out)
        self.assertEqual(self.df_calls(), ["target"])

    def test_the_floor_stands_after_fmt_and_before_the_first_phase_that_resolves_or_compiles(self):
        names = re.findall(r'\nphase\s+(?:"([^"]+)"|(\S+))', (HERE / "gate.sh").read_text())
        order = [a or b for a, b in names]
        self.assertEqual(order.count("disk floor"), 1, order)
        floor = order.index("disk floor")
        self.assertLess(order.index("fmt"), floor, "after fmt, where the guard's own tests stop a throwaway repository")
        for compiles in ("features", "clippy", "test build"):
            self.assertLess(floor, order.index(compiles), f"before `{compiles}`: {order}")

    def test_a_df_that_cannot_be_read_refuses_the_gate_and_says_so(self):
        # A gate that cannot show it has room is not told it has; the override is named, and df is not asked under it.
        for what, env in (("garbage", {"STUB_DF_AVAIL_KB": "n/a"}), ("a failing df", {"STUB_DF_FAIL": "1"})):
            with self.subTest(what=what):
                rc, out, calls = self.gate(**env)
                self.assertEqual(rc, FLOOR, out)
                self.assertIn("gate: REFUSED: could not read the free space on", out)
                self.assertIn("THESEUS_GATE_MIN_FREE_GB", out)
                self.assertEqual(calls, ["fmt inc=0"])
                rc, out, calls = self.gate(THESEUS_GATE_MIN_FREE_GB="0", **env)
                self.assertEqual(rc, 0, out)
                self.assertTrue(self.ran_the_suite(calls), f"the suite did run: {calls}")

    def test_the_number_the_gate_reads_is_the_real_dfs_available_column(self):
        # Every other test answers for df. This one lets the real one answer, so the parse is of what df prints.
        before = shutil.disk_usage(self.root / "target").free / 2**30
        rc, out, calls = self.gate(STUB_DF_REAL="1", THESEUS_GATE_MIN_FREE_GB="1")
        after = shutil.disk_usage(self.root / "target").free / 2**30
        self.assertEqual(rc, 0, out)
        m = re.search(r"gate: disk floor: (\d+)\.(\d) GB free on (\S+) \(floor 1 GB\)", out)
        self.assertIsNotNone(m, out)
        read = float(f"{m[1]}.{m[2]}")
        # Free space moves while the gate runs (other builds write here), so it is held to the readings either side.
        self.assertGreaterEqual(read, min(before, after) - 0.2, (before, read, after))
        self.assertLessEqual(read, max(before, after) + 0.2, (before, read, after))
        self.assertTrue(self.ran_the_suite(calls), f"the suite did run: {calls}")


# A stand-in cargo for deny-daily.sh: logs its argv and keeps a copy of the --config file it is given (the script deletes it).
DENY_CARGO = r"""#!/usr/bin/env bash
echo "$*" >>"$STUB_CALLS"
while [ $# -gt 0 ]; do
  if [ "$1" = --config ]; then cp "$2" "$STUB_CONFIG_COPY.$(wc -l <"$STUB_CALLS" | tr -d ' ')"; fi
  shift
done
exit 0
"""


class DenyDaily(unittest.TestCase):
    """scripts/deny-daily.sh is the half of the deny contract that must never touch the gate's database."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        tmp = Path(self.tmp.name)
        self.home, self.repo = tmp / "home", tmp / "repo"
        bin_dir = self.home / ".cargo" / "bin"
        bin_dir.mkdir(parents=True)
        (bin_dir / "bd").write_text("#!/bin/sh\nexit 0\n")
        (bin_dir / "bd").chmod(0o755)
        (tmp / "cargo").write_text(DENY_CARGO)
        (tmp / "cargo").chmod(0o755)
        self.repo.mkdir()
        self.calls = tmp / "calls.log"
        self.env = {**os.environ, "HOME": str(self.home), "GIT_CONFIG_GLOBAL": "/dev/null", "GIT_AUTHOR_NAME": "t",
                    "GIT_AUTHOR_EMAIL": "", "GIT_COMMITTER_NAME": "t", "GIT_COMMITTER_EMAIL": "",
                    "THESEUS_REPO": str(self.repo), "THESEUS_CARGO": str(tmp / "cargo"), "THESEUS_DENY_DRY_RUN": "1",
                    "THESEUS_DENY_DB": str(tmp / "daily-db"), "THESEUS_DENY_LOG": str(tmp / "daily.log"),
                    "STUB_CALLS": str(self.calls), "STUB_CONFIG_COPY": str(tmp / "cfg"), "TMPDIR": str(tmp)}
        self.tmp_path = tmp

    def repo_with(self, deny_toml):
        (self.repo / "deny.toml").write_text(deny_toml)
        for args in (("init", "-q", "-b", "main"), ("add", "-A"), ("commit", "-q", "-m", "x")):
            p = run(["git", *args], self.repo, self.env)
            self.assertEqual(p.returncode, 0, p.stderr)

    def deny_daily(self):
        p = run(["bash", str(HERE / "deny-daily.sh")], self.repo, self.env)
        calls = self.calls.read_text().splitlines() if self.calls.exists() else []
        return p.returncode, p.stdout + p.stderr, calls

    def test_the_real_deny_toml_gets_the_jobs_own_database_and_both_calls_carry_the_config(self):
        self.repo_with((REPO / "deny.toml").read_text())
        rc, out, calls = self.deny_daily()
        self.assertEqual(rc, 0, out)
        self.assertEqual(len(calls), 2, calls)
        for c in calls:
            self.assertIn("--config", c)
        self.assertIn(" fetch db index", calls[0])
        self.assertTrue(calls[1].endswith(" check"), calls)
        db = str(self.tmp_path / "daily-db")
        for n in (1, 2):
            lines = (self.tmp_path / f"cfg.{n}").read_text().splitlines()
            at = lines.index("[advisories]")
            self.assertEqual(lines[at + 1], f'db-path = "{db}"', "db-path sits directly under [advisories]")
        self.assertFalse((self.home / ".cargo" / "advisory-dbs").exists(), "the gate's database was never created")

    def test_a_deny_toml_without_an_advisories_table_stops_with_exit_3_and_calls_nothing(self):
        self.repo_with('[licenses]\nallow = ["MIT"]\n')
        rc, out, calls = self.deny_daily()
        self.assertEqual(rc, 3, out)
        self.assertIn("has no [advisories] table", out)
        self.assertEqual(calls, [])


# A stand-in cargo for build.sh: makes target/<profile>/theseusd (7 bytes) and, when STUB_BUILD_MOVE is set, edits a tracked file.
BUILD_CARGO = r"""#!/usr/bin/env bash
mkdir -p target/release
printf 'daemon\n' >target/release/theseusd
[ -z "${STUB_BUILD_MOVE:-}" ] || echo "edited during the build" >>notes.txt
exit 0
"""


class BuildStamp(unittest.TestCase):
    """build.sh stamps the tree it started from, and says so when the tree moved under the build."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        tmp = Path(self.tmp.name)
        self.home, self.root = tmp / "home", tmp / "tree"
        bin_dir = self.home / ".cargo" / "bin"
        bin_dir.mkdir(parents=True)
        (bin_dir / "cargo").write_text(BUILD_CARGO)
        (bin_dir / "cargo").chmod(0o755)
        (self.root / "scripts").mkdir(parents=True)
        for name in ("build.sh", "tree-stamp.sh"):
            (self.root / "scripts" / name).write_text((HERE / name).read_text())
            (self.root / "scripts" / name).chmod(0o755)
        (self.root / "scripts" / "discard-eh-frame.ld").write_text("")
        (self.root / "notes.txt").write_text("a tracked file\n")
        (self.root / ".gitignore").write_text("target\n")
        self.env = {k: v for k, v in os.environ.items() if k not in ("CARGO_TARGET_DIR", "CARGO_HOME", "SOURCE_DATE_EPOCH")}
        self.env.update({"HOME": str(self.home), "GIT_CONFIG_GLOBAL": "/dev/null", "GIT_AUTHOR_NAME": "t",
                         "GIT_AUTHOR_EMAIL": "", "GIT_COMMITTER_NAME": "t", "GIT_COMMITTER_EMAIL": ""})
        for args in (("init", "-q", "-b", "main"), ("add", "-A"), ("commit", "-q", "-m", "x")):
            self.assertEqual(run(["git", *args], self.root, self.env).returncode, 0)

    def build(self, **env):
        return run(["bash", "scripts/build.sh"], self.root, {**self.env, **env})

    def test_a_tree_that_moved_during_the_build_gets_no_stamp_and_both_identities_are_printed(self):
        stamp = self.root / "target" / "release" / ".theseus-build-stamp"
        stamp.parent.mkdir(parents=True)
        stamp.write_text("an old stamp\n")
        head = run(["git", "rev-parse", "HEAD"], self.root, self.env).stdout.strip()
        p = self.build(STUB_BUILD_MOVE="1")
        self.assertNotEqual(p.returncode, 0, p.stdout + p.stderr)
        self.assertFalse(stamp.exists(), "no stamp, and the old one is removed")
        self.assertIn(f"before: {head} clean", p.stderr)
        self.assertRegex(p.stderr, r"after:  " + head + r" [0-9a-f]{16}")

    def test_an_unmoved_tree_is_stamped_with_its_before_identity_and_the_byte_count(self):
        head = run(["git", "rev-parse", "HEAD"], self.root, self.env).stdout.strip()
        p = self.build()
        self.assertEqual(p.returncode, 0, p.stdout + p.stderr)
        stamp = (self.root / "target" / "release" / ".theseus-build-stamp").read_text()
        self.assertEqual(stamp, f"{head} clean 7\n")


if __name__ == "__main__":
    unittest.main()
