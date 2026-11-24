#!/usr/bin/env python3
"""The paired replay (issue 44, §10 of the context design): for a place's last
N exchanges, what context was RECORDED as sent for each turn, beside what
that turn's VIEW laid, from a scratch daemon on a COPY of a store.

    python3 bench/view/replay.py --bin-dir target/release --store /tmp/state-copy \\
        --place 'local:/path/of/the/place' --turns 50 --out /tmp/replay

It costs nothing: no model is called (the scratch daemon's model is an
address nothing listens on; `scratch.py`).

The two sides:

- **recorded**: the ledger's measuring row of each exchange (`context.turn`,
  read whole through `ledger.tail`): the parts of the context actually sent,
  in tokens (est.), and `prev`, whether the place's previous exchange was in
  it;
- **view**: the turn's own view record, as `theseus view show TURN --tally
  --json` reads it (never `--text`): the first view the turn was assembled
  with (a turn that calls tools again has more; their number is `views`),
  its counts by slot, what the budget left out and why, and its pieces by
  kind. Every turn since the cut has its record; a turn with none (one from
  before) has no view side, and is counted under `no_view_record`, never as
  absent from the view.

Whether the view held the previous exchange (`prev_in_view`) is read from
the ranges its pieces lay, interiors included, resolved against each
place's exchange order: `ids` when an `exchanges` piece lays it, `folded`
when it lies only inside a `folds` piece (a fold's first and last are
exchange ids too: `ladder/view.rs`, `shown`); a range whose ends are not
both in one place's order makes the answer `null` (`unknown_range`), never
`false`.

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
# The daemon's enums, by field (theseus-core: exchange/ledger.rs `Prev`, view/mod.rs `Slot`, `Reason` and
# `Space`, the budgeter's `DropWhy`).
PREV = {"none", "transcript", "place_tail", "missed"}
SLOTS = {"frame", "who_and_where", "waiting", "in_play", "past", "elsewhere", "here", "called", "now"}
DROP_WHY = {"merged", "over_budget", "unit_cut", "in_window"}
REASONS = {"floor", "window", "ladder", "elsewhere", "past", "waiting", "at_hand", "in_play", "named", "linked",
           "reached", "task", "recalled", "correction", "fixed"}
KINDS = {"exchanges", "folds", "facts", "card", "task", "waiting", "recalled", "code"}
# The measuring row's parts of the sent context (`exchange/ledger.rs`, `Sent`).
SENT_PARTS = ("header", "tools", "files", "replies", "calls", "words", "results_whole", "results_cleared",
              "recall", "other", "total")


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


def merged(m, allowed: set[str]) -> dict[str, dict]:
    """A map of names to counts, the names checked; names that become OTHER add up."""
    out: dict[str, Counter] = {}
    for k, c in (m.items() if isinstance(m, dict) else []):
        name = k if k in allowed else OTHER
        out.setdefault(name, Counter()).update(pc(c))
    return {k: dict(v) for k, v in out.items()}


# ---- the recorded side


def recorded_side(data: dict) -> dict:
    sent = data.get("sent") if isinstance(data.get("sent"), dict) else {}
    return {
        "tokens": {p: count(sent.get(p)) for p in SENT_PARTS},
        "prev": one_of(data.get("prev"), PREV),
        "cold": data.get("cold") is True,
        "overlap": data.get("overlap") is True,
    }


def turn_rows(rows: list[dict], place: str) -> list[dict]:
    """The place's measuring rows, oldest first."""
    return [r for r in rows if isinstance(r.get("data"), dict) and r["data"].get("place") == place]


# ---- the view's side


def orders_of(rows: list[dict]) -> dict[str, list[str]]:
    """Each place's exchange ids, oldest first, from every measuring row."""
    out: dict[str, list[str]] = {}
    for r in rows:
        d = r.get("data") if isinstance(r.get("data"), dict) else {}
        e = an_id(d.get("exchange"))
        if e not in (None, OTHER) and isinstance(d.get("place"), str):
            out.setdefault(d["place"], []).append(e)
    return out


def laid(view: dict, orders: dict[str, list[str]], kind: str) -> tuple[set[str], int]:
    """The exchange ids a view's pieces of `kind` (`exchanges` or `folds`) lay, range interiors included, and
    how many of those ranges could not be resolved against one place's order."""
    at = {e: (p, i) for p, ids in orders.items() for i, e in enumerate(ids)}
    held, unresolved = set(), 0
    for slot in view.get("slots") or []:
        for piece in (slot.get("pieces") or []) if isinstance(slot, dict) else []:
            if not isinstance(piece, dict) or piece.get("kind") != kind:
                continue
            first, last = an_id(piece.get("first")), an_id(piece.get("last"))
            if first in (None, OTHER) or last in (None, OTHER):
                unresolved += 1
            elif first == last:
                held.add(first)
            elif first in at and last in at and at[first][0] == at[last][0]:
                ids = orders[at[first][0]]
                i, j = sorted((at[first][1], at[last][1]))
                held.update(ids[i:j + 1])
            else:
                unresolved += 1
    return held, unresolved


