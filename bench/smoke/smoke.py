"""The release smoke (theseus-yf6pj): five parts, each PASS or FAIL against the
floor the last release wrote, in about an hour, for a few dollars, on one host.

It is a release check, not a measurement: nothing here is reportable, and
nothing is written under docs/benchmarks/. `scripts/release-smoke.sh` runs the
parts and calls this module to judge each one, write the run's report, and,
with `--write-floor` only, the floor.

    python3 bench/smoke/smoke.py tb RUN_DIR JOB_DIR WALL_S
    python3 bench/smoke/smoke.py route RUN_DIR [--live PACK] (REPLAY.json WALL_S [--effort EFFORT.json])...
    python3 bench/smoke/smoke.py recall RUN_DIR EXIT SECONDS LOG
    python3 bench/smoke/smoke.py resume RUN_DIR RESUME.json
    python3 bench/smoke/smoke.py dm RUN_DIR DM.json
    python3 bench/smoke/smoke.py report RUN_DIR SHA BINARY [--write-floor]
    python3 bench/smoke/smoke.py fail RUN_DIR PART WHY      (a part that could not run)
    python3 bench/smoke/smoke.py spent JOB_DIR              (tb's finished trials' dollars)
    python3 bench/smoke/smoke.py bust RUN_DIR RUN CASES PACK...   (a run's cache-busting copies)

Each part command writes `RUN_DIR/parts/<part>.json` and prints its one line;
`report` writes `RUN_DIR/report.md` and exits 0 on PASS, 1 on FAIL.

A PASS needs a positive count first (10 trials ran, 3 runs replayed, the recall
tests ran, 3 kills happened, 5 replies arrived): an empty set meets every "no
failures" bar, so it never counts as one. Standard library only; Python >= 3.11.

The route part's bars are the owner's 80% of each question's own count, with one
exception he made on 2026-11-03 09:18 ("Range of measurement seems good too"):
the `model` question is held to the measured range the floor records
(`route.model_range`), because three runs of it read 48-51 of 62 and a single
number the instrument cannot promise is not a bar. `must` and `effort` keep the
80% rule. See `model_range` for what a recorded range does and does not excuse.
"""

from __future__ import annotations

import csv
import json
import math
import re
import sys
import time
import tomllib
import unittest
from pathlib import Path
from typing import Any

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
sys.path.insert(0, str(REPO / "bench" / "harbor"))
import price_basis  # noqa: E402

PARTS = ("tb", "route", "recall", "resume", "dm")
FLOOR_FILE = HERE / "floor.json"
# How far a release's cost per solved task and its wall may rise over the floor.
SLACK = 1.30
# The routing bars (the owner, 2026-10-30 16:01: "the bars should all be set to about 80% of the total cases"):
# each bar is this ratio of ITS OWN count, rounded up, never an integer written down (a fixed 54 is how this part came
# to gate on a retired bar, theseus-h66v4.4). The replay prints its own bars (`ceil(of * 4 / 5)`, route_cases.rs'
# `bar_need`) and this part reads the counts from them; the rule is written here again only to catch a replay whose
# bar is not the owner's.
BAR_NUM = 4
BAR_DEN = 5
RUNS_WANTED = 3
DM_TURNS = 5
DM_REPLY_S = 60.0
KILLS_WANTED = 3


class SmokeError(Exception):
    """A smoke that cannot be judged, or a floor that must not be written."""


def price_digest() -> str:
    return price_basis.digest_of(price_basis.CURRENT)


# --- the slice ------------------------------------------------------------

def load_slice(path: Path) -> dict[str, Any]:
    with open(path, "rb") as f:
        raw = tomllib.load(f)
    stable, controls = raw.get("stable", {}), raw.get("controls", {})
    return {"stable": list(stable), "controls": list(controls), "reasons": {**stable, **controls},
            "sources": list(raw.get("sources", []))}


def check_slice(s: dict[str, Any], root: Path) -> list[str]:
    """What in the slice its source CSVs do not bear out; empty when all of it holds."""
    runs: dict[str, list[bool]] = {}
    for src in s["sources"]:
        with open(root / src, newline="") as f:
            for r in csv.DictReader(f):
                passed = r["reward"].strip() in ("1", "1.0") and not r.get("exception")
                runs.setdefault(r["task"], []).append(passed)
    bad = []
    for t in s["stable"]:
        got = runs.get(t)
        if not got:
            bad.append(f"{t}: in no source run")
        elif not all(got):
            bad.append(f"{t}: stable, but passed {sum(got)} of {len(got)}")
    for t in s["controls"]:
        got = runs.get(t)
        if not got:
            bad.append(f"{t}: in no source run")
        elif any(got):
            bad.append(f"{t}: a control, but passed {sum(got)} of {len(got)}")
    return bad


# --- part 1: the TB2 slice -------------------------------------------------

