"""What the driver reads from /proc (theseus-qy2a): the survey's processes, by
their working directory and command line, never by a name pattern; and the
client's resident set."""

import os
import signal
from pathlib import Path


def _cmdline(pid):
    try:
        return Path(f"/proc/{pid}/cmdline").read_bytes().split(b"\0")
    except OSError:
        return []


def survey_pids(project):
    """Every live process running `slow_survey.py` in `project` (its cwd)."""
    project = str(Path(project).resolve())
    out = []
    for d in Path("/proc").iterdir():
        if not d.name.isdigit():
            continue
        args = _cmdline(d.name)
        if not any(a.endswith(b"slow_survey.py") for a in args):
            continue
        try:
            if os.readlink(d / "cwd") != project:
                continue
            if Path(d / "stat").read_text().split(")")[-1].split()[0] == "Z":
                continue
        except OSError:
            continue
        out.append(int(d.name))
    return out


def stop_all(pids):
    for pid in pids:
        try:
            os.kill(pid, signal.SIGKILL)
        except OSError:
            pass


def children(pid):
    try:
        return [int(c) for c in Path(f"/proc/{pid}/task/{pid}/children").read_text().split()]
    except OSError:
        return []


def rss_kb(pid):
    try:
        for line in Path(f"/proc/{pid}/status").read_text().splitlines():
            if line.startswith("VmRSS:"):
                return int(line.split()[1])
    except OSError:
        pass
    return 0


def comm(pid):
    try:
        return Path(f"/proc/{pid}/comm").read_text().strip()
    except OSError:
        return ""


def client_rss(shell_pid):
    """(pid, VmRSS kB, comm) of the program in front of the pane's shell: its
    first child that is not the survey; (None, 0, '') when the shell is
    alone."""
    for pid in children(shell_pid):
        if any(a.endswith(b"slow_survey.py") for a in _cmdline(pid)):
            continue
        return pid, rss_kb(pid), comm(pid)
    return None, 0, ""
