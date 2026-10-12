"""infra/aws/hand/rootless.py, the hand image without Docker (theseus-ongv), run offline.

The Dockerfile's reading, and the RUN layer's diff against a small base made here: what it keeps, what it whites out,
what it leaves out, and whose files they are. The chown shim runs too, where `cc` and an unprivileged user namespace
exist. No crane, no network, no image.

    python3 -m unittest discover -s infra/aws/test -p 'test_*.py'
"""
import io
import os
import shutil
import subprocess
import sys
import tarfile
import tempfile
import unittest
from pathlib import Path

HAND = Path(__file__).resolve().parent.parent / "hand"
sys.path.insert(0, str(HAND))
import rootless  # noqa: E402


class Reading(unittest.TestCase):
    def test_the_hand_dockerfile_reads(self):
        r = rootless.parse((HAND / "Dockerfile").read_text())
        self.assertRegex(r["base"], r"^debian:bookworm-slim@sha256:[0-9a-f]{64}$")
        self.assertEqual(set(r["args"]), {"AWSCLI_VERSION", "AWSCLI_SHA256"})
        self.assertEqual(len(r["runs"]), 1)
        self.assertIn("apt-get install", r["runs"][0])
        self.assertIn("useradd --uid 1000", r["runs"][0])
        self.assertNotIn("hadolint", r["runs"][0])
        self.assertEqual(r["copies"], [{"src": "theseusd", "dest": "/usr/local/bin/theseusd", "mode": 0o755}])
        self.assertEqual(r["env"], [["HOME", "/tmp"], ["THESEUS_HAND_DIR", "/tmp"]])
        self.assertEqual((r["user"], r["workdir"]), ("1000", "/tmp"))
        self.assertEqual((r["entrypoint"], r["cmd"]), (["/usr/local/bin/theseusd"], ["hand"]))

    def test_what_it_does_not_do_is_refused(self):
        pinned = "FROM debian@sha256:" + "0" * 64 + "\n"
        for text in (
            "FROM debian:bookworm-slim\n",
            pinned + "FROM debian@sha256:" + "1" * 64 + "\n",
            pinned + "ADD x /x\n",
            pinned + 'RUN ["true"]\n',
            pinned + "COPY a /a\nRUN true\n",
            pinned + "COPY --chown=1:1 a /a\n",
            pinned + 'ENV A="b c"\n',
            pinned + "CMD hand\n",
            pinned + "RUN true \\\n",
        ):
            with self.subTest(text=text), self.assertRaises(rootless.Refused):
                rootless.parse(text)


def member(t, name, data=None, kind=tarfile.REGTYPE, mode=0o644, uid=0, gid=0, link=""):
    ti = tarfile.TarInfo(name)
    ti.type, ti.mode, ti.uid, ti.gid, ti.linkname = kind, mode, uid, gid, link
    if data is not None:
        ti.size = len(data)
        t.addfile(ti, io.BytesIO(data))
    else:
        t.addfile(ti)