def read_trials(job_dir: Path) -> list[dict[str, Any]]:
    """One row a Harbor trial under `job_dir` (any depth of `<task>__<id>/result.json`)."""
    rows = []
    table = price_basis.TABLES[price_basis.CURRENT]
    for res in sorted(job_dir.rglob("*__*/result.json")):
        r = json.loads(res.read_text())
        eff_path = res.parent / "agent" / "efficiency.json"
        eff = json.loads(eff_path.read_text()) if eff_path.exists() else {}
        reward = ((r.get("verifier_result") or {}).get("rewards") or {}).get("reward")
        by_model = eff.get("by_model") or {}
        # No record, or one with no model tokens, is "no price", never $0.00: free trials would lower the cost per
        # solved task.
        model = price_basis.reprice(by_model, table) if by_model else None
        jev = (eff.get("jev") or {}).get("cost_usd")
        if jev is None and eff.get("model_cost_usd") is not None:
            jev = float(eff.get("cost_usd") or 0) - float(eff["model_cost_usd"])
        cost = None if model is None else round(model + max(float(jev or 0), 0.0), 6)
        rows.append({
            "task": r.get("task_name") or res.parent.name.split("__")[0],
            "trial": res.parent.name,
            "passed": reward is not None and float(reward) >= 1.0,
            "exception": (r.get("exception_info") or {}).get("exception_type"),
            "cost_usd": cost,
            "wall_s": eff.get("wall_s"),
            "turns": eff.get("model_calls"),
            "models": {m: int(v.get("calls") or 0) for m, v in by_model.items()},
            "price_digest": price_digest(),
        })
    return rows


def spent(job_dir: Path) -> float:
    """What the run's FINISHED trials cost, from their turn traces: a trial in
    flight has none yet, so this reads low by up to one batch."""
    total = 0.0
    for p in job_dir.rglob("*__*/agent/theseus-turn.json"):
        try:
            c = json.loads(p.read_text()).get("cost_usd")
        except (OSError, ValueError):
            continue
        if isinstance(c, (int, float)):
            total += float(c)
    return total


def _over(value: float, floor: float) -> bool:
    return value > floor * SLACK + 1e-9


def _delta(value: float, floor: float | None) -> str | None:
    if not floor:
        return None
    return f"{(value / floor - 1) * 100:+.0f}%"


def tb_part(rows: list[dict[str, Any]], s: dict[str, Any], floor: dict | None, wall_s: float) -> dict[str, Any]:
    want = s["stable"] + s["controls"]
    by_task = {r["task"]: r for r in rows if r["task"] in want}
    problems = []
    if len(by_task) != len(want):
        problems.append(f"{len(by_task)} of {len(want)} tasks ran")
    failed = [t for t in s["stable"] if t in by_task and not by_task[t]["passed"]]
    if failed:
        problems.append("stable failed: " + ", ".join(failed))
    passed_controls = [t for t in s["controls"] if t in by_task and by_task[t]["passed"]]
    if passed_controls:
        problems.append("control passed: " + ", ".join(passed_controls))
    # A trial whose environment never came up (Docker, the grader) is the host's failure, not Theseus's: say so.
    errored = sorted({r["exception"] for r in by_task.values() if r["exception"] and r["turns"] is None})
    if errored:
        n = sum(1 for r in by_task.values() if r["exception"] and r["turns"] is None)
        problems.append(f"{n} trials errored before the agent ran ({', '.join(errored)}): the host, not the candidate")
    unpriced = [t for t, r in by_task.items() if r["cost_usd"] is None]
    if unpriced:
        problems.append("no price for: " + ", ".join(unpriced))
    solved = sum(1 for r in by_task.values() if r["passed"])
    cost = sum(r["cost_usd"] or 0 for r in by_task.values())
    per_solved = cost / solved if solved else math.inf
    notes, fl = [], (floor or {}).get("tb")
    if fl:
        if fl.get("price_digest") != price_digest():
            notes.append(f"dollars not compared: the floor is on price table {fl.get('price_digest')}, "
                         f"this run on {price_digest()}")
        elif _over(per_solved, fl["cost_per_solved_usd"]):
            problems.append(f"cost per solved ${per_solved:.3f} > {SLACK:.0%} of the floor's "
                            f"${fl['cost_per_solved_usd']:.3f}")
        if _over(wall_s, fl["wall_s"]):
            problems.append(f"wall {wall_s:.0f} s > {SLACK:.0%} of the floor's {fl['wall_s']:.0f} s")
    return {"part": "tb", "ok": not problems, "count": len(by_task), "number": per_solved,
            "unit": "$/solved", "floor": fl["cost_per_solved_usd"] if fl else None,
            "delta": _delta(per_solved, fl["cost_per_solved_usd"]) if fl else None,
            "note": "; ".join(problems + notes) or f"{solved} solved, both controls failed",
            "cost_usd": round(cost, 4), "wall_s": wall_s, "solved": solved,
            "price_digest": price_digest(), "trials": sorted(by_task.values(), key=lambda r: want.index(r["task"]))}


# --- part 2: routing ------------------------------------------------------

# Where a cache-busting copy's pack version starts: above anything anybody writes by hand
# (route.v10 is 10), so a bumped copy is never mistaken for a version of the pack's own history.
BUMP_BASE = 1_000_000


