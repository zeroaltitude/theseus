#!/usr/bin/env python3
"""Build cell 10's dataset from the M1 Max's recall scores: the .json, the .csv
and the numbers the report's prose needs.

    recall_report.py fetch              copy each finished seed's scores here
    recall_report.py build 11 13        write the dataset and print the prose numbers

The published comparison is the 2026-10-18 full run at seed 7, and **the rivals
are not rerun** (the rivals-not-rerun rule): Claude Code and Pi carry nothing
from one session to the next and scored 0 and 1 of 48 across sessions, which is
the published comparison and a structural fact about those harnesses rather than
a number that moves with a build. So this cell is Theseus at two new seeds
against Theseus at seed 7, plus that standing cross-session contrast.

Two seeds is a spread, not an interval. Every number here is reported per seed
and as a range, never pooled into a single confident figure, because n=2.
"""
import csv
import json
import statistics as st
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
RUNS = HERE.parent / "runs/c10-recall"
DOCS = Path.home() / "lanes/gauntlet/docs/benchmarks"
DATE = "2026-10-26"
NAME = f"{DATE}-recall-seeds-1.0.3"

# The published 2026-10-18 run at seed 7, for the comparison column. Taken from
# docs/benchmarks/2026-10-18-recall-full-run.{md,json}, not retyped from memory.
# RECOMPUTED from 2026-10-18-recall-full-run.csv on THIS file's own grouping, not
# copied from that report's prose. Two traps, both found by checking denominators:
#   - its prose says "60 of 64" within a session, which groups buckets differently;
#     on the grouping here (every bucket that is not `session` or `days`) it is 71/88;
#   - its "57 undelivered" is the count its delivery check FLAGGED. 46 were recovered
#     by hand and 10 remain unproven in the published data file, so 10 is the number
#     a new run's 0 should be compared against.
# Its overall rate is over 134 scored recall probes; a new seed has 144.
SEED7 = {
    "recall_accuracy": 0.731, "recall_right": 98, "recall_n": 134,
    "cross_right": 27, "cross_n": 46,
    "within_right": 71, "within_n": 88, "abstain_gave_value": 38, "abstain_n": 60,
    # TWO COST BASES, and conflating them is how this file first got it wrong.
    # `run_cost_usd` is the whole 600-turn progression (the published report's
    # prose "$11.44"); `cost_usd` is the sum over the 204 PROBE turns only
    # (the published CSV sums to $4.8412, per-probe $0.0237). A new seed's
    # probe-sum must be compared with $4.84, never with $11.44.
    "run_cost_usd": 11.44,
    "cost_usd": 4.8412, "undelivered": 10, "undelivered_flagged": 57,
    "abstention_accuracy": 0.3667, "overhead_tokens": 7335, "cost_usd_per_probe": 0.0237,
    # 2, not 19. 19 is CLAUDE CODE's confident-wrong count in the same summary
    # block; I read the wrong arm's row out of a dump and published it, which
    # hid a real regression (this build is at 21 and 11 against 2).
    "confident_wrong": 2, "confident_wrong_rate": 0.0103,
    "note": "recomputed from the published CSV on this report's grouping; see the comment in recall_report.py",
}
CROSS = ("session", "days")


def fetch(seeds):
    RUNS.mkdir(parents=True, exist_ok=True)
    for s in seeds:
        out = RUNS / f"seed{s}-scores.json"
        if out.exists() and out.stat().st_size > 1000:
            print(f"seed {s}: already here")
            continue
        p = subprocess.run([sys.executable, str(HERE / "arm_peek.py"),
                            f"cat ~/recall-g/seed{s}-scores/scores.json"],
                           capture_output=True, timeout=180)
        text = p.stdout.decode(errors="replace")
        if not text.strip().startswith("{"):
            print(f"seed {s}: not ready ({text.strip()[:90]})")
            continue
        out.write_text(text)
        print(f"seed {s}: fetched {len(text)} bytes")


def verify_seed7() -> list[str]:
    """Re-check every SEED7 constant against the published dataset.

    Three of this report's numbers were wrong before publication or just after,
    all from the same cause: taking a figure from a prior run's PROSE, or from
    the wrong arm's row in a dump, instead of from its data file. This makes the
    check mechanical. It runs on every build and prints a loud line if a
    constant has drifted.
    """
    pub = DOCS / "2026-10-18-recall-full-run.json"
    if not pub.exists():
        return ["published dataset not found; SEED7 unverified"]
    s = json.load(open(pub))["summary"]["theseus/baseline"]
    want = {
        "recall_accuracy": s.get("recall_accuracy"),
        "abstention_accuracy": s.get("abstention_accuracy"),
        "confident_wrong": s.get("confident_wrong"),
        "undelivered": s.get("undelivered"),
        "overhead_tokens": (s.get("overhead") or {}).get("measured"),
    }
    bad = []
    for k, v in want.items():
        mine = SEED7.get(k)
        if mine is None or v is None:
            continue
        if abs(float(mine) - float(v)) > max(0.002, 0.02 * abs(float(v))):
            bad.append(f"SEED7[{k}] = {mine} but the published dataset says {v}")
    return bad


