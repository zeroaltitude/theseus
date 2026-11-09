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
"""

import os
import re
import subprocess
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent
MOVED = 75

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
    away-and-back) git checkout -q --detach && git checkout -q main ;;
  esac
}
case "$what" in
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
    echo "     Summary [   1.000s] 9 tests run: 9 passed, 0 skipped"
    ;;
esac
exit 0
"""

NODE = "#!/bin/sh\necho v1.2.3\n"
NPM = "#!/bin/sh\n[ \"$2\" != test ] || echo 'ℹ tests 1'\nexit 0\n"
PASSES = "#!/bin/sh\nexit 0\n"

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
    "cockpit/package.json": "{}\n",
    "cockpit/src/protocol.gen/types.ts": "// generated\n",
    "crates/theseus-core/src/sandbox/self_build.rs": 'pub const NODE_PIN: &str = "1.2.3";\n',
    "scripts/shape.sh": PASSES,
    "scripts/build.sh": PASSES,
    "scripts/recall-tests.sh": PASSES,
    "scripts/size-step.sh": PASSES,
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
        self.git_env = {**os.environ, "HOME": str(self.home), "GIT_CONFIG_GLOBAL": "/dev/null",
                        "GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "",
                        "GIT_COMMITTER_NAME": "t", "GIT_COMMITTER_EMAIL": ""}
        bin_dir = self.home / ".cargo" / "bin"
        bin_dir.mkdir(parents=True)
        (self.home / ".cache").mkdir()
        for name, text in (("cargo", CARGO), ("node", NODE), ("npm", NPM)):
            self.put(bin_dir / name, text)
        self.root.mkdir()
        files = dict(COMMITTED)
        for name in REAL:
            files[f"scripts/{name}"] = (HERE / name).read_text()
        for name, text in files.items():
            self.put(self.root / name, text)
        for sh in ("gate.sh", "keel-guard.py", "shape.sh", "build.sh", "recall-tests.sh", "size-step.sh"):
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
        outer = ("CARGO_INCREMENTAL", "THESEUS_GATE_ROOT", "THESEUS_GATE_DOOR", "THESEUS_KEEL_BASE")
        base = {k: v for k, v in self.git_env.items() if k not in outer}
        e = {**base, "THESEUS_GATE_NO_BENCH": "1", "THESEUS_GATE_NO_SCRUB": "1", "STUB_LOG": str(self.log),
             "STUB_MOVE": "edit", **env}
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


if __name__ == "__main__":
    unittest.main()
