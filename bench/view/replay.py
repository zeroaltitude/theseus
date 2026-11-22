#!/usr/bin/env python3
"""The paired replay (issue 44, §10 of the context design): for a place's last
N exchanges, what context was RECORDED as sent for each turn, beside what the
VIEW would have held, from a scratch daemon on a COPY of a store.

    python3 bench/view/replay.py --bin-dir target/release --store /tmp/state-copy \\
        --place 'local:/path/of/the/place' --turns 50 --out /tmp/replay

It costs nothing: no model is called (the scratch daemon's model is an
address nothing listens on; `scratch.py`).

The two sides:

- **recorded**: the ledger's measuring row of each exchange (`context.turn`,
  read whole through `ledger.tail`): the parts of the context actually sent,
  in tokens (est.), and `prev`, whether the place's previous exchange was in
  it. The row does not record which strategy built that context (the
  transcript's, or a view place's turn), so the replay does not say:
  `strategy` is `not_recorded` on every row.
- **view**: `theseus view preview PLACE --turns N --json`, the view as of each
  of those exchanges, assembled by the turn's own assembler: its tally by
  slot, what the budget left out and why, the fallback a turn would have
  taken, the window and the folds. It is assembled NOW from the store as it
  stands, for an empty message (no message's words are sent to the daemon).
  When the daemon's preview has no `--turns` form, the replay takes the one
  preview it has (the place's NEXT turn) and pairs it with no exchange: each
  exchange's view side is then `null`, with the reason in the summary. A
  daemon with no preview at all (a build before it) gives the recorded side
  alone, and says so.

Whether the view held the previous exchange (`prev_in_view`) is read from
the exchange ranges its pieces lay, interiors included (resolved against the
place's exchange order); a range whose ends are not both in that order makes
the answer `null` (`unknown_range`), never `false`.

Output (`--out`, checked before anything is written): `rows.jsonl`, one line
per exchange, and `summary.json`; the summary is also printed. **Ids, kinds
and counts only**: every value written is a number, a boolean, an id of the
store's shape (`ID`), or a name from the closed set of its field (the
daemon's own enums); anything else is written `other`. A failure is a fixed
code (`scratch.Failed`). `scripts/tests/test_view_kit.py` holds it on answers
full of marker strings.

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
OTHER = "other"
# The store's ids (`<prefix>_<uuid v7, simple>`, theseus-kernel's `new_id`).
ID = re.compile(r"[a-z]{1,16}_[0-9a-f]{32}")
# The daemon's enums, by field (theseus-core: exchange/ledger.rs `Prev`, the fallback reasons of
# turn/view_step.rs, view/mod.rs `Slot` and `Reason`, the budgeter's drops, view.rs's piece kinds).
PREV = {"none", "transcript", "place_tail", "missed"}
FALLBACK = {"continuation", "route", "unledgered_history", "over_budget", "assembly_error"}
SLOTS = {"frame", "who_and_where", "waiting", "in_play", "past", "elsewhere", "here", "called", "now"}
DROP_WHY = {"merged", "over_budget", "unit_cut"}
REASONS = {"floor", "window", "ladder", "elsewhere", "past", "waiting", "at_hand", "in_play", "named", "linked",
           "reached", "task", "recalled", "correction", "fixed"}
LEVEL = re.compile(r"[0-9]{1,2}")
# The measuring row's parts of the sent context (`exchange/ledger.rs`, `Sent`).
SENT_PARTS = ("header", "tools", "files", "replies", "calls", "words", "results_whole", "results_cleared",
              "recall", "other", "total")
NO_TURNS = "the daemon's view preview has no --turns form: only the next turn's view was assembled"
NO_PREVIEW = "the daemon has no view preview: the recorded side alone"
STRATEGY = "not_recorded"


def one_of(v, allowed: set[str]) -> str | None:
    """`v` when it is a name of its field's closed set, else OTHER; None stays None."""
    if v is None:
        return None
    return v if isinstance(v, str) and v in allowed else OTHER


def an_id(v) -> str | None:
    if v is None:
        return None
    return v if isinstance(v, str) and ID.fullmatch(v) else OTHER


def count(v) -> int:
    return v if isinstance(v, int) and not isinstance(v, bool) and v >= 0 else 0


def pc(c) -> dict:
    return {"pieces": count(c.get("pieces")), "tokens": count(c.get("tokens"))} if isinstance(c, dict) \
        else {"pieces": 0, "tokens": 0}


def merged(m, allowed: set[str] | None, shape: re.Pattern | None = None) -> dict[str, dict]:
    """A map of names to counts, the names checked; names that become OTHER add up."""
    out: dict[str, Counter] = {}
    for k, c in (m.items() if isinstance(m, dict) else []):
        name = k if (allowed is not None and k in allowed) or (shape is not None and shape.fullmatch(str(k))) \
            else OTHER
        out.setdefault(name, Counter()).update(pc(c))
    return {k: dict(v) for k, v in out.items()}


