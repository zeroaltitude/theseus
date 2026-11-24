"""Tests for scripts/keel-ack.sh, the owner's acknowledgement given from chat (issue 15).

Run: python3 -m unittest -q scripts/tests/test_keel_ack.py

Each case builds a throwaway repository whose main lists the owner's key in scripts/keel-signers, raises a ceiling on
a lane, runs the script, and asks the real keel guard whether the acknowledgement passes. Keys are made in the case's
own GNUPGHOME or ssh agent, so no case touches the operator's keys.

NO CASE MAY REACH THE REAL WINDOWS ssh-keygen (it asks the owner's 1Password to sign): setUp points
scripts/keel-ssh-sign.sh at a path that does not exist, and the cases that test the Windows path give it a fake
Windows program and a fake `wslpath`, both scripts in the case's own directory.
"""
import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
ACK = HERE.parent / "keel-ack.sh"
SIGN = HERE.parent / "keel-ssh-sign.sh"
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
        # Never the real Windows program, whatever this machine is (see the docstring).
        self.env["KEEL_SSH_WINDOWS"] = str(self.tmp / "no-windows-program")
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


FAKE_WSLPATH = """#!/bin/sh
# A stand-in for wslpath: -w gives a "Windows" path, W:<linux path>; -u turns one back.
case "$1" in -w) echo "W:$2" ;; -u) echo "${2#W:}" ;; esac
"""
FAKE_WINDOWS = """#!/bin/sh
# A stand-in for Windows' ssh-keygen: records its arguments, refuses a path that is not a "Windows" one, and signs
# with the throwaway private key, as 1Password's agent would after the owner's approval.
echo "$@" >> "$FAKE_LOG"
[ "$FAKE_FAIL" = 1 ] && exit 1
key="" buf=""
while [ $# -gt 0 ]; do
  case "$1" in
  -f) key=$2; shift 2; continue ;;
  -U) echo "-U reached the Windows program" >&2; exit 2 ;;
  esac
  buf=$1; shift
done
case "$key$buf" in *W:*W:*) ;; *) echo "not Windows paths: $key $buf" >&2; exit 3 ;; esac
ssh-keygen -q -Y sign -n git -f "$FAKE_PRIV" "${buf#W:}" || exit 4
"""


