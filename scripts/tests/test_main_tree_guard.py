"""Tests for scripts/main-tree-guard.sh and scripts/hooks/post-checkout (issue 15: the main tree stays on main).

Run: python3 -m unittest -q scripts/tests/test_main_tree_guard.py

Each case builds a throwaway repository with a main working tree and a linked worktree (a lane), installs the guard
the way an operator does, and then does what moved the real main tree on 2026-11-13: a checkout of another commit
in the main tree.
"""
import os
import subprocess
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
SCRIPTS = os.path.abspath(os.path.join(HERE, ".."))
GIT = ["-c", "user.name=t", "-c", "user.email=t@example.invalid", "-c", "commit.gpgsign=false",
       "-c", "init.defaultBranch=main", "-c", "advice.detachedHead=false"]


class MainTreeGuard(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.main = os.path.join(self.tmp.name, "repo")
        self.lane = os.path.join(self.tmp.name, "lane")
        self.env = dict(os.environ, HOME=self.tmp.name, GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_SYSTEM=os.devnull)
        # Run from inside a rebase or pull, the suite would inherit its reflog action and every checkout here would
        # be logged under that name (review round 4).
        self.env.pop("GIT_REFLOG_ACTION", None)
        os.makedirs(os.path.join(self.main, "scripts", "hooks"))
        # The repository under test carries the guard as the real one does: the scripts are its own files.
        for rel in ("main-tree-guard.sh", "hooks/post-checkout"):
            with open(os.path.join(SCRIPTS, rel), encoding="utf-8") as src:
                self.put(os.path.join("scripts", rel), src.read(), 0o755)
        self.put("notes.txt", "one\n")
        self.git("init", "-q")
        self.git("add", "-A")
        self.git("commit", "-q", "-m", "one")
        self.one = self.git("rev-parse", "HEAD").stdout.strip()
        self.put("notes.txt", "two\n")
        self.git("commit", "-q", "-am", "two")
        self.git("branch", "builder", self.one)

    def put(self, rel, text, mode=0o644):
        path = os.path.join(self.main, rel)
        with open(path, "w", encoding="utf-8") as f:
            f.write(text)
        os.chmod(path, mode)

    def git(self, *args, cwd=None, check=True):
        return subprocess.run(["git", *GIT, *args], cwd=cwd or self.main, env=self.env, capture_output=True,
                              text=True, check=check)

    def guard(self, *args):
        return subprocess.run(["bash", os.path.join(self.main, "scripts", "main-tree-guard.sh"), *args],
                              cwd=self.main, env=self.env, capture_output=True, text=True)

    def head(self, cwd=None):
        return self.git("symbolic-ref", "-q", "HEAD", cwd=cwd, check=False).stdout.strip()

    def test_without_the_guard_a_checkout_moves_the_main_tree(self):
        # The world before the fix, so the cases below prove the guard and not git.
        self.assertEqual(self.git("checkout", "-q", self.one, check=False).returncode, 0)
        self.assertEqual(self.head(), "")

    def test_a_checkout_of_another_commit_in_the_main_tree_is_put_back_and_fails(self):
        self.assertEqual(self.guard("install").returncode, 0)
        for target in (self.one, self.one[:8], "builder"):
            r = self.git("checkout", target, check=False)
            self.assertNotEqual(r.returncode, 0, f"checkout {target} must fail: {r.stderr}")
            self.assertEqual(self.head(), "refs/heads/main", f"after checkout {target}")
            self.assertIn("it stays on main; it has been put back", r.stderr)
            self.assertIn("git worktree add", r.stderr)
            with open(os.path.join(self.main, "notes.txt"), encoding="utf-8") as f:
                self.assertEqual(f.read(), "two\n", "the files are main's again")

    def test_a_switch_to_a_new_branch_is_put_back_too(self):
        self.guard("install")
        r = self.git("switch", "-c", "scratch", check=False)
        self.assertNotEqual(r.returncode, 0)
        self.assertEqual(self.head(), "refs/heads/main")
        self.assertIn("the branch is still there", r.stderr)

    def test_a_lane_is_left_alone(self):
        self.guard("install")
        self.git("worktree", "add", "-q", "--detach", self.lane, "main")
        r = self.git("checkout", "-q", self.one, cwd=self.lane, check=False)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.git("rev-parse", "HEAD", cwd=self.lane).stdout.strip(), self.one)
        self.assertEqual(self.head(), "refs/heads/main", "and the main tree did not move")

    def test_main_itself_and_a_file_checkout_pass(self):
        self.guard("install")
        self.assertEqual(self.git("checkout", "-q", "main", check=False).returncode, 0)
        self.put("notes.txt", "edited\n")
        self.assertEqual(self.git("checkout", "-q", "--", "notes.txt", check=False).returncode, 0)
        self.assertEqual(self.head(), "refs/heads/main")

    def test_local_edits_are_carried_back(self):
        self.guard("install")
        self.put("other.txt", "mine\n")
        self.git("add", "other.txt")
        self.git("commit", "-q", "-m", "other")
        self.put("other.txt", "mine, edited\n")
        self.git("checkout", "-q", self.one, check=False)  # other.txt is untouched by `one`, so the edit travels
        self.assertEqual(self.head(), "refs/heads/main")
        with open(os.path.join(self.main, "other.txt"), encoding="utf-8") as f:
            self.assertEqual(f.read(), "mine, edited\n")

    def test_a_rebase_of_main_in_the_main_tree_completes_on_main(self):
        # A failing post-checkout fails the rebase's own checkout halfway (git 2.34), so `pull --rebase` with a local
        # commit once ended detached, with no rebase left to finish. Review round 1 of issue 15 found it.
        self.guard("install")
        self.git("branch", "upstream", "main")
        self.put("other.txt", "local\n")
        self.git("add", "other.txt")
        self.git("commit", "-q", "-m", "local")
        self.git("worktree", "add", "-q", self.lane, "upstream")
        self.put("notes.txt", "three\n")
        with open(os.path.join(self.lane, "notes.txt"), "w", encoding="utf-8") as f:
            f.write("three\n")
        self.git("commit", "-q", "-am", "three", cwd=self.lane)
        self.git("checkout", "-q", "--", "notes.txt")
        r = self.git("rebase", "upstream", check=False)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.head(), "refs/heads/main")
        self.assertEqual(self.git("log", "-1", "--format=%s").stdout.strip(), "local")
        self.assertEqual(self.git("log", "-1", "--format=%s", "HEAD~1").stdout.strip(), "three")

    def upstream_ahead(self):
        # main = one, two, local; upstream = one, two, three, made in a lane so the main tree never moves.
        self.git("branch", "upstream", "main")
        self.put("other.txt", "local\n")
        self.git("add", "other.txt")
        self.git("commit", "-q", "-m", "local")
        self.git("worktree", "add", "-q", self.lane, "upstream")
        with open(os.path.join(self.lane, "notes.txt"), "w", encoding="utf-8") as f:
            f.write("three\n")
        self.git("commit", "-q", "-am", "three", cwd=self.lane)

    def test_an_apply_backend_rebase_of_another_branch_leaves_main_alone(self):
        # Review round 3 of issue 15: the apply backend detaches HEAD before its state directory exists, so a
        # put-back there left HEAD on main and the rebase committed the builder's work onto main itself.
        self.guard("install")
        self.upstream_ahead()
        before = self.git("rev-parse", "main").stdout.strip()
        r = self.git("rebase", "--apply", "main", "builder", check=False)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.git("rev-parse", "main").stdout.strip(), before, "main's ref did not move")
        self.assertNotIn("it has been put back", r.stdout + r.stderr)

    def apply_rebase_takes_the_upstream(self, *how):
        self.guard("install")
        self.upstream_ahead()
        r = self.git(*how, check=False)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.head(), "refs/heads/main")
        self.assertEqual(self.git("log", "--format=%s", "-3").stdout.split(), ["local", "three", "two"])

    def test_an_apply_backend_rebase_of_main_takes_the_upstream(self):
        self.apply_rebase_takes_the_upstream("rebase", "--apply", "upstream")

    def test_an_apply_backend_pull_rebase_takes_the_upstream(self):
        # `git pull --rebase` writes its HEAD reflog entry as `pull ...`, not `rebase ...`.
        self.apply_rebase_takes_the_upstream("-c", "rebase.backend=apply", "pull", "-q", "--rebase", ".", "upstream")

    def test_a_branch_created_at_another_commit_is_named_too(self):
        self.guard("install")
        r = self.git("switch", "-c", "elsewhere", self.one, check=False)
        self.assertNotEqual(r.returncode, 0)
        self.assertEqual(self.head(), "refs/heads/main")
        self.assertIn("the branch is still there", r.stderr)

    def test_a_checkout_logged_under_a_rebases_name_is_let_through_and_nothing_else(self):
        # The signal the hook reads is the reflog entry git writes for the move. A checkout git logs under a rebase's
        # or a pull's name (as one run by `rebase -x` is) is let through; a plain one is not, whatever the
        # environment holds otherwise.
        self.guard("install")
        env = dict(self.env, GIT_REFLOG_ACTION="rebase")
        r = subprocess.run(["git", *GIT, "checkout", "-q", self.one], cwd=self.main, env=env, capture_output=True,
                           text=True)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.head(), "")
        self.git("checkout", "-q", "main")
        env = dict(self.env, GIT_REFLOG_ACTION="checkout")
        r = subprocess.run(["git", *GIT, "checkout", "-q", self.one], cwd=self.main, env=env, capture_output=True,
                           text=True)
        self.assertNotEqual(r.returncode, 0)
        self.assertEqual(self.head(), "refs/heads/main")

    def hook_after(self, entry, env_action):
        # HEAD moved off main under the reflog entry `entry`, with no checkout, then the installed hook run by hand
        # as git would, with GIT_REFLOG_ACTION set to `env_action` (or unset).
        two = self.git("rev-parse", "main").stdout.strip()
        self.git("update-ref", "--no-deref", "-m", entry.format(one=self.one), "HEAD", self.one)
        env = dict(self.env)
        if env_action:
            env["GIT_REFLOG_ACTION"] = env_action
        return subprocess.run(["sh", os.path.join(self.main, ".git", "hooks", "post-checkout"), two, self.one, "1"],
                              cwd=self.main, env=env, capture_output=True, text=True)

    def test_the_reflog_decides_and_the_environment_does_not(self):
        # Review round 5: where the reflog and GIT_REFLOG_ACTION agree, a hook reading either passes, so the two are
        # set to disagree here. A hook that read the variable (round 4's) fails both halves on any git.
        self.guard("install")
        r = self.hook_after("rebase (start): checkout {one}", None)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.head(), "", "a rebase's move is left alone")
        self.git("checkout", "-q", "main")
        r = self.hook_after("checkout: moving from main to {one}", "rebase")
        self.assertNotEqual(r.returncode, 0)
        self.assertEqual(self.head(), "refs/heads/main", "a checkout's move is put back")

    def test_a_rebase_of_another_branch_is_not_covered(self):
        # The recorded limit (review round 2 of issue 15): the hook cannot fail a rebase without breaking it, and
        # git 2.34 shows nothing it prints there, so a rebase of another branch ends on that branch. The docs say so.
        self.guard("install")
        r = self.git("rebase", "main", "builder", check=False)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.head(), "refs/heads/builder")

    def test_a_bisect_in_the_main_tree_runs_and_its_reset_ends_on_main(self):
        self.guard("install")
        for n in ("three", "four", "five"):
            self.put("notes.txt", n + "\n")
            self.git("commit", "-q", "-am", n)
        r = self.git("bisect", "start", "main", self.one, check=False)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.head(), "", "bisecting, the tree is on a commit between")
        self.assertIn("bisect reset", r.stderr)
        self.assertEqual(self.git("bisect", "good", check=False).returncode, 0)
        r = self.git("bisect", "reset", check=False)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.head(), "refs/heads/main")

    def test_install_from_a_lane_is_refused(self):
        self.git("worktree", "add", "-q", "--detach", self.lane, "main")
        r = subprocess.run(["bash", os.path.join(self.lane, "scripts", "main-tree-guard.sh"), "install"],
                           cwd=self.lane, env=self.env, capture_output=True, text=True)
        self.assertEqual(r.returncode, 1, r.stdout)
        self.assertIn("linked worktree", r.stdout)
        self.assertFalse(os.path.exists(os.path.join(self.main, ".git", "hooks", "post-checkout")))

    def test_install_is_idempotent_and_check_says_so(self):
        self.assertEqual(self.guard("check").returncode, 1)
        self.assertIn("installed (", self.guard("install").stdout)
        self.assertIn("installed and current", self.guard("install").stdout)
        r = self.guard("check")
        self.assertEqual(r.returncode, 0, r.stdout)
        # An older copy of ours is replaced, and check names it older until then.
        hook = os.path.join(self.main, ".git", "hooks", "post-checkout")
        with open(hook, "a", encoding="utf-8") as f:
            f.write("# an older line\n")
        self.assertIn("older", self.guard("check").stdout)
        self.assertEqual(self.guard("install").returncode, 0)
        self.assertEqual(self.guard("check").returncode, 0)

    def test_a_hook_of_someone_elses_is_never_replaced(self):
        hook = os.path.join(self.main, ".git", "hooks", "post-checkout")
        with open(hook, "w", encoding="utf-8") as f:
            f.write("#!/bin/sh\necho theirs\n")
        r = self.guard("install")
        self.assertEqual(r.returncode, 1)
        self.assertIn("did not write", r.stdout)
        with open(hook, encoding="utf-8") as f:
            self.assertEqual(f.read(), "#!/bin/sh\necho theirs\n")

    def test_the_installed_hook_is_a_copy_a_checkout_cannot_change(self):
        self.guard("install")
        hook = os.path.join(self.main, ".git", "hooks", "post-checkout")
        self.assertFalse(os.path.islink(hook))
        self.put("scripts/hooks/post-checkout", "#!/bin/sh\nexit 0\n", 0o755)
        r = self.git("checkout", self.one, check=False)
        self.assertNotEqual(r.returncode, 0)
        self.assertEqual(self.head(), "refs/heads/main")


if __name__ == "__main__":
    unittest.main()
