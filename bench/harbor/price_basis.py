"""Which price table billed a record, and a guard against comparing two.

A comparison column whose price table moved between the two runs has caused
four published corrections (regress-1026 §8). `theseus-rko44` moved the Sonnet
5.5 cache-read price from $0.10 to $0.20 a million tokens on 2026-10-25, so a
round before it and a round after it are billed on different tables, and a ratio
of their recorded dollars is a price change and a behaviour change mixed. The
ambiguity was always in the files: a record's `cost_usd` is as billed, and its
tokens say what it would have cost on any other table.

- `table_digest` names a table: the first twelve hex digits of the sha256 of its
  canonical JSON.
- `measure` says which known table a recorded cost fits, from the record's own
  tokens, never from its date: a harness that carries its own catalog can bill
  the old entry after the move (OpenHands did, on 2026-10-27).
- `stamp` is what `efficiency.write` puts in `efficiency.json`:
  `price_digest`, `price_basis` (how it was learned) and `list_price_digest` (the
  table `list_cost_usd` is priced on).
- `compare` refuses a ratio across two digests, says which it saw, and prints one
  only on one basis, or on a table both arms were repriced onto.

Only the cache-read entry moved; input, output and cache-write are identical on
both tables.
"""

from __future__ import annotations

import copy
import hashlib
import json
from typing import Any, Callable, Iterable, Mapping

CLASSES = ("input", "cache_read", "cache_write", "output")
CURRENT = "list-2026-10-25"
PRE_RKO44 = "list-pre-rko44"

# The same rows as `efficiency.LIST_PRICES` (a test holds them equal), dollars a
# million tokens. `PRE_RKO44` is that table before the Sonnet 5.5 cache-read move.
_CURRENT_TABLE: dict[str, dict[str, float]] = {
    "claude-sonnet-5-5": {"input": 2.0, "cache_read": 0.2, "cache_write": 2.5, "output": 10.0},
    "claude-opus-5-5": {"input": 4.0, "cache_read": 0.2, "cache_write": 5.0, "output": 20.0},
    "claude-fable-5-1": {"input": 10.0, "cache_read": 0.25, "cache_write": 12.5, "output": 50.0},
    "claude-haiku-5-5": {"input": 0.1, "cache_read": 0.01, "cache_write": 0.125, "output": 0.5},
    "claude-sonnet-5": {"input": 2.0, "cache_read": 0.2, "cache_write": 2.5, "output": 10.0},
    "claude-opus-5": {"input": 5.0, "cache_read": 0.5, "cache_write": 6.25, "output": 25.0},
    "glm-5.3": {"input": 1.4, "cache_read": 0.26, "cache_write": 1.4, "output": 4.4},
    "kimi-k3.1-pro": {"input": 2.0, "cache_read": 0.2, "cache_write": 2.0, "output": 10.0},
    "deepseek-v4-pro": {"input": 1.32, "cache_read": 0.044, "cache_write": 1.32, "output": 3.96},
    "gpt-6.1-sol": {"input": 2.0, "cache_read": 0.1, "cache_write": 2.5, "output": 10.0},
    "gpt-5.6-sol": {"input": 4.0, "cache_read": 0.4, "cache_write": 5.0, "output": 20.0},
    "gpt-6-astra": {"input": 10.0, "cache_read": 1.0, "cache_write": 12.5, "output": 50.0},
}
_PRE_RKO44_TABLE = copy.deepcopy(_CURRENT_TABLE)
_PRE_RKO44_TABLE["claude-sonnet-5-5"]["cache_read"] = 0.10

TABLES: dict[str, dict[str, dict[str, float]]] = {
    CURRENT: _CURRENT_TABLE,
    PRE_RKO44: _PRE_RKO44_TABLE,
}


class CrossBasisError(ValueError):
    """A comparison that would cross two price tables, or that cannot show it does not."""


def table_digest(table: Mapping[str, Mapping[str, float]]) -> str:
    canon = json.dumps({m: {c: float(p[c]) for c in sorted(p)} for m, p in sorted(table.items())},
                       separators=(",", ":"), sort_keys=True)
    return hashlib.sha256(canon.encode()).hexdigest()[:12]


def digest_of(name: str) -> str:
    return table_digest(TABLES[name])


def name_of(digest: str | None) -> str | None:
    for name in TABLES:
        if digest_of(name) == digest:
            return name
    return None


def reprice(by_model: Mapping[str, Mapping[str, Any]], table: Mapping[str, Mapping[str, float]]) -> float | None:
    """`by_model`'s tokens at `table`'s prices; None when a model with tokens is not in it."""
    total = 0.0
    for name, m in by_model.items():
        n = sum(int(m.get(c) or 0) for c in CLASSES)
        if not n:
            continue
        p = table.get(name.split("/")[-1])
        if p is None:
            return None
        total += sum(int(m.get(c) or 0) * p[c] for c in CLASSES) / 1e6
    return round(total, 6)


