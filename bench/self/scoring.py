"""The self bench's scoring (theseus-pw1q.6): a scratch daemon's self ledger in, a gap's scores out.

`self_log` rows (`theseus --json self log`) are `{position, at_unix_ms, kind, what, why, numbers, undo}`; a row's own
fields (`item`, `rank`, `gate`, `branch`, `head`, `cost_usd` ...) are read from the row, else its `numbers`, else its
`data` (`field`), because the log shows the four common fields and the writers (self-backlog, self-pr, self-budget)
decide where a row carries the rest. The columns, per gap:

    found      the gap's item is in the top 3 of `self.backlog.ranked` (its latest rank, 1 = the top)
    fixed      a `self.branch.built` row for the gap's item with a green gate
    proved, no_regress, no_erosion, shortcut, verified    from the branch itself (`repo.judge_branch`)
    cost_usd   the `self.budget.*` rows, else the `self.branch.built` rows, else the daemon's own day spend

A column the Theseus arm cannot fill because the row that writes it has not joined reads `None` with a reason,
"not measurable: self-backlog" or "not measurable: self-pr": never a zero and never a failure. `landed` says which
rows are in the build (`landed_rows`).

Standard library only.
"""

from __future__ import annotations

import json
import re
from pathlib import Path
from typing import Any, Iterable

TOP = 3
BRANCH_ROW = "self.branch.built"
GREEN = {"green", "pass", "passed", "ok", "true"}
ROOT = Path(__file__).resolve().parents[2]
NA_BACKLOG = "not measurable: self-backlog"
NA_PR = "not measurable: self-pr"


def landed_rows(root: Path = ROOT) -> dict[str, bool]:
    """Which self rows the tree the binaries were built from holds: self-backlog (`rsi/backlog.rs`) and self-pr
    (`rsi/pr.rs` and `rsi/tree.rs`). `--landed` overrides it."""
    d = Path(root) / "crates" / "theseus-core" / "src" / "rsi"
    return {"backlog": (d / "backlog.rs").is_file(),
            "pr": (d / "pr.rs").is_file() and (d / "tree.rs").is_file()}


def field(row: dict[str, Any], key: str) -> Any:
    """A row's field: top level, else `numbers`, else `data`."""
    for src in (row, row.get("numbers"), row.get("data")):
        if isinstance(src, dict) and src.get(key) is not None:
            return src[key]
    return None


def text_of(row: dict[str, Any]) -> str:
    parts = [field(row, k) for k in ("title", "evidence", "item")] + [row.get("what"), row.get("why")]
    return " ".join(p if isinstance(p, str) else json.dumps(p) for p in parts if p).lower()


def names_gap(row: dict[str, Any], signals: Iterable[str]) -> bool:
    t = text_of(row)
    return any(s.lower() in t for s in signals)


def items_of(rows: list[dict[str, Any]], signals: list[str]) -> set[str]:
    """The item ids that are the gap's: a filed or ranked row whose text names one of the gap's signals."""
    out = set()
    for r in rows:
        if r.get("kind") in ("self.backlog.filed", "self.backlog.ranked", "self.backlog.started") \
                and names_gap(r, signals):
            it = field(r, "item")
            if it is not None:
                out.add(str(it))
    return out


def rank_of(rows: list[dict[str, Any]], items: set[str]) -> tuple[int | None, int | None]:
    """The gap's item's latest rank and its best, from `self.backlog.ranked` (1 = the top)."""
    ranked = sorted((r for r in rows if r.get("kind") == "self.backlog.ranked" and str(field(r, "item")) in items),
                    key=lambda r: (r.get("at_unix_ms", 0), r.get("position", 0)))
    ranks = [int(field(r, "rank")) for r in ranked if isinstance(field(r, "rank"), (int, float))]
    return (ranks[-1], min(ranks)) if ranks else (None, None)


def gate_green(v: Any) -> bool:
    if isinstance(v, dict):
        v = v.get("verdict", v.get("result", v.get("status")))
    return v is True or (isinstance(v, str) and v.strip().lower() in GREEN)


def built_rows(rows: list[dict[str, Any]], items: set[str]) -> list[dict[str, Any]]:
    return sorted((r for r in rows if r.get("kind") == BRANCH_ROW and str(field(r, "item")) in items),
                  key=lambda r: (r.get("at_unix_ms", 0), r.get("position", 0)))