class Layer(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp(prefix="hand-rootless-test-"))
        self.addCleanup(shutil.rmtree, self.tmp, True)
        self.base_tar = self.tmp / "base.tar"
        with tarfile.open(self.base_tar, "w") as t:
            member(t, ".", kind=tarfile.DIRTYPE, mode=0o755)
            for d in ("etc", "usr", "usr/bin", "var", "var/lists", "proc", "dev", "tmp"):
                member(t, d, kind=tarfile.DIRTYPE, mode=0o755)
            member(t, "etc/passwd", b"root:x:0:0\n")
            member(t, "etc/shadow", b"root:*:1\n", mode=0o640, gid=42)
            member(t, "etc/motd", b"same\n")
            member(t, "etc/gone", b"bye\n")
            member(t, "etc/pager", kind=tarfile.SYMTYPE, mode=0o777, link="/usr/bin/more")
            member(t, "usr/bin/perl", b"perl\n", mode=0o755)
            member(t, "usr/bin/perl5", kind=tarfile.LNKTYPE, mode=0o755, link="usr/bin/perl")
            member(t, "var/lists/a", b"a\n")
            member(t, "var/lists/b", b"b\n")
        self.rootfs = self.tmp / "rootfs"
        self.rootfs.mkdir()
        with tarfile.open(self.base_tar) as t:
            t.extractall(self.rootfs, filter="fully_trusted", numeric_owner=True)
        r = self.rootfs
        (r / "etc/passwd").write_text("root:x:0:0\nhand:x:1000:1000\n")
        shadow = r / "etc/shadow"
        new = r / "etc/shadow+"
        new.write_text("root:*:1\nhand:!:1\n")
        new.chmod(0o640)
        new.replace(shadow)
        (r / "etc/gone").unlink()
        (r / "etc/pager").unlink()
        (r / "etc/pager").symlink_to("/usr/bin/less")
        shutil.rmtree(r / "var/lists")
        (r / "usr/bin/ssh-agent").write_text("agent\n")
        (r / "usr/bin/ssh-agent").chmod(0o2755)
        (r / "home/hand").mkdir(parents=True)
        (r / "home/hand/.profile").write_text("x\n")
        (r / "proc/1").mkdir()
        (r / "dev/null").touch()
        (r / "etc/resolv.conf").write_text("nameserver 10.0.0.1\n")
        (r / ".theseus-rootless").mkdir()
        (r / ".theseus-rootless/owners.so").write_text("so\n")

        def ino(rel, uid, gid):
            st = os.lstat(r / rel)
            return f"{st.st_dev} {st.st_ino} {uid} {gid}\n"

        # What the shim would have seen: chgrp _ssh, useradd's chown of the home and the shadow copied as root.
        (r / ".theseus-rootless/owners.log").write_text(
            ino("usr/bin/ssh-agent", -1, 101) + ino("home/hand", 1000, 1000) + ino("home/hand/.profile", 1000, 1000)
            + ino("etc/shadow", 0, 0))
        keep, whiteouts = rootless.diff(rootless.base_index(self.base_tar), str(r),
                                        rootless.owners(r / ".theseus-rootless/owners.log"))
        self.whiteouts = whiteouts
        out = self.tmp / "run.tar.gz"
        rootless.write_layer(str(out), str(r), keep, whiteouts)
        with tarfile.open(out) as t:
            self.layer = {m.name: m for m in t.getmembers()}

    def test_new_and_changed_files_are_kept_and_the_same_ones_are_not(self):
        for name in ("etc/passwd", "etc/shadow", "etc/pager", "usr/bin/ssh-agent", "home/hand/.profile"):
            self.assertIn(name, self.layer)
        for name in ("etc/motd", "usr/bin/perl", "usr/bin/perl5"):
            self.assertNotIn(name, self.layer)
        self.assertEqual(self.layer["etc/pager"].linkname, "/usr/bin/less")
        self.assertTrue(self.layer["home/hand"].isdir())

    def test_removed_paths_are_whited_out_once_and_never_the_root(self):
        self.assertEqual(self.whiteouts, ["etc/gone", "var/lists"])
        self.assertIn("etc/.wh.gone", self.layer)
        self.assertIn("var/.wh.lists", self.layer)
        self.assertFalse(any(n.endswith(".wh..") or n == ".wh." for n in self.layer))
        self.assertNotIn("var/lists/.wh.a", self.layer)

    def test_the_namespace_and_the_build_own_files_are_left_out(self):
        for name in self.layer:
            self.assertFalse(name.startswith(("proc/", "dev/", ".theseus-rootless")), name)
        self.assertNotIn("etc/resolv.conf", self.layer)

    def test_owners_come_from_the_shim_then_the_base_then_root(self):
        agent = self.layer["usr/bin/ssh-agent"]
        self.assertEqual((agent.uid, agent.gid, agent.mode), (0, 101, 0o2755))
        self.assertEqual((self.layer["home/hand"].uid, self.layer["home/hand"].gid), (1000, 1000))
        self.assertEqual(self.layer["home/hand/.profile"].uid, 1000)
        shadow = self.layer["etc/shadow"]
        self.assertEqual((shadow.uid, shadow.gid, shadow.mode), (0, 42, 0o640))
        self.assertEqual((self.layer["etc/passwd"].uid, self.layer["etc/passwd"].gid), (0, 0))


def userns_ok():
    if not shutil.which("cc") or not shutil.which("unshare"):
        return False
    return subprocess.run(["unshare", "--map-root-user", "true"], capture_output=True).returncode == 0


@unittest.skipUnless(userns_ok(), "needs cc and an unprivileged user namespace")
class Shim(unittest.TestCase):
    def test_a_chgrp_to_an_unmapped_group_passes_and_is_recorded(self):
        tmp = Path(tempfile.mkdtemp(prefix="hand-rootless-shim-"))
        self.addCleanup(shutil.rmtree, tmp, True)
        so, log, f = tmp / "owners.so", tmp / "owners.log", tmp / "agent"
        subprocess.run(["cc", "-shared", "-fPIC", "-O2", "-Wall", "-o", str(so),
                        str(HAND / "rootless-owners.c"), "-ldl"], check=True)
        f.write_text("agent\n")
        plain = subprocess.run(["unshare", "--map-root-user", "chgrp", "101", str(f)], capture_output=True)
        self.assertNotEqual(plain.returncode, 0)
        shimmed = subprocess.run(["unshare", "--map-root-user", "env", f"LD_PRELOAD={so}",
                                  f"THESEUS_OWNERS_LOG={log}", "chgrp", "101", str(f)], capture_output=True, text=True)
        self.assertEqual(shimmed.returncode, 0, shimmed.stderr)
        st = os.stat(f)
        self.assertEqual(rootless.owners(log).get((st.st_dev, st.st_ino)), (None, 101))


if __name__ == "__main__":
    unittest.main()
