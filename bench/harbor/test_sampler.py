"""Tests for the harness sampler (theseus-7gir.12).

    python3 -m unittest discover -s bench/harbor

The parsers and the classes read fixture `/proc` trees; the live tests run
on this host's `/proc` (Linux only, skipped elsewhere): a copy of `sh` under a
harness name runs a busy child, whose CPU must land in work.
"""

from __future__ import annotations

import json
import os
import resource
import shutil
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path

import sampler as sm

HERE = Path(__file__).resolve().parent
SAMPLER = HERE / "sampler.py"
LINUX = sys.platform.startswith("linux") and Path("/proc/self/stat").exists()


def stat_line(pid, comm, ppid, utime=0, stime=0, cutime=0, cstime=0, start=100):
    # 52 fields as Linux writes them; the ones the sampler reads are set.
    rest = ["S", str(ppid), str(pid), str(pid), "0", "-1", "4194560", "0", "0", "0", "0",
            str(utime), str(stime), str(cutime), str(cstime), "20", "0", "1", "0", str(start)]
    rest += ["0"] * 32
    return "%d (%s) %s\n" % (pid, comm, " ".join(rest))


class FixtureProc:
    """A fixture `/proc`: a directory per pid with its stat, status, and
    cmdline, rewritten between samples."""

    def __init__(self, root: Path):
        self.root = root

    def put(self, pid, comm, ppid, argv=None, rss=0, hwm=0, **times):
        d = self.root / str(pid)
        d.mkdir(parents=True, exist_ok=True)
        (d / "stat").write_text(stat_line(pid, comm, ppid, **times))
        (d / "status").write_text("Name:\t%s\nVmHWM:\t%8d kB\nVmRSS:\t%8d kB\nThreads:\t1\n"
                                  % (comm, hwm, rss))
        (d / "cmdline").write_bytes(b"\0".join(a.encode() for a in (argv or [comm])) + b"\0")

    def drop(self, pid):
        shutil.rmtree(self.root / str(pid))


class Parsers(unittest.TestCase):
    def test_a_stat_line_is_counted_from_its_last_parenthesis(self):
        line = stat_line(4242, "tokio-rt (wk 1)", 7, utime=305, stime=112, cutime=40, cstime=2,
                         start=99)
        self.assertEqual(sm.parse_stat(line), {
            "pid": 4242, "comm": "tokio-rt (wk 1)", "ppid": 7, "utime": 305, "stime": 112,
            "cutime": 40, "cstime": 2, "starttime": 99})
        self.assertIsNone(sm.parse_stat("no parenthesis"))
        self.assertIsNone(sm.parse_stat("1 (x) S 1 2"), "a short line")
        self.assertIsNone(sm.parse_stat(""))

    def test_a_status_gives_rss_and_its_peak(self):
        text = ("Name:\ttheseusd\nUmask:\t0022\nVmPeak:\t  412344 kB\nVmHWM:\t   45120 kB\n"
                "VmRSS:\t   41984 kB\nThreads:\t17\n")
        self.assertEqual(sm.parse_status(text), (41984, 45120))
        self.assertEqual(sm.parse_status("Name:\tkthreadd\n"), (0, 0))

    def test_a_cmdline_and_the_cgroup_files(self):
        self.assertEqual(sm.parse_cmdline(b"theseusd\0job-wrapper\0--spool\0/s\0"),
                         ["theseusd", "job-wrapper", "--spool", "/s"])
        self.assertEqual(sm.parse_cpu_stat("usage_usec 1115704819\nuser_usec 10\n"), 1115704819)
        self.assertIsNone(sm.parse_cpu_stat("user_usec 10\n"))
        self.assertEqual(sm.parse_cgroup_path("1:cpu:/\n0::/\n"), "/")
        self.assertEqual(sm.parse_cgroup_path("0::/docker/abc\n"), "/docker/abc")
        self.assertIsNone(sm.parse_cgroup_path("4:memory:/x\n"))

    def test_the_cgroup_is_found_under_the_unified_mount_or_a_hybrids(self):
        with tempfile.TemporaryDirectory() as t:
            proc, cg = Path(t, "proc"), Path(t, "cgroup")
            (proc / "self").mkdir(parents=True)
            (proc / "self" / "cgroup").write_text("0::/\n")
            self.assertIsNone(sm.find_cgroup(str(proc), str(cg)), "no cpu.stat: none")
            (cg / "unified").mkdir(parents=True)
            (cg / "unified" / "cpu.stat").write_text("usage_usec 5\n")
            self.assertIsNone(sm.find_cgroup(str(proc), str(cg)),
                              "no cgroup.type: the machine's root, not a container's")
            (cg / "unified" / "cgroup.type").write_text("domain\n")
            self.assertEqual(sm.find_cgroup(str(proc), str(cg)), str(cg / "unified"))
            (cg / "cpu.stat").write_text("usage_usec 7\n")
            (cg / "cgroup.type").write_text("domain\n")
            (cg / "memory.peak").write_text("%d\n" % (300 * 1024 * 1024))
            self.assertEqual(sm.find_cgroup(str(proc), str(cg)), str(cg))
            self.assertEqual(sm.read_cgroup(str(cg)), (7, 300 * 1024))