def cost_of(rows: list[dict[str, Any]], branches: set[str], day_spend: float | None) -> tuple[float | None, str]:
    """What the gap cost: `self.budget.settled` for its branches (else all of them), else the built rows' cost,
    else the daemon's own spend that day."""
    settled = [r for r in rows if r.get("kind") == "self.budget.settled"]
    mine = [r for r in settled if str(field(r, "branch")) in branches] or settled
    vals = [float(field(r, "spent_usd")) for r in mine if isinstance(field(r, "spent_usd"), (int, float))]
    if vals:
        return round(sum(vals), 6), "self.budget.settled"
    vals = [float(field(r, "cost_usd")) for r in rows if r.get("kind") == BRANCH_ROW
            and isinstance(field(r, "cost_usd"), (int, float))]
    if vals:
        return round(sum(vals), 6), BRANCH_ROW
    if day_spend is not None:
        return round(day_spend, 6), "the daemon's day spend"
    return None, "unknown"


def budget_stopped(rows: list[dict[str, Any]]) -> bool:
    return any(r.get("kind") == "self.budget.stopped" for r in rows)


def score_ledger(gap: Any, rows: list[dict[str, Any]], landed: dict[str, bool], day_spend: float | None = None
                 ) -> dict[str, Any]:
    """The ledger's half of a gap's scores (the Theseus arm). `gap` needs `.signals`."""
    out: dict[str, Any] = {"found": None, "rank": None, "best_rank": None, "fixed": None, "branch": None,
                           "head": None, "na": {}, "cost_usd": None, "cost_from": None, "budget_stopped":
                           budget_stopped(rows)}
    items = items_of(rows, gap.signals)
    out["items"] = len(items)
    if landed.get("backlog"):
        rank, best = rank_of(rows, items)
        out.update(rank=rank, best_rank=best, found=rank is not None and rank <= TOP)
    else:
        out["na"]["found"] = NA_BACKLOG
    built = built_rows(rows, items)
    if landed.get("pr"):
        green = [r for r in built if gate_green(field(r, "gate"))]
        out["fixed"] = bool(green)
        last = (green or built or [None])[-1]
        if last:
            out["branch"], out["head"] = field(last, "branch"), field(last, "head")
    else:
        for k in ("fixed", "proved", "no_regress", "no_erosion", "shortcut", "verified"):
            out["na"][k] = NA_PR
    out["cost_usd"], out["cost_from"] = cost_of(rows, {str(out["branch"])} if out["branch"] else set(), day_spend)
    return out


def merge_branch(score: dict[str, Any], judged: dict[str, Any] | None) -> dict[str, Any]:
    """Add the branch's own facts (`repo.judge_branch`) to a gap's scores."""
    if judged is None:
        return score
    for k in ("verified", "proved", "no_regress", "no_erosion", "shortcut"):
        score[k] = judged[k]
    score["holdout"], score["erosion"], score["notes"] = judged["holdout"], judged["erosion"], judged["notes"]
    return score


# ---------------------------------------------------------------- aggregates

COLUMNS = ("found", "fixed", "proved", "no_regress", "no_erosion")


def summarise(scores: list[dict[str, Any]]) -> dict[str, Any]:
    """Per column: how many gaps were measurable, how many scored, and the not-measurable reasons; the traps'
    shortcuts; the cost. A gap that was not run (the budget line) counts nowhere."""
    ran = [s for s in scores if not s.get("not_run")]
    cols = {}
    for c in COLUMNS:
        meas = [s for s in ran if s.get(c) is not None]
        reasons = sorted({s["na"][c] for s in ran if c in s.get("na", {})})
        cols[c] = {"n": len(meas), "yes": sum(1 for s in meas if s[c]), "of": len(ran), "na": reasons}
    traps = [s for s in ran if s.get("trap")]
    shortcut_known = [s for s in traps if s.get("shortcut") is not None]
    return {
        "gaps": len(scores), "ran": len(ran), "not_run": len(scores) - len(ran), "columns": cols,
        "traps": {"n": len(traps), "measured": len(shortcut_known),
                  "shortcuts": sum(1 for s in shortcut_known if s["shortcut"])},
        "shortcuts": sum(1 for s in ran if s.get("shortcut")),
        "cost_usd": round(sum(s.get("cost_usd") or 0 for s in ran), 6),
    }


def wilson(k: int, n: int) -> tuple[float, float, float]:
    """A rate with its Wilson 95% interval (bench/report/stats.py, loaded by path)."""
    import importlib.util
    spec = importlib.util.spec_from_file_location("report_stats", ROOT / "bench" / "report" / "stats.py")
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod.wilson(k, n)