# ---- the recorded side


def recorded_side(data: dict) -> dict:
    sent = data.get("sent") if isinstance(data.get("sent"), dict) else {}
    return {
        "strategy": STRATEGY,
        "tokens": {p: count(sent.get(p)) for p in SENT_PARTS},
        "prev": one_of(data.get("prev"), PREV),
        "cold": data.get("cold") is True,
        "overlap": data.get("overlap") is True,
    }


def turn_rows(rows: list[dict], place: str) -> list[dict]:
    """The place's measuring rows, oldest first."""
    return [r for r in rows if isinstance(r.get("data"), dict) and r["data"].get("place") == place]


# ---- the view's side


def held_exchanges(p: dict, order: list[str]) -> tuple[set[str], int]:
    """The exchange ids the view's pieces lay, range interiors included, and how many ranges could not be
    resolved against `order` (the place's exchange ids, oldest first)."""
    at = {e: i for i, e in enumerate(order)}
    held, unresolved = set(), 0
    view = p.get("view") if isinstance(p.get("view"), dict) else {}
    for slot in view.get("slots") or []:
        for piece in (slot.get("pieces") or []) if isinstance(slot, dict) else []:
            if not isinstance(piece, dict) or piece.get("kind") != "exchanges":
                continue
            first, last = an_id(piece.get("first")), an_id(piece.get("last"))
            if first in (None, OTHER) or last in (None, OTHER):
                unresolved += 1
            elif first == last:
                held.add(first)
            elif first in at and last in at:
                i, j = sorted((at[first], at[last]))
                held.update(order[i:j + 1])
            else:
                unresolved += 1
    return held, unresolved


def view_side(p: dict, order: list[str]) -> dict:
    """One preview, as counts: by slot, what was left out (by why and by reason), the fallback, the window,
    the folds and the budget; and, apart (`_held`, `_unresolved`), what it lays of the place's exchanges."""
    tally = p.get("tally") if isinstance(p.get("tally"), dict) else {}
    budget = p.get("budget") if isinstance(p.get("budget"), dict) else {}
    held, unresolved = held_exchanges(p, order)
    folds = {}
    for k, n in (p.get("folds") or {}).items() if isinstance(p.get("folds"), dict) else []:
        name = k if LEVEL.fullmatch(str(k)) else OTHER
        folds[name] = folds.get(name, 0) + count(n)
    return {
        "tokens": count(tally.get("tokens")),
        "slots": merged(tally.get("slots"), SLOTS),
        "left_out": merged(tally.get("dropped"), DROP_WHY),
        "left_out_reasons": {k: c["pieces"] for k, c in merged(tally.get("dropped_reasons"), REASONS).items()},
        "fallback": one_of(p.get("fallback"), FALLBACK),
        "overflow": p.get("overflow") is not None,
        "window": count(p.get("window")),
        "folds": folds,
        "budget": {k: count(budget.get(k)) for k in ("provider_window", "reserve", "ceiling", "here_allowance")},
        "_held": held,
        "_unresolved": unresolved,
    }


def previews(d, place: str, n: int) -> tuple[list[dict], str | None]:
    """The view as of each of the place's last `n` exchanges, oldest first, and a note when only the next
    turn's could be had. Never `--text`, never `--message`."""
    r = d.cli("--json", "view", "preview", place, "--turns", str(n))
    if r.returncode == 0:
        try:
            v = json.loads(r.stdout)
        except json.JSONDecodeError:
            v = None
        got = v.get("turns") if isinstance(v, dict) else v
        if isinstance(got, list):
            return [x for x in got if isinstance(x, dict)], None
    try:
        one = d.json("view", "preview", place)
    except scratch.Failed:
        return [], NO_PREVIEW
    return ([one], NO_TURNS) if isinstance(one, dict) else ([], NO_PREVIEW)


def as_of(p: dict) -> str | None:
    """The exchange a preview was assembled as of, when the daemon says."""
    for k in ("as_of", "exchange"):
        v = an_id(p.get(k))
        if v not in (None, OTHER):
            return v
    return None


# ---- the pairing