def view_side(shown: dict, orders: dict[str, list[str]]) -> dict | None:
    """A turn's record (`view.show`'s answer), as counts: its first view, by slot, what was left out (by why
    and by reason) and its pieces by kind; and, apart (`_held`, `_folded`, `_unresolved`), what it lays of
    the store's exchanges. None when the turn has no view record."""
    views = [v for v in shown.get("views") or [] if isinstance(v, dict)] if isinstance(shown, dict) else []
    if not views:
        return None
    first = views[0]
    tallies = [t for t in shown.get("tallies") or [] if isinstance(t, dict)]
    tally = next((t for t in tallies if t.get("compilation") == first.get("compilation")), {})
    kinds: Counter = Counter()
    for slot in first.get("slots") or []:
        for piece in (slot.get("pieces") or []) if isinstance(slot, dict) else []:
            if isinstance(piece, dict):
                kinds[one_of(piece.get("kind"), KINDS)] += 1
    held, u1 = laid(first, orders, "exchanges")
    folded, u2 = laid(first, orders, "folds")
    totals = first.get("totals") if isinstance(first.get("totals"), dict) else {}
    return {
        "views": len(views),
        "tokens": count(tally.get("tokens")) or count(totals.get("tokens")),
        "slots": merged(tally.get("slots"), SLOTS),
        "left_out": merged(tally.get("dropped"), DROP_WHY),
        "left_out_reasons": {k: c["pieces"] for k, c in merged(tally.get("dropped_reasons"), REASONS).items()},
        "kinds": dict(kinds),
        "_held": held,
        "_folded": folded - held,
        "_unresolved": u1 + u2,
    }


def records(d, rows: list[dict]) -> dict[str, dict]:
    """Each row's turn's record, by exchange id: `view show <turn> --tally`, never `--text`."""
    out = {}
    for r in rows:
        ex = an_id(r["data"].get("exchange"))
        if ex not in (None, OTHER):
            out[ex] = d.json("view", "show", ex, "--tally")
    return out


# ---- the pairing


def pair(rows: list[dict], shown: dict[str, dict], orders: dict[str, list[str]]) -> list[dict]:
    """One row per exchange: its ids, the recorded side, its own view record's side (or None) and whether
    that view held the previous exchange: laid whole (`ids`), only inside a fold (`folded`), unknown when a
    range cannot be resolved, else not held."""
    out = []
    for r in rows:
        data = r["data"]
        ex = an_id(data.get("exchange"))
        prev = an_id(data.get("prev_exchange"))
        side = view_side(shown.get(ex) or {}, orders)
        held, basis = None, "no_view_record"
        if side is not None:
            ids, folded, unresolved = side.pop("_held"), side.pop("_folded"), side.pop("_unresolved")
            if prev is None:
                basis = "no_previous"
            elif prev in ids:
                held, basis = True, "ids"
            elif prev in folded:
                held, basis = True, "folded"
            elif unresolved:
                basis = "unknown_range"
            else:
                held, basis = False, "ids"
        out.append({
            "exchange": ex,
            "prev_exchange": prev,
            "recorded": recorded_side(data),
            "view": side,
            "prev_in_view": held,
            "prev_in_view_basis": basis,
        })
    return out


def summarize(place_rows: list[dict]) -> dict:
    r_tokens: Counter = Counter()
    v_tokens: Counter = Counter()
    prev_r: Counter = Counter()
    prev_v: Counter = Counter()
    left: dict[str, Counter] = {}
    left_reasons: Counter = Counter()
    kinds: Counter = Counter()
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
        key = {True: "held" if r["prev_in_view_basis"] == "ids" else "folded", False: "not_held"}.get(
            r["prev_in_view"], "no_previous" if r["prev_in_view_basis"] == "no_previous" else "unknown")
        prev_v[key] += 1
        for why, c in v["left_out"].items():
            left.setdefault(why, Counter()).update(c)
        left_reasons.update(v["left_out_reasons"])
        kinds.update(v["kinds"])
    n = len(place_rows)
    return {
        "exchanges": n,
        "with_a_view_record": with_view,
        "no_view_record": n - with_view,
        "tokens": {
            "recorded": {"sum": dict(r_tokens), "mean": {k: round(x / n) for k, x in r_tokens.items()} if n else {}},
            "view": {"sum": dict(v_tokens),
                     "mean": {k: round(x / with_view) for k, x in v_tokens.items()} if with_view else {}},
        },
        "previous_exchange": {"recorded": dict(prev_r), "view": dict(prev_v)},
        "left_out_by_why": {k: dict(c) for k, c in left.items()},
        "left_out_by_reason": dict(left_reasons),
        "pieces_by_kind": dict(kinds),
    }


def replay(d, place: str, n: int) -> tuple[list[dict], dict]:
    every = scratch.ledger_rows(d, TURN_KIND)
    mine = turn_rows(every, place)
    rows = mine[-n:] if n > 0 else mine
    paired = pair(rows, records(d, rows), orders_of(every))
    return paired, summarize(paired)


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