def fetch_run_costs(seeds):
    """The whole-progression cost a seed, summed over all 600 turns.

    `scores.json` carries only the probe turns, so the arm's real spend is not
    in it -- and the published report's headline dollar figure IS the whole run.
    Comparing a new seed's probe-sum with that number understates the gap by
    about 2.5x, which is exactly the mistake this function exists to prevent."""
    out = {}
    for s in seeds:
        r = subprocess.run([sys.executable, str(HERE / "arm_peek.py"),
                            f'python3 -c \'import json,os;print(sum(float(json.loads(l).get("cost_usd") or 0) '
                            f'for l in open(os.path.expanduser("~/recall-g/seed{s}/turns.jsonl")) if l.strip()))\''],
                           capture_output=True, timeout=180)
        try:
            out[s] = round(float(r.stdout.decode().strip().splitlines()[-1]), 4)
        except (ValueError, IndexError):
            out[s] = None
    return out


def per_seed(path):
    d = json.load(open(path))
    arm = next(iter(d["arms"]))
    s, p = d["arms"][arm], d["probes"]
    rec = [x for x in p if x["kind"] != "abstention"]
    ab = [x for x in p if x["kind"] == "abstention"]
    cross = [x for x in rec if x["bucket"] in CROSS]
    within = [x for x in rec if x["bucket"] not in CROSS]
    by_bucket = {}
    for b in sorted({x["bucket"] for x in rec}):
        g = [x for x in rec if x["bucket"] == b]
        by_bucket[b] = {"right": sum(x["correct"] for x in g), "n": len(g)}
    return {
        "arm": arm, "digest": d["digest"], "stale_rule": d["stale_rule"],
        "probes": len(p), "scored": s["scored"], "failed": s["failed"],
        "undelivered": s["undelivered"],
        "recall_accuracy": round(s["recall_accuracy"], 4),
        "recall_right": sum(x["correct"] for x in rec), "recall_n": len(rec),
        "cross_right": sum(x["correct"] for x in cross), "cross_n": len(cross),
        "within_right": sum(x["correct"] for x in within), "within_n": len(within),
        "abstention_accuracy": round(s["abstention_accuracy"], 4),
        "abstain_gave_value": sum(1 for x in ab if not x["correct"]), "abstain_n": len(ab),
        "confident_wrong": s["confident_wrong"],
        "confident_wrong_rate": round(s["confident_wrong_rate"], 4),
        "cites_when": round(s["cites_when"], 4), "cites_where": round(s["cites_where"], 4),
        "half_life": s.get("half_life"),
        "by_carrier": {k: round(v, 4) for k, v in (s.get("by_carrier") or {}).items()},
        "by_value_kind": {k: round(v, 4) for k, v in (s.get("by_value_kind") or {}).items()},
        "by_kind": {k: round(v, 4) for k, v in (s.get("by_kind") or {}).items()},
        # The system-prompt-and-tools overhead the run measured for itself: the
        # published comparison moved on this (15,835 -> 7,335 tokens) and it is
        # what the context window is planned against.
        "overhead_tokens": (s.get("overhead") or {}).get("measured"),
        "cost_usd_per_probe": round(s["cost_usd_per_probe"], 6),
        # The probe turns only. See SEED7's comment: the published report's
        # headline dollar figure is the WHOLE run, a different quantity.
        "cost_usd": round(sum(x["cost_usd"] for x in p), 4),
        "run_cost_usd": None,          # filled by fetch_run_costs()
        "latency_ms_p50": round(st.median(x["latency_ms"] for x in p)),
        "by_bucket": by_bucket,
        "_probes": p,
    }