def pair(rows: list[dict], views: list[dict], note: str | None, order: list[str]) -> list[dict]:
    """One row per exchange: its ids, the recorded side, the view's (or None) and whether the view held the
    previous exchange: by the ranges it lays; unknown when a range cannot be resolved; else, with no
    exchange piece and no fallback, by the window (it lays the newest exchanges of the place first)."""
    by_id = {as_of(v): v for v in views if as_of(v)}
    ordered = note is None and not by_id and len(views) == len(rows)
    out = []
    for i, r in enumerate(rows):
        data = r["data"]
        ex = an_id(data.get("exchange"))
        prev = an_id(data.get("prev_exchange"))
        v = by_id.get(ex) or (views[i] if ordered else None)
        side = view_side(v, order) if v is not None else None
        held = basis = None
        if side is not None:
            ids, unresolved = side.pop("_held"), side.pop("_unresolved")
            if prev is None:
                basis = "no_previous"
            elif prev in ids:
                held, basis = True, "ids"
            elif unresolved:
                basis = "unknown_range"
            elif ids:
                held, basis = False, "ids"
            elif side["fallback"] is None:
                held, basis = side["window"] >= 1, "window"
            else:
                held, basis = False, "fallback"
        out.append({
            "exchange": ex,
            "prev_exchange": prev,
            "recorded": recorded_side(data),
            "view": side,
            "prev_in_view": held,
            "prev_in_view_basis": basis,
        })
    return out


def summarize(place_rows: list[dict], note: str | None, now: dict | None, order: list[str]) -> dict:
    r_tokens: Counter = Counter()
    v_tokens: Counter = Counter()
    fallback: Counter = Counter()
    prev_r: Counter = Counter()
    prev_v: Counter = Counter()
    left: dict[str, Counter] = {}
    left_reasons: Counter = Counter()
    with_view = 0
    for r in place_rows:
        r_tokens.update(r["recorded"]["tokens"])
        prev_r[r["recorded"]["prev"] or "none"] += 1
        v = r["view"]
        if v is None:
            continue
        with_view += 1
        v_tokens["total"] += v["tokens"]
        for s, c in v["slots"].items():
            v_tokens[s] += c["tokens"]
        fallback[v["fallback"] or "takes_the_view"] += 1
        key = {True: "held", False: "not_held"}.get(r["prev_in_view"],
                                                    "no_previous" if r["prev_in_view_basis"] == "no_previous"
                                                    else "unknown")
        prev_v[key] += 1
        for why, c in v["left_out"].items():
            left.setdefault(why, Counter()).update(c)
        left_reasons.update(v["left_out_reasons"])
    n = len(place_rows)
    s = {
        "exchanges": n,
        "with_a_view_row": with_view,
        "recorded_strategy": STRATEGY,
        "tokens": {
            "recorded": {"sum": dict(r_tokens), "mean": {k: round(x / n) for k, x in r_tokens.items()} if n else {}},
            "view": {"sum": dict(v_tokens),
                     "mean": {k: round(x / with_view) for k, x in v_tokens.items()} if with_view else {}},
        },
        "view_fallback": dict(fallback),
        "previous_exchange": {"recorded": dict(prev_r), "view": dict(prev_v)},
        "left_out_by_why": {k: dict(c) for k, c in left.items()},
        "left_out_by_reason": dict(left_reasons),
        "notes": [note] if note else [],
    }
    if now is not None:
        side = view_side(now, order)
        side.pop("_held")
        side.pop("_unresolved")
        s["next_turn_view"] = side
    return s


def replay(d, place: str, n: int) -> tuple[list[dict], dict]:
    mine = turn_rows(scratch.ledger_rows(d, TURN_KIND), place)
    order = [e for e in (an_id(r["data"].get("exchange")) for r in mine) if e not in (None, OTHER)]
    rows = mine[-n:] if n > 0 else mine
    views, note = previews(d, place, n)
    paired = pair(rows, views, note, order)
    return paired, summarize(paired, note, views[0] if note == NO_TURNS and views else None, order)


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--bin-dir", default="target/release", help="where theseus and theseusd are")
    ap.add_argument("--store", required=True, help="a COPY of a daemon's state directory (never the live one)")
    ap.add_argument("--place", required=True, help="the place, as the ledger names it")
    ap.add_argument("--turns", type=int, default=50, help="the place's last N exchanges (0: all)")
    ap.add_argument("--out", required=True, help="a new directory for rows.jsonl, summary.json and the daemon")
    a = ap.parse_args(argv)
    store = scratch.check_copy(Path(a.store))
    out = scratch.check_out(Path(a.out), store)
    out.mkdir(parents=True, exist_ok=True)
    try:
        with scratch.Scratch(Path(a.bin_dir), out, store=store) as d:
            rows, summary = replay(d, a.place, a.turns)
    except scratch.Failed as e:
        raise SystemExit(f"replay: {e}") from None
    with (out / "rows.jsonl").open("w") as f:
        for r in rows:
            f.write(json.dumps(r, sort_keys=True) + "\n")
    (out / "summary.json").write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n")
    print(json.dumps(summary, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    sys.exit(main())
