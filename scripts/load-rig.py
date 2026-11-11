#!/usr/bin/env python3
"""CPU load for a measurement or a test, that cannot outlive the process that asked for it (theseus-1eemp, B5).

    load-rig.py [--spinners N] [--cpus LIST] [--nice N] [--settle S] [--max-secs S] [--] [COMMAND...]

Starts N busy loops (default 8), optionally pinned to CPUS ("0-3") and niced, waits SETTLE seconds (default 11,
for PSI's avg10 to catch up), then runs COMMAND in the foreground (none: it prints the `some` line of
/proc/pressure/cpu and ends). Its exit status is COMMAND's.

WHY IT EXISTS. On 2026-11-10 a builder's inline `for i in 1..8; do (while :; do :; done) & done; sleep 11; ...; exit 0`
left eight immortal spinners behind when its shell ended. They ran 11 h 48 m, held `/proc/pressure/cpu some avg10` at
94 % and so the self loop's start guard shut, and nothing said so. A comment asking the next operator to clean up is
not a fix: the load itself must not be able to outlive its owner. Every load shape in the repo's measurements starts
here, so there is one place to get that right.

HOW IT CANNOT ORPHAN. Each spinner (and COMMAND) is a child of this process with PR_SET_PDEATHSIG = SIGKILL, set
in the child before it does anything: the KERNEL kills it when this process dies, by any path, including SIGKILL,
the one a `trap` cannot catch (a tool's timeout sends it). The alternatives, and why not: a trap or a process
group is user-space code that never runs on SIGKILL or a crash; a cgroup survives them but needs systemd or
root and does not exist in a CI container. Two more lines of defence do the ordinary cases promptly: SIGTERM,
SIGINT and SIGHUP end this process through the same cleanup as a normal exit, which kills and REAPS every child; and
every spinner stops itself after MAX-SECS (default 300) whatever happens, so the worst case of a bug in the first
two is five minutes, not twelve hours. The checks that hold all of it are scripts/tests/test_load_rig.py's.
"""
import argparse
import ctypes
import os
import signal
import subprocess
import sys
import time

PR_SET_PDEATHSIG = 1


def die_with_parent(parent):
    """In a child, before it does anything: SIGKILL me when `parent` dies. False when it already has."""
    if ctypes.CDLL(None, use_errno=True).prctl(PR_SET_PDEATHSIG, int(signal.SIGKILL), 0, 0, 0) != 0:
        raise OSError(ctypes.get_errno(), "prctl(PR_SET_PDEATHSIG)")
    # The parent may have died between the fork and the prctl, in which case no signal will ever come.
    return os.getppid() == parent


def spin(parent, cpus, nice, max_secs):
    """The child's body; never returns."""
    try:
        if not die_with_parent(parent):
            os._exit(0)
        if cpus:
            os.sched_setaffinity(0, cpus)
        if nice:
            os.nice(nice)
        end = time.monotonic() + max_secs
        while time.monotonic() < end:
            for _ in range(200_000):
                pass
    finally:
        os._exit(0)


def parse_cpus(text):
    cpus = set()
    for part in text.split(","):
        lo, _, hi = part.partition("-")
        cpus.update(range(int(lo), int(hi or lo) + 1))
    return cpus


def stop_all(pids):
    """Kill every child and wait for each: nothing is left running or zombied when this returns."""
    for pid in pids:
        try:
            os.kill(pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
    for pid in pids:
        try:
            os.waitpid(pid, 0)
        except ChildProcessError:
            pass
    pids.clear()


def main(argv):
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--spinners", type=int, default=8)
    ap.add_argument("--cpus", type=parse_cpus, default=None, help='pin the spinners, e.g. "0-3"')
    ap.add_argument("--nice", type=int, default=0)
    ap.add_argument("--settle", type=float, default=11.0, help="seconds before COMMAND, for PSI's avg10")
    ap.add_argument("--max-secs", type=float, default=300.0, help="each spinner's own deadline")
    ap.add_argument("command", nargs="*")
    args = ap.parse_args(argv)

    me = os.getpid()
    pids = []

    def on_signal(signum, _frame):
        # Raise into the `finally`: one cleanup path for a normal end, an error and a signal.
        raise SystemExit(128 + signum)

    for s in (signal.SIGTERM, signal.SIGINT, signal.SIGHUP):
        signal.signal(s, on_signal)
    try:
        for _ in range(args.spinners):
            pid = os.fork()
            if pid == 0:
                spin(me, args.cpus, args.nice, args.max_secs)
            pids.append(pid)
        time.sleep(args.settle)
        if not args.command:
            with open("/proc/pressure/cpu") as f:
                print(f.readline().rstrip())
            return 0
        child = subprocess.Popen(args.command, preexec_fn=lambda: die_with_parent(me) or os._exit(0))
        pids.append(child.pid)
        rc = child.wait()
        pids.remove(child.pid)
        return rc
    finally:
        stop_all(pids)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