def build(seeds):
    rows = {}
    for s in seeds:
        f = RUNS / f"seed{s}-scores.json"
        if not f.exists():
            print(f"seed {s}: no scores yet; skipping")
            continue
        rows[s] = per_seed(f)
    if not rows:
        sys.exit("no seeds scored yet")
    for s, c in fetch_run_costs(list(rows)).items():
        rows[s]["run_cost_usd"] = c

    data = {
        "report": NAME, "suite": "recall", "date": DATE, "kind": "recall",
        "stale_rule": next(iter(rows.values()))["stale_rule"],
        "build": "92e3dbed (the frozen 1.0.3 behaviour build)",
        "host": "M1 Max, aarch64 (the published seed-7 run was on the WSL box)",
        "arms": [{"key": f"theseus/baseline@seed{s}", "label": f"Theseus 1.0.3, seed {s}"}
                 for s in rows],
        "summary": {f"theseus/baseline@seed{s}": {k: v for k, v in r.items() if k != "_probes"}
                    for s, r in rows.items()},
        "compare": {"2026-10-18 seed 7, Theseus": SEED7,
                    "note": "rivals not rerun: Claude Code 0 of 48 and Pi 1 of 48 across "
                            "sessions on seed 7, a structural fact about harnesses that "
                            "carry nothing between sessions"},
    }
    out = RUNS / f"{NAME}.json"
    out.write_text(json.dumps(data, indent=1, sort_keys=False) + "\n")

    csv_path = RUNS / f"{NAME}.csv"
    cols = ["arm", "seed", "kind", "salience", "bucket", "planned", "status", "correct",
            "turns_since", "tokens_since", "cost_usd", "latency_ms"]
    with open(csv_path, "w", newline="") as fh:
        w = csv.writer(fh)
        w.writerow(cols)
        for s, r in rows.items():
            for x in r["_probes"]:
                w.writerow([r["arm"], s] + [x.get(c) for c in cols[2:]])

    print(f"wrote {out.name} and {csv_path.name}\n")
    rng = lambda f: f"{min(f(r) for r in rows.values()):.1f}-{max(f(r) for r in rows.values()):.1f}"
    for line in verify_seed7():
        print("  !! " + line)
    print("PROSE NUMBERS")
    for s, r in rows.items():
        print(f"  seed {s}: recall {100*r['recall_accuracy']:.1f}% "
              f"({r['recall_right']}/{r['recall_n']}), cross-session {r['cross_right']}/{r['cross_n']} "
              f"= {100*r['cross_right']/r['cross_n']:.1f}%, within {r['within_right']}/{r['within_n']} "
              f"= {100*r['within_right']/r['within_n']:.1f}%, abstention gave a value "
              f"{r['abstain_gave_value']}/{r['abstain_n']}, confident-wrong {r['confident_wrong']}, "
              f"undelivered {r['undelivered']}, probes ${r['cost_usd']:.2f}, "
              f"whole run ${r['run_cost_usd'] or 0:.2f}, p50 {r['latency_ms_p50']} ms")
    print(f"\n  range across seeds: recall {rng(lambda r: 100*r['recall_accuracy'])}%, "
          f"probe-cost ${rng(lambda r: r['cost_usd'])}, "
          f"WHOLE-RUN cost ${rng(lambda r: r['run_cost_usd'] or 0)}, "
          f"abstention accuracy {rng(lambda r: 100*r['abstention_accuracy'])}%, "
          f"overhead {rng(lambda r: r['overhead_tokens'] or 0)} tokens")
    print(f"  seed 7 published:  recall {100*SEED7['recall_accuracy']:.1f}%, "
          f"cross {SEED7['cross_right']}/{SEED7['cross_n']} "
          f"= {100*SEED7['cross_right']/SEED7['cross_n']:.1f}%, "
          f"within {SEED7['within_right']}/{SEED7['within_n']}, abstention gave a value "
          f"{SEED7['abstain_gave_value']}/{SEED7['abstain_n']}, undelivered "
          f"{SEED7['undelivered']}, probes ${SEED7['cost_usd']:.2f}, "
          f"whole run ${SEED7['run_cost_usd']:.2f}")
    print("\n  by bucket:")
    buckets = sorted({b for r in rows.values() for b in r["by_bucket"]})
    print("    " + "seed".ljust(6) + "".join(b[:12].ljust(14) for b in buckets))
    for s, r in rows.items():
        cells = []
        for b in buckets:
            v = r["by_bucket"].get(b)
            cells.append((f"{v['right']}/{v['n']}" if v else "-").ljust(14))
        print("    " + str(s).ljust(6) + "".join(cells))
    return 0


def main() -> int:
    cmd = sys.argv[1] if len(sys.argv) > 1 else ""
    seeds = [int(a) for a in sys.argv[2:]] or [11, 13]
    if cmd == "fetch":
        fetch(seeds); return 0
    if cmd == "build":
        fetch(seeds); return build(seeds)
    sys.exit(__doc__)


if __name__ == "__main__":
    sys.exit(main())
