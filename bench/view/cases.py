#!/usr/bin/env python3
"""The labelled cases (issue 44, §10 of the context design): what a good
context must hold for a message, frozen before any selector is compared, and
the scorer that says held or missed per case for a strategy's context.

    python3 bench/view/cases.py check bench/view/cases/synthetic.v2.json
    python3 bench/view/cases.py score bench/view/cases/synthetic.v2.json held.json
    python3 bench/view/cases.py build bench/view/cases/synthetic.v2.json --bin-dir target/debug --out /tmp/vc

**The sets**: `synthetic.v1` (24 cases, frozen) and `synthetic.v2`, the
same less the four `forbidden_disclosure` cases: nothing is withheld since
2026-11-22, so a sealed line a context must not hold is a rule that no
longer exists. v2 is the set to measure the view with.

**The format** (`view-cases.v1`, frozen: a change is a new format name):
`{"format", "set", "cases": [...]}`, and each case

- `id` (a safe name: `SAFE`, one path component, never `.` or `..`), and
  `class`: `callback_here`, `callback_elsewhere`,
  `second_option_later`, `corrected_fact`, `task_update`,
  `forbidden_disclosure` (§10's classes);
- `history`: the scripted messages, oldest first, each `{"id", "place",
  "say"}`; a place is a symbolic name (`here`, `there`), a safe name like an
  id, one place each;
- `message`: `{"place", "say"}`, the message the context is assembled for;
- `evidence`: the history ids a good context must hold, all of them;
- `referent`: the history id the message's reference resolves to, or null;
- `current` and `stale` (optional): the id that holds a fact's current value
  and those that held it before. Holding a stale one without the current is
  `stale_alone`;
- `forbidden` (optional): ids a context must never hold (a sealed thread);
- `needs` (optional): what the builder must be able to make for the case to
  mean anything (`task`, `seal`). The builder builds what it can and lists
  the rest as `not_built`, never as scored.

**Scoring**: a strategy's context is `{"strategy", "held": {case id:
[history ids]}}`. A case is `held` when every evidence id is held, the
referent is held, no stale value is held alone and nothing forbidden is held;
else `missed`, with which of those failed. Counts by class. A case the context
has no entry for is `absent`, never `missed`. A case that needs what the
builder cannot make (`needs` outside `BUILDS`) is `not_built` in every score,
whatever a context says of it, and listed by id with its reason.

**Building** (`build`): each case in a scratch daemon of its own on an empty
store (`scratch.py`), one CLI session per symbolic place in a directory of its
own (`session.open`'s `dir`, which makes the place `local:<dir>`), each line
sent as a turn to a STAND-IN model (bench/recall's `standin.py`, a local
process that answers "Noted." and charges nothing), the
exchange each line made read from the measuring row (`context.turn`). Then
two contexts per case: the TRANSCRIPT's (every earlier line of the message's
own place: one session per place, too short to compact), and the VIEW's
(`theseus view preview <place> --message <say> --json`: the exchange ranges
its `exchanges` pieces lay, mapped back to history ids). Both are written as
`held` files and scored. What the view's `folds` pieces lay (a fold's first
and last are exchange ids) is reported apart, as `held_folded`: a fold is
the line's summary, not its words, so it is not scored as held. The cases are synthetic, so `--message` sends nothing
anyone wrote; the kit writes ids and counts only.

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

FORMAT = "view-cases.v1"
# A case's id and a symbolic place: one path component the builder may join under its output, nothing else.
SAFE = re.compile(r"[A-Za-z0-9_.-]{1,64}")
CLASSES = ("callback_here", "callback_elsewhere", "second_option_later", "corrected_fact", "task_update",
           "forbidden_disclosure")
NEEDS = ("task", "seal")
# What the builder can make today: plain turns in private places. A task's own record and a seal are not.
BUILDS = ()
REPLY = "Noted."


class Invalid(ValueError):
    pass


def load(path: Path) -> dict:
    s = json.loads(Path(path).read_text())
    check(s)
    return s


def safe(name) -> bool:
    return isinstance(name, str) and bool(SAFE.fullmatch(name)) and name not in (".", "..")


def beneath(root: Path, p: Path) -> Path:
    """`p`, resolved, or `Invalid` when it is not strictly beneath `root` (resolved)."""
    root, q = Path(root).resolve(), Path(p).resolve()
    if root not in q.parents:
        raise Invalid(f"a destination outside the output: {q}")
    return q


def check(s: dict) -> None:
    """Raise `Invalid` with every fault of a case set, or return."""
    faults = []
    if s.get("format") != FORMAT:
        faults.append(f"format is {s.get('format')!r}, not {FORMAT}")
    seen = set()
    for c in s.get("cases") or []:
        cid = c.get("id")
        if not safe(cid) or cid in seen:
            faults.append(f"case id {cid!r} missing or repeated")
        seen.add(cid)
        if c.get("class") not in CLASSES:
            faults.append(f"{cid}: class {c.get('class')!r}")
        ids = [h.get("id") for h in c.get("history") or []]
        if not ids or len(set(ids)) != len(ids):
            faults.append(f"{cid}: history ids missing or repeated")
        if not all(safe(h.get("id")) for h in c.get("history") or []):
            faults.append(f"{cid}: a history id that is not a safe name")
        if not all(safe(h.get("place")) and h.get("say") for h in c.get("history") or []):
            faults.append(f"{cid}: a history line without place or say")
        m = c.get("message") or {}
        if not (safe(m.get("place")) and m.get("say")):
            faults.append(f"{cid}: message without place or say")
        refs = list(c.get("evidence") or []) + list(c.get("stale") or []) + list(c.get("forbidden") or [])
        refs += [x for x in (c.get("referent"), c.get("current")) if x is not None]
        if not c.get("evidence"):
            faults.append(f"{cid}: no evidence")
        bad = [r for r in refs if r not in ids]
        if bad:
            faults.append(f"{cid}: names {bad}, not in its history")
        if set(c.get("evidence") or []) & set(c.get("forbidden") or []):
            faults.append(f"{cid}: evidence and forbidden overlap")
        if any(n not in NEEDS for n in c.get("needs") or []):
            faults.append(f"{cid}: needs {c.get('needs')}")
    if not s.get("cases"):
        faults.append("no cases")
    if faults:
        raise Invalid("; ".join(faults))


def verdict(case: dict, held: set[str]) -> dict:
    """One case against the history ids a context held."""
    missing = [e for e in case["evidence"] if e not in held]
    ref = case.get("referent")
    stale_alone = bool(case.get("current")) and case["current"] not in held \
        and any(x in held for x in case.get("stale") or [])
    leaked = [f for f in case.get("forbidden") or [] if f in held]
    why = []
    if missing:
        why.append("evidence")
    if ref is not None and ref not in held:
        why.append("referent")
    if stale_alone:
        why.append("stale_alone")
    if leaked:
        why.append("forbidden")
    return {"case": case["id"], "class": case["class"], "verdict": "missed" if why else "held",
            "why": why, "evidence_missing": len(missing), "leaked": len(leaked)}


def not_built(case: dict) -> list[str]:
    """What a case needs that the builder cannot make: such a case is never scored."""
    return [n for n in case.get("needs") or [] if n not in BUILDS]


def score(cases: dict, context: dict) -> dict:
    """Each case's verdict, and counts by class (held, missed, absent, not_built) and by why; the cases not
    built, by id, with what they need."""
    held = context.get("held") or {}
    rows, by_class, by_why, unbuilt = [], {}, Counter(), {}
    for c in cases["cases"]:
        k = by_class.setdefault(c["class"], Counter())
        if not_built(c):
            k["not_built"] += 1
            unbuilt[c["id"]] = not_built(c)
            continue
        if c["id"] not in held:
            k["absent"] += 1
            continue
        v = verdict(c, set(held[c["id"]]))
        rows.append(v)
        k[v["verdict"]] += 1
        by_why.update(v["why"])
    return {"strategy": context.get("strategy"), "cases": rows,
            "by_class": {k: dict(v) for k, v in by_class.items()}, "by_why": dict(by_why), "not_built": unbuilt}


# ---- strategies' contexts, as history ids


def transcript_held(case: dict) -> list[str]:
    """The transcript's context: every earlier line of the message's own place (one session per place)."""
    return [h["id"] for h in case["history"] if h["place"] == case["message"]["place"]]


def view_held(preview: dict, exchange_of: dict[str, str], order: dict[str, list[str]],
              kind: str = "exchanges") -> list[str]:
    """The view's context: every history line whose exchange lies in a range a piece of `kind` lays (`first`
    to `last`, in its place's order): `exchanges`, what it holds; `folds`, what it holds only as a fold.
    `exchange_of` maps history id to exchange id; `order` each place's exchange ids, oldest first."""
    step_of = {e: h for h, e in exchange_of.items()}
    held: set[str] = set()
    view = preview.get("view") or {}
    for slot in view.get("slots") or []:
        for p in slot.get("pieces") or []:
            if p.get("kind") != kind:
                continue
            first, last = p.get("first"), p.get("last")
            for ids in order.values():
                if first in ids and last in ids:
                    i, j = sorted((ids.index(first), ids.index(last)))
                    held.update(step_of[e] for e in ids[i:j + 1] if e in step_of)
    return sorted(held)


