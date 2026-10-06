#!/usr/bin/env python3
"""What an agent's harness costs in a task's container, apart from the work
it runs (theseus-7gir.12).

    nice -n 19 python3 sampler.py --out DIR --names theseus,theseusd \\
        --wrapper-arg job-wrapper --interval-ms 250

Every interval it reads `/proc` and sorts each process into a class:

- **harness**: its executable's name (`/proc/<pid>/comm`) is in `--names`,
  the arm's harness processes, taken as data;
- **wrapper**: a harness process whose first argument is a `--wrapper-arg`
  (Theseus's job wrapper, `theseusd job-wrapper …`), kept apart. theseusd
  starts it through `/proc/self/exe`, so its comm is `exe`: such a process
  is named by its `argv[0]` instead;
- **work**: whatever descends from a harness or wrapper process and is not
  one, and stays work once it is (an orphan reparented to the container's
  init is still the command's);
- **outside**: the rest, the container's own (its init, Harbor's exec
  shells, this sampler), left out.

**CPU** is each process's `utime` and `stime` from `/proc/<pid>/stat`, in
clock ticks (10 ms at the usual 100 Hz): the deltas between samples go to the
process's class. A child that lived and was reaped between two samples is
never seen, but its time lands in its parent's `cutime` and `cstime`: what
those grow by, less the last-seen time of the children that vanished (and of
theirs, which they reaped), goes to work, or to the harness when every
process that vanished under it was harness (the CLI reaping its daemon).
A sample is not one instant: a parent whose `stat` was read just before it
reaped a child that is gone by the child's own read has a `cutime` that
holds the child only in the next sample, so the vanished wait for it once
(theseus-99by; matched at once, a loaded run's last interval of work went to
the reaper's class). theseus-sim's procfs.rs sums `schedstat` per thread to
the nanosecond instead; that misses a thread that has exited and has no
count of reaped children, which is what this needs.

**Memory** is `VmRSS` summed over a class's processes in one sample, its
peak the largest such sum, beside the largest single `VmHWM`.

**Totals.** Where the container's cgroup v2 `cpu.stat` reads, its
`usage_usec` from start to stop is the container's whole CPU, so a command
shorter than an interval still counts: the harness's share comes from the
samples, and the work is the rest (less the outside class, which holds this
sampler's own CPU from its first sample; `efficiency.py` does the sum). What
the sampler spent before its first sample, and after its last summary, is in
the work. `memory.peak`, where it reads, is the
container's peak since it was made, page cache included, so it is recorded
beside the samples, not used for them.

**What it misses:** a harness child that lives less than an interval is
counted as work; a reaped child's last partial interval goes to its
reaper's class; a child that a process outside the tree reaps (an orphan the
container's init collects) loses its last interval; a zombie's time counts
once it is reaped; a peak of summed RSS between two samples is not seen
(each process's own `VmHWM` is); a child read alive whose parent is read
after reaping it (a child with a lower pid than its parent's, once pids wrap)
is counted twice for its last interval.

**Output** in `--out`: `sampler.jsonl`, one line a sample (cumulative CPU,
summed RSS, and processes by class); `sampler.json`, the summary, rewritten
every few seconds as `running` and last as `ok` (or `failed`, with why);
`sampler.ready` after the first sample; `sampler.done` once the summary is
final. A SIGTERM or SIGINT stops it: one last sample, the summary, exit 0.

Standard library only, and Python 3.6 or later, since it runs on the task's
image, not on the host. The parsers take text, so tests read fixture trees.
"""

import json
import os
import signal
import sys
import time

CLASSES = ("harness", "wrapper", "work", "outside")
TREE = ("harness", "wrapper", "work")
SUMMARY = "sampler.json"
SAMPLES = "sampler.jsonl"
READY = "sampler.ready"
DONE = "sampler.done"
NONE = frozenset()
# The comm of a process started through /proc/self/exe, as theseusd starts
# its job wrappers (`theseusd job-wrapper …` in its command line).
EXE = "exe"


