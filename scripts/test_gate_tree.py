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
# The same, for a phase the gate invokes through `python3` rather than as a program of its own.
PASSES_PY = "import sys\n\nsys.exit(0)\n"

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
    # A stand-in, like shape.sh's and for the same reason: the `context ceiling` phase is a HARD one, so a
    # real run here would need this throwaway tree to carry context_files.rs and project_context.rs just to
    # declare a ceiling, and would redden the gate for the tree's own shape rather than for the gate's
    # wiring. The script's own logic is covered by scripts/tests/test_context_ceiling.py (24 cases), which
    # is where a ceiling bug should surface. Unlike line-pressure.py below, this phase CAN fail a gate, so
    # the "real script, interesting exit 2" argument does not transfer.
    "scripts/context-ceiling.py": PASSES_PY,
    "scripts/build.sh": PASSES,
    "scripts/recall-tests.sh": PASSES,
    "scripts/size-step.sh": PASSES,
    "scripts/prompt-step.sh": PASSES,
}
# The real scripts the gate runs here. line-pressure.py is real rather than a stand-in because the whole point of
# its phase is that it cannot fail a gate, and the interesting case is the one this repository IS: a tree with
# almost no Rust in it, where the script finds no function and exits 2. A stand-in that passes would prove nothing.
REAL = ("gate.sh", "keel-guard.py", "line-pressure.py")


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

    # ---- the warning phases (theseus-4vsrz): they report, and they never decide

    def test_the_line_pressure_phase_reports_and_cannot_fail_the_gate(self):
        """The owner's ruling, 2026-11-10: line-pressure.py is wired as a warning, so its exit status never
        decides the gate's. This tree has one .rs file holding a `const`, so the real script finds NO function
        and exits 2 — the exact case the ruling has to survive, and the gate still goes green."""
        rc, out, calls = self.gate()
        self.assertEqual(rc, 0, out)
        self.assertTrue(self.ran_the_suite(calls), f"the gate ran on past the phase: {calls}")
        self.assertIn("line pressure", out, "the phase said something")
        self.assertNotIn("FAILED in line pressure", out)
        # The script really did run and really did exit 2 here; the phase turned that into a line, not a verdict.
        p = run(["python3", "scripts/line-pressure.py"], self.root, self.git_env)
        self.assertEqual(p.returncode, 2, p.stdout + p.stderr)

    def test_the_warning_phases_run_between_the_file_ceiling_and_clippys_function_ceiling(self):
        """`shape` holds the file ceiling and `clippy` the function one; the warning about approaching the
        latter is only useful above the failure it predicts, so it runs before clippy and after shape."""
        names = re.findall(r'\nphase\s+(?:"([^"]+)"|(\S+))', (HERE / "gate.sh").read_text())
        order = [a or b for a, b in names]
        for warn in ("line pressure", "script tests"):
            self.assertEqual(order.count(warn), 1, order)
            self.assertLess(order.index("shape"), order.index(warn), f"`{warn}` after `shape`: {order}")
            self.assertLess(order.index(warn), order.index("clippy"), f"`{warn}` before `clippy`: {order}")

    def test_the_context_ceiling_phase_is_wired_beside_the_other_file_ceiling(self):
        """Deleting `phase "context ceiling"` from gate.sh left every other test in this file green
        (theseus-4vsrz, the ceiling review's §5.2), because the phase's own 24 cases test the script and
        nothing tested the wiring. A hard ceiling that nothing runs is the exact shape of defect that row
        exists to close, so the wiring is asserted here rather than left to the next reader to notice."""
        names = re.findall(r'\nphase\s+(?:"([^"]+)"|(\S+))', (HERE / "gate.sh").read_text())
        order = [a or b for a, b in names]
        self.assertEqual(order.count("context ceiling"), 1, order)
        self.assertLess(order.index("shape"), order.index("context ceiling"), order)
        self.assertLess(order.index("context ceiling"), order.index("clippy"), order)

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

    # ---- what the verdict line says a green did NOT run (theseus-4vsrz, B7)

    def verdict(self, out):
        """The `gate: ok…`/`gate: FAILED…` line, which is the last thing the gate prints."""
        lines = [l for l in out.split("\n") if l.startswith(("gate: ok", "gate: FAILED"))]
        self.assertEqual(len(lines), 1, f"one verdict line, got {lines}")
        return lines[0]

    def test_a_lane_gates_verdict_names_the_benches_it_did_not_run(self):
        # THE CASE THAT MOTIVATED THE ROW. THESEUS_GATE_NO_BENCH=1 is the NORMAL lane gate, and before this the
        # verdict read `gate: ok (skipped: scrub)` under it -- identical to a run with the benches -- with
        # `lifecycle` and `jobs` sitting in the phase table as though they had run. Both benches live in the
        # locked part, which is ANOTHER PROCESS, so this also holds the record channel that carries a
        # stand-down across the lock: a list kept only in the locked part's own memory would name neither.
        rc, out, calls = self.gate()
        self.assertEqual(rc, 0, out)
        v = self.verdict(out)
        self.assertIn("lifecycle", v, v)
        self.assertIn("jobs", v, v)
        self.assertIn("the turn bench's burst and its record", v, v)
        # And the table says it too, for a reader who sees only the tail.
        self.assertIn("gate: what this run did NOT do: ", out)

    def test_the_verdict_names_the_scrub_the_fleet_asks_it_to_skip(self):
        # The fleet and ARM runners both set THESEUS_GATE_NO_SCRUB=1, and on 2026-11-10 a real blocking name hit
        # was caught by a by-hand range scrub that a fleet green had said nothing about.
        rc, out, calls = self.gate()
        self.assertEqual(rc, 0, out)
        self.assertIn("scrub", self.verdict(out))

    def test_the_verdict_never_names_a_phase_that_ran(self):
        rc, out, calls = self.gate()
        self.assertEqual(rc, 0, out)
        v = self.verdict(out)
        for ran in ("fmt", "clippy", "cockpit", "shape", "features", "protocol types", "reader rule"):
            self.assertNotIn(ran, v, f"{ran} ran, so the verdict must not name it: {v}")

    def test_a_phase_whose_own_script_says_it_skipped_is_named_in_the_verdict(self):
        # size-step.sh, recall-tests.sh and prompt-step.sh each decide for themselves whether they can run and
        # say so on stdout, exiting 0 either way: the gate cannot tell from the status, so it reads what they
        # said. `size` is the common one -- a lane that has not made a release build skips it on every gate.
        self.put(self.root / "scripts" / "size-step.sh",
                 '#!/bin/sh\necho "size: skipped: no release build of deadbeef"\nexit 0\n')
        rc, out, calls = self.gate()
        self.assertEqual(rc, 0, out)
        self.assertIn("size", self.verdict(out))

    def test_a_phase_whose_own_script_ran_is_not_named(self):
        # The same wrapper, with a script that did its work: its output still reaches the log, and nothing is
        # registered. A wrapper that registered on the strength of being wrapped would report every run.
        self.put(self.root / "scripts" / "size-step.sh",
                 '#!/bin/sh\necho "size: 61.2 MB, 0.3 MB over the last recorded"\nexit 0\n')
        rc, out, calls = self.gate()
        self.assertEqual(rc, 0, out)
        self.assertIn("size: 61.2 MB", out, "the script's own output still reaches the log")
        self.assertNotIn("size", self.verdict(out))

    def test_a_filtered_suite_is_a_partial_run_and_the_verdict_says_so(self):
        # A default-filter skips tests in every run, which `suite filtered` has long said in the table. The
        # verdict line said nothing, and the table is 20 lines above the line a reader greps for.
        self.stale_filter()
        rc, out, calls = self.gate()
        self.assertEqual(rc, 0, out)
        self.assertIn("some of the suite (a default-filter)", self.verdict(out))

    def test_a_gate_that_could_not_take_the_lock_says_the_suite_did_not_run(self):
        # NOT A SKIP FLAG, AND THE WORST CASE OF THE CLASS (2026-11-10 15:44, the furthest any self build has
        # reached). `flock` execs this script again as the locked part, and the frozen copy it was given had no
        # execute bit, so flock exited 126 ("failed to execute ...: Permission denied") and the part never began.
        # What the gate said then: prose that the tests and benches did not run, a table reading
        # `lock wait 0  <- failed here` for a run that never waited for a lock, and `gate: FAILED in lock wait`.
        # A reader of the tail was sent to the lock, and the lock was fine.
        #
        # WHY A STAND-IN FLOCK AND NOT A CHMOD. Reproducing the cause literally -- `chmod 644` on the gate's own
        # copy, which bash still runs by name and flock still cannot exec -- also makes scripts/gate.sh read as
        # CHANGED, and a changed gate.sh turns on the `keel tests` phase, which runs a suite this harness has no
        # tree for: the gate then dies 1000 lines earlier, on an unrelated phase. So flock's failure is what is
        # stood in for, with its real message and its real status. The gate's input is identical either way, and
        # it is the whole input this test is about: a nonzero flock and a record with no `began` line in it.
        self.put(self.home / ".cargo" / "bin" / "flock",
                 # `flock -o LOCK SELF --locked-part …`, so $3 is the copy it could not exec. Only this one call
                 # exists; the gate refuses outright to be run with a flock around itself.
                 '#!/bin/sh\necho "flock: failed to execute $3: Permission denied" >&2\nexit 126\n')
        rc, out, _ = self.gate()
        self.assertNotEqual(rc, 0, out)
        self.assertIn("the shared lock was not taken, so the tests and benches did not run", out)
        self.assertIn("gate: FAILED in lock wait", out,
                      "the phase's own name is the other half of this defect, and another row's line")
        # The half this row owns: on a FAILED run the summary names the stand-down too, so the misnamed phase
        # is no longer the only thing a reader of the tail has to go on.
        self.assertIn("gate: what this run did NOT do:", out)
        self.assertIn("the tests and benches (the shared lock was not taken)", out)