# ---- building a case's store


def build_case(case: dict, bin_dir: Path, out: Path) -> dict:
    """One case: its store built on a stand-in, then both contexts as history ids (or why not)."""
    import scratch  # noqa: PLC0415  (the build alone needs the daemon's helpers)
    import standin  # noqa: PLC0415  (bench/recall, on scratch's path)

    check({"format": FORMAT, "cases": [case]})  # safe names before any path is made of them
    missing = not_built(case)
    if missing:
        return {"case": case["id"], "not_built": missing}
    out = Path(out).resolve()
    dirs = {h["place"]: beneath(out, out / "places" / h["place"]) for h in case["history"]}
    model = standin.StandIn([], REPLY)
    try:
        with scratch.Scratch(bin_dir, out, api_base=model.base, model=standin.MODEL) as d:
            sessions: dict[str, str] = {}
            exchange_of: dict[str, str] = {}
            place_of: dict[str, str] = {}
            order: dict[str, list[str]] = {}
            for h in case["history"]:
                cwd = dirs[h["place"]]
                cwd.mkdir(parents=True, exist_ok=True)
                if h["place"] not in sessions:
                    # Opened with its directory, which makes its place `local:<dir>` from its first turn on:
                    # `sessions open` sets none, and `ask --dir` moves a session only after the turn it opens.
                    v = d.json("rpc", "session.open",
                               json.dumps({"label": f"case-{h['place']}", "dir": str(cwd.resolve())}))
                    v = v.get("result", v)
                    sessions[h["place"]] = v.get("session_id") or v["id"]
                # From a terminal of the kit's own: the line is marked the owner's (scratch.py's module text).
                r = d.cli("--json", "ask", "-s", sessions[h["place"]], "-", input=h["say"], cwd=cwd, terminal=True)
                if r.returncode != 0:
                    raise scratch.Failed(f"E_ASK {case['id']} {h['id']} exit {r.returncode}")
                row = scratch.ledger_rows(d, "context.turn")[-1]["data"]
                exchange_of[h["id"]] = row["exchange"]
                place_of[h["place"]] = row["place"]
                order.setdefault(row["place"], []).append(row["exchange"])
            m = case["message"]
            place = place_of.get(m["place"])
            if place is None:
                return {"case": case["id"], "not_built": ["message_place_without_history"]}
            if len(set(place_of.values())) != len(place_of):
                # Two symbolic places the daemon took for one: a case about "elsewhere" would mean nothing.
                return {"case": case["id"], "not_built": ["places_not_distinct"]}
            try:
                preview = d.json("view", "preview", place, "--message", m["say"])
            except scratch.Failed:
                # A build before `view preview`: the transcript's context alone.
                return {"case": case["id"], "transcript": transcript_held(case), "view_not_built": "no_preview"}
    finally:
        model.close()
    return {"case": case["id"], "transcript": transcript_held(case),
            "view": view_held(preview, exchange_of, order),
            "view_folded": view_held(preview, exchange_of, order, "folds"), "fallback": preview.get("fallback")}