def parse_stat(text):
    """A `/proc/<pid>/stat` line as a dict: `pid`, `comm`, `ppid`, `utime`,
    `stime`, `cutime`, `cstime` (clock ticks) and `starttime`. The name sits
    in parentheses and may hold spaces and parentheses, so the fields are
    counted from the last `)`. None when it does not read as one."""
    try:
        open_at, close_at = text.index("("), text.rindex(")")
        words = text[close_at + 1:].split()
        # After the name: state is field 3, so ppid (4) is words[1], utime
        # (14) words[11], then stime, cutime and cstime, and starttime (22)
        # words[19].
        return {
            "pid": int(text[:open_at]),
            "comm": text[open_at + 1:close_at],
            "ppid": int(words[1]),
            "utime": int(words[11]),
            "stime": int(words[12]),
            "cutime": int(words[13]),
            "cstime": int(words[14]),
            "starttime": int(words[19]),
        }
    except (ValueError, IndexError):
        return None


def parse_status(text):
    """`VmRSS` and `VmHWM` of a `/proc/<pid>/status`, in kB. A line that is
    absent reads as 0: a kernel thread has neither."""
    rss = hwm = 0
    for line in text.splitlines():
        key, _, rest = line.partition(":")
        if key in ("VmRSS", "VmHWM"):
            words = rest.split()
            n = int(words[0]) if words and words[0].isdigit() else 0
            if key == "VmRSS":
                rss = n
            else:
                hwm = n
    return rss, hwm


def parse_cmdline(data):
    """A `/proc/<pid>/cmdline`'s arguments: NUL-separated bytes."""
    return [a.decode("utf-8", "replace") for a in data.split(b"\0") if a]


def parse_cpu_stat(text):
    """A cgroup v2 `cpu.stat`'s `usage_usec`, or None."""
    for line in text.splitlines():
        words = line.split()
        if len(words) == 2 and words[0] == "usage_usec" and words[1].isdigit():
            return int(words[1])
    return None


def parse_cgroup_path(text):
    """The cgroup v2 path in a `/proc/<pid>/cgroup` (its `0::` line), or None."""
    for line in text.splitlines():
        if line.startswith("0::"):
            return line[3:].strip() or "/"
    return None


def _read(path, binary=False):
    # os.open and one read: a sample reads a few hundred small files, and
    # open()'s buffered text layer costs more than the read.
    try:
        fd = os.open(path, os.O_RDONLY)
    except OSError:
        return None
    try:
        chunks = []
        while True:
            b = os.read(fd, 65536)
            if not b:
                break
            chunks.append(b)
    except OSError:
        return None
    finally:
        os.close(fd)
    data = b"".join(chunks)
    return data if binary else data.decode("utf-8", "replace")


def find_cgroup(proc_root="/proc", sys_root="/sys/fs/cgroup"):
    """The directory of this process's cgroup v2, where its `cpu.stat`
    reads: under the unified mount, or a hybrid's `unified/`. None when
    there is none (cgroup v1 alone, a path this namespace cannot see, or the
    machine's root)."""
    path = parse_cgroup_path(_read(os.path.join(proc_root, "self", "cgroup")) or "")
    if path is None:
        return None
    rel = path.lstrip("/")
    for base in (sys_root, os.path.join(sys_root, "unified")):
        d = os.path.join(base, rel) if rel else base
        # The machine's own root cgroup has no `cgroup.type`: a `0::/` seen
        # from a host (or a container sharing its cgroup namespace) is the
        # whole machine, never the container's.
        if not os.path.exists(os.path.join(d, "cgroup.type")):
            continue
        if parse_cpu_stat(_read(os.path.join(d, "cpu.stat")) or "") is not None:
            return d
    return None