def measure(by_model: Mapping[str, Mapping[str, Any]], recorded: float | None,
            rel_tol: float = 2e-3) -> tuple[str | None, str]:
    """(digest, how): which known table `recorded` fits. `how` is `measured` when
    exactly one does, `ambiguous` when several do (a bill with no cache reads fits
    both), `unmatched` when none, `unmeasurable` when a model is on no table and
    `unpriced` when the record has no cost."""
    if recorded is None:
        return None, "unpriced"
    fits = []
    for name, table in TABLES.items():
        priced = reprice(by_model, table)
        if priced is None:
            return None, "unmeasurable"
        if abs(priced - recorded) <= max(1e-4, rel_tol * abs(recorded)):
            fits.append(name)
    if len(fits) == 1:
        return digest_of(fits[0]), "measured"
    return None, "ambiguous" if fits else "unmatched"


def stamp(rec: Mapping[str, Any]) -> dict[str, Any]:
    """The keys `efficiency.write` adds to a record. A model's own bill is the
    one measured when Jev's dollars are in `cost_usd` (`model_cost_usd`)."""
    cost = rec.get("model_cost_usd")
    if cost is None:
        cost = rec.get("cost_usd")
    digest, how = measure(rec.get("by_model") or {}, cost)
    return {"price_digest": digest, "price_basis": how, "list_price_digest": digest_of(CURRENT)}


def _seen(rows: Iterable[Mapping[str, Any]]) -> list[str]:
    return sorted({r["price_digest"] for r in rows if r.get("price_digest")})


def _describe(arms: Mapping[str, list]) -> str:
    return "; ".join(f"{a}: {', '.join(_seen(rs)) or 'no price digest'}" for a, rs in arms.items())


def require_one_basis(arms: Mapping[str, list]) -> str:
    """The one digest every arm is billed on, or CrossBasisError saying which it saw."""
    for a, rows in arms.items():
        if not rows:
            raise CrossBasisError(f"arm {a!r} has no rows: an empty arm shows nothing about a basis")
    for a, rows in arms.items():
        seen = _seen(rows)
        if not seen:
            raise CrossBasisError(
                f"arm {a!r} has no price digest on any row, so it cannot be shown to share a "
                f"table with the others ({_describe(arms)}); restamp it or reprice both arms")
        if len(seen) > 1:
            raise CrossBasisError(
                f"arm {a!r} is itself billed on {len(seen)} price tables ({', '.join(seen)}); "
                f"reprice it onto one table before comparing")
    seen = {_seen(rows)[0] for rows in arms.values()}
    if len(seen) > 1:
        raise CrossBasisError(
            f"refusing a ratio across price tables ({_describe(arms)}): the ratio would mix a "
            f"price change with a behaviour change. Reprice both arms onto one table "
            f"(known: {', '.join(f'{n}={digest_of(n)}' for n in TABLES)})")
    return seen.pop()


def compare(arms: Mapping[str, list], value: Callable[[Mapping[str, Any]], float | None],
            reprice_to: str | None = None, stat: Callable[[list[float]], float] | None = None) -> dict[str, Any]:
    """A two-arm ratio (the second arm over the first) of `value` per row, mean by
    default, and only on one basis. With `reprice_to` (a table name) both arms are
    repriced onto it from their per-model tokens and the result says so; a row that
    cannot be repriced refuses the whole comparison."""
    if len(arms) != 2:
        raise CrossBasisError(f"a comparison is between two arms, not {len(arms)}")
    names = list(arms)
    seen = {a: _seen(rows) for a, rows in arms.items()}
    if reprice_to is None:
        basis, repriced = require_one_basis(arms), False
        vals = {a: [v for v in (value(r) for r in rows) if v is not None] for a, rows in arms.items()}
    else:
        table = TABLES[reprice_to]
        basis, repriced = digest_of(reprice_to), True
        vals = {}
        for a, rows in arms.items():
            if not rows:
                raise CrossBasisError(f"arm {a!r} has no rows: nothing to reprice")
            out = []
            for r in rows:
                p = reprice(r.get("by_model") or {}, table)
                if p is None or not (r.get("by_model") or {}):
                    raise CrossBasisError(
                        f"cannot reprice a row of arm {a!r} onto {reprice_to}: it carries no "
                        f"per-model usage (or a model not on the table), so its dollars stay on "
                        f"whatever table billed them")
                out.append(p)
            vals[a] = out
    for a, v in vals.items():
        if not v:
            raise CrossBasisError(f"arm {a!r} has no priced rows")
    f = stat or (lambda xs: sum(xs) / len(xs))
    lo, hi = f(vals[names[0]]), f(vals[names[1]])
    return {"basis": basis, "basis_name": name_of(basis), "repriced": repriced, "seen": seen,
            "n": {a: len(v) for a, v in vals.items()}, names[0]: lo, names[1]: hi,
            "ratio": hi / lo if lo else None}
