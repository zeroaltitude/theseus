"""The head-to-head speed bench (theseus-7gir.13): Theseus against Claude Code on one stand-in model, row by row.

    python3 bench/h2h/run.py --bin-dir target/release-thin --runs 10 --out /tmp/h2h
    python3 bench/h2h/report.py /tmp/h2h/run.json

Both arms talk to `theseus-sim fake-model` (bench/h2h/standin.py), whose fixed first-byte delay and chunking make the
model's own time the same for both, so what differs is the harness. Every run of every arm starts fresh: a new
scratch daemon (Theseus) or a new scratch home (Claude Code), and the work directory's notes.txt written again. The
arms alternate, ABBA by pairs (oneshot.order). The rows (README.md says each one's stamps):

    T1     start to a ready prompt (interactive)
    T2     Enter to the request's arrival (interactive); spawn to the request (one-shot)
    T3     the stand-in's first byte to the reply's first word on screen (interactive, one-shot)
    T4     per tool call, the response's last byte to the next request's arrival (read, shell; task3's edit)
    T5     a long session resumed: to its last reply and a ready prompt (interactive); to the request (one-shot)
    T6     CPU per turn (interactive: the surface and the daemon; one-shot: the process and its children)
    T7     idle CPU and memory, the surface open and nothing asked
    T8     T1 to T4 again with a neighbour writing and fsyncing beside them
    task3  read, edit, run a command, answer: Enter (or spawn) to the reply's end
    first_request_bytes  the first request's bytes

Every raw sample goes to `<out>/run.json`, with the machine (nproc, load averages before and after, kernel), the
versions, and the stand-in's settings. Without Claude Code (absent, or --theseus-only), the Theseus arm runs alone
and run.json says so.

Standard library only.
"""

from __future__ import annotations

import argparse
import importlib.util
import json
import os
import platform
import shutil
import subprocess
import sys
import time
import uuid
from datetime import datetime
from pathlib import Path
from typing import Any

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import claude_code as cc  # noqa: E402
import joins  # noqa: E402
import oneshot  # noqa: E402
import standin  # noqa: E402
import theseus as th  # noqa: E402


def _load(name: str, path: Path):
    """A module by its path, under a name of its own: bench/h2h/pty.py is not the standard library's pty."""
    if name in sys.modules:
        return sys.modules[name]
    spec = importlib.util.spec_from_file_location(name, path)
    mod = importlib.util.module_from_spec(spec)
    sys.modules[name] = mod
    spec.loader.exec_module(mod)
    return mod


ptyd = _load("h2h_pty", HERE / "pty.py")

ROWS = ["T1", "T2", "T3", "T4", "T5", "T6", "T7", "T8", "task3", "first_request_bytes"]
ARMS = [th.ARM, cc.ARM]


def marker() -> str:
    return "M" + uuid.uuid4().hex[:7].upper()


# ------------------------------------------------------------------ the arms

class Arm:
    """What the runner asks of a harness: a fresh start, its surfaces, and what to sample."""
    name = ""

    def fresh(self, root: Path) -> None: ...
    def surface(self, log: Path, resume: str | None = None) -> ptyd.Pty: ...
    def ready(self, p: ptyd.Pty, timeout: float) -> int | None: ...
    def before_type(self, p: ptyd.Pty) -> None: ...
    def oneshot_argv(self, prompt: str, session: str | None = None, new: str | None = None) -> list[str]: ...
    def pids(self, p: ptyd.Pty | None) -> list[int]: ...
    def stop(self) -> None: ...


