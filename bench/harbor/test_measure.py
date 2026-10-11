"""Tests for what the three arms share in a container (theseus-n6p5,
theseus-sgpx, theseus-7gir.23). Standard library only.

    python3 -m unittest discover -s bench/harbor

The stop is run for real, on this host's `/proc`: stand-ins under a name of
their own (never `pi` or `claude`, which a developer's machine may run), their
children, one that ignores SIGTERM, and a bystander the script must leave.
"""

from __future__ import annotations

import os
import shlex
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


UNSHARE = shutil.which("unshare")


class Namespace(unittest.TestCase):
    """The stop by what the run started (theseus-8xp0), run for real in a PID namespace of its own
    (`unshare -rpf --mount-proc`), as a container is: its `/proc` holds only what the scenario starts, so the stop
    cannot reach a process of this host. Stand-ins under names of their own (never `claude` or `pi`).

    The driver shell is the namespace's PID 1, where an orphan is reparented. It starts a bystander, takes the
    snapshot, starts the agent, runs the stop, and prints what is left under the test's names."""

    def setUp(self):
        self.assertIsNotNone(UNSHARE, "these tests need `unshare` (util-linux) and user namespaces")
        self.tmp = tempfile.TemporaryDirectory()
        self.bin = Path(self.tmp.name)
        shutil.copy(SH, self.bin / "stopme-agent")
        shutil.copy(SLEEP, self.bin / "stopme-child")
        shutil.copy(SLEEP, self.bin / "stopme-pace")
        self.base = self.bin / "pids.before"

    def tearDown(self):
        self.tmp.cleanup()

    def scenario(self, agent: str, *, grace_s: int = 1, baseline: bool = True, after: str = "") -> dict[str, str]:
        """Run the driver; the survivors by pid, `comm cmdline`. `agent` is the agent's script (`$BIN` is the
        stand-ins' directory); `after` is shell run after the agent starts and before the stop."""
        b = self.bin.as_posix()
        stop = measure.stop_agent_script(("stopme-agent",), grace_s, self.base.as_posix() if baseline else None)
        driver = (
            f'BIN={b}; $BIN/stopme-child 300 & bystander=$!; '
            f'{measure.snapshot_script(self.base.as_posix())}\n'
            f'$BIN/stopme-agent -c {shlex.quote(agent.replace("$BIN", b))} & '
            'sleep 0.7; ' + after + f'sh -c {shlex.quote(stop)}; echo "stop exit $?"; sleep 1; '
            'for d in /proc/[0-9]*; do p=${d#/proc/}; c=$(cat $d/comm 2>/dev/null) || continue; '
            'st=$(cat $d/stat 2>/dev/null) || continue; case "${st##*) }" in Z*) continue;; esac; '
            'case "$c" in stopme-*) echo "alive $p $c $(tr "\\0" " " < $d/cmdline)";; esac; done; '
            'echo "bystander $bystander"'
        )
        r = subprocess.run([UNSHARE, "-rpf", "--mount-proc", "--kill-child", SH, "-c", driver],
                           capture_output=True, text=True, timeout=90)
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        self.assertIn("stop exit 0", r.stdout)
        self.out = r.stdout
        alive = {ln.split()[1]: " ".join(ln.split()[2:]) for ln in r.stdout.splitlines() if ln.startswith("alive ")}
        self.bystander = next(ln.split()[1] for ln in r.stdout.splitlines() if ln.startswith("bystander "))
        return alive

    def only_the_bystander(self, alive: dict[str, str]) -> None:
        self.assertEqual(list(alive), [self.bystander], f"left running: {alive}\n{self.out}")

    def test_a_command_orphaned_through_an_exited_subshell_is_stopped(self):
        """`(cmd &)`: the subshell exits and the command is reparented to PID 1, so no tree from the agent's name
        reaches it, and it runs on while the verifier does. One of one was left on main's script."""
        alive = self.scenario("( $BIN/stopme-child 301 & ); $BIN/stopme-child 302 & wait")
        self.only_the_bystander(alive)

    def test_a_process_that_ignores_sigterm_and_forks_during_the_grace_leaves_nothing(self):
        """The agent ignores SIGTERM and forks a child every 0.2 s (which ignore it too). Everything it forked
        before the stop, and during the 2 s of grace, is gone after it: main's script left the ones forked during
        the grace."""
        alive = self.scenario("trap '' TERM; while :; do $BIN/stopme-child 305 & $BIN/stopme-pace 0.2; done",
                              grace_s=2)
        self.only_the_bystander(alive)

    def test_what_a_stopped_forker_would_have_forked_is_never_forked(self):
        """The forker is stopped before it is signalled, and again before the SIGKILL (the SIGCONT let it run
        through the grace), so the children it forks in between are all found: none survives the stop."""
        alive = self.scenario("trap '' TERM; while :; do $BIN/stopme-child 305 & $BIN/stopme-pace 0.05; done",
                              grace_s=1)
        self.only_the_bystander(alive)

    def test_a_listed_process_is_left_and_a_recycled_pid_is_not(self):
        """The list holds `pid:starttime`: a pid in it with another start time is a recycled pid, another process,
        and is stopped; one listed with its own start time is left."""
        b = self.bin.as_posix()
        stop = measure.stop_agent_script(("stopme-agent",), 1, self.base.as_posix())
        driver = (
            f'BIN={b}; $BIN/stopme-child 300 & kept=$!; $BIN/stopme-child 303 & recycled=$!; '
            'for p in $kept $recycled; do st=$(cat /proc/$p/stat); rest=${st##*) }; set -- $rest; shift 19; '
            'if [ $p = $recycled ]; then echo "$p:0"; else echo "$p:$1"; fi; done '
            f'> {self.base}; echo "1:1" >> {self.base}; '
            f'sh -c {shlex.quote(stop)}; sleep 0.5; '
            'for p in $kept $recycled; do st=$(cat /proc/$p/stat 2>/dev/null) || { echo "$p gone"; continue; }; '
            'case "${st##*) }" in Z*) echo "$p gone";; *) echo "$p alive";; esac; done; '
            'echo "kept $kept recycled $recycled"'
        )
        r = subprocess.run([UNSHARE, "-rpf", "--mount-proc", "--kill-child", SH, "-c", driver],
                           capture_output=True, text=True, timeout=60)
        kept, recycled = (r.stdout.split("kept ")[1].split() + ["", ""])[:3:2]
        self.assertIn(f"{kept} alive", r.stdout, r.stdout + r.stderr)
        self.assertIn(f"{recycled} gone", r.stdout, r.stdout + r.stderr)

    def test_without_a_list_the_agent_is_found_by_name_as_before(self):
        alive = self.scenario("$BIN/stopme-child 302 & $BIN/stopme-child 303 & wait", baseline=False)
        self.only_the_bystander(alive)

    def test_a_list_that_cannot_be_read_falls_back_to_the_name(self):
        self.base.write_text("")  # an empty list is no list
        b = self.bin.as_posix()
        stop = measure.stop_agent_script(("stopme-agent",), 1, self.base.as_posix())
        driver = (f'BIN={b}; $BIN/stopme-child 300 & by=$!; $BIN/stopme-agent -c "$BIN/stopme-child 302 & wait" & '
                  f'a=$!; sleep 0.5; sh -c {shlex.quote(stop)}; sleep 0.5; '
                  'st=$(cat /proc/$by/stat) && echo "bystander ${st##*) }"; '
                  'cat /proc/$a/stat >/dev/null 2>&1 && case "$(cat /proc/$a/stat)" in *") Z"*) echo agent-gone;; '
                  '*) echo agent-alive;; esac || echo agent-gone')
        r = subprocess.run([UNSHARE, "-rpf", "--mount-proc", "--kill-child", SH, "-c", driver],
                           capture_output=True, text=True, timeout=60)
        self.assertIn("agent-gone", r.stdout, r.stdout + r.stderr)
        self.assertNotIn("bystander Z", r.stdout)

    def test_the_snapshot_lists_every_process_with_its_start_time(self):
        r = subprocess.run([UNSHARE, "-rpf", "--mount-proc", "--kill-child", SH, "-c",
                            f"sleep 5 & {measure.snapshot_script(self.base.as_posix())}"],
                           capture_output=True, text=True, timeout=60)
        self.assertEqual(r.returncode, 0, r.stderr)
        lines = self.base.read_text().split()
        self.assertGreaterEqual(len(lines), 2, lines)
        for ln in lines:
            pid, _, start = ln.partition(":")
            self.assertTrue(pid.isdigit() and start.isdigit(), ln)
        self.assertIn("1", [ln.split(":")[0] for ln in lines], "the namespace's PID 1")
        self.assertFalse((self.bin / "pids.before.tmp").exists())

    def test_the_arms_keep_the_list_in_their_state_dir_and_pass_it_to_the_stop(self):
        here = Path(__file__).resolve().parent
        for name in ("claude_code_agent.py", "measured.py", "pi_agent.py"):
            src = (here / name).read_text()
            self.assertIn("measure.snapshot_script(baseline)", src, name)
            self.assertIn("measure.BASELINE_FILE", src, name)
            self.assertIn("baseline=baseline", src, name)
        self.assertEqual(measure.BASELINE_FILE, "pids.before")


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
                                   "version_asked": "2.1.290", "build_commit": None})


if __name__ == "__main__":
    unittest.main()