class SkipRegistry(unittest.TestCase):
    """Every place the gate stands a check down registers itself, or this reddens (theseus-4vsrz, B7).

    `gate: ok` read identically for a run that made every check and one that made most of them. The fix is a
    list the sites write into, and the hazard it brings is that the list is only as honest as its coverage: the
    first draft inferred the list from phase labels, exactly one site sets such a label, and so a lane gate
    under THESEUS_GATE_NO_BENCH=1 -- the normal lane gate -- reported one stand-down out of five and read as
    though that were all of them. A reporting fix that under-reports is a new instance of the defect it fixes.

    So the rule is checked, not trusted. These are static reads of scripts/gate.sh: they cost no gate run and
    they catch the case no passing run can, which is a stand-down nobody wired up.

    THE ONE PHASE DELIBERATELY NOT REGISTERED, and the reason is worth keeping. The gate's first phase -- the
    one that decides which copy of this script judges the tree -- returns early in four places, and NONE of
    them registers. Two are not stand-downs at all: under the `judge` word this copy IS the judge, and under
    the `own` word the first pass already ran it, so its work happened in the process above. The other two (no
    main to compare against; main's copy has no such phase yet) are real weakenings, and the runner that meets
    them says so itself, loudly, in its own log. And registering them is not free: scripts/keel-guard.py holds
    every line of gate.sh matching `keel|door` case-insensitively, each of those four conditions names the
    environment variable, and so every edit that would add a registration to them MODIFIES a held line and
    turns a free change into one needing the owner's signed acknowledgement. That is a real cost against a
    thin benefit, so it is written down here rather than paid. Anyone who wants to pay it should know what the
    bill is before they start.
    """

    GATE = (REPO / "scripts" / "gate.sh").read_text()

    # Lines that print this language but are the REPORTER, or prose about someone else's count, rather than a
    # place that stood a check down. Keyed by their text and not their line number: on 2026-11-10 a review of
    # this very patch cited six reader line numbers and five of the six had already moved.
    REPORTERS = {
        # The two verdict lines and the line under the phase table: where the list is printed.
        'echo "gate: ok (main\'s gate judged this tree)$(skipped_clause)"',
        'echo "gate: ok$(skipped_clause)"',
        'echo "gate: what this run did NOT do: $(skipped_list)"',
        "printf ' (skipped: %s)' \"$(skipped_list)\"",
        # The record that carries a locked-part stand-down across the lock.
        "printf 'skipped 0 %s\\n' \"$s\"",
        # nextest's own "skipped" count, which includes the #[ignore]d tests: prose about the suite's summary,
        # beside a filter whose stand-down is registered where it is detected (suite_filters, suite_applied).
        'echo "gate: the suite above ran under a default-filter (suite filters, above): its \\"skipped\\" count'
        ' includes the tests the filter excluded"',
    }

    # Lines that say a check did not happen AND RED THE GATE on the next statement. These need no registration:
    # the list exists so a GREEN cannot read as more complete than it was, and a run that ends `gate: FAILED`
    # claims no completeness at all. Kept separate from REPORTERS because the reason is different -- a reporter
    # prints the list, these end the run -- and a future line that lands here by mistake should have to argue for
    # itself. Add one only if the `exit` is unconditional and in the same block.
    REDS_THE_GATE = {
        'echo "gate: the generator test did not run once and pass (theseus-5epuo): the files below were not'
        ' regenerated from the Rust types"',
    }

    def functions(self):
        """{name: body} for gate.sh's top-level functions, which all open and close at column 0."""
        out, name, body = {}, None, []
        for line in self.GATE.split("\n"):
            m = re.match(r"^([A-Za-z_][A-Za-z_0-9]*)\(\) \{", line)
            if m:
                name, body = m[1], []
            elif line == "}" and name:
                out[name] = "\n".join(body)
                name = None
            elif name is not None:
                body.append(line)
        return out

    # `skip` as a command word: at the head of a line, or after a separator, a group opener, or a
    # `then`/`else`/`do`. The lookahead keeps the function's own definition (`skip() {`) and the
    # skipped_list/skipped_clause helpers out of it.
    #
    # THE GROUP OPENERS ARE NOT DECORATION. Without them this missed `… && { skip; return 0; }`, which is how
    # a one-line site is most naturally written, and the miss ran in both directions: the inventory below
    # stopped counting that site, and the rule above would have called it unregistered. Found by planting the
    # five defects these tests exist to catch and noticing that the fifth did not red.
    CALL = re.compile(r"(?:^|[;|&({]|\bthen|\belse|\bdo)[ \t]*skip(?=[ \t;]|$)", re.M)

    def registers(self, body):
        return self.CALL.search(body) is not None

    def test_every_place_that_says_it_skipped_registers_itself(self):
        # The rule: a line that TELLS a reader a check did not run must also tell the verdict line. Both halves
        # matter -- a phase that printed its stand-down and registered nothing is exactly what lifecycle and
        # jobs did, in the table, on every lane gate, for as long as the benches have been skippable.
        #
        # `did not run` IS IN THE VOCABULARY BECAUSE A SITE ESCAPED WITHOUT IT. The first version of this
        # pattern held only the word "skipped" and its neighbours, and on 2026-11-10 at 15:44 a self build
        # failed on a site that says "the shared lock was not taken, so the tests and benches did not run"
        # and registered nothing. A rule that reads as a rule about every stand-down, while matching only
        # the ones that use one word for it, is this row's own defect one level up: a guard that names SOME
        # of what it guards. There are exactly two lines in the vocabulary that this word adds; both are
        # accounted for, one by registering and one in REDS_THE_GATE.
        says = re.compile(r"\bskipped\b|\bSKIPPED\b|NOT scrubbed|NOT checked|floor: off|did not run")
        funcs = self.functions()
        unregistered = []
        for line in self.GATE.split("\n"):
            s = line.strip()
            if not s.startswith(("echo ", "printf ")) or not says.search(s):
                continue
            if s in self.REPORTERS or s in self.REDS_THE_GATE:
                continue
            owner = next((n for n, b in funcs.items() if line in b.split("\n")), None)
            if owner is None or not self.registers(funcs[owner]):
                unregistered.append(f"{owner or '(top level)'}: {s[:90]}")
        self.assertEqual(
            unregistered, [],
            "each of these says a check did not run, and nothing registers it, so the verdict line will claim "
            "a completeness the run did not have. Call `skip` at the site (no argument registers the running "
            "phase; a phrase for a partial one), or add the line to REPORTERS above with its reason:\n  "
            + "\n  ".join(unregistered))

    # WHERE THIS RULE'S REACH STOPS, written down rather than overstated. It asks whether the FUNCTION holding a
    # saying-line registers anything, not whether THAT LINE does. For eleven of the twelve sites the two are the
    # same question, because the function exists to run one check. `locked_checks` is the exception and holds two
    # unlike sites: it replays the locked part's `skipped` records (`skip "$name"`), and it has its own stand-down
    # when the lock was never taken. So the second passes this rule on the strength of the first, and would pass
    # it even if it registered nothing -- which is exactly the state the 15:44 build caught it in.
    #
    # The obvious tightening -- demand a `skip` call within N lines of the saying-line, or at the same indent in
    # the same block -- buys that one function at the price of a textual rule over bash layout that can red on a
    # legitimate site (one that registers at the top of its phase and prints later, which two of the twelve do).
    # So the site is held BEHAVIOURALLY instead, by GateTree's
    # test_a_gate_that_could_not_take_the_lock_says_the_suite_did_not_run, which fails the real gate.sh the real
    # way: a copy flock cannot exec. A behavioural test is the better instrument here anyway -- it holds the
    # verdict a reader actually sees, not the shape of the line that produces it.


    def test_a_helper_script_that_says_it_skipped_is_run_through_the_wrapper(self):
        # Three phases are scripts that decide for themselves whether they can run and say so on stdout,
        # exiting 0 either way: size-step.sh, recall-tests.sh, prompt-step.sh. The gate cannot tell from the
        # status, so it runs them through ran_or_skipped, which reads what they said. A fourth such script
        # wired in as a bare command would skip silently and this names it.
        funcs = self.functions()
        phases = re.findall(r"^ *(?:if .*; then +)?phase +(\"[^\"]+\"|'[^']+'|\S+) +(.*)$", self.GATE, re.M)
        missing = []
        for raw, command in phases:
            name = raw.strip("\"'")
            # One level of indirection: `phase size size_step` runs a function that runs the script.
            text = command + "\n" + funcs.get(command.split()[0], "")
            for script in re.findall(r"scripts/[A-Za-z0-9_.-]+\.(?:sh|py)", text):
                path = REPO / script
                if not path.exists():
                    continue
                if not re.search(rf"^ *echo \"{re.escape(name)}: skipped", path.read_text(), re.M):
                    continue
                invocation = next(l for l in text.split("\n") if script in l)
                if "ran_or_skipped" not in invocation:
                    missing.append(f"phase {name}: {script} says `{name}: skipped` and is run bare")
        self.assertEqual(
            missing, [],
            "a phase whose own script decides it cannot run must be invoked through ran_or_skipped, or its "
            "stand-down reaches the log and never the verdict:\n  " + "\n  ".join(missing))

    def test_a_stand_down_under_the_lock_is_carried_back_to_the_gate_above_it(self):
        # Five of the sites run in the locked part, which is another process: the two benches, the turn
        # bench's record, the size check and the suite's filter. They are most of what a lane gate stands
        # down, so a list that did not cross the lock would miss nearly everything this exists to report, and
        # it would do it silently -- the parent's array would simply be shorter. The channel is locked.rec.
        funcs = self.functions()
        self.assertRegex(funcs["save_locked_record"], r"printf 'skipped 0 %s\\n'",
                         "the locked part must write its stand-downs into the record it hands back")
        self.assertRegex(funcs["locked_checks"], r"skipped\)\s*\n\s*skip \"\$name\"",
                         "the gate above must read them back into its own list")
        for site in ("lifecycle_bench", "jobs_bench", "turn_step", "size_step", "suite_filters"):
            self.assertTrue(self.registers(funcs[site]) or "ran_or_skipped" in funcs[site],
                            f"{site} runs under the lock and must register its stand-down")

    def test_the_clause_is_appended_to_both_verdict_lines_and_replaces_neither(self):
        # The patch this grew from gave one ready-to-paste line for "two call sites", and the two sites are not
        # the same string: one carries a parenthetical that a test asserts verbatim and that the fleet runner
        # greps the log for, as its check that a tree which changed this script was not judged by main's copy
        # alone. Pasting the line at both reddened that test and switched that check off; it would have passed
        # every gate. So both sites keep every word they had, and the clause goes after it.
        self.assertIn('echo "gate: ok (main\'s gate judged this tree)$(skipped_clause)"', self.GATE)
        self.assertIn('echo "gate: ok$(skipped_clause)"', self.GATE)
        self.assertEqual(self.GATE.count("skipped_clause)"), 2, "two verdict lines carry the clause")

    def test_the_registered_sites_are_the_ones_this_test_knows_about(self):
        # The inventory, so that a reader of this file has the whole list in one place and an author who adds a
        # site is made to write it down here. It fails in both directions on purpose.
        expected = {
            "deny_check": 1,          # no advisory database: bans, licences and sources only
            "disk_floor": 1,          # THESEUS_GATE_MIN_FREE_GB=0
            "gate_tests": 1,          # this tree's gate.sh and tests are main's
            "jobs_bench": 1,          # THESEUS_GATE_NO_BENCH=1
            "keel_tests": 1,          # the guard and its suite are the base's
            "lifecycle_bench": 1,     # THESEUS_GATE_NO_BENCH=1
            "ran_or_skipped": 1,      # whatever the wrapped script said of itself
            "script_tests": 1,        # scripts/tests matched no test at all
            "scrub_added": 3,         # the flag; no scrubber on the host; nothing added to scan
            "suite_applied": 1,       # nextest says it applied a default-filter
            "suite_filters": 1,       # the banner says one is active
            # The only site whose words come from OUTSIDE the gate. A self build runs below a view that grants
            # it a pty, device nodes and the calls that build a sandbox, so the suite's own subjects can be
            # tested rather than left out; the launcher is the one thing that knows what that view still
            # withholds, and it says so in THESEUS_VIEW_WITHHELD. The gate registers it like any other
            # stand-down, so it reaches the verdict and not only the log. Unset for a lane gate, which
            # registers nothing here (theseus-1eemp).
            "summary": 1,             # THESEUS_VIEW_WITHHELD: what a self build's view withholds
            "turn_step": 1,           # the burst and the history entry, not the bench
            # One of these two is not a site: the replay of the locked part's own registrations, read back across
            # the lock. The other is a real site and the only one that is not a check standing itself down -- the
            # lock was never taken, so the suite and the benches never ran and no site inside them was reached.
            "locked_checks": 2,
        }
        found = {n: len(self.CALL.findall(b)) for n, b in self.functions().items() if self.registers(b)}
        self.assertEqual(found, expected,
                         "a site was added, removed or moved: update this inventory and say what the site is")


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
