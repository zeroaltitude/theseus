"""threadbench's cost model (theseus-9ofg): each arm's measured token breakdown, re-priced at the owner's gaps.

A run replays the owner's messages back to back, so every prompt cache stays warm; the owner's day does not (gaps
with a median of 6.2 minutes, more than 5 minutes 55% of the time). The model, the same for every arm:

- a session's prompt cache lives TTL after its last request (5 minutes, or 1 hour at the dearer 1-hour write);
- a message whose gap since its session's previous message is over the TTL misses: the cache reads of its first
  request become cache writes (a turn of `calls` requests reads its prefix `calls` times, so one call's share,
  reads / calls, is the first request's), and the later requests of the turn read what the first wrote;
- the first message of a session wrote its prefix already (the measured run did that too), so nothing changes;
- the gap is the owner's, measured between a session's own messages: an arm with a session per stream sees longer
  gaps than one session does, and that is exactly where splitting loses cache warmth;
- prices are the catalog's (`bench/recall/generate.py`, held to catalog.rs by its test): input, output, cache read,
  cache write at 5 minutes; the 1-hour write is twice the input price (Anthropic's published ratio).

The model prices tokens, not behaviour: an arm that would have worked differently on a cold cache is not modelled.

Standard library only.
"""

from __future__ import annotations

import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import th_format as tf  # noqa: E402,F401
from generate import PRICES  # noqa: E402

TTLS = {"5m": 5.0, "1h": 60.0}


def rates(model: str) -> tuple[float, float, float, float]:
    """(input, output, cache read, cache write at 5 minutes) per million tokens."""
    m = model.split("/", 1)[-1]
    if m not in PRICES:
        raise KeyError(f"no price for {m}")
    return PRICES[m]


def write_rate(r: tuple[float, float, float, float], ttl: str) -> float:
    return r[3] if ttl == "5m" else 2.0 * r[0]


def turn_usd(tok: dict, r: tuple[float, float, float, float], ttl: str, miss: bool, calls: int = 1) -> float:
    """One message's dollars from its token breakdown (input, output, cache_read, cache_write), the cache
    missed or not."""
    read, write = float(tok.get("cache_read", 0)), float(tok.get("cache_write", 0))
    if miss:
        moved = read / max(1, calls)
        read, write = read - moved, write + moved
    return (tok.get("input", 0) * r[0] + tok.get("output", 0) * r[1] + read * r[2] + write * write_rate(r, ttl)) / 1e6


def session_gaps(prog: tf.Progression, session_of: list[int]) -> list[float | None]:
    """Minutes since the previous message of the same session, per message (None: the session's first)."""
    last: dict[int, float] = {}
    out: list[float | None] = []
    for m in prog.messages:
        s = session_of[m.index]
        out.append(round(m.t - last[s], 1) if s in last else None)
        last[s] = m.t
    return out


def reprice(turns: list[dict], gaps: list[float | None], model: str, ttl: str) -> dict:
    """The run's dollars at the owner's gaps and `ttl`: total, the misses, and the same tokens with no misses."""
    r = rates(model)
    ttl_min = TTLS[ttl]
    total = warm = 0.0
    misses = 0
    for t, g in zip(turns, gaps):
        tok = t["tokens"]
        calls = max(1, int(t.get("calls") or 1))
        miss = g is not None and g > ttl_min
        misses += miss
        total += turn_usd(tok, r, ttl, miss, calls)
        warm += turn_usd(tok, r, ttl, False, calls)
    return {"usd": round(total, 6), "warm_usd": round(warm, 6), "misses": misses, "ttl": ttl}
