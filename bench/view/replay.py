#!/usr/bin/env python3
"""The paired replay (issue 44, §10 of the context design): for a place's last
N exchanges, what the TRANSCRIPT's request was made of and what the VIEW
would have held, side by side, from a scratch daemon on a COPY of a store.

    python3 bench/view/replay.py --bin-dir target/release --store /tmp/state-copy \\
        --place 'local:/path/of/the/place' --turns 50 --out /tmp/replay

It costs nothing: no model is called (the scratch daemon's model is an
address nothing listens on; `scratch.py`).

The two sides:

- **transcript**: the ledger's measuring row of each exchange
  (`context.turn`, read whole through `ledger.tail`): the sent context's
  parts in tokens (est.), and `prev`, whether the place's previous exchange
  was in the context sent. This is what the request WAS, recorded when it
  ran.
- **view**: `theseus view preview PLACE --turns N --json`, the view as of each
  of those exchanges, assembled by the turn's own assembler: its tally by
  slot, what the budget left out and why, the fallback a turn would have
  taken, the window and the folds. It is assembled NOW from the store as it
  stands, for an empty message (no message's words are sent to the daemon).
  When the daemon's preview has no `--turns` form, the replay takes the one
  preview it has (the place's NEXT turn) and pairs it with no exchange: each
  exchange's view side is then `null`, with the reason in the summary. A
  daemon with no preview at all (a build before it) gives the transcript's
  side alone, and says so.

Output (`--out`): `rows.jsonl`, one line per exchange, and `summary.json`;
the summary is also printed. **Ids, kinds and counts only**: every value
written is a number, a boolean, or a word from a closed alphabet (`word`),
picked from the daemon's answers field by field; a piece's text, a message
and a place's name are never copied, even when the daemon's answer carries
them. `scripts/tests/test_view_kit.py` holds it on answers full of marker
strings.

Standard library only.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from collections import Counter
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import scratch  # noqa: E402

TURN_KIND = "context.turn"
# A word the kit may write: an id, a kind, a reason. Anything else is written as OTHER.
_WORD = re.compile(r"[A-Za-z0-9_.:-]{1,80}")
OTHER = "other"
# The measuring row's parts of the sent context (`exchange/ledger.rs`, `Sent`).
SENT_PARTS = ("header", "tools", "files", "replies", "calls", "words", "results_whole", "results_cleared",
              "recall", "other", "total")
NO_TURNS = "the daemon's view preview has no --turns form: only the next turn's view was assembled"
NO_PREVIEW = "the daemon has no view preview: the transcript's side alone"


def word(v) -> str | None:
    """`v` when it is a word of the closed alphabet (an id, a kind, a reason), else OTHER; None stays None."""
    if v is None:
        return None
    return v if isinstance(v, str) and _WORD.fullmatch(v) else OTHER


def count(v) -> int:
    return v if isinstance(v, int) and not isinstance(v, bool) and v >= 0 else 0


def counts(m) -> dict[str, int]:
    """A map of words to counts, kept word by word."""
    return {word(k): count(n) for k, n in m.items()} if isinstance(m, dict) else {}


# ---- the transcript's side


def transcript_side(data: dict) -> dict:
    sent = data.get("sent") if isinstance(data.get("sent"), dict) else {}
    return {
        "tokens": {p: count(sent.get(p)) for p in SENT_PARTS},
        "prev": word(data.get("prev")),
        "cold": data.get("cold") is True,
        "overlap": data.get("overlap") is True,
    }


def turn_rows(rows: list[dict], place: str, n: int) -> list[dict]:
    """The place's last `n` measuring rows, oldest first."""
    mine = [r for r in rows if isinstance(r.get("data"), dict) and r["data"].get("place") == place]
    return mine[-n:] if n > 0 else mine


# ---- the view's side


def view_side(p: dict) -> dict:
    """One preview, as counts: by slot, what was left out (by why and by reason), the fallback, the window,
    the folds and the budget, and the exchange ids the view lays (for `prev_in_view`; never text)."""
    tally = p.get("tally") if isinstance(p.get("tally"), dict) else {}
    slots = {word(s): {"pieces": count(c.get("pieces")), "tokens": count(c.get("tokens"))}
             for s, c in (tally.get("slots") or {}).items() if isinstance(c, dict)}
    left = {word(s): {"pieces": count(c.get("pieces")), "tokens": count(c.get("tokens"))}
            for s, c in (tally.get("dropped") or {}).items() if isinstance(c, dict)}
    left_reasons = {word(s): count(c.get("pieces")) for s, c in (tally.get("dropped_reasons") or {}).items()
                    if isinstance(c, dict)}
    held: set[str] = set()
    view = p.get("view") if isinstance(p.get("view"), dict) else {}
    for slot in view.get("slots") or []:
        for piece in (slot.get("pieces") or []) if isinstance(slot, dict) else []:
            if isinstance(piece, dict) and piece.get("kind") == "exchanges":
                held.update(w for w in (word(piece.get("first")), word(piece.get("last"))) if w and w != OTHER)
    budget = p.get("budget") if isinstance(p.get("budget"), dict) else {}
    return {
        "tokens": count(tally.get("tokens")),
        "slots": slots,
        "left_out": left,
        "left_out_reasons": left_reasons,
        "fallback": word(p.get("fallback")),
        "overflow": p.get("overflow") is not None,
        "window": count(p.get("window")),
        "folds": counts(p.get("folds")),
        "budget": {k: count(budget.get(k)) for k in ("provider_window", "reserve", "ceiling", "here_allowance")},
        "_held": held,
    }


