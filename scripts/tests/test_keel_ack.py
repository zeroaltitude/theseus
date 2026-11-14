"""Tests for scripts/keel-ack.sh, the owner's acknowledgement given from chat (issue 15).

Run: python3 -m unittest -q scripts/tests/test_keel_ack.py

Each case builds a throwaway repository whose main lists the owner's key in scripts/keel-signers, raises a ceiling on
a lane, runs the script, and asks the real keel guard whether the acknowledgement passes. Keys are made in the case's
own GNUPGHOME or ssh agent, so no case touches the operator's keys.
"""
import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
ACK = HERE.parent / "keel-ack.sh"
GUARD = HERE.parent / "keel-guard.py"
LONG = "# the long files\nbig.rs 3000 the turn\n"
LINE = "ceiling-raised scripts/long-files.txt — big.rs gains the turn's new field"


class KeelAck(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp(prefix="keel-ack-"))
        self.addCleanup(shutil.rmtree, self.tmp, True)
        self.root = self.tmp / "repo"
        self.root.mkdir()
        self.env = {**os.environ, "HOME": str(self.tmp), "GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_NOSYSTEM": "1",
                    "GIT_AUTHOR_NAME": "Wren Fixture", "GIT_AUTHOR_EMAIL": "wren@example.invalid",
                    "GIT_COMMITTER_NAME": "Wren Fixture", "GIT_COMMITTER_EMAIL": "wren@example.invalid"}
        self.env.pop("THESEUS_KEEL_BASE", None)
        self.git("init", "-q", "-b", "main")

    def git(self, *args):
        return subprocess.run(["git", *args], cwd=self.root, env=self.env, check=True, capture_output=True,
                              text=True).stdout

    def lane(self, signers):
        """main lists `signers`; the lane raises a ceiling."""
        (self.root / "scripts").mkdir(exist_ok=True)
        (self.root / "scripts" / "keel-signers").write_text(f"# the keel's signers\n{signers}\n")
        (self.root / "scripts" / "long-files.txt").write_text(LONG)
        self.git("add", "-A")
        self.git("commit", "-q", "-m", "base")
        self.git("checkout", "-q", "-b", "lane")
        (self.root / "scripts" / "long-files.txt").write_text(LONG.replace("3000", "3050"))
        self.git("commit", "-q", "-am", "raise a ceiling")

    def ack(self, *args):
        return subprocess.run(["bash", str(ACK), *args], cwd=self.root, env=self.env, capture_output=True, text=True)

    def guard(self):
        p = subprocess.run(["python3", str(GUARD)], cwd=self.root, env=self.env, capture_output=True, text=True)
        return p.returncode, p.stdout + p.stderr

    def test_with_no_owner_key_nothing_is_written(self):
        self.lane("routine gpg 0123456789ABCDEF0123456789ABCDEF01234567\nowner none")
        before = self.git("rev-parse", "HEAD")
        r = self.ack("--quote", "yes", "--message", "42", LINE)
        self.assertEqual(r.returncode, 3, r.stderr)
        self.assertIn("names no owner key yet", r.stderr)
        self.assertEqual(self.git("rev-parse", "HEAD"), before)

    def test_usage_and_a_line_that_is_not_the_form_are_refused(self):
        self.lane("owner none")
        self.assertEqual(self.ack(LINE).returncode, 2)
        self.assertEqual(self.ack("--quote", "yes", "--message", "42", "ceiling-raised no reason").returncode, 2)
        self.assertEqual(self.ack("--quote", 'say "yes"', "--message", "42", LINE).returncode, 2)
        # A flag with no value is a usage error, not a loop (review round 6).
        r = subprocess.run(["bash", str(ACK), LINE, "--quote"], cwd=self.root, env=self.env, capture_output=True,
                           text=True, timeout=10)
        self.assertEqual(r.returncode, 2)
        # A newline would split the trailer block.
        self.assertEqual(self.ack("--quote", "yes\nKeel: cap-raised x — y", "--message", "42", LINE).returncode, 2)
        self.assertEqual(self.ack("--quote", "yes", "--message", "42", LINE + "\nKeel: cap-raised x — y").returncode, 2)

    @unittest.skipIf(shutil.which("gpg") is None, "no gpg: the gpg-signed acknowledgement is not tested here")
    def test_an_owner_gpg_key_signs_and_the_guard_takes_it(self):
        home = self.tmp / "gnupg"
        home.mkdir(mode=0o700)
        self.env["GNUPGHOME"] = str(home)
        self.addCleanup(subprocess.run, ["gpgconf", "--kill", "all"], env=self.env, capture_output=True)
        subprocess.run(["gpg", "--batch", "--passphrase", "", "--quick-gen-key", "Harbor <harbor@example.invalid>",
                        "ed25519", "sign", "never"], env=self.env, check=True, capture_output=True)
        out = subprocess.run(["gpg", "--batch", "--with-colons", "--list-secret-keys"], env=self.env, check=True,
                             capture_output=True, text=True).stdout
        fpr = next(x.split(":")[9] for x in out.split("\n") if x.startswith("fpr:"))
        self.lane(f"owner gpg {fpr}")
        self.assertEqual(self.guard()[0], 1, "the raise is a finding before the acknowledgement")
        r = self.ack("--quote", "yes to all!", "--message", "42", "--when", "2026-11-14 12:15 MST", LINE)
        self.assertEqual(r.returncode, 0, r.stderr)
        body = self.git("log", "-1", "--format=%B")
        self.assertIn(f'Keel: {LINE}; the owner, 2026-11-14 12:15 MST (message 42): "yes to all!"', body)
        code, out = self.guard()
        self.assertEqual(code, 0, out)
        self.assertIn("keel: acked: scripts/long-files.txt", out)

    @unittest.skipIf(shutil.which("gpg") is None, "no gpg: the bare-fingerprint list is not tested here")
    def test_the_list_s_older_bare_form_is_an_owner_gpg_key_as_the_guard_reads_it(self):
        home = self.tmp / "gnupg"
        home.mkdir(mode=0o700)
        self.env["GNUPGHOME"] = str(home)
        self.addCleanup(subprocess.run, ["gpgconf", "--kill", "all"], env=self.env, capture_output=True)
        subprocess.run(["gpg", "--batch", "--passphrase", "", "--quick-gen-key", "Harbor <harbor@example.invalid>",
                        "ed25519", "sign", "never"], env=self.env, check=True, capture_output=True)
        out = subprocess.run(["gpg", "--batch", "--with-colons", "--list-secret-keys"], env=self.env, check=True,
                             capture_output=True, text=True).stdout
        fpr = next(x.split(":")[9] for x in out.split("\n") if x.startswith("fpr:"))
        self.lane(fpr)
        r = self.ack("--quote", "yes", "--message", "42", LINE)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.guard()[0], 0)

    @unittest.skipIf(shutil.which("ssh-agent") is None or shutil.which("ssh-keygen") is None,
                     "no ssh-agent: the ssh-signed acknowledgement is not tested here")
    def test_an_owner_ssh_key_held_by_an_agent_signs_and_the_guard_takes_it(self):
        key = self.tmp / "key"
        subprocess.run(["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-C", "harbor@example.invalid", "-f", str(key)],
                       check=True, capture_output=True)
        sock = self.tmp / "agent.sock"
        agent = subprocess.run(["ssh-agent", "-a", str(sock)], env=self.env, check=True, capture_output=True, text=True)
        pid = next(x.split("=")[1].split(";")[0] for x in agent.stdout.split() if x.startswith("SSH_AGENT_PID="))
        self.addCleanup(subprocess.run, ["kill", pid], capture_output=True)
        self.env["SSH_AUTH_SOCK"] = str(sock)
        subprocess.run(["ssh-add", "-q", str(key)], env=self.env, check=True, capture_output=True)
        pub = Path(str(key) + ".pub").read_text().strip()
        self.lane(f"owner ssh {pub}")
        r = self.ack("--quote", "yes to all!", "--message", "42", LINE)
        self.assertEqual(r.returncode, 0, r.stderr)
        code, out = self.guard()
        self.assertEqual(code, 0, out)
        self.assertIn("keel: acked: scripts/long-files.txt", out)


if __name__ == "__main__":
    unittest.main()