class TheseusArm(Arm):
    name = th.ARM

    def __init__(self, bin_dir: Path, base_url: str, work: Path) -> None:
        self.bin_dir, self.base_url, self.work = bin_dir, base_url, work
        self.d: th.Daemon | None = None
        self.label = ""
        self.daemon_starts: list[float] = []  # each scratch daemon's spawn -> first health answer, beside T1

    def fresh(self, root: Path) -> None:
        self.stop()
        self.d = th.Daemon(self.bin_dir, root, self.base_url, self.work)
        self.daemon_starts.append(round(self.d.start(), 3))
        self.label = "h2h" + uuid.uuid4().hex[:6]
        self.session = self.d.open_session(self.label)

    def surface(self, log: Path, resume: str | None = None) -> ptyd.Pty:
        return ptyd.Pty(th.tui_argv(self.d), env=self.d.env, cwd=str(self.work), log=log)

    def ready(self, p: ptyd.Pty, timeout: float) -> int | None:
        """The board up with the session on it; then Enter opens the session (not timed in T1)."""
        stamps = [p.wait_for(m, timeout) for m in (*th.READY_MARKERS, self.label)]
        if None in stamps:
            return None
        p.key("enter")
        return max(stamps)

    def before_type(self, p: ptyd.Pty) -> None:
        """`i` opens the input line once the session is on screen; until then it does nothing, so press it again.
        Typing before the line is open would send the prompt's letters as the board's keys."""
        for _ in range(20):
            at = p.mark()
            p.write(b"i")
            if p.wait_for(th.INPUT_MARKER, 0.5, after=at) is not None:
                return
        raise RuntimeError("the TUI's input line did not open")

    def oneshot_argv(self, prompt: str, session: str | None = None, new: str | None = None) -> list[str]:
        return th.ask_argv(self.d, prompt, session or self.session)

    @property
    def env(self) -> dict[str, str]:
        return self.d.env

    def pids(self, p: ptyd.Pty | None) -> list[int]:
        return [x for x in ((p.pid if p else None), self.d.pid if self.d else None) if x]

    def version(self) -> str | None:
        return self.d.version() if self.d else None

    def stop(self) -> None:
        if self.d:
            self.d.stop()
            self.d = None


class ClaudeArm(Arm):
    name = cc.ARM

    def __init__(self, binary: str, base_url: str, work: Path) -> None:
        self.binary, self.base_url, self.work = binary, base_url, work
        self.env: dict[str, str] = {}

    def fresh(self, root: Path) -> None:
        self.env = cc.scratch(root, self.work, self.base_url)
        self.session = None

    def surface(self, log: Path, resume: str | None = None) -> ptyd.Pty:
        return ptyd.Pty(cc.interactive_argv(self.binary, resume), env=self.env, cwd=str(self.work), log=log)

    def ready(self, p: ptyd.Pty, timeout: float) -> int | None:
        return p.wait_for(cc.READY_MARKER, timeout)

    def before_type(self, p: ptyd.Pty) -> None:
        pass

    def oneshot_argv(self, prompt: str, session: str | None = None, new: str | None = None) -> list[str]:
        return cc.oneshot_argv(self.binary, prompt, session_id=new, resume=session)

    def pids(self, p: ptyd.Pty | None) -> list[int]:
        return [p.pid] if p else []

    def stop(self) -> None:
        pass


# ------------------------------------------------------------------ the runs

class TurnStuck(Exception):
    """A turn whose reply never ended on screen. Typing on would queue the next prompt behind it."""