def read_cgroup(d):
    """A cgroup's CPU so far, in µs, and its `memory.peak` in kB (None
    where it does not read)."""
    usage = parse_cpu_stat(_read(os.path.join(d, "cpu.stat")) or "")
    peak = (_read(os.path.join(d, "memory.peak")) or "").strip()
    return usage, (int(peak) // 1024 if peak.isdigit() else None)


def read_procs(proc_root, want_cmdline):
    """Every process under `proc_root` now, by pid: its stat fields, with
    `argv` where `want_cmdline(stat)` asks for it. A process that exits
    between the listing and a read is left out, or keeps what was read."""
    procs = {}
    try:
        names = os.listdir(proc_root)
    except OSError:
        return procs
    for name in names:
        if name.isdigit():
            stat = parse_stat(_read(os.path.join(proc_root, name, "stat")) or "")
            if stat is not None:
                procs[stat["pid"]] = stat
    for stat in procs.values():
        d = os.path.join(proc_root, str(stat["pid"]))
        if want_cmdline(stat):
            stat["argv"] = parse_cmdline(_read(os.path.join(d, "cmdline"), binary=True) or b"")
    return procs


def read_memory(proc_root, pid):
    """A process's `VmRSS` and `VmHWM` now, in kB (0 and 0 once it is gone)."""
    return parse_status(_read(os.path.join(proc_root, str(pid), "status")) or "")


class Tracker:
    """The classes' CPU and memory over a run, one sample at a time.

    `observe(procs)` takes `read_procs`'s map. The first call is the
    baseline: an outside process's time until then is not counted, and a
    harness process's (or a descendant's) is, all of it, since it was
    started for the agent."""

    def __init__(self, names, wrapper_args=(), ignore_pids=(), proc_root="/proc"):
        self.names = set(names)
        self.wrapper_args = set(wrapper_args)
        self.ignore = set(ignore_pids)
        self.proc_root = proc_root
        self.known = {}  # (pid, starttime) -> {"cls", "ppid", "own", "child"}
        self.cpu = dict((c, 0) for c in CLASSES)  # ticks
        self.peak_rss = dict((c, 0) for c in CLASSES)  # kB: the largest sum in one sample
        self.peak_hwm = dict((c, 0) for c in CLASSES)  # kB: the largest single process
        self.max_procs = dict((c, 0) for c in CLASSES)
        self.seen = dict((c, set()) for c in CLASSES)
        self.last_rss = dict((c, 0) for c in CLASSES)
        self.last_procs = dict((c, 0) for c in CLASSES)
        self.samples = 0

    def wants_cmdline(self, stat):
        return stat["comm"] in self.names or stat["comm"] == EXE

    @staticmethod
    def name_of(p):
        """A process's executable name: its comm, or, for one started
        through /proc/self/exe (whose comm is `exe`), its argv[0]'s base."""
        argv = p.get("argv") or []
        if p["comm"] == EXE and argv:
            return os.path.basename(argv[0])
        return p["comm"]

    def classify(self, procs):
        """Each pid's class now. A harness name wins (a wrapper by its first
        argument); then a process that was work stays work; then a
        descendant of a harness or wrapper process is work; the rest is
        outside."""
        out = {}
        for pid in procs:
            chain = []
            p = procs.get(pid)
            while p is not None and p["pid"] not in out:
                c = self._own_class(p)
                if c is not None:
                    out[p["pid"]] = c
                    break
                chain.append(p["pid"])
                parent = p["ppid"]
                p = procs.get(parent) if parent != p["pid"] and parent not in chain else None
            above = out.get(p["pid"], "outside") if p is not None else "outside"
            for q in reversed(chain):
                out[q] = "work" if above in TREE else "outside"
                above = out[q]
        return out

    def _own_class(self, p):
        """A process's class from itself alone, or None when its parent says."""
        if p["pid"] in self.ignore:
            return "outside"
        if self.name_of(p) in self.names:
            argv = p.get("argv") or []
            return "wrapper" if len(argv) > 1 and argv[1] in self.wrapper_args else "harness"
        was = self.known.get((p["pid"], p["starttime"]))
        if was is not None and was["cls"] == "work":
            return "work"
        return None

    def observe(self, procs):
        first = self.samples == 0
        classes = self.classify(procs)
        now = dict(((pid, p["starttime"]), p) for pid, p in procs.items())
        gone = [k for k in self.known if k not in now]
        gone_by_parent = {}
        for k in gone:
            gone_by_parent.setdefault(self.known[k]["ppid"], []).append(k)

        def reaped(keys, walked, depth=0):
            """What the vanished `keys` were last seen to have used, with
            the vanished children each of them reaped in turn, and their
            classes: a parent's `cutime` holds all of it. Each key is added
            to `walked`."""
            used, classes = 0, set()
            for k in keys:
                v = self.known[k]
                walked.add(k)
                used += v["own"] + v["child"]
                classes.add(v["cls"])
                below = gone_by_parent.get(k[0], []) if depth < 64 else []
                u, c = reaped([b for b in below if b != k and b not in walked], walked, depth + 1)
                used, classes = used + u, classes | c
            return used, classes
        carried = set()
        rss = dict((c, 0) for c in CLASSES)
        count = dict((c, 0) for c in CLASSES)
        for key, p in now.items():
            c = classes[p["pid"]]
            own = p["utime"] + p["stime"]
            child = p["cutime"] + p["cstime"]
            was = self.known.get(key)
            if was is not None:
                d_own, d_child = own - was["own"], child - was["child"]
            elif first and c == "outside":
                d_own = d_child = 0
            else:
                d_own, d_child = own, child
            self.cpu[c] += max(d_own, 0)
            # What its reaped children used that no sample saw: their time
            # since their last sample, and children never seen at all.
            below = gone_by_parent.get(p["pid"]) if was is not None else None
            walked = set() if below else NONE
            seen_used, vanished = reaped(below, walked) if below else (0, NONE)
            avail = d_child + was["bank"] if was is not None else d_child
            bank = 0
            if seen_used > avail and any(not self.known[k].get("carried") for k in walked):
                # A sample is not one instant: this parent's `stat` was read
                # before it reaped them, and they were gone by their own
                # read. Its `cutime` holds them next sample: they wait for
                # it once, with what it grew by now.
                carried |= walked
                bank, unseen = avail, 0
            else:
                unseen = avail - seen_used
            if unseen > 0:
                if vanished == set(["harness"]):
                    to = "harness"
                elif c in TREE or vanished & set(TREE):
                    to = "work"
                else:
                    to = "outside"
                self.cpu[to] += unseen
            self.known[key] = {"cls": c, "ppid": p["ppid"], "own": own, "child": child, "bank": bank}
            if c in TREE:
                # The container's own are never read: memory is the tree's.
                r, h = read_memory(self.proc_root, p["pid"])
                rss[c] += r
                self.peak_hwm[c] = max(self.peak_hwm[c], h)
            count[c] += 1
            self.seen[c].add(key)
        for k in gone:
            if k in carried:
                self.known[k]["carried"] = True
            else:
                del self.known[k]
        for c in CLASSES:
            self.peak_rss[c] = max(self.peak_rss[c], rss[c])
            self.max_procs[c] = max(self.max_procs[c], count[c])
        self.last_rss, self.last_procs = rss, count
        self.samples += 1

    def classes(self, tick_hz):
        return dict(
            (c, {
                "cpu_s": round(self.cpu[c] / float(tick_hz), 3),
                "peak_rss_kb": self.peak_rss[c],
                "peak_hwm_kb": self.peak_hwm[c],
                "max_procs": self.max_procs[c],
                "processes": len(self.seen[c]),
            })
            for c in CLASSES
        )


def _write_json(path, value):
    tmp = path + ".tmp"
    with open(tmp, "w") as f:
        json.dump(value, f, sort_keys=True)
        f.write("\n")
    os.replace(tmp, path)


def _touch(path):
    open(path, "w").close()


class Sampler:
    """The loop: a sample every `interval_ms`, its line, and the summary."""

    def __init__(self, out, names, wrapper_args=(), interval_ms=250, proc_root="/proc",
                 sys_root="/sys/fs/cgroup", max_secs=6 * 3600, summary_every_s=5.0):
        self.out = out
        self.interval_ms = max(int(interval_ms), 10)
        self.proc_root = proc_root
        self.max_secs = max_secs
        self.summary_every = summary_every_s
        self.tick_hz = os.sysconf("SC_CLK_TCK")
        self.names, self.wrapper_args = list(names), list(wrapper_args)
        self.tracker = Tracker(names, wrapper_args, ignore_pids=(os.getpid(),), proc_root=proc_root)
        self.stopping = False
        self.lines = None
        self.cgroup = find_cgroup(proc_root, sys_root)
        self.cg_start = read_cgroup(self.cgroup)[0] if self.cgroup else None
        self.started = time.time()
        self.cpu0 = time.process_time()

    def stop(self, *_):
        self.stopping = True

    def sample(self):
        t = self.tracker
        t.observe(read_procs(self.proc_root, t.wants_cmdline))
        line = {
            "t_ms": int((time.time() - self.started) * 1000),
            "cpu_s": dict((c, round(t.cpu[c] / float(self.tick_hz), 3)) for c in TREE),
            "rss_kb": dict((c, t.last_rss[c]) for c in TREE),
            "procs": dict((c, t.last_procs[c]) for c in TREE),
        }
        self.lines.write(json.dumps(line, sort_keys=True) + "\n")

    def summary(self, status, reason=None):
        wall = time.time() - self.started
        own = time.process_time() - self.cpu0
        cgroup = None
        if self.cgroup:
            usage, peak = read_cgroup(self.cgroup)
            cpu = None
            if usage is not None and self.cg_start is not None:
                cpu = round((usage - self.cg_start) / 1e6, 3)
            cgroup = {"path": self.cgroup, "cpu_s": cpu, "memory_peak_kb": peak}
        return {
            "status": status,
            "reason": reason,
            "interval_ms": self.interval_ms,
            "samples": self.tracker.samples,
            "started_unix_ms": int(self.started * 1000),
            "wall_s": round(wall, 3),
            "tick_hz": self.tick_hz,
            "names": self.names,
            "wrapper_args": self.wrapper_args,
            "classes": self.tracker.classes(self.tick_hz),
            "cgroup": cgroup,
            "sampler": {
                "cpu_s": round(own, 3),
                "core_share": round(own / wall, 5) if wall > 0 else None,
            },
            "python": "%d.%d.%d" % tuple(sys.version_info[:3]),
        }

    def run(self):
        summary_path = os.path.join(self.out, SUMMARY)
        status, reason = "ok", None
        try:
            with open(os.path.join(self.out, SAMPLES), "w") as self.lines:
                self.sample()
                _touch(os.path.join(self.out, READY))
                interval = self.interval_ms / 1000.0
                last_summary = time.monotonic()
                deadline = time.monotonic() + self.max_secs
                next_at = time.monotonic() + interval
                while not self.stopping:
                    if time.monotonic() >= deadline:
                        reason = "stopped at its own limit (%d s)" % self.max_secs
                        break
                    wait = next_at - time.monotonic()
                    if wait > 0:
                        time.sleep(wait)
                    if self.stopping:
                        break
                    # A late wake (the CPU is busy, and this runs at nice 19)
                    # skips the samples it missed rather than bunching them.
                    next_at = max(next_at + interval, time.monotonic())
                    self.sample()
                    if time.monotonic() - last_summary >= self.summary_every:
                        self.lines.flush()
                        _write_json(summary_path, self.summary("running"))
                        last_summary = time.monotonic()
                self.sample()
        except Exception as e:  # never the trial's failure: say why, and stop
            status, reason = "failed", "%s: %s" % (type(e).__name__, e)
        try:
            _write_json(summary_path, self.summary(status, reason))
        finally:
            _touch(os.path.join(self.out, DONE))
        return 0


INTERVAL_MS = 250  # the default; BENCH_SAMPLE_MS sets it (bench/README.md)


def start_script(sampler, out, state, names, wrapper_args=(), interval_ms=INTERVAL_MS):
    """The POSIX shell that starts the sampler, at nice 19, before an agent's
    command, and waits (at most 3 s) for its first sample, so the agent's
    first moments are in it. An image with no python3 (or one older than 3.6)
    gets a summary that says `unavailable` and why, and the trial goes on:
    nothing here fails. The sampler's pid goes to `<state>/sampler.pid`."""
    import shlex

    q = shlex.quote
    sp, o, st = q(sampler), q(out), q(state)
    args = "--out %s --names %s --interval-ms %d" % (o, q(",".join(names)), int(interval_ms))
    for a in wrapper_args:
        args += " --wrapper-arg " + q(a)
    unavailable = (
        "printf '{\"status\": \"unavailable\", \"reason\": \"%%s\"}\\n' \"$why\" > %s/%s"
        % (o, SUMMARY)
    )
    return (
        "mkdir -p %s %s 2>/dev/null; rm -f %s/%s %s/%s %s/sampler.pid; why=; " % (o, st, o, DONE, o, READY, st)
        + "if ! command -v python3 >/dev/null 2>&1; then why='no python3 on PATH'; "
        "elif ! python3 -c 'import sys; sys.exit(sys.version_info < (3, 6))' >/dev/null 2>&1; "
        "then why=\"python3 is older than 3.6 ($(python3 -V 2>&1))\"; fi; "
        'if [ -n "$why" ]; then %s; else ' % unavailable
        + "nice=; command -v nice >/dev/null 2>&1 && nice='nice -n 19'; "
        "$nice python3 %s %s </dev/null >/dev/null 2>>%s/sampler.log & echo $! > %s/sampler.pid; "
        % (sp, args, o, st)
        + "i=0; while [ ! -e %s/%s ] && [ $i -lt 30 ]; do sleep 0.1 2>/dev/null || break; "
        "i=$((i+1)); done; fi" % (o, READY)
    )


def stop_script(out, state, wait_tenths=50):
    """The shell that stops the sampler: a SIGTERM, then a wait (at most
    `wait_tenths` tenths of a second) for its final summary. A sampler that
    is not running, or never started, is no error."""
    import shlex

    o, st = shlex.quote(out), shlex.quote(state)
    return (
        'spid=$(cat %s/sampler.pid 2>/dev/null); if [ -n "$spid" ]; then '
        'kill -TERM "$spid" 2>/dev/null; i=0; '
        "while [ ! -e %s/%s ] && [ $i -lt %d ]; do sleep 0.1 2>/dev/null || break; i=$((i+1)); done; "
        "rm -f %s/sampler.pid; fi" % (st, o, DONE, int(wait_tenths), st)
    )


def main(argv):
    import argparse

    ap = argparse.ArgumentParser(description="The harness's CPU and memory, apart from its work.")
    ap.add_argument("--out", required=True)
    ap.add_argument("--names", required=True, help="the harness's process names, comma-separated")
    ap.add_argument("--wrapper-arg", action="append", default=[],
                    help="a harness process with this first argument is a job wrapper")
    ap.add_argument("--interval-ms", type=int, default=250)
    ap.add_argument("--max-secs", type=int, default=6 * 3600)
    ap.add_argument("--proc", default="/proc")
    ap.add_argument("--cgroup-root", default="/sys/fs/cgroup")
    a = ap.parse_args(argv)
    os.makedirs(a.out, exist_ok=True)
    for name in (DONE, READY):
        try:
            os.unlink(os.path.join(a.out, name))
        except OSError:
            pass
    try:
        s = Sampler(a.out, [n for n in a.names.split(",") if n], a.wrapper_arg, a.interval_ms,
                    proc_root=a.proc, sys_root=a.cgroup_root, max_secs=a.max_secs)
    except Exception as e:
        _write_json(os.path.join(a.out, SUMMARY),
                    {"status": "failed", "reason": "%s: %s" % (type(e).__name__, e)})
        _touch(os.path.join(a.out, READY))
        _touch(os.path.join(a.out, DONE))
        return 0
    signal.signal(signal.SIGTERM, s.stop)
    signal.signal(signal.SIGINT, s.stop)
    return s.run()


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
