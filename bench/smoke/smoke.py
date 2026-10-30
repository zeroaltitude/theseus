"""The release smoke (theseus-yf6pj): five parts, each PASS or FAIL against the
floor the last release wrote, in about an hour, for a few dollars, on one host.

It is a release check, not a measurement: nothing here is reportable, and
nothing is written under docs/benchmarks/. `scripts/release-smoke.sh` runs the
parts and calls this module to judge each one, write the run's report, and,
with `--write-floor` only, the floor.

    python3 bench/smoke/smoke.py tb RUN_DIR JOB_DIR WALL_S
    python3 bench/smoke/smoke.py route RUN_DIR (REPLAY.json WALL_S)...
    python3 bench/smoke/smoke.py recall RUN_DIR EXIT SECONDS LOG
    python3 bench/smoke/smoke.py resume RUN_DIR RESUME.json
    python3 bench/smoke/smoke.py dm RUN_DIR DM.json
    python3 bench/smoke/smoke.py report RUN_DIR SHA BINARY [--write-floor]
    python3 bench/smoke/smoke.py fail RUN_DIR PART WHY      (a part that could not run)
    python3 bench/smoke/smoke.py spent JOB_DIR              (tb's finished trials' dollars)

Each part command writes `RUN_DIR/parts/<part>.json` and prints its one line;
`report` writes `RUN_DIR/report.md` and exits 0 on PASS, 1 on FAIL.

A PASS needs a positive count first (10 trials ran, 3 runs replayed, the recall
tests ran, 3 kills happened, 5 replies arrived): an empty set meets every "no
failures" bar, so it never counts as one. Standard library only; Python >= 3.11.
"""

from __future__ import annotations

import csv
import json
import math
import sys
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
# The routing bar of the 1.0.4 cases (spec §4.3): 54 of the 60 on the model, every must.
CASES_MODEL_BAR = 54
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
        model = price_basis.reprice(by_model, table)
        jev = float(eff.get("cost_usd") or 0) - float(eff.get("model_cost_usd") or eff.get("cost_usd") or 0)
        cost = None if model is None else round(model + max(jev, 0.0), 6)
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

def route_run(raw: dict[str, Any], wall_s: float) -> dict[str, Any]:
    """One `theseus --json judge replay --cases` result, as the part reads it. The two
    pin tests (W39, W40) are daemon tests held elsewhere, and count as met, as the
    design's 60 does."""
    c = raw.get("cases") or {}
    held = int(c.get("held_elsewhere") or 0)
    return {"cases": len(c.get("cases") or []), "answered": int(c.get("answered") or 0),
            "model": int(c.get("model_ok") or 0) + held, "total": len(c.get("cases") or []) + held,
            "musts": int(c.get("must_total") or 0), "musts_held": int(c.get("must_ok") or 0),
            "bar": c.get("bar"), "cost_usd": float(raw.get("cost_usd") or 0), "wall_s": wall_s}