def bump_version(pack_text: str, version: int) -> tuple[str, str]:
    """`pack_text` with its `version` set to `version`, and the name that gives the pack.

    Jev caches a judgment under the pack it was asked with, so three replays of one pack are one
    sample served three times: the 1.0.4 release smoke's three route runs each cost $0.004718 and
    each scored 51 of 62, to six decimal places, while the same tree replayed with the version
    bumped read 49, 51 and 52 (theseus-h420q). The version is the only thing that moves -- the
    questions, the options and the instructions stay the pack's own -- so what is measured is the
    pack under test, three times.
    """
    out, n = re.subn(r"(?m)^version\s*=\s*\d+\s*$", f"version = {version}", pack_text, count=1)
    if n != 1:
        raise SmokeError(f"the pack has no `version = <n>` line to bump ({n} found)")
    parsed = tomllib.loads(out)
    if parsed.get("version") != version:
        raise SmokeError(f"the bumped pack reads version {parsed.get('version')!r}, not {version}")
    return out, f"{parsed['id']}.v{version}"


def also_grades(cases_text: str, names: list[str]) -> str:
    """`cases_text` with `names` added to its `also` list, so the bumped packs may grade it.

    A cases file names the one pack it is written for (`pack = "route.v10"`) and refuses every
    other unless its `also` names it (`route_cases.rs`'s `CaseSet::parse`). `pack` is left alone:
    it is what the replay reports as the set's pack, and what `route_part` reads to tell whether
    the daemon's ladder acts with it ("n/a until the flip").
    """
    have = tomllib.loads(cases_text)
    want = [n for n in names if n != have.get("pack") and n not in have.get("also", [])]
    if not want:
        return cases_text
    listed = ", ".join(json.dumps(n) for n in list(have.get("also", [])) + want)
    if "also" in have:
        out, n = re.subn(r"(?m)^also\s*=\s*\[[^\]]*\]\s*$", f"also = [{listed}]", cases_text, count=1)
    else:
        out, n = re.subn(r"(?m)^(pack\s*=\s*.*)$", lambda m: f"{m.group(1)}\nalso = [{listed}]",
                         cases_text, count=1)
    if n != 1:
        raise SmokeError("the cases file has no `pack` or `also` line to write")
    got = tomllib.loads(out)
    if got.get("pack") != have.get("pack") or not all(w in got.get("also", []) for w in want):
        raise SmokeError(f"the cases file's pack or also did not take: "
                         f"{got.get('pack')!r} {got.get('also')!r}")
    return out


def bust(run_dir: Path, run: int, packs: list[Path], cases: Path, at: int | None = None) -> list[Path]:
    """Copies of `packs` and of `cases` for run `run`, each pack's version bumped so the run asks
    Jev and is not served its last answer. Returns the pack copies in order, then the cases copy.

    One version for every pack of a run is enough: a version is only ever read beside its own
    pack's id. `at` is the clock, for a test.
    """
    version = BUMP_BASE + (int(time.time()) if at is None else at) * 10 + run
    out, named = [], []
    for p in packs:
        text, name = bump_version(p.read_text(), version)
        to = run_dir / f"{p.stem}-run{run}.toml"
        to.write_text(text)
        out.append(to)
        named.append(name)
    to = run_dir / f"{cases.stem}-run{run}.toml"
    to.write_text(also_grades(cases.read_text(), named))
    return out + [to]


