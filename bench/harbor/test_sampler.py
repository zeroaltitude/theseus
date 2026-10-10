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
import statistics
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path
from unittest import mock

import sampler as sm

HERE = Path(__file__).resolve().parent
SAMPLER = HERE / "sampler.py"
LINUX = sys.platform.startswith("linux") and Path("/proc/self/stat").exists()


def sampler_pids(mark: str) -> list[int]:
    """The samplers running now whose command line names `mark` (a test's
    own directory), zombies left out."""
    out = []
    for proc in Path("/proc").iterdir():
        if not proc.name.isdigit():
            continue
        try:
            cmd = (proc / "cmdline").read_bytes()
            state = (proc / "stat").read_text().rpartition(")")[2].split()[0]
        except (OSError, IndexError):
            continue
        if b"sampler.py" in cmd and mark.encode() in cmd and state != "Z":
            out.append(int(proc.name))
    return out


def stop_samplers(test: unittest.TestCase, out: Path, state: Path, mark: Path, wait_s: float = 30) -> None:
    """A test's cleanup (theseus-99by): the stop script, then a wait for
    every sampler of `mark` to be gone, before the test's directory goes.
    One still running after `wait_s` (the stop script's SIGTERM unanswered)
    is killed, and the test fails: a sampler left behind is a finding, not
    a flake."""
    subprocess.run(["sh", "-c", sm.stop_script(str(out), str(state))], timeout=60)
    deadline = time.monotonic() + wait_s
    while sampler_pids(str(mark)) and time.monotonic() < deadline:
        for pid in sampler_pids(str(mark)):
            try:
                os.kill(pid, 15)
            except OSError:
                pass
        time.sleep(0.1)
    left = sampler_pids(str(mark))
    for pid in left:
        try:
            os.kill(pid, 9)
        except OSError:
            pass
    test.assertEqual(left, [], "a sampler outlived its test")


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

    def test_a_reaper_read_before_its_reap_is_matched_a_sample_later(self):
        """A sample is not one instant: Harbor's shell's `stat` is read just
        before it reaps the CLI, which is gone by the time its own is read.
        The shell's `cutime` holds the CLI and its work only in the next
        sample, and they are still the tree's (theseus-99by: a loaded run's
        work read up to 0.17 s low, its last interval sent outside)."""
        p = self.proc
        p.put(10, "sh", 1)
        p.put(20, "theseus", 10, utime=1)
        p.put(21, "bash", 20, utime=100)
        self.sample()
        p.drop(21)
        p.drop(20)
        self.sample()  # the shell read before the reap: its cutime still 0
        p.put(10, "sh", 1, cutime=1 + 2 + 100 + 30)
        self.sample()
        self.assertEqual(self.cpu(), {"harness": 1, "wrapper": 0, "work": 132, "outside": 0})

    def test_a_child_gone_before_its_parents_cutime_holds_it_is_counted_once(self):
        p = self.proc
        p.put(20, "theseus", 1, utime=1)
        p.put(21, "bash", 20, utime=50)
        self.sample()
        p.drop(21)
        self.sample()  # the CLI read before it reaped bash
        p.put(20, "theseus", 1, utime=1, cutime=80)  # bash's 80 in all
        self.sample()
        self.assertEqual(self.cpu(), {"harness": 1, "wrapper": 0, "work": 80, "outside": 0})
        # A child its parent never accounts (autoreaped) is matched once, not waited on.
        p.put(22, "bash", 20, utime=10)
        self.sample()
        p.drop(22)
        self.sample()
        self.sample()
        p.put(20, "theseus", 1, utime=1, cutime=80 + 5)  # another child, 5 ticks, never seen
        self.sample()
        self.assertEqual(self.cpu()["work"], 95)

    def test_a_child_read_alive_before_its_parent_reaps_it_is_counted_twice_a_known_miss(self):
        """sampler.py's "What it misses": a child with a lower pid than its
        parent's is read alive in a sample in which its parent, read after,
        has already reaped it. Its whole time is counted twice, not only its
        last interval: 160 ticks of work for a true 80."""
        p = self.proc
        p.put(30, "theseus", 1, utime=1)
        p.put(21, "bash", 30, utime=50)
        self.sample()
        p.put(21, "bash", 30, utime=80)  # read alive, 80 ticks in all
        p.put(30, "theseus", 1, utime=1, cutime=80)  # and reaped, by the parent read after it
        self.sample()
        self.sample()
        self.assertEqual(self.cpu(), {"harness": 1, "wrapper": 0, "work": 160, "outside": 0})

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
        self.addCleanup(self.tmp.cleanup)
        self.dir = Path(self.tmp.name)
        self.out = self.dir / "out"

    def run_sampled(self, name, loops, interval_ms, linger_s=0):
        """The summary of a sampled run of `busy_harness`, and what the run
        used in all (the harness and its child), as the kernel counts it."""
        s = subprocess.Popen([sys.executable, str(SAMPLER), "--out", str(self.out), "--names",
                              name, "--interval-ms", str(interval_ms)])
        self.addCleanup(stop_samplers, self, self.out, self.dir, self.dir)
        self.addCleanup(s.wait, 30)
        try:
            deadline = time.monotonic() + 10
            while not (self.out / sm.READY).exists():
                self.assertLess(time.monotonic(), deadline, "the sampler never sampled")
                time.sleep(0.01)
            before = children_cpu()
            subprocess.run(busy_harness(self.dir, name, loops, linger_s), check=True, timeout=900)
            used = children_cpu() - before
            time.sleep(interval_ms / 1000.0 * 1.5)
        finally:
            s.terminate()
            s.wait(timeout=30)
        self.assertEqual(s.returncode, 0)
        self.assertTrue((self.out / sm.DONE).exists())
        return json.loads((self.out / sm.SUMMARY).read_text()), used

    def test_a_busy_child_is_work_and_the_harness_and_sampler_stay_near_zero(self):
        summary, used = self.run_sampled("harnessx", 2_000_000, 250)
        c = summary["classes"]
        self.assertEqual(summary["status"], "ok")
        self.assertGreater(used, 0.5, "the child did enough to measure")
        self.assertGreaterEqual(summary["samples"], 4)
        self.assertAlmostEqual(c["work"]["cpu_s"], used, delta=0.05)
        self.assertLess(c["harness"]["cpu_s"], 0.05, c)
        self.assertGreater(c["harness"]["peak_rss_kb"], 0)
        self.assertGreater(c["work"]["peak_rss_kb"], 0)
        self.assertEqual(c["harness"]["processes"], 1)
        lines = (self.out / sm.SAMPLES).read_text().splitlines()
        self.assertEqual(len(lines), summary["samples"])
        self.assertEqual(json.loads(lines[-1])["cpu_s"]["work"], c["work"]["cpu_s"])

    def test_a_child_shorter_than_an_interval_still_counts(self):
        # The harness lives on past the child, as an agent does past a
        # command: the child ends long before the second sample, so it is
        # only reaped (the one work process a sample sees is the harness's
        # `sleep`; a starved machine can stretch the child into a sample
        # too, and its CPU must still be work).
        summary, used = self.run_sampled("harnessy", 300_000, 4000, linger_s=4.5)
        c = summary["classes"]
        self.assertIn(c["work"]["processes"], (1, 2))
        self.assertAlmostEqual(c["work"]["cpu_s"], used, delta=0.05)
        self.assertLess(c["harness"]["cpu_s"], 0.05, c)