class Runner:
    def __init__(self, args: argparse.Namespace, arms: dict[str, Arm], logs: dict[str, Path], out: Path) -> None:
        self.args, self.arms, self.logs, self.out = args, arms, logs, out
        self.rows = set(args.rows)
        self.samples: list[dict[str, Any]] = []
        self.failures: list[dict[str, Any]] = []

    def add(self, row: str, arm: str, run: int, kind: str, value: float | None, unit: str = "ms",
            **extra: Any) -> None:
        if value is None:
            self.failures.append({"row": row, "arm": arm, "run": run, "kind": kind, "why": "no value"})
            return
        self.samples.append({"row": row, "arm": arm, "run": run, "kind": kind, "value": round(value, 4),
                             "unit": unit, **extra})

    def turn(self, arm: Arm, p: ptyd.Pty, word: str) -> dict[str, Any]:
        """One typed prompt to its reply's end, with the stand-in's log joined and CPU sampled around it."""
        m = marker()
        prompt = f"{word} marker={m}"
        arm.before_type(p)
        at = p.mark()
        p.type(prompt)
        p.wait_for(f"marker={m}", 10, after=at)  # the echo: Enter goes after the surface has the text
        reply_word = standin.WORK[word][1]
        # The reply's markers are new words, never echoed: watched on the grid as well as the transcript, so a word
        # a TUI draws in two moves is still seen whole.
        p.watch(f"{reply_word}-{m}", f"end-{m}")
        before = ptyd.sample_tree(arm.pids(p))
        enter = p.key("enter")
        end = p.wait_for(f"end-{m}", self.args.turn_timeout)
        after = ptyd.sample_tree(arm.pids(p))
        stamps = {f"{reply_word}-{m}": p.stamp(f"{reply_word}-{m}"), f"end-{m}": end}
        n = len(standin.WORK[word][0]) + 1
        entries = standin.wait_requests(self.logs[arm.name],
                                        lambda e: not e.get("side") and e["arrival_ns"] >= enter, n, 10)
        t = joins.turn(enter, entries, m, reply_word, stamps, standin.step_kinds(word))
        t["cpu_ms"] = joins.cpu_seconds(before, after) * 1000
        if end is None:  # the reply never ended on screen: nothing of this turn is trusted, and the session stops
            raise TurnStuck(f"{arm.name}: {word} marker={m} never showed end-{m}")
        return t

    def session_run(self, arm: Arm, i: int, neighbour: bool) -> None:
        """T1 to T4, T6, T7, task3 and the first request's bytes, in one interactive session."""
        row = (lambda r: "T8") if neighbour else (lambda r: r)
        kind = (lambda r, k: f"{r} {k}") if neighbour else (lambda r, k: k)
        tag = "n" if neighbour else ""
        p = arm.surface(self.out / "pty" / f"{arm.name}-{i}{tag}.rec")
        try:
            self._session(arm, p, i, neighbour, row, kind)
        except TurnStuck as e:
            self.failures.append({"row": "session", "arm": arm.name, "run": i, "kind": "interactive", "why": str(e)})
        finally:
            p.close()

    def _session(self, arm: Arm, p: ptyd.Pty, i: int, neighbour: bool, row, kind) -> None:
        """The session's rows, in order; a turn that never ends raises TurnStuck."""
        ready = arm.ready(p, self.args.ready_timeout)
        self.add(row("T1"), arm.name, i, kind("T1", "interactive"), joins.ms(ready - p.spawn_ns) if ready else None)
        if ready is None:
            return
        if self.rows & {"T2", "T3", "T6", "T8", "first_request_bytes"}:
            t = self.turn(arm, p, "h2h-hello")
            self.add(row("T2"), arm.name, i, kind("T2", "interactive"), t["t2_ms"])
            self.add(row("T3"), arm.name, i, kind("T3", "interactive"), t["t3_ms"])
            if not neighbour:
                self.add("T6", arm.name, i, "interactive", t["cpu_ms"])
                self.add("first_request_bytes", arm.name, i, "interactive", t["first_request_bytes"], "bytes")
        if self.rows & {"T4", "T8"}:
            for word in ("h2h-read", "h2h-shell"):
                for c in self.turn(arm, p, word)["t4"]:
                    self.add(row("T4"), arm.name, i, kind("T4", c["kind"]), c["ms"])
        if "task3" in self.rows and not neighbour:
            t = self.turn(arm, p, "h2h-task3")
            self.add("task3", arm.name, i, "interactive", t["turn_ms"], requests=t["requests"],
                     edited=("edited" in (self.args.work / arm.name / "notes.txt").read_text()))
            for c in t["t4"]:
                self.add("T4", arm.name, i, f"{c['kind']} (task3)", c["ms"])
        if "T7" in self.rows and not neighbour:
            samples = []
            deadline = time.monotonic() + self.args.idle_secs
            while time.monotonic() < deadline:
                samples.append(ptyd.sample_tree(arm.pids(p)))
                time.sleep(0.5)
            idle = joins.idle(samples)
            self.add("T7", arm.name, i, "cpu", idle["cpu_pct"], "pct")
            self.add("T7", arm.name, i, "rss", idle["rss_kb_mean"], "kb")

    def oneshot_run(self, arm: Arm, i: int) -> None:
        for word in ("h2h-hello", "h2h-task3"):
            if word == "h2h-task3" and "task3" not in self.rows:
                continue
            m = marker()
            rec = oneshot.run(arm.oneshot_argv(f"{word} marker={m}", new=str(uuid.uuid4())), env=arm.env,
                              cwd=str(self.args.work / arm.name), marker=f"end-{m}", timeout=self.args.turn_timeout)
            standin.wait_requests(self.logs[arm.name], lambda e: rec["spawn_ns"] <= e["arrival_ns"], 1, 5)
            r = oneshot.rows(rec, standin.read_log(self.logs[arm.name]), m)
            if word == "h2h-task3":
                self.add("task3", arm.name, i, "one-shot", r["wall_ms"], requests=r["requests"])
                continue
            self.add("T2", arm.name, i, "one-shot", r["to_request_ms"])
            self.add("T3", arm.name, i, "one-shot", r["t3_ms"])
            self.add("T6", arm.name, i, "one-shot", r["cpu_ms"], maxrss_kb=r["maxrss_kb"])
            self.add("first_request_bytes", arm.name, i, "one-shot", r["first_request_bytes"], "bytes",
                     side_requests=r["side_requests"])

    def make_long(self, arm: Arm, turns: int) -> tuple[str | None, str]:
        """A session of `turns` turns through the stand-in; returns its id and its last reply's end marker."""
        sid = str(uuid.uuid4()) if arm.name == cc.ARM else None
        last = ""
        for k in range(turns):
            last = f"L{k:04d}"
            if arm.name == cc.ARM:
                argv = arm.oneshot_argv(f"h2h-hello marker={last}", **({"new": sid} if k == 0 else {"session": sid}))
            else:
                argv = arm.oneshot_argv(f"h2h-hello marker={last}")
            oneshot.run(argv, env=arm.env, cwd=str(self.args.work / arm.name), timeout=self.args.turn_timeout)
        return (sid or getattr(arm, "session", None)), f"end-{last}"

    def resume_run(self, arm: Arm, i: int, sid: str, last: str) -> None:
        p = arm.surface(self.out / "pty" / f"{arm.name}-resume-{i}.rec", resume=sid)
        try:
            ready = arm.ready(p, self.args.ready_timeout)
            shown = p.wait_for(last, self.args.ready_timeout)
            if ready and shown:
                self.add("T5", arm.name, i, "interactive", joins.ms(max(ready, shown) - p.spawn_ns))
            else:
                self.add("T5", arm.name, i, "interactive", None)
        finally:
            p.close()
        m = marker()
        argv = arm.oneshot_argv(f"h2h-hello marker={m}", session=sid)
        rec = oneshot.run(argv, env=arm.env, cwd=str(self.args.work / arm.name), marker=f"end-{m}",
                          timeout=self.args.turn_timeout)
        standin.wait_requests(self.logs[arm.name], lambda e: rec["spawn_ns"] <= e["arrival_ns"], 1, 5)
        r = oneshot.rows(rec, standin.read_log(self.logs[arm.name]), m)
        self.add("T5", arm.name, i, "one-shot", r["to_request_ms"], request_bytes=r["first_request_bytes"])