class Classes(unittest.TestCase):
    """The tree: init and Harbor's shell outside; `theseus` and its
    `theseusd` the harness; a job wrapper apart; `bash` and `cc1` under it,
    work."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.proc = FixtureProc(Path(self.tmp.name))
        self.t = sm.Tracker(["theseus", "theseusd"], ["job-wrapper"], ignore_pids=[99],
                            proc_root=self.tmp.name)

    def tearDown(self):
        self.tmp.cleanup()

    def sample(self):
        self.t.observe(sm.read_procs(str(self.proc.root), self.t.wants_cmdline))

    def cpu(self):
        return dict((c, self.t.cpu[c]) for c in sm.CLASSES)

    def test_each_process_lands_in_its_class(self):
        p = self.proc
        p.put(1, "init", 0, rss=1000, utime=500)
        p.put(10, "sh", 1, rss=2000, utime=300)
        p.put(99, "python3", 10, rss=9000)  # the sampler
        p.put(20, "theseus", 10, rss=8000, hwm=8500, utime=4)
        p.put(21, "theseusd", 20, ["theseusd", "--state-dir", "/s"], rss=30000, hwm=31000, utime=10)
        # theseusd starts its wrapper through /proc/self/exe: comm `exe`.
        p.put(30, "exe", 21, ["theseusd", "job-wrapper", "--correlation-id", "act_1"],
              rss=3000, utime=1)
        p.put(31, "bash", 30, rss=4000, utime=2)
        p.put(32, "cc1", 31, rss=120000, hwm=150000, utime=50)
        procs = sm.read_procs(str(p.root), self.t.wants_cmdline)
        self.assertEqual(self.t.classify(procs), {
            1: "outside", 10: "outside", 99: "outside", 20: "harness", 21: "harness",
            30: "wrapper", 31: "work", 32: "work"})
        self.sample()
        # The first sample: the harness's tree counts from its start, the
        # container's own from now.
        self.assertEqual(self.cpu(), {"harness": 14, "wrapper": 1, "work": 52, "outside": 0})
        self.assertEqual((self.t.peak_rss["harness"], self.t.peak_hwm["harness"]), (38000, 31000))
        self.assertEqual((self.t.peak_rss["work"], self.t.peak_hwm["work"]), (124000, 150000))
        self.assertEqual(self.t.peak_rss["outside"], 0, "the container's own are never read")

        # cc1 works on, the daemon a little; then cc1 is reaped by bash.
        p.put(21, "theseusd", 20, ["theseusd"], rss=32000, utime=12)
        p.put(32, "cc1", 31, rss=100000, utime=150)
        p.put(10, "sh", 1, utime=900)  # outside, never counted
        self.sample()
        self.assertEqual(self.cpu(), {"harness": 16, "wrapper": 1, "work": 152, "outside": 600})
        p.drop(32)
        p.put(31, "bash", 30, rss=4000, utime=2, cutime=170)  # cc1 used 20 more ticks
        self.sample()
        self.assertEqual(self.cpu()["work"], 172, "a reaped child's last interval is work")
        self.assertEqual(self.t.peak_rss["work"], 124000, "the peak stays the largest sum")

    def test_a_reaped_child_no_sample_saw_is_work(self):
        p = self.proc
        p.put(20, "theseus", 1, utime=1)
        p.put(21, "theseusd", 20, ["theseusd"], utime=5)
        self.sample()
        # Between two samples the daemon ran `git` (30 ticks) and reaped it,
        # and used 2 ticks of its own.
        p.put(21, "theseusd", 20, ["theseusd"], utime=7, cutime=25, cstime=5)
        self.sample()
        self.assertEqual(self.cpu(), {"harness": 8, "wrapper": 0, "work": 30, "outside": 0})

    def test_a_cli_reaping_its_daemon_keeps_the_daemons_tail_as_harness(self):
        p = self.proc
        p.put(20, "theseus", 1, utime=1)
        p.put(21, "theseusd", 20, ["theseusd"], utime=5, cutime=40)  # 40: work it reaped
        self.sample()
        self.assertEqual(self.cpu(), {"harness": 6, "wrapper": 0, "work": 40, "outside": 0})
        p.drop(21)
        p.put(20, "theseus", 1, utime=1, cutime=5 + 40 + 3)  # the daemon's last 3 ticks
        self.sample()
        self.assertEqual(self.cpu(), {"harness": 9, "wrapper": 0, "work": 40, "outside": 0})

    def test_a_harness_and_its_work_that_end_together_leave_the_work_as_work(self):
        p = self.proc
        p.put(10, "sh", 1)
        p.put(20, "theseus", 10, utime=1)
        p.put(21, "bash", 20, utime=100)
        self.sample()
        # Both end between two samples: bash used 30 more ticks, the CLI 2,
        # and Harbor's shell reaped the CLI, which had reaped bash.
        p.drop(21)
        p.drop(20)
        p.put(10, "sh", 1, cutime=1 + 2 + 100 + 30)
        self.sample()
        self.assertEqual(self.cpu(), {"harness": 1, "wrapper": 0, "work": 132, "outside": 0})

    def test_an_orphan_stays_work_and_a_reused_pid_starts_over(self):
        p = self.proc
        p.put(1, "init", 0)
        p.put(20, "claude", 1)
        p.put(40, "node", 20, utime=10)
        t = sm.Tracker(["claude"], proc_root=str(p.root))
        read = lambda: sm.read_procs(str(p.root), t.wants_cmdline)
        t.observe(read())
        p.put(40, "node", 1, utime=20)  # its parent exited: init adopted it
        t.observe(read())
        self.assertEqual(t.cpu["work"], 20)
        p.put(40, "node", 1, utime=3, start=900)  # a new process on the old pid
        t.observe(read())
        self.assertEqual(t.cpu["work"], 20, "an orphan's new namesake is not the command's")
        self.assertEqual(t.cpu["outside"], 3)

    def test_a_process_started_through_proc_self_exe_is_named_by_its_argv(self):
        p = self.proc
        p.put(21, "theseusd", 1, ["/i/bin/theseusd", "--stdio"])
        p.put(30, "exe", 21, ["theseusd", "job-wrapper", "--spool", "/s"])
        p.put(31, "exe", 21, ["/i/bin/theseusd", "--stdio"])
        p.put(32, "exe", 30, ["/usr/bin/some-tool"])
        procs = sm.read_procs(str(p.root), self.t.wants_cmdline)
        self.assertEqual(self.t.classify(procs),
                         {21: "harness", 30: "wrapper", 31: "harness", 32: "work"})

    def test_a_harness_name_wins_over_its_parent(self):
        """Claude Code's CLI is `claude` in comm; a subagent it starts is the
        harness too, and so is a daemon's own child of its name."""
        p = self.proc
        p.put(20, "claude", 1)
        p.put(21, "claude", 20)
        p.put(22, "bash", 21)
        t = sm.Tracker(["claude"], proc_root=str(p.root))
        procs = sm.read_procs(str(p.root), t.wants_cmdline)
        self.assertEqual(t.classify(procs), {20: "harness", 21: "harness", 22: "work"})


