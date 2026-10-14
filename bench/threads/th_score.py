#!/usr/bin/env python3
"""threadbench's scorer (theseus-9ofg): run directories in, `scores.json` out.

    python3 bench/threads/th_score.py /tmp/tb-a3 /tmp/tb-a5 --out /tmp/tb-scores

For each run (what `th_drive.py` wrote):

- **Per-stream correctness**: the stream's end-state oracle on the run's workspace (1 only when every check
  holds), and the stream's in-stream recall probes (direct and indirect) answered right.
- **Contamination**: an in-stream probe whose answer (the reply, or the file it wrote) gives a colliding stream's
  value of the same kind, counted whether or not the right value is there too.
- **Embargo**: the sealed stream's markers in any other stream's files (a leak), and the embargo probes' own checks.
- **Helpful cross-stream recall**: a cross probe asked while another stream is active: the right value, and the
  source named (cited).
- **Abstention** (a stream with no deadline, or ticket, or port): no value of the kind, and an admission.
- **Misroute recovery**: after "that was for X", the note is in X's file and absent from the file it was filed in.
- **Routing**: top-1 and the ask rates (on the ambiguous, on the clear), from `routing.jsonl` when an arm routes
  (the threads arms; the owner-split arms route by construction and are marked so).
- **Context**: compiled request tokens per turn (provider input, cache read and cache write, per model call),
  their mean, p90 and curve; **dollars** measured (the harness's own count) and modelled at the owner's gaps
  (`th_price.py`, 5 minutes and 1 hour); **cache-read share** from the provider's usage fields; **latency** (wall
  per turn; first token only where the driver recorded it); **harness CPU and peak RSS** (the driven processes).

A probe whose fact never reached the arm (its marker is not in the transcript) is excluded and counted, never
scored as a miss. Standard library only.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import th_format as tf  # noqa: E402
import checks  # noqa: E402
import th_oracle as orc  # noqa: E402
import th_price as price  # noqa: E402
import th_drive as td  # noqa: E402

sys.path.insert(0, str(HERE.parent / "report"))
import stats  # noqa: E402

OWNER_ROUTED = ("A5", "A6")  # the owner picks the session: routing is the owner's, and perfect by construction


def quantile(xs: list[float], q: float) -> float | None:
    return round(stats.quantile(xs, q), 1) if xs else None


def read_calls(path: Path, harness: str, index: int, turn: dict) -> tuple[int | None, float | None]:
    """A turn's model calls and its first token, from what the harness itself reported (the driver kept its raw
    answer): Theseus's `loops` and `first_token_ms`; Claude Code's `num_turns`; Pi's assistant messages. None
    where the harness reports none."""
    raw = path / "raw" / f"t{index:04d}.json{'l' if harness == 'pi' else ''}"
    try:
        text = raw.read_text()
    except OSError:
        return None, None
    try:
        if harness == "pi":
            n = 0
            for line in text.splitlines():
                ev = json.loads(line)
                m = ev.get("message") or {}
                if ev.get("type") == "message_end" and m.get("role") == "assistant" and m.get("usage"):
                    n += 1
            return (n or None), None
        v = json.loads(text)
    except (ValueError, AttributeError):
        return None, None
    if harness == "claude-code":
        return (v.get("num_turns") if isinstance(v.get("num_turns"), int) else None), None
    ftl = v.get("first_token_ms")
    return (v.get("loops") if isinstance(v.get("loops"), int) else None), (float(ftl) if ftl is not None else None)


def load_run(path: Path) -> dict:
    path = Path(path)
    run = json.loads((path / "run.json").read_text())
    arm = json.loads((path / "arm.json").read_text())
    prog = tf.load(path)
    turns = [json.loads(x) for x in (path / "turns.jsonl").read_text().splitlines() if x.strip()]
    delivered = json.loads((path / "delivered.json").read_text()) if (path / "delivered.json").exists() else {}
    routing = []
    if (path / "routing.jsonl").exists():
        routing = [json.loads(x) for x in (path / "routing.jsonl").read_text().splitlines() if x.strip()]
    view = json.loads((path / "progression.json").read_text())
    for t in turns:
        c, f = read_calls(path, arm["harness"], t["index"], t)
        if c is not None:
            t["calls"] = c
        if f is not None and t.get("first_token_ms") is None:
            t["first_token_ms"] = f
    return {"dir": path, "run": run, "arm": arm, "prog": prog, "turns": turns, "delivered": delivered,
            "routing": routing, "session_of": [t["session"] for t in view["turns"]]}


def calls_of(turn: dict, harness: str) -> int:
    """The model calls of a turn: what the harness reported (`calls`, read from its raw answer), else tool calls + 1
    (an upper bound: parallel tool calls share a call), and Claude Code's own count."""
    if isinstance(turn.get("calls"), int) and turn["calls"] >= 1:
        return turn["calls"]
    n = turn.get("tool_calls")
    n = n if isinstance(n, int) else (len(n) if isinstance(n, list) else 0)
    return max(1, n) if harness == "claude-code" else n + 1


