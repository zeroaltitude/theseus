"""Tests for what the three arms share in a container (theseus-n6p5,
theseus-sgpx, theseus-7gir.23). Standard library only.

    python3 -m unittest discover -s bench/harbor

The stop is run for real, on this host's `/proc`: stand-ins under a name of
their own (never `pi` or `claude`, which a developer's machine may run), their
children, one that ignores SIGTERM, and a bystander the script must leave.
"""

from __future__ import annotations

import os
import shutil
import signal
import subprocess
import tempfile
import time
import unittest
from pathlib import Path

import efficiency as ef
import measure

NAME = "stopme-agent"
SH = shutil.which("sh") or "sh"
SLEEP = shutil.which("sleep") or "sleep"


def alive(pid: int) -> bool:
    """Running, not a zombie nobody has reaped yet."""
    try:
        stat = Path(f"/proc/{pid}/stat").read_text()
    except OSError:
        return False
    return stat.rsplit(") ", 1)[1].split()[0] != "Z"


def children(pid: int) -> list[int]:
    out = []
    for d in Path("/proc").iterdir():
        if d.name.isdigit():
            try:
                stat = (d / "stat").read_text()
            except OSError:
                continue
            if int(stat.rsplit(") ", 1)[1].split()[1]) == pid:
                out.append(int(d.name))
    return out


def until(cond, secs=10.0):
    end = time.monotonic() + secs
    while time.monotonic() < end:
        if cond():
            return True
        time.sleep(0.05)
    return cond()


@unittest.skipUnless(Path("/proc/self/stat").exists(), "needs /proc")
class StopAgent(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.bin = Path(self.tmp.name)
        shutil.copy(SH, self.bin / NAME)
        shutil.copy(SLEEP, self.bin / "bystander")
        self.procs: list[subprocess.Popen] = []

    def tearDown(self):
        for p in self.procs:  # each stand-in's own process group, by the pid this test started
            try:
                os.killpg(p.pid, signal.SIGKILL)
            except OSError:
                pass
            p.wait()
        self.tmp.cleanup()

    def start(self, script: str, name: str = NAME) -> subprocess.Popen:
        p = subprocess.Popen([str(self.bin / name), "-c", script] if name == NAME
                             else [str(self.bin / name), "300"], start_new_session=True)
        self.procs.append(p)
        return p

    def stop(self, grace_s: int = 1) -> float:
        began = time.monotonic()
        r = subprocess.run([SH, "-c", measure.stop_agent_script((NAME,), grace_s)], timeout=60)
        self.assertEqual(r.returncode, 0)
        return time.monotonic() - began

    def tree(self, p: subprocess.Popen, want: int) -> list[int]:
        self.assertTrue(until(lambda: len(children(p.pid)) >= want), "the stand-in's children did not start")
        return children(p.pid)

    def test_the_agent_and_what_it_started_are_stopped_and_a_bystander_is_not(self):
        agent = self.start("sleep 300 & sleep 300 & wait")
        bystander = self.start("", "bystander")
        kids = self.tree(agent, 2)
        self.assertEqual(os.readlink(f"/proc/{agent.pid}/exe"), os.path.realpath(self.bin / NAME))
        self.stop()
        self.assertTrue(until(lambda: agent.poll() is not None), "the agent still runs")
        self.assertTrue(until(lambda: not any(alive(k) for k in kids)), "a child it started still runs")
        self.assertIsNone(bystander.poll(), "a process of another name was signalled")
        self.assertTrue(alive(bystander.pid))

    def test_a_child_the_first_sigterm_orphans_is_still_found(self):
        # The agent exits on SIGTERM at once; its child, which ignores it, is
        # reparented. The tree was read before any signal, so it still falls to
        # the SIGKILL after the grace.
        agent = self.start("trap 'exit 0' TERM; (trap '' TERM; exec sleep 300) & wait")
        kids = self.tree(agent, 1)
        took = self.stop(grace_s=1)
        self.assertTrue(until(lambda: agent.poll() is not None))
        self.assertTrue(until(lambda: not any(alive(k) for k in kids)), "the orphaned child still runs")
        self.assertGreaterEqual(took, 0.9, "the child was killed before its grace")

    def test_an_agent_that_ignores_sigterm_is_killed_after_the_grace(self):
        agent = self.start("trap '' TERM; while :; do sleep 1; done")
        time.sleep(0.3)
        took = self.stop(grace_s=1)
        self.assertTrue(until(lambda: agent.poll() is not None), "the agent survived the SIGKILL")
        self.assertEqual(agent.returncode, -signal.SIGKILL)
        self.assertGreaterEqual(took, 0.9)
        self.assertLess(took, 10)

    def test_an_agent_that_stops_at_the_sigterm_is_not_waited_for(self):
        agent = self.start("sleep 300 & wait")
        self.tree(agent, 1)
        took = self.stop(grace_s=20)
        self.assertEqual(agent.wait(timeout=10), -signal.SIGTERM)
        self.assertLess(took, 10, "it waited out the grace")

    def test_no_agent_running_is_no_error(self):
        self.assertLess(self.stop(grace_s=5), 5)

    def test_the_arms_names_are_the_samplers_and_the_grace_is_short(self):
        self.assertEqual(ef.ARMS["pi"]["names"], ("pi",))
        self.assertEqual(ef.ARMS["claude-code"]["names"], ("claude",))
        self.assertLessEqual(measure.GRACE_S, 5)
        script = measure.stop_agent_script(ef.ARMS["claude-code"]["names"])
        self.assertTrue(script.startswith('names="claude"; '))
        self.assertNotIn("pkill", script)
        self.assertNotIn("killall", script)


class Version(unittest.TestCase):
    def run_script(self, command: str, logs: Path) -> None:
        subprocess.run([SH, "-c", measure.version_script(command, logs.as_posix())], check=True)

    def test_the_version_is_written_to_the_logs_and_read_back(self):
        with tempfile.TemporaryDirectory() as d:
            logs = Path(d)
            self.run_script("echo 'notice: an invented line'; echo 1.0.4", logs)
            self.assertEqual((logs / measure.VERSION_FILE).read_text(), "notice: an invented line\n1.0.4\n")
            self.assertEqual(ef.version_read(logs, lambda t: t.splitlines()[-1].strip()), "1.0.4")
            self.assertEqual(ef.version_read(logs), "notice: an invented line\n1.0.4")

    def test_a_failed_read_leaves_nothing_and_never_fails(self):
        with tempfile.TemporaryDirectory() as d:
            logs = Path(d)
            self.run_script("echo not a version; exit 3", logs)
            self.assertFalse((logs / measure.VERSION_FILE).exists())
            self.assertIsNone(ef.version_read(logs))

    def test_a_record_names_the_effort_and_the_versions_asked_and_read(self):
        with tempfile.TemporaryDirectory() as d:
            logs = Path(d)
            (logs / measure.VERSION_FILE).write_text("2.1.288 (Claude Code)\n")
            rec = ef.stamp(logs, {"schema": ef.SCHEMA}, "medium", "2.1.290", lambda t: t.split()[0])
            self.assertEqual(rec, {"schema": ef.SCHEMA, "effort": "medium", "version": "2.1.288",
                                   "version_asked": "2.1.290"})


if __name__ == "__main__":
    unittest.main()