def previews(d, place: str, n: int) -> tuple[list[dict], str | None]:
    """The view as of each of the place's last `n` exchanges, oldest first, and a note when only the next
    turn's could be had. Never `--text`, never `--message`."""
    r = d.cli("--json", "view", "preview", place, "--turns", str(n))
    if r.returncode == 0:
        v = json.loads(r.stdout)
        got = v.get("turns") if isinstance(v, dict) else v
        if isinstance(got, list):
            return [x for x in got if isinstance(x, dict)], None
    try:
        one = d.json("view", "preview", place)
    except RuntimeError:
        return [], NO_PREVIEW
    return [one], NO_TURNS


def as_of(p: dict) -> str | None:
    """The exchange a preview was assembled as of, when the daemon says."""
    for k in ("as_of", "exchange"):
        if isinstance(p.get(k), str):
            return word(p[k])
    return None


# ---- the pairing


def pair(rows: list[dict], views: list[dict], note: str | None) -> list[dict]:
    """One row per exchange: its ids, the transcript's side, the view's (or None) and whether the view held
    the previous exchange (`prev_in_view`: by its id among the pieces the view lays; else, with no fallback,
    by the window: the window lays the newest exchanges of the place, the previous one first)."""
    by_id = {as_of(v): v for v in views if as_of(v)}
    ordered = note is None and not by_id and len(views) == len(rows)
    out = []
    for i, r in enumerate(rows):
        data = r["data"]
        ex = word(data.get("exchange"))
        prev = word(data.get("prev_exchange"))
        v = by_id.get(ex) or (views[i] if ordered else None)
        side = view_side(v) if v is not None else None
        held = None
        basis = None
        if side is not None:
            ids = side.pop("_held")
            if prev is None:
                held, basis = None, "no_previous"
            elif ids:
                held, basis = prev in ids, "ids"
            elif side["fallback"] is None:
                held, basis = side["window"] >= 1, "window"
            else:
                held, basis = False, "fallback"
        out.append({
            "exchange": ex,
            "prev_exchange": prev,
            "transcript": transcript_side(data),
            "view": side,
            "prev_in_view": held,
            "prev_in_view_basis": basis,
        })
    return out


def summarize(place_rows: list[dict], note: str | None, now: dict | None) -> dict:
    t_tokens: Counter = Counter()
    v_tokens: Counter = Counter()
    fallback: Counter = Counter()
    prev_t: Counter = Counter()
    prev_v: Counter = Counter()
    left: dict[str, Counter] = {}
    left_reasons: Counter = Counter()
    with_view = 0
    for r in place_rows:
        t_tokens.update(r["transcript"]["tokens"])
        prev_t[r["transcript"]["prev"] or "none"] += 1
        v = r["view"]
        if v is None:
            continue
        with_view += 1
        v_tokens["total"] += v["tokens"]
        for s, c in v["slots"].items():
            v_tokens[s] += c["tokens"]
        fallback[v["fallback"] or "takes_the_view"] += 1
        prev_v[{True: "held", False: "not_held", None: "no_previous"}[r["prev_in_view"]]] += 1
        for why, c in v["left_out"].items():
            left.setdefault(why, Counter()).update(c)
        left_reasons.update(v["left_out_reasons"])
    n = len(place_rows)
    s = {
        "exchanges": n,
        "with_a_view_row": with_view,
        "tokens": {
            "transcript": {"sum": dict(t_tokens), "mean": {k: round(x / n) for k, x in t_tokens.items()} if n else {}},
            "view": {"sum": dict(v_tokens),
                     "mean": {k: round(x / with_view) for k, x in v_tokens.items()} if with_view else {}},
        },
        "view_fallback": dict(fallback),
        "previous_exchange": {"transcript": dict(prev_t), "view": dict(prev_v)},
        "left_out_by_why": {k: dict(c) for k, c in left.items()},
        "left_out_by_reason": dict(left_reasons),
        "notes": [note] if note else [],
    }
    if now is not None:
        side = view_side(now)
        side.pop("_held")
        s["next_turn_view"] = side
    return s


def replay(d, place: str, n: int) -> tuple[list[dict], dict]:
    rows = turn_rows(scratch.ledger_rows(d, TURN_KIND), place, n)
    views, note = previews(d, place, n)
    paired = pair(rows, views, note)
    return paired, summarize(paired, note, views[0] if note and views else None)


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--bin-dir", default="target/release", help="where theseus and theseusd are")
    ap.add_argument("--store", required=True, help="a COPY of a daemon's state directory (never the live one)")
    ap.add_argument("--place", required=True, help="the place, as the ledger names it")
    ap.add_argument("--turns", type=int, default=50, help="the place's last N exchanges (0: all)")
    ap.add_argument("--out", required=True, help="a new directory for rows.jsonl, summary.json and the daemon")
    a = ap.parse_args(argv)
    out = Path(a.out)
    if out.exists() and any(out.iterdir()):
        raise SystemExit(f"replay: {out} is not empty")
    out.mkdir(parents=True, exist_ok=True)
    with scratch.Scratch(Path(a.bin_dir), out, store=Path(a.store)) as d:
        rows, summary = replay(d, a.place, a.turns)
    with (out / "rows.jsonl").open("w") as f:
        for r in rows:
            f.write(json.dumps(r, sort_keys=True) + "\n")
    (out / "summary.json").write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n")
    print(json.dumps(summary, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    sys.exit(main())
