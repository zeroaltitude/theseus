"""The head-to-head bench's one-shot runs (theseus-7gir.13): a harness run once for one turn, from spawn to exit.

`run(argv, ...)` spawns the command, stamps its first byte of output and the first appearance of a marker, and reaps
it with `os.wait4`, whose rusage gives the run's CPU (user + system, its reaped children's included) and peak RSS.
`join(entries, record)` gives the stand-in's requests that arrived while it ran, and `rows(record, entries, marker)`
the one-shot's numbers: spawn -> first request (T1's one-shot form, and T5's one-shot resume), the turn's T3 against
the marker's stamp, the bytes of the first request, the CPU. `interleave` runs the arms A/B alternately, ABBA by
pairs, with a reset between runs.

Standard library only.
"""

from __future__ import annotations

import os
import signal
import subprocess
import threading
import time
from typing import Any, Callable

import joins


def run(argv: list[str], env: dict[str, str] | None = None, cwd: str | None = None, marker: str | None = None,
        timeout: float = 120.0) -> dict[str, Any]:
    """One run: its stamps (spawn, first byte, marker, exit), exit code, CPU seconds, peak RSS and output size."""
    rec: dict[str, Any] = {"argv0": os.path.basename(argv[0]), "first_byte_ns": None, "marker_ns": None,
                           "out_bytes": 0, "timed_out": False}
    spawn = time.monotonic_ns()
    p = subprocess.Popen(argv, env=env, cwd=cwd, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                         stderr=subprocess.STDOUT, start_new_session=True)
    rec["spawn_ns"] = spawn
    seen = bytearray()
    want = marker.encode() if marker else None

    def read() -> None:
        while chunk := p.stdout.read1(65536):
            ns = time.monotonic_ns()
            if rec["first_byte_ns"] is None:
                rec["first_byte_ns"] = ns
            rec["out_bytes"] += len(chunk)
            if want and rec["marker_ns"] is None:
                seen.extend(chunk)
                if want in seen:
                    rec["marker_ns"] = ns

    reader = threading.Thread(target=read, daemon=True)
    reader.start()
    timer = threading.Timer(timeout, lambda: (rec.__setitem__("timed_out", True), _kill(p.pid)))
    timer.start()
    _, status, ru = os.wait4(p.pid, 0)
    rec["exit_ns"] = time.monotonic_ns()
    timer.cancel()
    p.returncode = os.waitstatus_to_exitcode(status)
    reader.join(5)
    p.stdout.close()
    rec.update(exit=p.returncode, cpu_s=ru.ru_utime + ru.ru_stime, maxrss_kb=ru.ru_maxrss,
               wall_ms=joins.ms(rec["exit_ns"] - spawn))
    return rec


def _kill(pid: int) -> None:
    try:
        os.killpg(pid, signal.SIGKILL)
    except ProcessLookupError:
        pass


def join(entries: list[dict[str, Any]], rec: dict[str, Any]) -> list[dict[str, Any]]:
    """The stand-in's requests that arrived while the run lived (spawn to exit), side requests included."""
    return [e for e in entries if rec["spawn_ns"] <= e["arrival_ns"] <= rec["exit_ns"]]


def rows(rec: dict[str, Any], entries: list[dict[str, Any]], marker: str | None = None) -> dict[str, Any]:
    """A one-shot's numbers: spawn -> its turn's first request (`to_request_ms`), T3 against the marker's stamp on
    stdout, the request count and side requests, the first request's bytes, CPU and peak RSS."""
    mine = join(entries, rec)
    reqs = joins.turn_requests(mine, marker)
    first = reqs[0] if reqs else None
    return {
        "to_request_ms": joins.ms(first["arrival_ns"] - rec["spawn_ns"]) if first else None,
        "first_output_ms": joins.ms(rec["first_byte_ns"] - rec["spawn_ns"]) if rec.get("first_byte_ns") else None,
        "t3_ms": joins.t3(reqs[-1] if reqs else None, rec.get("marker_ns")),
        "wall_ms": rec.get("wall_ms"),
        "requests": len(reqs),
        "side_requests": sum(1 for e in mine if e.get("side")),
        "first_request_bytes": first.get("req_bytes") if first else None,
        "cpu_ms": rec["cpu_s"] * 1000 if rec.get("cpu_s") is not None else None,
        "maxrss_kb": rec.get("maxrss_kb"),
        "exit": rec.get("exit"),
    }


def order(arms: list[str], i: int) -> list[str]:
    """Run i's arm order, ABBA by pairs: A B, B A, A B, ... so neither arm always goes first after a reset."""
    return list(arms) if i % 2 == 0 else list(reversed(arms))


def interleave(arms: list[str], runs: int, one: Callable[[str, int], Any],
               reset: Callable[[str, int], None] | None = None) -> dict[str, list[Any]]:
    """`one(arm, i)` for each run i and each arm, A/B interleaved; `reset(arm, i)` before each. Results by arm."""
    out: dict[str, list[Any]] = {a: [] for a in arms}
    for i in range(runs):
        for arm in order(arms, i):
            if reset:
                reset(arm, i)
            out[arm].append(one(arm, i))
    return out
