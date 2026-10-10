"""The head-to-head bench's joins (theseus-7gir.13): screen stamps against the stand-in's log, as the rows' numbers.

Every time here is CLOCK_MONOTONIC nanoseconds, the bench's (`time.monotonic_ns()`) and the stand-in's alike, so a
stamp on the screen and a request's arrival are one clock and subtract. Pure functions: no I/O, no clock.

- T2: the Enter that sent a prompt -> the arrival of the turn's first request (the request leaving the harness).
- T3: that request's first byte out of the stand-in -> the reply's first marker on screen.
- T4: per tool call, the last byte of the response that asked for it -> the arrival of the next request, which
  carries its result. Labelled read, edit or shell by the step's tool.
- T6: CPU per turn, from two samples of the harness's processes.

A turn's requests are the non-side ones (side requests are Claude Code's small-model calls without tools) between
its Enter and its reply's end on screen, narrowed to those that name its marker when the log names it (a `marker`
field, or the opening text). Claude Code can open a message with a system reminder that pushes the prompt past the
logged opening, so the window, not the opening, is what a join stands on.
"""

from __future__ import annotations

from typing import Any, Iterable

MS = 1e6


def ms(ns: int | None) -> float | None:
    return None if ns is None else ns / MS


def has_marker(e: dict[str, Any], marker: str) -> bool:
    """Whether a logged request names the turn: its `marker` (when the stand-in logs one) or its opening text."""
    return e.get("marker") == marker or marker in (e.get("opening") or "")


def turn_requests(entries: Iterable[dict[str, Any]], marker: str | None = None, after_ns: int | None = None,
                  before_ns: int | None = None) -> list[dict[str, Any]]:
    """The turn's model requests in arrival order: not side requests, arriving in [after_ns, before_ns] when those
    are given. With a marker, those that name it; but when no request in the log names it at all (Claude Code opens
    a message with a system reminder, which can push the prompt past the logged opening), the window alone."""
    entries = list(entries)
    window = sorted((e for e in entries if not e.get("side")
                     and (after_ns is None or e["arrival_ns"] >= after_ns)
                     and (before_ns is None or e["arrival_ns"] <= before_ns)), key=lambda e: e["arrival_ns"])
    if marker is None or not any(has_marker(e, marker) for e in entries):
        return window
    return [e for e in window if has_marker(e, marker)]


def side_requests(entries: Iterable[dict[str, Any]], start_ns: int, end_ns: int) -> list[dict[str, Any]]:
    return [e for e in entries if e.get("side") and start_ns <= e["arrival_ns"] <= end_ns]


def t2(enter_ns: int, entries: list[dict[str, Any]], marker: str | None = None,
       before_ns: int | None = None) -> float | None:
    """Enter -> the turn's first request's arrival, in ms; None when no request came."""
    reqs = turn_requests(entries, marker, enter_ns, before_ns)
    return ms(reqs[0]["arrival_ns"] - enter_ns) if reqs else None


def reply_request(entries: list[dict[str, Any]], marker: str | None = None, after_ns: int | None = None,
                  before_ns: int | None = None) -> dict[str, Any] | None:
    """The request whose response is the turn's text: its last request (the earlier ones asked for tools)."""
    reqs = turn_requests(entries, marker, after_ns, before_ns)
    return reqs[-1] if reqs else None


def t3(request: dict[str, Any] | None, shown_ns: int | None) -> float | None:
    """The stand-in's first byte of the reply -> its first marker on screen, in ms."""
    if not request or shown_ns is None or request.get("first_byte_ns") is None:
        return None
    return ms(shown_ns - request["first_byte_ns"])


def t4(entries: list[dict[str, Any]], kinds: list[str], marker: str | None = None, after_ns: int | None = None,
       before_ns: int | None = None) -> list[dict[str, Any]]:
    """Per tool call in a turn: [{step, kind, ms}], request k's response's last byte -> request k+1's arrival.

    `kinds[k]` is the tool request k's response asks for (read, edit, shell): a step is placed by its position in
    the turn, not by the stand-in's count of tool results, which a harness that merges messages can shift."""
    reqs = turn_requests(entries, marker, after_ns, before_ns)
    out = []
    for k, (cur, nxt) in enumerate(zip(reqs, reqs[1:])):
        if k >= len(kinds) or cur.get("last_byte_ns") is None:
            continue
        out.append({"step": k, "kind": kinds[k], "ms": ms(nxt["arrival_ns"] - cur["last_byte_ns"])})
    return out


def t4_by_kind(calls: list[dict[str, Any]]) -> dict[str, list[float]]:
    by: dict[str, list[float]] = {}
    for c in calls:
        by.setdefault(c["kind"], []).append(c["ms"])
    return by


def cpu_seconds(before: dict[str, Any], after: dict[str, Any]) -> float:
    """CPU (user + system) spent between two `sample_tree` samples, in seconds."""
    return (after["cpu_ticks"] - before["cpu_ticks"]) / (after.get("clk_tck") or 100)


def t6(samples: list[tuple[dict[str, Any], dict[str, Any]]]) -> list[float]:
    """CPU per turn in ms, one value per (before, after) pair of samples around a turn."""
    return [cpu_seconds(a, b) * 1000 for a, b in samples]


def idle(samples: list[dict[str, Any]]) -> dict[str, float | None]:
    """T7 from samples taken while nothing runs: CPU as a share of one core over the span, and RSS's mean and peak."""
    if len(samples) < 2:
        return {"cpu_pct": None, "rss_kb_mean": None, "rss_kb_max": None, "secs": None}
    span = (samples[-1]["ns"] - samples[0]["ns"]) / 1e9
    rss = [s["rss_kb"] for s in samples]
    return {"cpu_pct": 100 * cpu_seconds(samples[0], samples[-1]) / span if span > 0 else None,
            "rss_kb_mean": sum(rss) / len(rss), "rss_kb_max": max(rss), "secs": span}


def turn(enter_ns: int, entries: list[dict[str, Any]], marker: str, reply_word: str,
         stamps: dict[str, int | None], kinds: list[str] | None = None) -> dict[str, Any]:
    """One interactive turn's rows: T2, T3, T4 (when it called tools) and the end, from the screen's stamps
    (`<word>-<marker>` and `end-<marker>`) and the stand-in's log."""
    first = f"{reply_word}-{marker}"
    end = stamps.get(f"end-{marker}")
    reqs = turn_requests(entries, marker, enter_ns, end)
    return {
        "marker": marker,
        "requests": len(reqs),
        "t2_ms": t2(enter_ns, entries, marker, end),
        "t3_ms": t3(reqs[-1] if reqs else None, stamps.get(first)),
        "t4": t4(entries, kinds or [], marker, enter_ns, end),
        "turn_ms": ms(end - enter_ns) if end else None,
        "first_request_bytes": reqs[0].get("req_bytes") if reqs else None,
        "keepalive": [bool(r.get("conn_req")) for r in reqs],
    }