def request_tokens(turn: dict, harness: str) -> float:
    t = turn.get("tokens") or {}
    return (t.get("input", 0) + t.get("cache_read", 0) + t.get("cache_write", 0)) / calls_of(turn, harness)


def value_in(text: str, value: str) -> bool:
    return checks.has_word(checks.normalize(text), checks.normalize(value))


def current_values(prog: tf.Progression, kind: str, at: int, excl: str, right: str = "") -> list[tuple[str, str]]:
    """The other streams' values of `kind` in force at message `at`, but the right one: (stream, value)."""
    out = []
    for s in prog.streams:
        if s.id == excl:
            continue
        f = prog.current_fact(s.id, kind, at)
        if f is not None and f.value != right:
            out.append((s.id, f.value))
    return out


_IDEAL: dict[str, dict[str, set[str]]] = {}


def stream_checks(prog: tf.Progression, sid: str, root: Path) -> list[tuple[str, bool]]:
    if prog.track == "prefix":
        import th_prefix
        return th_prefix.oracle(sid, root)
    return orc.stream_checks(sid, root)


def ideal_checks(prog: tf.Progression) -> dict[str, set[str]]:
    """Per stream, the oracle checks that hold after the ideal arm's work on THIS progression. A full progression
    holds them all; a smoke, which stops each stream early, holds the ones its first messages earn, and an arm's
    stream is correct when it holds every check the ideal holds."""
    import tempfile
    key = prog.digest()
    if key not in _IDEAL:
        with tempfile.TemporaryDirectory(prefix="tb-ideal-") as d:
            orc.materialize(prog, Path(d))
            _IDEAL[key] = {s.id: {n for n, ok in stream_checks(prog, s.id, Path(d)) if ok} for s in prog.streams}
    return _IDEAL[key]


def probe_text(prog: tf.Progression, p: tf.Probe, turns: list[dict], root: Path) -> str:
    """What an in-stream probe's answer says: the reply, and for an indirect one the file it wrote."""
    if p.kind == "in_indirect" and p.file:
        try:
            return (root / p.file).read_text(errors="replace")
        except OSError:
            return ""
    return turns[p.msg].get("reply") or ""


def score_probe(prog: tf.Progression, p: tf.Probe, run: dict) -> dict:
    root = run["dir"] / "workspace"
    turn = run["turns"][p.msg] if p.msg < len(run["turns"]) else {}
    row = {"id": p.id, "kind": p.kind, "stream": p.stream, "msg": p.msg, "fact": p.fact}
    if not turn:
        return {**row, "excluded": "the run ended before this message"}
    if turn.get("exit") not in (0, None):
        row["turn_failed"] = True
    if p.fact and not run["delivered"].get(p.fact, {"delivered": True})["delivered"]:
        return {**row, "excluded": "the fact never reached the arm"}
    reply = turn.get("reply") or ""
    lines = checks.run(p.check, reply, root)
    ok = all(v for _, v in lines)
    row.update(ok=ok, lines=[v for _, v in lines])
    if p.kind in ("in_direct", "in_indirect"):
        f = prog.facts_by_id()[p.fact]
        said = probe_text(prog, p, run["turns"], root)
        others = current_values(prog, f.kind, p.msg, p.stream, f.value)
        row["contaminated"] = [s for s, v in others if value_in(said, v)]
        row["stale"] = bool(p.stale and value_in(said, p.stale))
        if p.sibling:  # a prefix-track header probe: the near-miss sibling's value is a false fit
            row["false_fit"] = (not ok) and value_in(said, p.sibling)
    elif p.kind == "cross":
        row["value_ok"] = all(v for _, v in lines[:-1])
        row["cited"] = lines[-1][1]
        f = prog.facts_by_id()[p.fact]
        row["contaminated"] = [s for s, v in current_values(prog, f.kind, p.msg, p.stream, f.value) if value_in(reply, v)]
    elif p.kind == "abstention":
        row["gave_value"] = not lines[0][1]
    elif p.kind == "embargo":
        row["leaked"] = orc.leaked_in(root, [p.file], prog.sealed_markers) if p.file else []
    return row


