#!/usr/bin/env python3
"""Mark the trials of a cell that overlap the conductor's WSL gate windows.

    gatewindows.py <run-dir> [--csv OUT.csv]

The conductor ran four full join gates on this host (an 8,703-test suite at 8
threads plus a release build each) and wrote the windows as `GAUNTLET NOTE: WSL
gate <start>-<end>` lines in the context doc. A trial that overlaps one of them
ran under neighbour IO, which memory `theseus-bench-neighbour-io` and the plan's
validity section both warn about. This says which trials those were, rather than
leaving it to be discovered in the numbers.

Trial timestamps in `result.json` are UTC with a `Z`; the windows are MST
(UTC-7). Overlap is on the whole trial, setup included, because the gate's load
hits a `docker build` and an `apt-get install` as hard as it hits the agent.
"""
import csv
import glob
import json
import os
import sys
from datetime import datetime, timedelta, timezone

MST = timezone(timedelta(hours=-7))

# (label, start, end) in MST on 2026-10-26, from the context doc's
# `GAUNTLET NOTE: WSL gate` lines.
WINDOWS = [
    ("gate-1 p3s-75t4q",   "08:32:19", "09:11:09"),
    ("gate-2 p3s-75t4q-b", "11:00:28", "11:45:44"),
    ("gate-3 p3s-75t4q-c", "12:05:57", "12:21:00"),
    ("gate-4 p3s-75t4q-d", "12:22:47", "12:48:45"),
]
DAY = "2026-10-26"


def windows():
    out = []
    for label, a, b in WINDOWS:
        out.append((label,
                    datetime.fromisoformat(f"{DAY}T{a}").replace(tzinfo=MST),
                    datetime.fromisoformat(f"{DAY}T{b}").replace(tzinfo=MST)))
    return out


def parse(ts):
    if not ts:
        return None
    return datetime.fromisoformat(ts.replace("Z", "+00:00"))


def rows(run_dir):
    wins = windows()
    out = []
    for rj in sorted(glob.glob(os.path.join(run_dir, "jobs", "*", "*__*", "result.json"))):
        r = json.load(open(rj))
        s, f = parse(r.get("started_at")), parse(r.get("finished_at"))
        if not s or not f:
            continue
        hit = [label for label, a, b in wins if s < b and f > a]
        rw = os.path.join(os.path.dirname(rj), "verifier", "reward.txt")
        passed = ""
        if os.path.exists(rw):
            passed = "pass" if open(rw).read().strip().startswith("1") else "fail"
        eff = os.path.join(os.path.dirname(rj), "agent", "efficiency.json")
        cost = ""
        if os.path.exists(eff):
            try:
                cost = f'{json.load(open(eff)).get("cost_usd") or 0:.4f}'
            except (ValueError, OSError):
                pass
        out.append({
            "trial": r.get("trial_name", ""),
            "task": r.get("task_name", ""),
            "start_mst": s.astimezone(MST).strftime("%H:%M:%S"),
            "end_mst": f.astimezone(MST).strftime("%H:%M:%S"),
            "result": passed,
            "cost_usd": cost,
            "gates": ";".join(hit),
        })
    return out


def main():
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    run_dir = sys.argv[1]
    rs = rows(run_dir)
    marked = [r for r in rs if r["gates"]]
    print(f"{os.path.basename(run_dir.rstrip('/'))}: {len(rs)} finished trials, "
          f"{len(marked)} under a gate window")
    for label, a, b in windows():
        n = [r for r in marked if label in r["gates"]]
        if n:
            p = sum(1 for r in n if r["result"] == "pass")
            print(f"  {label} {a:%H:%M:%S}-{b:%H:%M:%S}: {len(n)} trials, {p} pass")
    clean = [r for r in rs if not r["gates"]]
    for name, group in (("under a gate", marked), ("clean", clean)):
        if group:
            p = sum(1 for r in group if r["result"] == "pass")
            print(f"  {name}: {len(group)} trials, {p} pass = {100*p/len(group):.1f}%")
    if "--csv" in sys.argv:
        out = sys.argv[sys.argv.index("--csv") + 1]
        with open(out, "w", newline="") as fh:
            w = csv.DictWriter(fh, fieldnames=list(rs[0].keys()))
            w.writeheader()
            w.writerows(rs)
        print("wrote", out)


if __name__ == "__main__":
    main()
