"""Tests for scripts/load-rig.py (theseus-1eemp, 1.0.5 B5): the load it starts cannot outlive the process that asked.

Run: python3 -m unittest -q scripts/tests/test_load_rig.py

A survivor is found by a tag in the ENVIRONMENT of the rig the test starts (`LOAD_RIG_TAG`, a fresh uuid per run),
which every spinner inherits: `/proc/<pid>/environ` is read for each process, so nothing here depends on matching a
command line (a `pgrep -f` or `ps | grep` also matches the shell that runs it). A zombie has no environment and
holds no CPU, so it is not counted. The spinners are few (3) and short-lived; the cases that matter are the ones a
`trap` cannot cover: the rig killed with SIGKILL, which is what a tool timeout sends.
"""
import os
import signal
import subprocess
import sys
import time
import unittest
import uuid

HERE = os.path.dirname(os.path.abspath(__file__))
RIG = os.path.join(HERE, "..", "load-rig.py")


def tagged(tag):
    """The pids whose environment carries this run's tag."""
    want = f"LOAD_RIG_TAG={tag}".encode()
    out = []
    for d in os.listdir("/proc"):
        if not d.isdigit():
            continue
        try:
            with open(f"/proc/{d}/environ", "rb") as f:
                if want in f.read().split(b"\0"):
                    out.append(int(d))
        except OSError:
            continue
    return out


def wait_for(pred, secs):
    end = time.monotonic() + secs
    while time.monotonic() < end:
        if pred():
            return True
        time.sleep(0.05)
    return pred()


class LoadRig(unittest.TestCase):
    def start(self, *args, settle="30"):
        self.tag = uuid.uuid4().hex
        env = dict(os.environ, LOAD_RIG_TAG=self.tag)
        p = subprocess.Popen(
            [sys.executable, "-I", RIG, "--spinners", "3", "--settle", settle, "--max-secs", "60", *args],
            env=env,
            stdout=subprocess.DEVNULL,
        )
        # The rig and its three spinners are up.
        self.assertTrue(wait_for(lambda: len(tagged(self.tag)) == 4, 5), tagged(self.tag))
        self.addCleanup(self.reap, p)
        return p

    def reap(self, p):
        for pid in tagged(self.tag):
            try:
                os.kill(pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
        p.wait()

    def test_a_normal_end_leaves_nothing_and_commands_status_is_its_own(self):
        tag = uuid.uuid4().hex
        env = dict(os.environ, LOAD_RIG_TAG=tag)
        r = subprocess.run(
            [sys.executable, "-I", RIG, "--spinners", "3", "--settle", "0.3", "--", "sh", "-c", "exit 7"],
            env=env,
            capture_output=True,
        )
        self.assertEqual(r.returncode, 7, r.stderr)
        self.assertEqual(tagged(tag), [])

    def test_sigterm_ends_the_rig_and_every_spinner(self):
        p = self.start()
        p.send_signal(signal.SIGTERM)
        self.assertEqual(p.wait(), 128 + signal.SIGTERM)
        self.assertEqual(tagged(self.tag), [])

    def test_sigkill_of_the_rig_takes_the_spinners_with_it(self):
        # The case no trap covers: the kernel, not the rig, does the killing.
        p = self.start()
        p.send_signal(signal.SIGKILL)
        p.wait()
        self.assertTrue(wait_for(lambda: tagged(self.tag) == [], 3), f"orphaned: {tagged(self.tag)}")

    def test_the_command_dies_with_the_rig_too(self):
        tag = uuid.uuid4().hex
        env = dict(os.environ, LOAD_RIG_TAG=tag)
        p = subprocess.Popen(
            [sys.executable, "-I", RIG, "--spinners", "1", "--settle", "0.1", "--", "sleep", "60"],
            env=env,
        )
        self.addCleanup(lambda: [os.kill(x, signal.SIGKILL) for x in tagged(tag)] and p.wait())
        self.assertTrue(wait_for(lambda: len(tagged(tag)) == 3, 5), tagged(tag))  # rig, spinner, sleep
        p.send_signal(signal.SIGKILL)
        p.wait()
        self.assertTrue(wait_for(lambda: tagged(tag) == [], 3), f"orphaned: {tagged(tag)}")

    def test_a_spinner_stops_itself_at_its_deadline(self):
        # The rig sleeps in its settle and is alive the whole time: only the spinner's own deadline can end them.
        tag = uuid.uuid4().hex
        env = dict(os.environ, LOAD_RIG_TAG=tag)
        p = subprocess.Popen(
            [sys.executable, "-I", RIG, "--spinners", "3", "--settle", "30", "--max-secs", "1"],
            env=env,
            stdout=subprocess.DEVNULL,
        )
        self.addCleanup(lambda: [os.kill(x, signal.SIGKILL) for x in tagged(tag)] and p.wait())
        self.assertTrue(wait_for(lambda: len(tagged(tag)) == 4, 5), tagged(tag))
        self.assertTrue(wait_for(lambda: tagged(tag) == [p.pid], 5), f"still running: {tagged(tag)}")


if __name__ == "__main__":
    unittest.main()