def rate(k: int, n: int) -> dict:
    lo, hi = (stats.wilson(k, n)[1:] if n else (None, None))
    return {"k": k, "n": n, "rate": round(k / n, 4) if n else None,
            "lo": round(lo, 4) if lo is not None else None, "hi": round(hi, 4) if hi is not None else None}


def score_run(run: dict) -> dict:
    prog, turns = run["prog"], run["turns"]
    arm = run["arm"]
    root = run["dir"] / "workspace"
    harness = arm["harness"]
    meta = run["run"]
    model = meta.get("model", "anthropic/claude-sonnet-5-5")
    rows = [score_probe(prog, p, run) for p in prog.probes if p.kind in tf.CHECK_KINDS]
    scored = [r for r in rows if "excluded" not in r]
    # ---- streams
    ideal = ideal_checks(prog)
    streams = {}
    for s in prog.streams:
        cks = stream_checks(prog, s.id, root)
        want = ideal[s.id]
        mine = [r for r in scored if r["stream"] == s.id and r["kind"] in ("in_direct", "in_indirect")]
        streams[s.id] = {"oracle": 1.0 if all(ok for n, ok in cks if n in want) else 0.0,
                         "ideal_checks": len(want), "ideal_passed": sum(1 for n, ok in cks if ok and n in want),
                         "checks": [{"name": n, "ok": ok} for n, ok in cks],
                         "checks_passed": sum(ok for _, ok in cks), "checks_total": len(cks),
                         "recall": rate(sum(r["ok"] for r in mine), len(mine))}
    ins = [r for r in scored if r["kind"] in ("in_direct", "in_indirect")]
    kinds = {}
    for k in tf.CHECK_KINDS:
        rs = [r for r in scored if r["kind"] == k]
        kinds[k] = {**rate(sum(r["ok"] for r in rs), len(rs)),
                    "excluded": sum(1 for r in rows if r["kind"] == k and "excluded" in r)}
    cross = [r for r in scored if r["kind"] == "cross"]
    leaks = orc.leaks(root, streams=tuple(s.id for s in prog.streams), markers=prog.sealed_markers)
    emb = [r for r in scored if r["kind"] == "embargo"]
    absn = [r for r in scored if r["kind"] == "abstention"]
    # ---- tokens, dollars, cache
    rq = [request_tokens(t, harness) for t in turns]
    tot = {k: sum((t.get("tokens") or {}).get(k, 0) for t in turns) for k in ("input", "output", "cache_read", "cache_write")}
    prompt = tot["input"] + tot["cache_read"] + tot["cache_write"]
    measured = [t.get("cost_usd") for t in turns]
    sessions = run["session_of"]
    gaps = price.session_gaps(prog, sessions)
    pt = [{"tokens": t.get("tokens") or {}, "calls": calls_of(t, harness)} for t in turns]
    modeled = {ttl: price.reprice(pt, gaps, model, ttl) for ttl in price.TTLS}
    lat = [t["latency_ms"] for t in turns if t.get("latency_ms") is not None]
    ftl = [t["first_token_ms"] for t in turns if t.get("first_token_ms") is not None]
    # ---- routing
    if arm["arm"] in OWNER_ROUTED:
        routing = {"owner_routed": True}
    elif run["routing"]:
        rr = run["routing"]
        by = {r["msg"]: r for r in rr}
        amb = [m for m in prog.messages if m.ambiguous and m.index in by]
        clr = [m for m in prog.messages if not m.ambiguous and m.index in by and m.role != "correction"]
        routing = {"top1": rate(sum(by[m.index].get("stream") == m.stream for m in clr), len(clr)),
                   "asked_on_ambiguous": rate(sum(bool(by[m.index].get("asked")) for m in amb), len(amb)),
                   "asked_on_clear": rate(sum(bool(by[m.index].get("asked")) for m in clr), len(clr))}
    else:
        routing = {"none": "the arm has one context and no router: nothing to score"}
    mis = [r for r in scored if r["kind"] == "misroute"]
    hdr = [r for r in ins if r.get("false_fit") is not None]
    header = {"n": len(hdr), "correct": sum(r["ok"] for r in hdr), "false_fit": sum(1 for r in hdr if r["false_fit"]),
              "other_wrong": sum(1 for r in hdr if not r["ok"] and not r["false_fit"])}
    return {
        "arm": arm["arm"], "name": arm["name"], "key": arm["key"], "harness": harness, "layout": arm["layout"],
        "seed": prog.seed, "size": prog.size, "digest": prog.digest(),
        "model": model, "effort": meta.get("effort"), "turns": len(turns), "sessions": len(set(sessions)),
        "failed_turns": sum(1 for t in turns if t.get("exit") not in (0, None)),
        "streams": streams,
        "oracle_mean": round(sum(s["oracle"] for s in streams.values()) / len(streams), 4),
        "kinds": kinds, "probes": rows,
        "excluded": sum(1 for r in rows if "excluded" in r),
        "contamination": {**rate(sum(1 for r in ins if r["contaminated"]), len(ins)),
                          "by_stream": {s.id: sum(1 for r in ins if r["stream"] == s.id and r["contaminated"])
                                        for s in prog.streams}},
        "stale": rate(sum(1 for r in ins if r.get("stale")), len([r for r in ins if prog.facts_by_id()[r["fact"]].supersedes])),
        "cross": {"value": rate(sum(r["value_ok"] for r in cross), len(cross)),
                  "cited": rate(sum(r["cited"] and r["value_ok"] for r in cross), len(cross)),
                  "contaminated": rate(sum(1 for r in cross if r["contaminated"]), len(cross))},
        "embargo": {"leaks": [] if prog.track == "prefix" else leaks, "leak_count": 0 if prog.track == "prefix" else len(leaks), "probes": rate(sum(r["ok"] for r in emb), len(emb))},
        "abstention": {**rate(sum(r["ok"] for r in absn), len(absn)),
                       "gave_value": sum(1 for r in absn if r["gave_value"])},
        "misroute": rate(sum(r["ok"] for r in mis), len(mis)),
        "header": header, "track": prog.track,
        "routing": routing,
        "tokens": {"request_mean": round(sum(rq) / len(rq), 1) if rq else None, "request_p90": quantile(rq, 0.9),
                   "request_curve": [round(x) for x in rq], "totals": tot, "prompt_total": prompt,
                   "cache_read_share": round(tot["cache_read"] / prompt, 4) if prompt else None},
        "usd": {"measured": round(sum(c for c in measured if c is not None), 4),
                "measured_turns": sum(1 for c in measured if c is not None),
                "per_turn": round(sum(c for c in measured if c is not None) / max(1, len(turns)), 5),
                "modeled": modeled, "model_prices_per_mtok": list(price.rates(model))},
        "latency": {"wall_ms_mean": round(sum(lat) / len(lat), 1) if lat else None, "wall_ms_p50": quantile(lat, 0.5),
                    "wall_ms_p90": quantile(lat, 0.9),
                    "first_token_ms_mean": round(sum(ftl) / len(ftl), 1) if ftl else None,
                    "first_token_note": None if ftl else "no first token recorded: only Theseus's turn result carries one"},
        "resources": meta.get("resources"),
        "compactions": len(meta.get("compactions") or []),
        "harness_version": meta.get("version"),
    }


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("runs", nargs="+", type=Path)
    ap.add_argument("--out", type=Path, required=True)
    a = ap.parse_args(argv)
    out = [score_run(load_run(p)) for p in a.runs]
    a.out.mkdir(parents=True, exist_ok=True)
    (a.out / "scores.json").write_text(json.dumps({"runs": out}, indent=1, sort_keys=True) + "\n")
    for r in out:
        print(f"{r['arm']} {r['name']}: oracle {r['oracle_mean']:.2f}; contamination {r['contamination']['k']}/"
              f"{r['contamination']['n']}; leaks {r['embargo']['leak_count']}; ${r['usd']['measured']:.2f} measured, "
              f"${r['usd']['modeled']['5m']['usd']:.2f} modelled (5 min)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