def route_part(runs: list[dict[str, Any]], floor: dict | None) -> dict[str, Any]:
    """Three replays of the route cases: each run 54 of 60 on the model, every must."""
    problems = []
    real = [r for r in runs if r["cases"] and r["answered"]]
    if len(real) < RUNS_WANTED:
        problems.append(f"{len(real)} of {RUNS_WANTED} runs answered any case")
    for i, r in enumerate(real):
        if r["model"] < CASES_MODEL_BAR:
            problems.append(f"run {i + 1}: {r['model']}/{r['total']} on the model < {CASES_MODEL_BAR}")
        if r["musts_held"] < r["musts"] or not r["musts"]:
            problems.append(f"run {i + 1}: {r['musts_held']}/{r['musts']} musts")
    worst = float(min((r["model"] for r in real), default=0))
    wall = max((r["wall_s"] for r in real), default=0.0)
    fl = (floor or {}).get("route")
    if fl and _over(wall, fl["wall_s"]):
        problems.append(f"slowest run's wall {wall:.0f} s > {SLACK:.0%} of the floor's {fl['wall_s']:.0f} s")
    met = sum(1 for r in real if r["bar"])
    note = (f"{len(real)} runs, worst {worst:.0f}/60 on the model, every must held; the daemon's whole bar met "
            f"{met}/{len(real)} (it asks effort, context and confidence too)")
    if problems:
        note = "; ".join(problems) + f"; the daemon's whole bar met {met}/{len(real)}"
    return {"part": "route", "ok": not problems, "count": len(real), "number": worst, "unit": "worst model/60",
            "floor": fl.get("model_worst") if fl else None, "delta": None, "note": note,
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
    if r.get("interrupted_rows") is not None and r["interrupted_rows"] != kills:
        problems.append(f"{r['interrupted_rows']} execution.interrupted rows for {kills} kills")
    problems += r.get("notes") or []
    missing = sorted(set(range(1, steps + 1)) - set(r["effects"]))
    if missing:
        problems.append(f"steps with no effect: {missing}")
    return {"part": "resume", "ok": not problems, "count": kills, "number": float(resumed), "unit": "resumed",
            "floor": float(KILLS_WANTED), "delta": None,
            "note": "; ".join(problems) or f"{resumed}/{kills} resumed, {len(r['effects'])} effects, none repeated",
            "cost_usd": 0.0, "wall_s": r.get("wall_s") or 0.0, "record": r}


def dm_part(replies: list[dict[str, Any]], alarms: int, cost_usd: float = 0.0) -> dict[str, Any]:
    problems = []
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
        return "PASS"
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
           f"Release smoke of `{run['sha']}` on {run['date']} (binary: {run['binary']}). Not reportable: a release "
           "check against the floor in `bench/smoke/floor.json`.", "",
           "| part | result | number | floor | delta |", "|---|---|---|---|---|"]
    for n in PARTS:
        p = by.get(n)
        if p is None:
            out.append(f"| {n} | did not run | - | - | - |")
            continue
        out.append(f"| {n} | {'PASS' if p['ok'] else 'FAIL'}: {p['note']} | {_fmt(p['number'])} {p['unit']} "
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
    passed, else the previous floor's (a failed reading is never a floor).
    Nothing passed that a floor reads (tb, route): no floor at all."""
    f: dict[str, Any] = {k: v for k, v in (previous or {}).items() if k in ("tb", "route")}
    wrote = []
    for p in parts:
        if not p["ok"]:
            continue
        if p["part"] == "tb":
            f["tb"] = {"cost_per_solved_usd": round(p["number"], 6), "wall_s": p["wall_s"],
                       "price_digest": p["price_digest"], "sha": sha, "date": date}
            wrote.append("tb")
        elif p["part"] == "route":
            f["route"] = {"wall_s": p["slowest_s"], "model_worst": p["number"], "sha": sha, "date": date}
            wrote.append("route")
    if not wrote:
        raise SmokeError("no part a floor reads passed, so no floor is written: " + verdict(parts))
    return {"written": {"sha": sha, "date": date, "parts": wrote, "verdict": verdict(parts)}, **f}


# --- the command line -----------------------------------------------------

def load_floor() -> dict | None:
    return json.loads(FLOOR_FILE.read_text()) if FLOOR_FILE.exists() else None


def _save(run_dir: Path, p: dict[str, Any]) -> int:
    (run_dir / "parts").mkdir(parents=True, exist_ok=True)
    (run_dir / "parts" / f"{p['part']}.json").write_text(json.dumps(p, indent=1) + "\n")
    print(f"smoke {p['part']}: {'PASS' if p['ok'] else 'FAIL'} {_fmt(p['number'])} {p['unit']} "
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
    if cmd == "route":
        runs = [route_run(json.loads(Path(x).read_text()), float(w)) for x, w in zip(rest[::2], rest[1::2])]
        return _save(run_dir, route_part(runs, load_floor()))
    if cmd == "recall":
        log = Path(rest[2]).read_text() if Path(rest[2]).exists() else ""
        return _save(run_dir, recall_part(int(rest[0]), float(rest[1]), count_recall_tests(),
                                          "skipped" in log))
    if cmd == "resume":
        return _save(run_dir, resume_part(json.loads(Path(rest[0]).read_text())))
    if cmd == "dm":
        d = json.loads(Path(rest[0]).read_text())
        return _save(run_dir, dm_part(d["replies"], int(d["alarms"]), float(d.get("cost_usd") or 0)))
    if cmd == "report":
        sha, binary = rest[0], rest[1]
        parts = [json.loads((run_dir / "parts" / f"{n}.json").read_text())
                 for n in PARTS if (run_dir / "parts" / f"{n}.json").exists()]
        date = run_dir.name.split("-" + sha[:8])[0]
        (run_dir / "report.md").write_text(report_md(parts, {"sha": sha, "date": date, "binary": binary}))
        v = verdict(parts)
        print(f"smoke: {v}; report {run_dir / 'report.md'}")
        if "--write-floor" in rest:
            f = floor_from(parts, sha, date, load_floor())
            FLOOR_FILE.write_text(json.dumps(f, indent=1) + "\n")
            print(f"smoke: floor written to {FLOOR_FILE} from {', '.join(f['written']['parts'])}")
        return 0 if v == "PASS" else 1
    print(f"smoke.py: unknown command {cmd!r}", file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