def busy_harness(tmp: Path, name: str, loops: int, linger_s: float = 0) -> list[str]:
    """A copy of `sh` named `name` whose child (an `sh`, so work) counts to
    `loops`, and which then sleeps `linger_s`; the trailing `true` keeps the
    shell from exec'ing its last command."""
    harness = tmp / name
    shutil.copy("/bin/sh", harness)
    harness.chmod(0o755)
    spin = "i=0; while [ $i -lt %d ]; do i=$((i+1)); done" % loops
    return [str(harness), "-c", "/bin/sh -c '%s'; sleep %s; true" % (spin, linger_s)]


def children_cpu() -> float:
    r = resource.getrusage(resource.RUSAGE_CHILDREN)
    return r.ru_utime + r.ru_stime


@unittest.skipUnless(LINUX, "needs Linux's /proc")
class ThisHost(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = Path(self.tmp.name)
        self.out = self.dir / "out"

    def tearDown(self):
        self.tmp.cleanup()

    def run_sampled(self, name, loops, interval_ms, linger_s=0):
        """The summary of a sampled run of `busy_harness`, and what the run
        used in all (the harness and its child), as the kernel counts it."""
        s = subprocess.Popen([sys.executable, str(SAMPLER), "--out", str(self.out), "--names",
                              name, "--interval-ms", str(interval_ms)])
        try:
            deadline = time.monotonic() + 10
            while not (self.out / sm.READY).exists():
                self.assertLess(time.monotonic(), deadline, "the sampler never sampled")
                time.sleep(0.01)
            before = children_cpu()
            subprocess.run(busy_harness(self.dir, name, loops, linger_s), check=True, timeout=120)
            used = children_cpu() - before
            time.sleep(interval_ms / 1000.0 * 1.5)
        finally:
            s.terminate()
            s.wait(timeout=30)
        self.assertEqual(s.returncode, 0)
        self.assertTrue((self.out / sm.DONE).exists())
        return json.loads((self.out / sm.SUMMARY).read_text()), used

    def test_a_busy_child_is_work_and_the_harness_and_sampler_stay_near_zero(self):
        summary, used = self.run_sampled("harnessx", 3_000_000, 250)
        c = summary["classes"]
        self.assertEqual(summary["status"], "ok")
        self.assertGreater(used, 1.0, "the child did enough to measure")
        self.assertGreaterEqual(summary["samples"], 4)
        self.assertAlmostEqual(c["work"]["cpu_s"], used, delta=0.05)
        self.assertLess(c["harness"]["cpu_s"], 0.05, c)
        self.assertGreater(c["harness"]["peak_rss_kb"], 0)
        self.assertGreater(c["work"]["peak_rss_kb"], 0)
        self.assertEqual(c["harness"]["processes"], 1)
        self.assertLess(summary["sampler"]["core_share"], 0.01, summary["sampler"])
        lines = (self.out / sm.SAMPLES).read_text().splitlines()
        self.assertEqual(len(lines), summary["samples"])
        self.assertEqual(json.loads(lines[-1])["cpu_s"]["work"], c["work"]["cpu_s"])

    def test_a_child_shorter_than_an_interval_still_counts(self):
        # The harness lives on past the child, as an agent does past a
        # command: the child ends long before the second sample, so it is
        # only reaped (the one work process a sample sees is the harness's
        # `sleep`).
        summary, used = self.run_sampled("harnessy", 300_000, 4000, linger_s=4.5)
        c = summary["classes"]
        self.assertEqual(c["work"]["processes"], 1)
        self.assertAlmostEqual(c["work"]["cpu_s"], used, delta=0.05)
        self.assertLess(c["harness"]["cpu_s"], 0.05, c)


@unittest.skipUnless(LINUX, "needs Linux's /proc")
class Scripts(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        d = Path(self.tmp.name)
        self.out, self.state = d / "logs", d / "state"

    def tearDown(self):
        self.tmp.cleanup()

    def sh(self, script, env=None):
        return subprocess.run(["/bin/sh", "-c", script], capture_output=True, text=True,
                              timeout=60, env=env)

    def test_the_stop_script_stops_the_sampler_and_leaves_its_summary(self):
        start = sm.start_script(str(SAMPLER), str(self.out), str(self.state), ["theseus"],
                                ["job-wrapper"], interval_ms=100)
        r = self.sh(start)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertTrue((self.out / sm.READY).exists(), "the start waits for its first sample")
        pid = int((self.state / "sampler.pid").read_text())
        self.assertTrue(Path("/proc/%d" % pid).exists())
        time.sleep(0.3)
        r = self.sh(sm.stop_script(str(self.out), str(self.state)))
        self.assertEqual(r.returncode, 0, r.stderr)
        summary = json.loads((self.out / sm.SUMMARY).read_text())
        self.assertEqual((summary["status"], summary["interval_ms"]), ("ok", 100))
        self.assertEqual(summary["wrapper_args"], ["job-wrapper"])
        self.assertGreaterEqual(summary["samples"], 2)
        self.assertFalse((self.state / "sampler.pid").exists())
        for _ in range(100):
            if not Path("/proc/%d" % pid).exists() or "Z" in Path("/proc/%d/stat" % pid).read_text().split()[2]:
                break
            time.sleep(0.05)
        else:
            self.fail("the sampler is still running")
        # A second stop, with nothing running, is no error.
        self.assertEqual(self.sh(sm.stop_script(str(self.out), str(self.state))).returncode, 0)

    def test_without_python3_it_says_unavailable_and_goes_on(self):
        bin_ = Path(self.tmp.name, "bin")
        bin_.mkdir()
        for tool in ("mkdir", "rm", "cat", "sleep"):
            os.symlink(shutil.which(tool), bin_ / tool)
        start = sm.start_script(str(SAMPLER), str(self.out), str(self.state), ["theseus"])
        t0 = time.monotonic()
        r = self.sh(start + "; echo went-on", env={"PATH": str(bin_)})
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("went-on", r.stdout)
        self.assertLess(time.monotonic() - t0, 2, "no wait for a sampler that never started")
        summary = json.loads((self.out / sm.SUMMARY).read_text())
        self.assertEqual(summary, {"status": "unavailable", "reason": "no python3 on PATH"})
        self.assertEqual(self.sh(sm.stop_script(str(self.out), str(self.state)),
                                 env={"PATH": str(bin_)}).returncode, 0)


if __name__ == "__main__":
    unittest.main()