@unittest.skipIf(shutil.which("ssh-keygen") is None, "no ssh-keygen: the ssh signing program is not tested here")
class SshSigner(unittest.TestCase):
    """scripts/keel-ssh-sign.sh: on WSL a signature goes through Windows' ssh-keygen, with Windows paths, and comes back
    beside git's buffer; everything else is plain ssh-keygen. Only fakes stand in for Windows here."""

    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp(prefix="keel-sign-"))
        self.addCleanup(shutil.rmtree, self.tmp, True)
        self.key = self.tmp / "key"
        subprocess.run(["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-C", "harbor@example.invalid", "-f",
                        str(self.key)], check=True, capture_output=True)
        self.pub = Path(str(self.key) + ".pub").read_text().strip()
        bin_dir = self.tmp / "bin"
        bin_dir.mkdir()
        for name, text in (("wslpath", FAKE_WSLPATH), ("ssh-keygen.exe", FAKE_WINDOWS)):
            (bin_dir / name).write_text(text)
            (bin_dir / name).chmod(0o755)
        (self.tmp / "wintmp").mkdir()
        self.log = self.tmp / "windows.log"
        self.env = {**os.environ, "HOME": str(self.tmp), "GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_NOSYSTEM": "1",
                    "PATH": f"{bin_dir}:{os.environ['PATH']}", "KEEL_SSH_WSL": "1",
                    "KEEL_SSH_WINDOWS": str(bin_dir / "ssh-keygen.exe"), "KEEL_SSH_WINTMP": str(self.tmp / "wintmp"),
                    "FAKE_LOG": str(self.log), "FAKE_PRIV": str(self.key), "FAKE_FAIL": "0"}
        self.env.pop("SSH_AUTH_SOCK", None)
        self.env.pop("THESEUS_KEEL_BASE", None)  # the gate sets it; this guard run judges the fixture repo
        self.data = self.tmp / "buffer"
        self.data.write_text("tree 0123\n\na commit to sign\n")
        (self.tmp / "key-tmp.pub").write_text(self.pub + "\n")

    def sign(self, *extra):
        return subprocess.run(["bash", str(SIGN), "-Y", "sign", "-n", "git", "-f", str(self.tmp / "key-tmp.pub"),
                               *extra, str(self.data)], env=self.env, capture_output=True, text=True)

    def verify(self):
        allowed = self.tmp / "allowed"
        allowed.write_text(f"keel-owner {self.pub}\n")
        with open(self.data, "rb") as f:
            return subprocess.run(["ssh-keygen", "-Y", "verify", "-f", str(allowed), "-I", "keel-owner", "-n", "git",
                                   "-s", str(self.data) + ".sig"], stdin=f, capture_output=True, text=True)

    def test_on_wsl_the_windows_program_signs_with_windows_paths_and_the_signature_comes_back(self):
        r = self.sign("-U")
        self.assertEqual(r.returncode, 0, r.stderr)
        args = self.log.read_text().split()
        self.assertEqual(args[:4], ["-Y", "sign", "-n", "git"])
        self.assertTrue(all(a.startswith("W:") for a in (args[5], args[6])), args)
        self.assertNotIn("-U", args)
        self.assertEqual(self.verify().returncode, 0, "the signature beside git's buffer verifies")
        self.assertEqual(list((self.tmp / "wintmp").iterdir()), [], "the copies Windows read are gone")

    def test_a_failed_windows_signature_fails_and_leaves_none(self):
        self.env["FAKE_FAIL"] = "1"
        self.assertNotEqual(self.sign().returncode, 0)
        self.assertFalse(Path(str(self.data) + ".sig").exists())

    def test_off_wsl_or_with_no_windows_program_it_is_plain_ssh_keygen(self):
        for env in ({"KEEL_SSH_WSL": "0"}, {"KEEL_SSH_WINDOWS": str(self.tmp / "absent.exe")}):
            self.env.update(env)
            r = subprocess.run(["bash", str(SIGN), "-Y", "sign", "-n", "git", "-f", str(self.key), str(self.data)],
                               env=self.env, capture_output=True, text=True)
            self.assertEqual(r.returncode, 0, r.stderr)
            self.assertEqual(self.verify().returncode, 0)
            self.assertFalse(self.log.exists(), "the Windows program was not called")
            os.remove(str(self.data) + ".sig")
            self.env.update({"KEEL_SSH_WSL": "1", "KEEL_SSH_WINDOWS": str(self.tmp / "bin" / "ssh-keygen.exe")})

    def test_what_is_not_a_signature_is_plain_ssh_keygen_even_on_wsl(self):
        r = subprocess.run(["bash", str(SIGN), "-l", "-f", str(self.tmp / "key-tmp.pub")], env=self.env,
                           capture_output=True, text=True)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("ED25519", r.stdout)
        self.assertFalse(self.log.exists())

    def test_keel_ack_signs_through_it_and_the_guard_takes_the_owner_s_ssh_key(self):
        root = self.tmp / "repo"
        root.mkdir()
        env = {**self.env, "GIT_AUTHOR_NAME": "Wren Fixture", "GIT_AUTHOR_EMAIL": "wren@example.invalid",
               "GIT_COMMITTER_NAME": "Wren Fixture", "GIT_COMMITTER_EMAIL": "wren@example.invalid"}
        git = lambda *a: subprocess.run(["git", *a], cwd=root, env=env, check=True, capture_output=True, text=True)
        git("init", "-q", "-b", "main")
        (root / "scripts").mkdir()
        (root / "scripts" / "keel-signers").write_text(f"routine gpg 0123456789ABCDEF0123456789ABCDEF01234567\n"
                                                       f"owner ssh {self.pub}\n")
        (root / "scripts" / "long-files.txt").write_text(LONG)
        git("add", "-A")
        git("commit", "-q", "-m", "base")
        git("checkout", "-q", "-b", "lane")
        (root / "scripts" / "long-files.txt").write_text(LONG.replace("3000", "3050"))
        git("commit", "-q", "-am", "raise a ceiling")
        r = subprocess.run(["bash", str(ACK), "--quote", "yes", "--message", "42", LINE], cwd=root, env=env,
                           capture_output=True, text=True)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertTrue(self.log.exists(), "the signature went through the Windows program")
        p = subprocess.run(["python3", str(GUARD)], cwd=root, env=env, capture_output=True, text=True)
        self.assertEqual(p.returncode, 0, p.stdout + p.stderr)
        self.assertIn("keel: acked: scripts/long-files.txt", p.stdout)


if __name__ == "__main__":
    unittest.main()