def neighbour_start(where: Path) -> subprocess.Popen:
    """A neighbour that writes 64 MiB and fsyncs it, over and over, beside the runs (T8)."""
    f = where / "neighbour.bin"
    return subprocess.Popen(["sh", "-c", f"while :; do dd if=/dev/zero of={f} bs=1M count=64 conv=fsync "
                                         f"status=none; done"], start_new_session=True)


def neighbour_stop(p: subprocess.Popen, where: Path) -> None:
    try:
        os.killpg(p.pid, 15)
    except ProcessLookupError:
        pass
    p.wait()
    (where / "neighbour.bin").unlink(missing_ok=True)


def machine() -> dict[str, Any]:
    cpu = ""
    try:
        cpu = next((ln.split(":", 1)[1].strip() for ln in open("/proc/cpuinfo") if ln.startswith("model name")), "")
    except OSError:
        pass
    return {"nproc": os.cpu_count(), "kernel": platform.release(), "cpu": cpu, "python": platform.python_version(),
            "loadavg": list(os.getloadavg())}


# ------------------------------------------------------------------ main

def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0], formatter_class=argparse.RawTextHelpFormatter)
    ap.add_argument("--rows", default=",".join(ROWS), help="comma-separated, of " + ", ".join(ROWS))
    ap.add_argument("--arms", default=",".join(ARMS))
    ap.add_argument("--runs", type=int, default=10)
    ap.add_argument("--out", type=Path, required=True)
    ap.add_argument("--bin-dir", type=Path, default=Path("target/release-thin"),
                    help="theseusd, theseus, theseus-tui (and theseus-sim, unless --sim)")
    ap.add_argument("--sim", help="theseus-sim (default: the one in --bin-dir)")
    ap.add_argument("--claude", help="claude (default: the one on PATH)")
    ap.add_argument("--any-claude-version", action="store_true",
                    help=f"run a Claude Code other than the pinned {cc.CLAUDE_CODE_VERSION}")
    ap.add_argument("--theseus-only", action="store_true")
    ap.add_argument("--ttfb-ms", type=int, default=300)
    ap.add_argument("--chunks", type=int, default=8)
    ap.add_argument("--chunk-ms", type=int, default=25)
    ap.add_argument("--long-turns", type=int, default=50, help="T5's session length in turns")
    ap.add_argument("--idle-secs", type=float, default=10)
    ap.add_argument("--ready-timeout", type=float, default=30)
    ap.add_argument("--turn-timeout", type=float, default=120)
    args = ap.parse_args(argv)
    args.rows = [r.strip() for r in args.rows.split(",") if r.strip()]
    if bad := set(args.rows) - set(ROWS):
        ap.error(f"unknown rows {sorted(bad)}")
    arm_names = [a.strip() for a in args.arms.split(",") if a.strip()]
    out = args.out.resolve()
    (out / "pty").mkdir(parents=True, exist_ok=True)
    args.work = out / "work"
    sim = args.sim or str(args.bin_dir / "theseus-sim")

    info: dict[str, dict[str, Any]] = {}
    claude = cc.find(args.claude)
    if cc.ARM in arm_names:
        why = ("--theseus-only" if args.theseus_only else "claude was not found" if not claude else None)
        ver = cc.version(claude) if claude and not why else None
        if not why and ver != cc.CLAUDE_CODE_VERSION and not args.any_claude_version:
            why = f"claude is {ver}, not the pinned {cc.CLAUDE_CODE_VERSION} (--any-claude-version runs it anyway)"
        info[cc.ARM] = {"available": not why, "why": why, "version": ver, "binary": claude}
        if why:
            print(f"Claude Code's arm is not run: {why}", file=sys.stderr)
            arm_names.remove(cc.ARM)

    started = datetime.now().astimezone()
    load_before = list(os.getloadavg())
    stand, arms, logs = {}, {}, {}
    try:
        for name in arm_names:
            notes = standin.reset_work(args.work / name)
            rules = standin.write_rules(out / f"rules-{name}.json", name, notes, ttfb_ms=args.ttfb_ms,
                                        chunks=args.chunks, chunk_ms=args.chunk_ms)
            logs[name] = out / f"standin-{name}.jsonl"
            stand[name] = standin.StandIn(sim, rules, logs[name], ttfb_ms=args.ttfb_ms, chunks=args.chunks,
                                          chunk_ms=args.chunk_ms)
            arms[name] = (TheseusArm(args.bin_dir.resolve(), stand[name].base_url, args.work / name)
                          if name == th.ARM else ClaudeArm(claude, stand[name].base_url, args.work / name))
        r = Runner(args, arms, logs, out)

        def reset(name: str, i: int, tag: str = "") -> None:
            standin.reset_work(args.work / name)
            root = out / "scratch" / f"{name}-{i}{tag}"
            shutil.rmtree(root, ignore_errors=True)
            arms[name].fresh(root)

        interactive = set(args.rows) & {"T1", "T2", "T3", "T4", "T6", "T7", "task3", "first_request_bytes"}
        if interactive:
            oneshot.interleave(arm_names, args.runs, lambda a, i: r.session_run(arms[a], i, False), reset)
        if set(args.rows) & {"T2", "T3", "T6", "task3", "first_request_bytes"}:
            oneshot.interleave(arm_names, args.runs, lambda a, i: r.oneshot_run(arms[a], i),
                               lambda a, i: reset(a, i, "o"))
        if "T8" in args.rows:
            nb = neighbour_start(out)
            try:
                oneshot.interleave(arm_names, args.runs, lambda a, i: r.session_run(arms[a], i, True),
                                   lambda a, i: reset(a, i, "n"))
            finally:
                neighbour_stop(nb, out)
        if "T5" in args.rows:
            for name in arm_names:
                reset(name, 0, "long")
                sid, last = r.make_long(arms[name], args.long_turns)
                for i in range(args.runs):
                    r.resume_run(arms[name], i, sid, last)
        versions = {"theseus": None, "claude-code": info.get(cc.ARM, {}).get("version")}
        if th.ARM in arms:
            if arms[th.ARM].d is None:
                arms[th.ARM].fresh(out / "scratch" / "version")
            versions["theseus"] = arms[th.ARM].version()
    finally:
        for a in arms.values():
            a.stop()
        for s in stand.values():
            s.stop()

    for name in arm_names:
        info[name] = {**info.get(name, {}), "available": True, "requests": len(standin.read_log(logs[name]))}
    record = {
        "bench": "head-to-head-speed", "issue": "theseus-7gir.13",
        "date": started.date().isoformat(), "started": started.isoformat(timespec="seconds"),
        "finished": datetime.now().astimezone().isoformat(timespec="seconds"),
        "rows": args.rows, "runs": args.runs, "arms": info, "theseus_only": cc.ARM not in arm_names,
        "machine": {**machine(), "loadavg_before": load_before, "loadavg_after": list(os.getloadavg())},
        "versions": versions,
        "standin": {"ttfb_ms": args.ttfb_ms, "chunks": args.chunks, "chunk_ms": args.chunk_ms},
        "long_turns": args.long_turns, "idle_secs": args.idle_secs,
        "daemon_start_ms": getattr(arms.get(th.ARM), "daemon_starts", []),
        "command": " ".join(["python3", "bench/h2h/run.py", *(argv if argv is not None else sys.argv[1:])]),
        "samples": r.samples, "failures": r.failures,
    }
    (out / "run.json").write_text(json.dumps(record, indent=1) + "\n")
    print(f"wrote {out / 'run.json'}: {len(r.samples)} samples, {len(r.failures)} without a value")
    return 0


if __name__ == "__main__":
    sys.exit(main())
