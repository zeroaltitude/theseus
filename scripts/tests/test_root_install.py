"""Tests for scripts/root-install.sh (issue 22: the administrator's one-time step for the helper).

Run: python3 -m unittest -q scripts/tests/test_root_install.py

Each case runs the script with `--root DIR`, so every path is below a throwaway directory and it runs as the test's own
user: nothing outside the directory is touched, and nothing is run as root. What this cannot show is the ownership
(`root:root`) of what the real run makes, and that sudo accepts the rule; the fleet box's live check does.
"""
import os
import re
import shutil
import subprocess
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
SCRIPT = os.path.abspath(os.path.join(HERE, "..", "root-install.sh"))
def read(path, mode="r"):
    with open(path, mode) as f:
        return f.read()


def write(path, text):
    with open(path, "w") as f:
        f.write(text)


ME = subprocess.run(["id", "-un"], capture_output=True, text=True, check=True).stdout.strip()


class RootInstall(unittest.TestCase):
    def test_a_failed_remove_never_reports_success(self):
        self.install()
        bindir = os.path.join(self.tmp.name, "bin")
        os.mkdir(bindir)
        failing = os.path.join(bindir, "rm")
        write(failing, "#!/bin/sh\nexit 1\n")
        os.chmod(failing, 0o755)
        out = subprocess.run(
            ["bash", SCRIPT, "--root", self.root, "remove", "--yes"],
            env={**os.environ, "PATH": bindir + os.pathsep + os.environ["PATH"]},
            capture_output=True, text=True,
        )
        self.assertNotEqual(out.returncode, 0)
        self.assertNotIn("removed. The daemon", out.stdout)
        self.assertTrue(os.path.exists(self.path("etc/sudoers.d/theseus-root-install")))

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = os.path.join(self.tmp.name, "root")
        for d in ("usr/local/sbin", "usr/local/lib", "var/lib", "etc/sudoers.d"):
            os.makedirs(os.path.join(self.root, d))
            os.chmod(os.path.join(self.root, d), 0o755)
        os.chmod(self.root, 0o755)
        self.helper = os.path.join(self.tmp.name, "helper")
        with open(self.helper, "wb") as f:
            f.write(b"\x7fELF fake helper")
        os.chmod(self.helper, 0o755)

    def run_script(self, *args, check=False):
        out = subprocess.run(["bash", SCRIPT, "--root", self.root, *args], capture_output=True, text=True)
        if check:
            self.assertEqual(out.returncode, 0, out.stdout + out.stderr)
        return out

    def path(self, rel):
        return os.path.join(self.root, rel)

    def install(self):
        return self.run_script("install", "--user", ME, "--helper", self.helper, "--yes", check=True)

    def test_install_makes_the_helper_the_directories_and_one_rule(self):
        self.install()
        helper = self.path("usr/local/sbin/theseus-root-install")
        self.assertEqual(read(helper, "rb"), b"\x7fELF fake helper")
        self.assertEqual(os.stat(helper).st_mode & 0o7777, 0o755)
        self.assertEqual(os.stat(self.path("usr/local/lib/theseus")).st_mode & 0o7777, 0o755)
        self.assertEqual(os.stat(self.path("var/lib/theseus-root-install")).st_mode & 0o7777, 0o755)
        self.assertEqual(os.stat(self.path("var/lib/theseus-root-install/stage")).st_mode & 0o7777, 0o700)
        rule = self.path("etc/sudoers.d/theseus-root-install")
        self.assertEqual(os.stat(rule).st_mode & 0o7777, 0o440)
        lines = [l for l in read(rule).splitlines() if l and not l.startswith("#")]
        self.assertIn("Defaults!/usr/local/sbin/theseus-root-install env_reset", lines)
        rules = [l for l in lines if not l.startswith("Defaults!")]
        self.assertEqual(len(rules), 1, rules)
        self.assertRegex(
            rules[0],
            r"^%s ALL=\(root\) NOPASSWD: NOSETENV: /usr/local/sbin/theseus-root-install (\[0-9a-f\]){40}$" % re.escape(ME),
        )
        # Nothing is left beside it: no temporary helper, no stray file.
        self.assertEqual(os.listdir(self.path("usr/local/sbin")), ["theseus-root-install"])
        self.assertEqual(os.listdir(self.path("etc/sudoers.d")), ["theseus-root-install"])

    def test_install_twice_is_the_same_as_once_and_remove_twice_is_harmless(self):
        self.install()
        first = {rel: read(self.path(rel), "rb") for rel in ("usr/local/sbin/theseus-root-install", "etc/sudoers.d/theseus-root-install")}
        out = self.install()
        self.assertIn("done.", out.stdout)
        for rel, data in first.items():
            self.assertEqual(read(self.path(rel), "rb"), data, rel)
        self.assertEqual(sorted(os.listdir(self.path("usr/local/sbin"))), ["theseus-root-install"])
        self.run_script("remove", "--yes", check=True)
        again = self.run_script("remove", "--yes", check=True)
        self.assertIn("no rule", again.stdout)
        self.assertIn("no helper", again.stdout)

    def test_the_rule_is_checked_by_visudo_when_it_is_there(self):
        self.install()
        visudo = shutil.which("visudo")
        if visudo is None:
            self.skipTest("no visudo on this machine")
        out = subprocess.run([visudo, "-cf", self.path("etc/sudoers.d/theseus-root-install")], capture_output=True, text=True)
        self.assertEqual(out.returncode, 0, out.stdout + out.stderr)

    def test_the_rule_pattern_matches_a_commit_and_nothing_else(self):
        self.install()
        rule = [l for l in read(self.path("etc/sudoers.d/theseus-root-install")).splitlines() if l and not l.startswith(("#", "Defaults!"))][0]
        pattern = rule.rsplit(" ", 1)[1]
        import fnmatch

        self.assertTrue(fnmatch.fnmatchcase("0123456789abcdef0123456789abcdef01234567", pattern))
        for bad in ("", "0123456789abcdef0123456789abcdef0123456", "0123456789abcdef0123456789abcdef012345678",
                    "0123456789abcdef0123456789abcdef0123456 x", "0123456789ABCDEF0123456789abcdef01234567",
                    "../../tmp/x" + "0" * 30):
            self.assertFalse(fnmatch.fnmatchcase(bad, pattern), bad)

    def test_check_says_installed_and_then_not(self):
        self.assertEqual(self.run_script("check").returncode, 1)
        self.install()
        out = self.run_script("check")
        self.assertEqual(out.returncode, 0, out.stdout)
        self.assertIn("result: installed", out.stdout)

    def test_remove_takes_away_what_install_made_and_keeps_the_builds(self):
        self.install()
        build = self.path("usr/local/lib/theseus/01234567")
        os.makedirs(build)
        out = self.run_script("remove", "--yes", check=True)
        self.assertFalse(os.path.exists(self.path("usr/local/sbin/theseus-root-install")))
        self.assertFalse(os.path.exists(self.path("etc/sudoers.d/theseus-root-install")))
        self.assertFalse(os.path.exists(self.path("var/lib/theseus-root-install")))
        self.assertTrue(os.path.isdir(build), "a build the service may run from is kept")
        self.assertIn("kept", out.stdout)

    def test_remove_keeps_staged_programs_and_a_rule_that_is_not_its_own(self):
        self.install()
        write(self.path("var/lib/theseus-root-install/stage/theseusd"), "x")
        out = self.run_script("remove", check=True)
        self.assertTrue(os.path.exists(self.path("var/lib/theseus-root-install/stage/theseusd")))
        self.assertIn("kept", out.stdout)
        self.install_other_rule = self.path("etc/sudoers.d/theseus-root-install")
        write(self.install_other_rule, "someone ALL=(ALL) NOPASSWD: ALL\n")
        out = self.run_script("remove")
        self.assertEqual(out.returncode, 1)
        self.assertTrue(os.path.exists(self.install_other_rule), "a file that is not this script's is left alone")

    def test_install_refuses_what_it_should(self):
        # a user that is root, one that does not exist, no helper, a helper that is not a program, a link, a bad parent
        for args in (["--user", "root"], ["--user", "no-such-user-here"], ["--user", ME]):
            out = self.run_script("install", "--yes", *args, "--helper", os.path.join(self.tmp.name, "absent"))
            self.assertEqual(out.returncode, 1, out.stdout + out.stderr)
        text = os.path.join(self.tmp.name, "text")
        write(text, "#!/bin/sh\n")
        os.chmod(text, 0o755)
        self.assertEqual(self.run_script("install", "--yes", "--user", ME, "--helper", text).returncode, 1)
        link = os.path.join(self.tmp.name, "link")
        os.symlink(self.helper, link)
        self.assertEqual(self.run_script("install", "--yes", "--user", ME, "--helper", link).returncode, 1)
        os.chmod(self.path("usr/local/sbin"), 0o777)
        self.assertEqual(self.run_script("install", "--yes", "--user", ME, "--helper", self.helper).returncode, 1)
        os.chmod(self.path("usr/local/sbin"), 0o755)
        self.assertEqual(os.listdir(self.path("etc/sudoers.d")), [], "nothing was written by any refusal")
        self.assertEqual(self.run_script("install", "--yes").returncode, 2)

    def test_a_dry_run_changes_nothing(self):
        out = self.run_script("install", "--user", ME, "--helper", self.helper, "--dry-run", check=True)
        self.assertIn("+ install", out.stdout)
        self.assertEqual(os.listdir(self.path("usr/local/sbin")), [])
        self.assertEqual(os.listdir(self.path("etc/sudoers.d")), [])
        self.assertFalse(os.path.exists(self.path("usr/local/lib/theseus")))

    def test_a_dry_run_by_a_user_who_is_not_root_prints_every_step_and_writes_nothing(self):
        """Found on the first live day: with no --root and no sudo, the rule's temporary file was made in the real
        sudoers directory and the run died there, after six steps. It names the last step, and writes no file. (The
        machine's own directories are only read; a machine without them skips the case.)"""
        if os.geteuid() == 0:
            self.skipTest("the case is about a user who is not root")
        real = "/etc/sudoers.d"
        if not os.path.isdir(real):
            self.skipTest("no sudoers directory on this machine")
        # The directory is often mode 0750, root's: then it cannot be listed, and the script could not have written
        # into it either, so what is checked is that the run succeeds and prints the last step.
        try:
            before = sorted(os.listdir(real))
        except PermissionError:
            before = None
        out = subprocess.run(
            ["bash", SCRIPT, "--dry-run", "install", "--user", ME, "--helper", self.helper, "--yes"],
            capture_output=True,
            text=True,
        )
        if out.returncode != 0 and "does not exist" in out.stderr + out.stdout:
            self.skipTest("this machine lacks a directory the install names")
        self.assertEqual(out.returncode, 0, out.stdout + out.stderr)
        self.assertIn("-m 0440", out.stdout, "the last step is printed too")
        self.assertIn("visudo -cf", out.stdout)
        if before is not None:
            self.assertEqual(sorted(os.listdir(real)), before, "a dry run writes nothing")


if __name__ == "__main__":
    unittest.main()