def build(cases: dict, bin_dir: Path, out: Path) -> dict:
    import scratch  # noqa: PLC0415

    out = scratch.check_out(out)
    check(cases)
    dests = [beneath(out, out / c["id"]) for c in cases["cases"]]  # every one, before the first write
    out.mkdir(parents=True, exist_ok=True)
    built = [build_case(c, bin_dir, d) for c, d in zip(cases["cases"], dests)]
    contexts = {}
    for strategy in ("transcript", "view"):
        ctx = {"strategy": strategy, "held": {b["case"]: b[strategy] for b in built if strategy in b}}
        (out / f"held-{strategy}.json").write_text(json.dumps(ctx, indent=1, sort_keys=True) + "\n")
        contexts[strategy] = score(cases, ctx)
    report = {"set": cases.get("set"), "scores": contexts,
              "not_built": {b["case"]: b["not_built"] for b in built if "not_built" in b},
              "view_not_built": sorted(b["case"] for b in built if "view_not_built" in b),
              "held_folded": {b["case"]: b["view_folded"] for b in built if b.get("view_folded")},
              "view_fallback": dict(Counter(b.get("fallback") or "takes_the_view" for b in built if "view" in b))}
    (out / "report.json").write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")
    return report


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    sub = ap.add_subparsers(dest="cmd", required=True)
    c = sub.add_parser("check", help="validate a case set")
    c.add_argument("cases")
    s = sub.add_parser("score", help="score a strategy's held file against a case set")
    s.add_argument("cases")
    s.add_argument("held")
    b = sub.add_parser("build", help="build each case's store on a stand-in model and score both strategies")
    b.add_argument("cases")
    b.add_argument("--bin-dir", default="target/debug")
    b.add_argument("--out", required=True)
    a = ap.parse_args(argv)
    cases = load(Path(a.cases))
    if a.cmd == "check":
        print(f"{FORMAT}: {len(cases['cases'])} cases, "
              + ", ".join(f"{k} {n}" for k, n in sorted(Counter(x['class'] for x in cases['cases']).items())))
    elif a.cmd == "score":
        r = score(cases, json.loads(Path(a.held).read_text()))
        print(json.dumps({k: r[k] for k in ("strategy", "by_class", "by_why", "not_built")}, indent=2,
                         sort_keys=True))
    else:
        r = build(cases, Path(a.bin_dir), Path(a.out))
        print(json.dumps(r, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    sys.exit(main())