def bar_need(of: int) -> int:
    """The least a bar over `of` cases needs: `of * 4 / 5` rounded up, in integers (route_cases.rs' `bar_need`)."""
    return -(-of * BAR_NUM // BAR_DEN)


def _bar(question: str, ok: int, of: int) -> dict[str, Any]:
    return {"question": question, "ok": ok, "of": of, "need": bar_need(of)}


def model_range(floor: dict | None, of: int | None = None) -> dict[str, Any] | None:
    """The `model` bar as a statement about the measured distribution, when the floor records one.

    The owner, 2026-11-03 09:18, on the round-2 report's §9 proposal ("give the release smoke a bar for the
    model that is a statement about the measured distribution ... not a number the instrument cannot
    reach"): "Range of measurement seems good too". So `route.model_range` in the floor holds what three
    runs of a release actually read -- `{"low", "high", "of"}` -- and the part fails when the worst of three
    falls BELOW that low. A reading above `high` is not a failure; it is a note that the range is stale.

    The 80% rule still governs every other question, and it still governs `model` itself when no range is
    recorded, or when the range was measured over a different number of cases: a range read over 62 cases
    says nothing about 70, and a release must never loosen a bar by changing the set underneath it.
    `must` keeps the 80% rule, whose 11 of 13 is also its measured floor."""
    r = ((floor or {}).get("route") or {}).get("model_range")
    if not isinstance(r, dict) or not all(isinstance(r.get(k), int) for k in ("low", "high", "of")):
        return None
    if r["of"] <= 0 or not (0 <= r["low"] <= r["high"] <= r["of"]):
        return None
    return r if of is None or of == r["of"] else None


def route_run(raw: dict[str, Any], wall_s: float, effort: dict[str, Any] | None = None) -> dict[str, Any]:
    """One `theseus --json judge replay --cases` result, as the part reads it.

    The counts come from the replay's own `bars` (one per question it graded, `model` and `must`, each `ok` of `of`
    with the `need` it computed), so the denominators are the ones the replay reports: its graded cases for the
    model (the two pins, W39 and W40, are daemon tests outside every denominator), and its `must_total` for the
    musts: the ones the cases file holds, not the design's 14th (W40 is asserted by `route_owner.rs`). A replay with no
    `bars` (a binary before theseus-h66v4.6) is read the same way from its totals.

    `effort` is the `effort.v1` replay of the same cases file (`--pack-file effort.v1.toml --cases route.v10.cases.toml`):
    "Jev chooses the model and effort" is graded at the tag, so its `effort` bar joins this run's bars and is graded
    like the others. Only that bar is taken, and an `effort` that is missing, or printed none, leaves the run with no
    effort bar, which `_run_problems` fails."""
    c = raw.get("cases") or {}
    graded = len(c.get("cases") or [])
    bars = [{"question": str(b.get("question") or ""), "ok": int(b.get("ok") or 0), "of": int(b.get("of") or 0),
             "need": int(b.get("need") or 0)} for b in c.get("bars") or []]
    if not bars:
        bars = [_bar("model", int(c.get("model_ok") or 0), graded),
                _bar("must", int(c.get("must_ok") or 0), int(c.get("must_total") or 0))]
    ec = (effort or {}).get("cases") or {}
    bars += [{"question": "effort", "ok": int(b.get("ok") or 0), "of": int(b.get("of") or 0),
              "need": int(b.get("need") or 0)} for b in ec.get("bars") or [] if b.get("question") == "effort"]
    by = {b["question"]: b for b in bars}
    model, must = by.get("model"), by.get("must")
    return {"cases": graded, "answered": int(c.get("answered") or 0), "bars": bars,
            "model": model["ok"] if model else 0, "total": model["of"] if model else 0,
            "musts": must["of"] if must else 0, "musts_held": must["ok"] if must else 0,
            "bar": c.get("bar"), "pack": c.get("pack") or raw.get("candidate"),
            # The packs this run actually asked Jev with, by their text (`candidate_sha256`): two
            # runs that share one asked the same question and were served one cached answer
            # (theseus-h420q, `bust`). None where the replay printed no digest.
            "digest": [d for d in (raw.get("candidate_sha256"),
                                   (effort or {}).get("candidate_sha256")) if d],
            "cost_usd": float(raw.get("cost_usd") or 0) + float((effort or {}).get("cost_usd") or 0),
            "wall_s": wall_s}


def _label(question: str) -> str:
    return {"model": "on the model", "must": "musts", "effort": "on effort"}.get(question, f"on {question}")


def _run_problems(i: int, r: dict[str, Any], ranged: bool = False) -> list[str]:
    """What stops run `i` from meeting the owner's bars. A bar over no cases is never met: the empty set holds every
    "nothing missed" bar, so the positive counts (cases graded, a model bar, musts found) are asserted here.

    `ranged`: the floor records a `model_range` over this run's own count, so the model question is held to that
    range across the runs (`route_part`) instead of to the 80% rule here. The rule is still checked against the
    replay's own printed bar, because a replay whose arithmetic is not the owner's is a finding either way."""
    out = []
    by = {b["question"]: b for b in r["bars"]}
    if "model" not in by:
        out.append(f"run {i}: the replay graded no model bar")
    for q, b in by.items():
        if not b["of"]:
            out.append(f"run {i}: no musts in the cases file" if q == "must" else f"run {i}: no cases graded for {q}")
            continue
        need = bar_need(b["of"])
        if b["need"] != need:
            out.append(f"run {i}: the replay's {q} bar of {b['need']} is not 80% of {b['of']} ({need})")
        if b["ok"] < need and not (q == "model" and ranged):
            out.append(f"run {i}: {b['ok']}/{b['of']} {_label(q)} < {need}")
    if "model" in by and "must" not in by:
        out.append(f"run {i}: the replay graded no must bar")
    if "effort" not in by:
        out.append(f"run {i}: the replay graded no effort bar (is the effort.v1 replay beside it?)")
    return out


def _cached(real: list[dict[str, Any]]) -> list[str]:
    """Runs that asked Jev the same question, so "worst of three" is one sample read three times.

    The 1.0.4 release smoke's three route runs each cost $0.004718 and each scored 51 of 62 to six
    decimal places (theseus-h420q). The driver gives each run its own pack version (`bust`), so
    the runs' pack digests are all different; where they are not, the number this part reports is
    worst-of-one however many runs ran, and that is a FAIL, not a note.

    A replay that printed no digest at all (a binary before `candidate_sha256`) is not judged
    here: there is nothing to compare, and the rest of the part still holds.
    """
    seen: dict[str, int] = {}
    for i, r in enumerate(real, 1):
        for d in r.get("digest") or []:
            if d in seen:
                return [f"runs {seen[d]} and {i} asked Jev with the same pack ({d[:16]}): "
                        f"one cached sample, not {len(real)} — the driver bumps the pack version "
                        f"per run (`smoke.py bust`)"]
            seen[d] = i
    return []


def _worst(real: list[dict[str, Any]], question: str) -> dict[str, Any] | None:
    """The bar of `question` that came closest to failing across the runs: the fewest right, as a share of its count."""
    bars = [b for r in real for b in r["bars"] if b["question"] == question and b["of"]]
    return min(bars, key=lambda b: (b["ok"] / b["of"], b["ok"]), default=None)


def _reading(b: dict[str, Any] | None, question: str) -> str:
    return f"{b['ok']}/{b['of']} {_label(question)}" if b else f"no {question} graded"


def _floor_reading(v: Any) -> Any:
    """A floor's worst reading as the report prints it: `ok/of` (a floor written before the counts were kept holds a
    bare number, printed as it is)."""
    return f"{v['ok']}/{v['of']}" if isinstance(v, dict) else v


def route_part(runs: list[dict[str, Any]], floor: dict | None, live: str | None = None) -> dict[str, Any]:
    """Three runs of the route cases, each meeting the owner's bars: 80% of its own count, rounded up, on the
    model (over the cases the replay graded), on the musts (over the musts it graded) and on effort within one
    rung (the `effort.v1` replay of the same cases, theseus-h66v4.4). Work and split are not graded here: whether
    their packs act in 1.0.4 is an open owner question, and a pack that does not act is "n/a" by the rule below.

    `live` is the route packs the daemon's ladder acts with (live or canary),
    comma-separated. The cases are written for one pack (route.v10): on a daemon
    whose ladder does not act with it, the bar does not apply, and three answered
    runs are "n/a until the flip", their reading kept for the record, never a
    FAIL and never a floor. An unknown or unreadable `live` is held to the bar."""
    problems = []
    real = [r for r in runs if r["cases"] and r["answered"]]
    if len(real) < RUNS_WANTED:
        problems.append(f"{len(real)} of {RUNS_WANTED} runs answered any case")
    # The model question's bar, when the floor records a range read over this candidate's own count.
    counts = {b["of"] for r in real for b in r["bars"] if b["question"] == "model" and b["of"]}
    rng = model_range(floor, next(iter(counts))) if len(counts) == 1 else None
    oks = [b["ok"] for r in real for b in r["bars"] if b["question"] == "model" and b["of"]]
    readings = ({"model": {"low": min(oks), "high": max(oks), "of": next(iter(counts)), "runs": len(oks)}}
                if oks and len(counts) == 1 else {})
    # Every note names a run by its place among all of them, answered or not.
    for i, r in enumerate(runs, 1):
        if not r["cases"]:
            problems.append(f"run {i}: no cases graded")
        elif r["answered"]:
            problems += _run_problems(i, r, ranged=rng is not None)
    problems += _cached(real)
    problems = list(dict.fromkeys(problems))
    wm, wu, we = _worst(real, "model"), _worst(real, "must"), _worst(real, "effort")
    stale = None
    if rng and wm:
        if wm["ok"] < rng["low"]:
            problems.append(
                f"worst {wm['ok']}/{wm['of']} on the model is under the floor's measured range "
                f"{rng['low']}-{rng['high']} of {rng['of']}")
        elif wm["ok"] > rng["high"]:
            stale = (f"worst {wm['ok']}/{wm['of']} on the model is over the floor's measured range "
                     f"{rng['low']}-{rng['high']}: the range is stale, rewrite it with --write-floor")
    worst = float(wm["ok"]) if wm else 0.0
    unit = "worst model ok"
    wall = max((r["wall_s"] for r in real), default=0.0)
    worst_bars = {q: {"ok": b["ok"], "of": b["of"]} for q, b in (("model", wm), ("must", wu), ("effort", we)) if b}
    fl = (floor or {}).get("route")
    # A floor entry written by hand to record a measured range carries no wall, and a wall of nothing is
    # not a wall every run is over: the check applies only to a floor that actually timed itself.
    if fl and fl.get("wall_s") and _over(wall, fl["wall_s"]):
        problems.append(f"slowest run's wall {wall:.0f} s > {SLACK:.0%} of the floor's {fl['wall_s']:.0f} s")
    met = sum(1 for r in real if r["bar"])
    cases_pack = next((r["pack"] for r in real if r.get("pack")), None)
    acting = (live or "").split(",") if live else []
    if not acting or not all(re.fullmatch(r"route\.v\d+", p) for p in acting):
        acting = []
    if acting and cases_pack and cases_pack not in acting and len(real) >= RUNS_WANTED:
        return {"part": "route", "ok": True, "na": True, "count": len(real), "number": worst,
                "unit": unit, "floor": None, "delta": None, "worst": worst_bars, "readings": readings,
                "note": f"n/a until the flip: the daemon routes with {', '.join(acting)}, "
                        f"the cases are {cases_pack}'s; read {_reading(wm, 'model')}, {_reading(wu, 'must')} "
                        f"and {_reading(we, 'effort')} at worst, for the record",
                "cost_usd": round(sum(r["cost_usd"] for r in real), 4),
                "wall_s": round(sum(r["wall_s"] for r in real), 1), "slowest_s": wall, "runs": runs}
    model_bar = (f"the measured range {rng['low']}-{rng['high']} of {rng['of']}" if rng
                 else f"bar {wm['need'] if wm else 0}")
    note = (f"{len(real)} runs, worst {_reading(wm, 'model')} ({model_bar}), "
            f"{_reading(wu, 'must')} (bar {wu['need'] if wu else 0}) and "
            f"{_reading(we, 'effort')} (bar {we['need'] if we else 0}), the last two 80% of their own count "
            f"rounded up; the daemon's whole bar met {met}/{len(real)} (for route.v10 that is the model's, "
            f"as graded here)" + (f"; {stale}" if stale else ""))
    if problems:
        note = "; ".join(problems) + f"; the daemon's whole bar met {met}/{len(real)}"
    return {"part": "route", "ok": not problems, "count": len(real), "number": worst, "unit": unit,
            "floor": _floor_reading(fl.get("model_worst")) if fl else None, "delta": None, "note": note,
            "worst": worst_bars, "readings": readings,
            "cost_usd": round(sum(r["cost_usd"] for r in real), 4),
            "wall_s": round(sum(r["wall_s"] for r in real), 1), "slowest_s": wall, "runs": runs}


def unavailable(part: str, why: str) -> dict[str, Any]:
    """A part that could not run: a FAIL with nothing counted, and why."""
    return {"part": part, "ok": False, "count": 0, "number": 0.0, "unit": "-", "floor": None,
            "delta": None, "note": f"did not run: {why}", "cost_usd": 0.0, "wall_s": 0.0}


# --- parts 3 to 5 ---------------------------------------------------------

def count_recall_tests() -> int:
    return unittest.TestLoader().discover(str(REPO / "bench" / "recall"), pattern="test_*.py",
                                          top_level_dir=str(REPO / "bench" / "recall")).countTestCases()


def recall_part(exit_code: int, seconds: float, tests: int, skipped: bool) -> dict[str, Any]:
    problems = []
    if skipped:
        problems.append("skipped: no Python >= 3.11 (a skip exits 0, and is not a pass)")
    if tests <= 0:
        problems.append("no tests found")
    if exit_code != 0:
        problems.append(f"exit {exit_code}")
    return {"part": "recall", "ok": not problems, "count": tests, "number": seconds, "unit": "s",
            "floor": None, "delta": None, "note": "; ".join(problems) or f"{tests} tests",
            "cost_usd": 0.0, "wall_s": seconds}


def resume_part(r: dict[str, Any]) -> dict[str, Any]:
    """`resume.py`'s record: the kills, the resumes, and the lines each step left on disk."""
    problems = []
    kills, resumed, steps = r["kills"], r["resumed"], r["steps"]
    if kills != KILLS_WANTED:
        problems.append(f"{kills} of {KILLS_WANTED} kills landed mid-tool")
    if resumed != kills or resumed == 0:
        problems.append(f"{resumed} of {kills} resumed")
    if not r["settled"]:
        problems.append("the turn never settled")
    for name in ("starts", "effects"):
        seen = r[name]
        extra = len(seen) - len(set(seen))
        if extra:
            problems.append(f"{extra} repeated {name} ({sorted(seen)})")
    ledger = {k: r.get(k) for k in ("interrupted_rows", "turn_ended", "turn_failed", "unknown_rows", "succeeded_rows")}
    if None in ledger.values():
        problems.append("the scratch ledger was not read: " + ", ".join(k for k, v in ledger.items() if v is None))
    else:
        if ledger["interrupted_rows"] != kills:
            problems.append(f"{ledger['interrupted_rows']} execution.interrupted rows for {kills} kills")
        if ledger["turn_ended"] < 2 or ledger["turn_failed"]:  # the opening "hello" turn's and the task's
            problems.append(f"the turn ended {ledger['turn_ended']} times and failed {ledger['turn_failed']}")
        if ledger["unknown_rows"]:
            problems.append(f"{ledger['unknown_rows']} calls answered 'outcome unknown' after a restart")
        if ledger["succeeded_rows"] != steps:
            problems.append(f"{ledger['succeeded_rows']} proc runs succeeded for {steps} steps")
    unseen = sorted(set(range(1, steps + 1)) - set(r.get("seen") or []))
    if unseen:
        problems.append(f"the model never saw the output of steps {unseen}")
    problems += r.get("notes") or []
    missing = sorted(set(range(1, steps + 1)) - set(r["effects"]))
    if missing:
        problems.append(f"steps with no effect: {missing}")
    return {"part": "resume", "ok": not problems, "count": kills, "number": float(resumed), "unit": "resumed",
            "floor": float(KILLS_WANTED), "delta": None,
            "note": "; ".join(problems) or f"{resumed}/{kills} resumed, {len(r['effects'])} effects, none repeated",
            "cost_usd": 0.0, "wall_s": r.get("wall_s") or 0.0, "record": r}


def dm_part(replies: list[dict[str, Any]], alarms: int, cost_usd: float = 0.0, turns_ended: int | None = None,
            unread: list[str] | None = None) -> dict[str, Any]:
    """`turns_ended` (the session's `turn.ended` rows) is the positive count behind "no alarms": an alarm count
    from a ledger that was not read, or read empty, is not a zero."""
    problems = []
    if unread:
        problems.append(f"the ledger of {', '.join(unread)} was not read")
    if turns_ended is not None and turns_ended < len(replies):
        problems.append(f"{turns_ended} turn.ended rows for {len(replies)} replies: the ledger read is short")
    if turns_ended is not None and turns_ended > len(replies):
        problems.append(f"{turns_ended} turn.ended rows for {len(replies)} replies: not a fresh session")
    if len(replies) != DM_TURNS:
        problems.append(f"{len(replies)} of {DM_TURNS} replies arrived")
    slow = [r["turn"] for r in replies if r["seconds"] > DM_REPLY_S]
    if slow:
        problems.append(f"over {DM_REPLY_S:.0f} s: turns {slow}")
    bare = [r["turn"] for r in replies if not r.get("footer")]
    if bare:
        problems.append(f"no footer: turns {bare}")
    wrong = [r["turn"] for r in replies if not r.get("expected")]
    if wrong:
        problems.append(f"unexpected reply: turns {wrong}")
    if alarms:
        problems.append(f"{alarms} notice or alarm rows")
    worst = max((r["seconds"] for r in replies), default=0.0)
    return {"part": "dm", "ok": not problems, "count": len(replies), "number": worst, "unit": "slowest s",
            "floor": DM_REPLY_S, "delta": None, "note": "; ".join(problems) or f"{len(replies)} replies, footers",
            "cost_usd": cost_usd, "wall_s": round(sum(r["seconds"] for r in replies), 1), "replies": replies}


# --- the verdict, the report, the floor -----------------------------------

def verdict(parts: list[dict[str, Any]]) -> str:
    seen = {p["part"]: p for p in parts}
    missing = [n for n in PARTS if n not in seen]
    failed = [n for n in PARTS if n in seen and not seen[n]["ok"]]
    if not missing and not failed:
        na = [n for n in PARTS if seen[n].get("na")]
        return "PASS" + (f" ({', '.join(na)} n/a until the flip)" if na else "")
    bits = failed + ([", ".join(missing) + ": did not run"] if missing else [])
    return f"FAIL ({', '.join(bits)})"


def _fmt(v: Any) -> str:
    if v is None:
        return "-"
    if isinstance(v, float):
        return "inf" if math.isinf(v) else f"{v:.3f}".rstrip("0").rstrip(".")
    return str(v)


def report_md(parts: list[dict[str, Any]], run: dict[str, Any]) -> str:
    by = {p["part"]: p for p in parts}
    out = [f"# {verdict(parts)}", "",
           f"Release smoke of `{run['sha']}` on {run['date']} ({run['binary']}). Not reportable: a release "
           "check against the floor in `bench/smoke/floor.json`.", "",
           "| part | result | number | floor | delta |", "|---|---|---|---|---|"]
    for n in PARTS:
        p = by.get(n)
        if p is None:
            out.append(f"| {n} | did not run | - | - | - |")
            continue
        # An n/a part's note says so itself ("n/a until the flip: ...").
        result = p["note"] if p.get("na") else f"{'PASS' if p['ok'] else 'FAIL'}: {p['note']}"
        out.append(f"| {n} | {result} | {_fmt(p['number'])} {p['unit']} "
                   f"(n={p['count']}) | {_fmt(p['floor'])} | {p['delta'] or '-'} |")
    cost = sum(p.get("cost_usd") or 0 for p in parts)
    wall = sum(p.get("wall_s") or 0 for p in parts)
    out += ["", f"**Cost: ${cost:.2f}** (price table `{price_digest()}`), **wall: {wall / 60:.1f} min** "
            "(the parts' own clocks).", ""]
    tb = by.get("tb")
    if tb and tb.get("trials"):
        out += ["## TB2 slice, per task", "", "| task | passed | $ | wall s | turns | models | exception |",
                "|---|---|---|---|---|---|---|"]
        for r in tb["trials"]:
            models = ", ".join(f"{m} {c}" for m, c in sorted(r["models"].items()))
            out.append(f"| {r['task']} | {'yes' if r['passed'] else 'no'} | {_fmt(r['cost_usd'])} | "
                       f"{_fmt(r['wall_s'])} | {r['turns']} | {models} | {r['exception'] or '-'} |")
        out.append("")
    return "\n".join(out)


def floor_from(parts: list[dict[str, Any]], sha: str, date: str, previous: dict | None = None) -> dict[str, Any]:
    """The floor after this run: each part's entry from this run when the part
    passed, else the previous floor's (a failed part's reading is never a floor).
    Nothing passed that a floor reads
    (tb, route): no floor at all."""
    f: dict[str, Any] = {k: v for k, v in (previous or {}).items() if k in ("tb", "route")}
    wrote = []
    for p in parts:
        if not p["ok"] or p.get("na"):
            continue
        # The floor is this release's passing reading, for the next release to be held to. The slack can compound
        # over releases; each tag's commit of floor.json shows the move, and a re-base is a hand edit.
        if p["part"] == "tb":
            f["tb"] = {"cost_per_solved_usd": round(p["number"], 6), "wall_s": p["wall_s"],
                       "price_digest": p["price_digest"], "sha": sha, "date": date}
            wrote.append("tb")
        elif p["part"] == "route":
            # Each worst reading keeps the count it was over: a bare 49 beside a later 52/65 means nothing.
            w = p.get("worst") or {}
            # `model_range` is the statement about the measured distribution the owner asked for on
            # 2026-11-03 ("Range of measurement seems good too"): the lowest and highest of this run's
            # readings, over the count they were read against. The next release's worst of three must not
            # fall below `low`. `model_worst` stays beside it, as the one number the report prints.
            rd = p.get("readings") or {}
            f["route"] = {"wall_s": p["slowest_s"], "model_worst": w["model"],
                          **({"model_range": rd["model"]} if rd.get("model") else {}),
                          **({"must_worst": w["must"]} if w.get("must") else {}),
                          **({"effort_worst": w["effort"]} if w.get("effort") else {}), "sha": sha, "date": date}
            wrote.append("route")
    if not wrote:
        raise SmokeError("no part a floor reads passed, so no floor is written: " + verdict(parts))
    return {"written": {"sha": sha, "date": date, "parts": wrote, "verdict": verdict(parts)}, **f}


# --- the command line -----------------------------------------------------

def _read_json(path: str) -> dict | None:
    """A replay the shell could not write, or wrote half of, is no replay: the part fails it as a missing bar."""
    try:
        v = json.loads(Path(path).read_text())
    except (OSError, ValueError):
        return None
    return v if isinstance(v, dict) else None


def load_floor() -> dict | None:
    return json.loads(FLOOR_FILE.read_text()) if FLOOR_FILE.exists() else None


def _save(run_dir: Path, p: dict[str, Any]) -> int:
    (run_dir / "parts").mkdir(parents=True, exist_ok=True)
    (run_dir / "parts" / f"{p['part']}.json").write_text(json.dumps(p, indent=1) + "\n")
    label = "n/a" if p.get("na") else "PASS" if p["ok"] else "FAIL"
    print(f"smoke {p['part']}: {label} {_fmt(p['number'])} {p['unit']} "
          f"(n={p['count']}): {p['note']}")
    return 0 if p["ok"] else 1


def main(argv: list[str]) -> int:
    if len(argv) < 2:
        print(__doc__, file=sys.stderr)
        return 2
    cmd, run_dir, rest = argv[0], Path(argv[1]), argv[2:]
    if cmd == "tb":
        s = load_slice(HERE / "tb-slice.toml")
        return _save(run_dir, tb_part(read_trials(Path(rest[0])), s, load_floor(), float(rest[1])))
    if cmd == "fail":
        return _save(run_dir, unavailable(rest[0], rest[1]))
    if cmd == "spent":
        print(f"{spent(run_dir):.4f}")
        return 0
    if cmd == "bust":
        # bust RUN_DIR RUN CASES PACK...  -> the pack copies then the cases copy, one a line.
        run, cases, packs = int(rest[0]), Path(rest[1]), [Path(p) for p in rest[2:]]
        for p in bust(run_dir, run, packs, cases):
            print(p)
        return 0
    if cmd == "route":
        live = None
        if rest[:1] == ["--live"]:
            live, rest = rest[1] or None, rest[2:]
        runs = []
        while rest:
            replay_json, wall, rest = rest[0], float(rest[1]), rest[2:]
            effort = None
            if rest[:1] == ["--effort"]:
                effort, rest = _read_json(rest[1]), rest[2:]
            runs.append(route_run(json.loads(Path(replay_json).read_text()), wall, effort))
        return _save(run_dir, route_part(runs, load_floor(), live))
    if cmd == "recall":
        log = Path(rest[2]).read_text() if Path(rest[2]).exists() else ""
        return _save(run_dir, recall_part(int(rest[0]), float(rest[1]), count_recall_tests(),
                                          "skipped" in log))
    if cmd == "resume":
        return _save(run_dir, resume_part(json.loads(Path(rest[0]).read_text())))
    if cmd == "dm":
        d = json.loads(Path(rest[0]).read_text())
        return _save(run_dir, dm_part(d["replies"], int(d["alarms"]), float(d.get("cost_usd") or 0),
                                      d.get("turns_ended", 0), d.get("ledger_unread")))
    if cmd == "report":
        sha, binary = rest[0], rest[1]
        parts = [json.loads((run_dir / "parts" / f"{n}.json").read_text())
                 for n in PARTS if (run_dir / "parts" / f"{n}.json").exists()]
        date = run_dir.name.split("-" + sha[:8])[0]
        (run_dir / "report.md").write_text(report_md(parts, {"sha": sha, "date": date, "binary": binary}))
        v = verdict(parts)
        print(f"smoke: {v}; report {run_dir / 'report.md'}")
        if "--write-floor" in rest:
            try:
                f = floor_from(parts, sha, date, load_floor())
            except SmokeError as e:
                print(f"smoke: {e}")
                return 1
            FLOOR_FILE.write_text(json.dumps(f, indent=1) + "\n")
            print(f"smoke: floor written to {FLOOR_FILE} from {', '.join(f['written']['parts'])}")
        return 0 if v.startswith("PASS") else 1
    print(f"smoke.py: unknown command {cmd!r}", file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