@unittest.skipUnless(LINUX, "needs Linux's /proc")
class Scripts(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        d = Path(self.tmp.name)
        self.out, self.state = d / "logs", d / "state"
        # Stopped before the directory goes, whatever the test did.
        self.addCleanup(stop_samplers, self, self.out, self.state, d)

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


# The sampler's own CPU grows with the processes it reads (a stat read each,
# per sample), so no bound that ignores their count measures it (theseus-t412).
# These bound the cost per process, where the count is known.
COST_N = 50
COST_SAMPLES = 30
COST_ROUNDS = 15
# Microseconds of the sampler's CPU per process read, one sample's worth. Each
# is twice the worst this 4-core VM measured over 15 runs of each count with
# `nice -n 19` beside four busy loops at nice 0 (11.7 us, at 4N).
FIXTURE_US_PER_PROC = 24.0
# What the sampler does per process is a count of reads, held exactly
# (`reads_of`, theseus-ufe5): a `stat` of every process, a `cmdline` of the
# harness names' (and `exe`'s) only, a `status` of the tree's (harness,
# wrapper and work: the memory), none of the container's own. A count catches
# any extra read on any host, which a time cannot: an extra `stat` read in the
# shared path costs the sampler and F5 alike and cancels in their ratio.
# The timed check is the median over rounds of the sampler's time over F5's
# (the sampler plus a `cmdline` read of every process), each round timing
# both back to back so a loaded host moves them together. On the 4-core VM,
# 10 runs quiet and 10 at nice 19 beside four busy loops at nice 0, the ratio
# was 0.703 to 0.725 (F4, `status` too, against F5: never under 0.97). The
# bound sits at 0.85: 17% over the sampler's worst, and a sampler that reads
# every `cmdline` is F5 itself, at 1.0.
SAMPLER_UNDER_F5 = 0.85


def sampler_pass(root: Path, read=None, want=None, tracker=sm.Tracker):
    """One sample's work over `root` as the sampler does it (`read_procs`, or
    `read` in its place, with `want` for the cmdline), after a baseline."""
    t = tracker(("harnessx",), ignore_pids=(os.getpid(),), proc_root=str(root))
    read, want = read or sm.read_procs, want or t.wants_cmdline
    t.observe(read(str(root), want))  # the baseline
    return lambda: t.observe(read(str(root), want))


def reads_of(one) -> dict:
    """What one pass reads, by file name (`stat`, `cmdline`, `status`), and
    how many times it classifies (`classify`, `own_class`): the counting seam
    over the sampler's one read function and its classification."""
    got = dict()

    def count(key):
        got[key] = got.get(key, 0) + 1

    real_read, real_classify, real_own = sm._read, sm.Tracker.classify, sm.Tracker._own_class

    def read(path, binary=False):
        count(os.path.basename(path))
        return real_read(path, binary)

    def classify(self, procs):
        count("classify")
        return real_classify(self, procs)

    def own_class(self, p):
        count("own_class")
        return real_own(self, p)

    with mock.patch.object(sm, "_read", read), \
            mock.patch.object(sm.Tracker, "classify", classify), \
            mock.patch.object(sm.Tracker, "_own_class", own_class):
        one()
    return got


def interleaved_us(passes: dict, rounds: int = COST_ROUNDS, samples: int = COST_SAMPLES) -> dict:
    """Each pass's CPU per process read, in microseconds, each round's: `passes`
    maps a name to (root, one pass over it); every round times each in turn,
    so a loaded machine moves them together. A name's list is its rounds', in
    order, so the rounds can be paired."""
    got = dict((k, []) for k in passes)
    for _ in range(rounds):
        for k, (root, one) in passes.items():
            n = len(os.listdir(root))
            start = time.process_time()
            for _ in range(samples):
                one()
            got[k].append((time.process_time() - start) / samples / n * 1e6)
    return got


def median_ratio(got: dict, top: str, bottom: str) -> float:
    """The median over rounds of `top`'s time over `bottom`'s in the same round."""
    return statistics.median(a / b for a, b in zip(got[top], got[bottom]))


def fixture_trial(root: Path, n: int) -> None:
    """`n` processes: a harness, its work, and the container's others."""
    p = FixtureProc(root)
    p.put(10, "harnessx", 1, rss=40000, hwm=45000, utime=5)
    p.put(11, "sh", 10, rss=2000, hwm=2000, utime=50)
    for i in range(n - 2):
        p.put(100 + i, "init" if i == 0 else "svc", 1, rss=1000, hwm=1000, utime=i)


def greedy_read(proc_root, want):
    """The review's F4: a read that opens each process's `status` and
    `cmdline` too."""
    procs = sm.read_procs(proc_root, want)
    for pid in procs:
        sm.read_memory(proc_root, pid)
        sm.parse_cmdline(sm._read(os.path.join(proc_root, str(pid), "cmdline"), binary=True) or b"")
    return procs


def stat_twice_read(proc_root, want):
    """A plant: every `stat` read again."""
    procs = sm.read_procs(proc_root, want)
    for pid in procs:
        sm.parse_stat(sm._read(os.path.join(proc_root, str(pid), "stat")) or "")
    return procs


def status_read(proc_root, want):
    """A plant: every process's `status` read, the container's own too."""
    procs = sm.read_procs(proc_root, want)
    for pid in procs:
        sm.read_memory(proc_root, pid)
    return procs


class ClassifiedTwice(sm.Tracker):
    """A plant: every sample sorts the processes into classes twice."""

    def classify(self, procs):
        super().classify(procs)
        return super().classify(procs)


@unittest.skipUnless(LINUX, "needs Linux's /proc")
class SamplerCost(unittest.TestCase):
    def setUp(self):
        t = tempfile.TemporaryDirectory()
        self.addCleanup(t.cleanup)
        self.small, self.large = Path(t.name, "n"), Path(t.name, "4n")
        fixture_trial(self.small, COST_N)
        fixture_trial(self.large, 4 * COST_N)

    def expected_reads(self, n):
        """One sample of `fixture_trial(n)`: a `stat` each; the harness's
        `cmdline`; the `status` of the harness and its work."""
        return {"stat": n, "cmdline": 1, "status": 2, "classify": 1, "own_class": n}

    def test_a_sample_reads_what_it_must_and_no_more(self):
        """Counted, not timed: the read count is the same on every host."""
        self.assertEqual(reads_of(sampler_pass(self.small)), self.expected_reads(COST_N))
        self.assertEqual(reads_of(sampler_pass(self.large)), self.expected_reads(4 * COST_N))

    def test_the_count_catches_each_plant_the_review_used(self):
        want = self.expected_reads(COST_N)
        plants = {
            "a status read of every process": sampler_pass(self.small, status_read),
            "the cmdline of every process": sampler_pass(self.small, want=lambda stat: True),
            "classification twice": sampler_pass(self.small, tracker=ClassifiedTwice),
            "a stat twice": sampler_pass(self.small, stat_twice_read),
            "F5 as the sampler": sampler_pass(self.small, want=lambda stat: True),
        }
        for name, one in plants.items():
            self.assertNotEqual(reads_of(one), want, name)

    def test_the_cost_per_process_read_is_bounded_and_does_not_grow_with_the_count(self):
        got = interleaved_us({"f5": (self.small, sampler_pass(self.small, want=lambda stat: True)),
                              "n": (self.small, sampler_pass(self.small)),
                              "4n": (self.large, sampler_pass(self.large))})
        # The sampler reads each `stat` once and tracks it: it costs less than
        # the same with every `cmdline` read, timed beside it, round by round.
        self.assertLess(median_ratio(got, "n", "f5"), SAMPLER_UNDER_F5, got)
        self.assertLess(min(got["n"]), FIXTURE_US_PER_PROC, "us per process, N processes")
        self.assertLess(min(got["4n"]), FIXTURE_US_PER_PROC, "us per process, 4N processes")
        # 4N costs no more than about 4x N plus a fixed part: its per-process
        # cost may not exceed N's by more than the fixed part spread over N.
        self.assertLess(min(got["4n"]), min(got["n"]) * 1.5 + 5.0, got)

    def test_reading_every_status_and_cmdline_each_sample_would_cost_more(self):
        """The bound has teeth: F4, which reads every process's `status` and
        `cmdline`, costs past it, timed the same way beside F5 (the
        every-`cmdline` read as the sampler's own is F5 itself: the count
        test above holds it)."""
        got = interleaved_us({"f5": (self.small, sampler_pass(self.small, want=lambda stat: True)),
                              "f4": (self.small, sampler_pass(self.small, greedy_read))})
        self.assertGreater(median_ratio(got, "f4", "f5"), SAMPLER_UNDER_F5, got)
        # And counted: F4 reads a `status` and a `cmdline` of every process.
        f4 = reads_of(sampler_pass(self.small, greedy_read))
        self.assertGreater(f4["status"] + f4["cmdline"], 2 * COST_N, f4)


# The sampler's share of a core at 250 ms in a PID namespace of this many
# sleeping processes (its own /proc, so procfs's generated reads are in it).
NAMESPACE_PROCS = 60
NAMESPACE_INTERVAL_MS = 250
# The bound is no constant of this host's (theseus-ufe5: against a floor timed
# apart, a mean of 8 s against a least, cache-heavy neighbours moved the share
# and not the floor): beside the real sampler, in the same namespace and over
# the same 8 s, an F5 variant of it runs (`F5_SAMPLER`: every process's
# `cmdline` read), and the real one's share is bounded against the variant's,
# so both see the same contention. The same 0.85 as the fixture's.
# The ways to make the namespace, the first that works: as any user (a user
# namespace maps them to root inside), then as root.
NAMESPACE_WAYS = (["--user", "--map-root-user"], [])
# The sampler with every process's `cmdline` wanted, run as a script.
F5_SAMPLER = ("import sys\nsys.path.insert(0, %r)\nimport sampler as sm\n"
              "sm.Tracker.wants_cmdline = lambda self, stat: True\nsys.exit(sm.main(sys.argv[1:]))\n"
              % str(HERE))


def namespace_way() -> tuple[list[str] | None, str | None]:
    """The `unshare` flags that make a PID namespace here, or None and why
    none can be made."""
    unshare = shutil.which("unshare")
    if unshare is None:
        return None, "no unshare on PATH"
    why = []
    for way in NAMESPACE_WAYS:
        try:
            r = subprocess.run([unshare, *way, "--pid", "--fork", "--mount-proc", "true"],
                               capture_output=True, text=True, timeout=30)
        except (OSError, subprocess.SubprocessError) as e:
            why.append("unshare %s failed: %s" % (" ".join(way + ["--pid"]), e))
            continue
        if r.returncode == 0:
            return way, None
        why.append("unshare %s refused: %s" % (" ".join(way + ["--pid"]), r.stderr.strip()))
    return None, "; ".join(why) + " (no user namespaces for this user, and not root)"


NAMESPACE, REFUSAL = namespace_way() if LINUX else (None, "not Linux")


@unittest.skipIf(REFUSAL, "a PID namespace can't be made: %s" % REFUSAL)
class InANamespace(unittest.TestCase):
    def test_the_samplers_share_of_a_core_is_bounded_at_a_set_count(self):
        t = tempfile.TemporaryDirectory()
        self.addCleanup(t.cleanup)
        out, f5_out = Path(t.name, "out"), Path(t.name, "f5")
        f5_script = Path(t.name, "f5_sampler.py")
        f5_script.write_text(F5_SAMPLER)
        flags = "--names none --interval-ms %d --max-secs 8" % NAMESPACE_INTERVAL_MS
        # Both run at once for the same 8 s: the namespace's shell is its init,
        # and ends it when both have (--kill-child: and should the test end
        # first, the namespace ends with it).
        script = ("for i in $(seq %d); do sleep 30 & done; "
                  "%s %s --out %s %s & real=$!; "
                  "%s %s --out %s %s & f5=$!; "
                  "wait $real; r=$?; wait $f5; f=$?; [ $r -eq 0 ] && [ $f -eq 0 ]"
                  % (NAMESPACE_PROCS, sys.executable, SAMPLER, out, flags,
                     sys.executable, f5_script, f5_out, flags))
        p = subprocess.Popen(["unshare", *NAMESPACE, "--pid", "--kill-child", "--mount-proc", "sh", "-c",
                              script])
        self.addCleanup(p.wait, 30)
        self.addCleanup(p.kill)
        self.assertEqual(p.wait(timeout=120), 0)
        summary = json.loads((out / sm.SUMMARY).read_text())
        f5 = json.loads((f5_out / sm.SUMMARY).read_text())
        self.assertEqual((summary["status"], f5["status"]), ("ok", "ok"))
        self.assertGreaterEqual(summary["samples"], 20)
        self.assertGreaterEqual(summary["classes"]["outside"]["processes"], NAMESPACE_PROCS)
        self.assertLess(summary["sampler"]["core_share"], f5["sampler"]["core_share"] * SAMPLER_UNDER_F5,
                        (summary["sampler"], f5["sampler"], NAMESPACE))


if __name__ == "__main__":
    unittest.main()
