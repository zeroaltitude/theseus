"""Scores one cell (an arm, run on a plan) from its results: every probe's check lines, then rates per family with
Wilson intervals, the preference matrix (kind × context), the rubric judge's means, and what was not scored.

A probe's verdict is `xchecks.verdict` of its lines: every line that applies to the arm holds. A line that cannot
apply to the arm (a `ledger` line for Claude Code, which keeps no such record) is None and left out; a probe whose
lines are all None is not scored for that arm. A turn that failed or answered nothing is a failure of its probe, never
a skip. A step the run never reached (the budget stopped it) is `unreached`, counted apart.

The persona-free ("generic") score counts only the probes that need neither the persona nor the card
(`needs` has neither `persona` nor `card`): what an arm with no persona can be fairly asked.

Standard library only.
"""

from __future__ import annotations

import sys
from collections import defaultdict
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent / "recall"))
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent / "report"))

import xchecks as xc  # noqa: E402
from stats import wilson  # noqa: E402

from plan import CONTEXTS, Plan  # noqa: E402


def probe_records(plan: Plan, results: dict[int, dict], tz_names: list[str]) -> list[dict]:
    recs = []
    for s in plan.steps:
        if s.kind != "probe":
            continue
        r = results.get(s.index)
        generic = not ({"persona", "card"} & set(s.needs))
        base = {"index": s.index, "probe": s.probe, "family": s.family, "dim": s.dim, "ctx": s.ctx, "pref": s.pref,
                "surface": s.surface, "profile": s.profile, "generic": generic, "needs": s.needs}
        if r is None or r.get("skipped"):
            recs.append({**base, "ok": None, "unreached": True, "lines": [], "reply": ""})
            continue
        reply = r.get("reply") or ""
        failed = r.get("exit") not in (0, None) or not reply.strip()
        facts = {"tz_names": tz_names}
        if "ledger" in r:
            facts["ledger"] = r["ledger"]
        lines = xc.run(s.checks, reply, facts)
        ok = False if failed else xc.verdict(lines)
        recs.append({**base, "ok": ok, "unreached": False, "lines": [[ln, v] for ln, v in lines],
                     "reply": reply, "error": r.get("error") if failed else None})
    return recs


def rate(recs: list[dict]) -> dict:
    scored = [r for r in recs if r["ok"] is not None]
    k = sum(1 for r in scored if r["ok"])
    n = len(scored)
    p, lo, hi = wilson(k, n) if n else (None, None, None)
    return {"k": k, "n": n, "p": p, "lo": lo, "hi": hi, "unscored": len(recs) - n,
            "unreached": sum(1 for r in recs if r.get("unreached"))}


def family_of(r: dict) -> str:
    """The surfaces and the routed profiles are families of their own; everything else is its probe's."""
    if r["surface"] != "cli":
        return "surfaces"
    if r["profile"]:
        return "models"
    return r["family"]


def by_family(recs: list[dict]) -> dict[str, dict]:
    groups: dict[str, list[dict]] = defaultdict(list)
    for r in recs:
        groups[family_of(r)].append(r)
    return {f: {**rate(rs), "generic": rate([r for r in rs if r["generic"]])} for f, rs in sorted(groups.items())}


def pref_matrix(recs: list[dict], compactions: set[int], plan: Plan) -> dict:
    """kind -> context -> {ok, forced}; `forced` (the compaction context only): a compaction ran between the
    preference's statement and its ask, from the run's own compaction marks (None where the arm reports none)."""
    out: dict[str, dict] = defaultdict(dict)
    for r in recs:
        if r["family"] != "preference":
            continue
        st = next(s for s in plan.steps if s.index == r["index"]).stated_at
        forced = None
        if r["ctx"] == "compaction":
            forced = any(st < c <= r["index"] for c in compactions)
        out[r["pref"]][r["ctx"]] = {"ok": r["ok"], "forced": forced}
    return {k: v for k, v in sorted(out.items())}


def by_context(recs: list[dict]) -> dict[str, dict]:
    return {c: rate([r for r in recs if r["family"] == "preference" and r["ctx"] == c]) for c in CONTEXTS
            if any(r["ctx"] == c for r in recs)}


def judge_means(judged: dict[int, dict], recs: list[dict]) -> dict:
    """Mean of each rubric dimension over the probes the judge scored, by family, and the share flagged flattery."""
    fam = {r["index"]: family_of(r) for r in recs}
    sums: dict[str, dict[str, list[int]]] = defaultdict(lambda: defaultdict(list))
    flat: dict[str, list[bool]] = defaultdict(list)
    errors = 0
    for i, j in judged.items():
        if "error" in j:
            errors += 1
            continue
        for d, v in j["scores"].items():
            sums[fam.get(i, "?")][d].append(v)
        flat[fam.get(i, "?")].append(j.get("flattery", False))
    return {"by_family": {f: {d: round(sum(v) / len(v), 2) for d, v in ds.items()} for f, ds in sorted(sums.items())},
            "flattery": {f: sum(v) / len(v) for f, v in sorted(flat.items())}, "n": sum(len(v) for v in flat.values()),
            "errors": errors}


def score_cell(plan: Plan, results: dict[int, dict], tz_names: list[str], compactions: list[int],
               judged: dict[int, dict] | None = None) -> dict:
    recs = probe_records(plan, results, tz_names)
    return {
        "overall": {**rate(recs), "generic": rate([r for r in recs if r["generic"]])},
        "families": by_family(recs),
        "contexts": by_context(recs),
        "preferences": pref_matrix(recs, set(compactions), plan),
        "judge": judge_means(judged or {}, recs),
        "probes": [{k: v for k, v in r.items() if k != "needs"} for r in recs],
    }
