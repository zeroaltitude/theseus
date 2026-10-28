#!/usr/bin/env python3
"""Build a Terminal-Bench cell's dataset and the numbers its report's prose
needs, in the schema the published runs use.

    tb_report.py <run-dir> [--baseline theseus-high-v1] [--csv] [--json OUT]
                 [--swap <regate-run-dir>]

Written for cell 1 and deliberately general, because cells 2-7 need the same
thing and a successor should not write it again.

Three things it does that a plain summary would not:

- **It separates the gate-contaminated trials from the clean ones** and reports
  both, using the same windows as `gatewindows.py`. A cell with a known
  contaminated window must never be published as one number.
- **It counts k per task rather than deriving it**, because `trials/tasks` is
  wrong the moment an arm skips a task -- the plan's §6.2, which the 10-25 Codex
  run's two safety refusals already broke.
- **It reports a Wilson interval on trials**, as the published rows do, and says
  `n` and `k` beside it so no one can read a k=1 interval as a k=3 one.

`--swap` is what makes the `c1r` rerun publishable rather than merely run: it
substitutes each contaminated replicate with its rerun from the regate cell, one
for one and per task, so the published cell keeps k=3 with three clean trials.
It refuses rather than guesses if the regate cell did not produce enough
replicates of a task, because a silent short-fall would quietly drop a task's k
from 3 to 2 and the row's `k` would then be a lie. The contaminated trials stay
in the dataset under `replaced`, never deleted -- the brief's "do not silently
drop them and do not silently keep them".
"""
import csv
import glob
import json
import math
import os
import statistics as st
import sys
from collections import Counter, defaultdict
from datetime import datetime
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from gatewindows import MST, parse, rows as gate_rows  # noqa: E402

# Published baselines, for the comparison column. Read from the datasets in
# docs/benchmarks, not retyped: see `--baseline`.
BASELINES = {
    "theseus-high-v1": {
        "report": "2026-10-18-terminal-bench-v1-high", "arm": "theseus",
        "label": "Theseus as shipped (routed), v1 freeze 351159fc, HIGH, 89x3",
    },
    "theseus-max-v1": {
        "report": "2026-10-18-terminal-bench-v1-max", "arm": "theseus",
        "label": "Theseus as shipped (routed), v1 freeze, MAX, hard 30",
    },
    # The rivals that DO have a published terminal-bench@2.0 row. OpenHands,
    # Aider and OpenClaw have none -- they appear only on the old 28-trial
    # `terminal-bench`, which is a different benchmark and not a baseline for
    # cells 2-4. Those cells are first measurements and get no comparison
    # column; see the --baseline guard in main().
    "claude-code-high-v1": {
        "report": "2026-10-18-terminal-bench-v1-high", "arm": "claude-code",
        "label": "Claude Code, v1 freeze, HIGH, 89x3",
    },
    "claude-code-max-v1": {
        "report": "2026-10-18-terminal-bench-v1-max", "arm": "claude-code",
        "label": "Claude Code, v1 freeze, MAX, hard 30",
    },
    "codex-high": {
        "report": "2026-10-25-terminal-bench-codex-high", "arm": "codex",
        "label": "Codex, HIGH, 89x1 (10-25)",
    },
}
# A cell whose arm has no published row of its own must not silently inherit
# the Theseus baseline: the block is headed "LIKE FOR LIKE" and would read as
# that arm's own prior run. These cells must say `--baseline <key>` to make a
# cross-arm comparison deliberate, or `--no-baseline` to publish without one.
ARMLESS = ("openhands", "aider", "openclaw")
DOCS = Path.home() / "lanes/gauntlet/docs/benchmarks"


def wilson(k, n, z=1.96):
    if not n:
        return (0.0, 0.0, 0.0)
    p = k / n
    d = 1 + z * z / n
    c = (p + z * z / (2 * n)) / d
    h = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n)) / d
    return (p, max(0.0, c - h), min(1.0, c + h))


def baseline(key):
    """The published arm's figures, with its OWN median and mean recomputed.

    `index.json`'s `usd_per_trial` and `agent_s_median` are single numbers, and
    `usd_per_trial` is a MEAN-like figure. Comparing this cell's *median* against
    it reads as a much smaller rise than the truth -- it told me the typical
    trial cost 1.63x when like-for-like it is 2.32x. So the baseline's own
    per-trial CSV is recomputed here and medians are compared with medians.
    """
    meta = BASELINES.get(key)
    if not meta:
        return None
    out = None
    idx = json.load(open(DOCS / "index.json"))
    for r in idx["runs"]:
        if r.get("report") == meta["report"] and r.get("arm") == meta["arm"]:
            out = {**meta, **r["metrics"], "trials": r.get("trials"), "tasks": r.get("tasks")}
            break
    if out is None:
        return None
    csv_path = DOCS / f'{meta["report"]}.csv'
    if csv_path.exists():
        with open(csv_path) as fh:
            rows = [r for r in csv.DictReader(fh) if r.get("arm", "").startswith(meta["arm"])]
        def col(name):
            return [float(r[name]) for r in rows
                    if r.get(name) not in (None, "", "None")]
        money, secs, calls = col("dollars"), col("agent_s"), col("model_calls")
        # The baseline's PASS RATE needs the same treatment as its cost. The
        # published 195/267 = 73.0% is RAW, and it carries 8 trials whose agent
        # never started (6 NonZeroAgentExitCodeError on `qemu-startup` and
        # `qemu-alpine-ssh`, 2 AgentSetupTimeoutError). Comparing this cell's
        # `agent_ran` block against that raw figure mixes bases and overstates
        # the gap by about two points -- the same family of error as comparing
        # a median against `usd_per_trial`. Recompute the baseline's own
        # agent-ran denominator so pass rates are compared on one rule.
        won = sum(1 for r in rows if (r.get("reward") or "").strip() in ("1", "1.0"))
        ran = [r for r in rows if (r.get("agent_s") or "").strip()]
        out["own"] = {
            "n": len(rows),
            "usd_median": round(st.median(money), 6) if money else None,
            "usd_mean": round(st.mean(money), 6) if money else None,
            "agent_s_median": round(st.median(secs), 2) if secs else None,
            "model_calls_median": st.median(calls) if calls else None,
            "pass_raw": [won, len(rows)],
            "pass_agent_ran": [won, len(ran)],
            "no_agent": len(rows) - len(ran),
        }
    return out


def trials(run_dir):
    marks = {r["trial"]: r["gates"] for r in gate_rows(run_dir)}
    out = []
    for rj in glob.glob(os.path.join(run_dir, "jobs", "*", "*__*", "result.json")):
        r = json.load(open(rj))
        d = os.path.dirname(rj)
        rw = os.path.join(d, "verifier", "reward.txt")
        eff = (((r.get("agent_result") or {}).get("metadata") or {}).get("efficiency") or {})
        ae = r.get("agent_execution") or {}
        out.append({
            "task": r.get("task_name"), "trial": r.get("trial_name"),
            "reward": 1 if os.path.exists(rw) and open(rw).read().strip().startswith("1") else 0,
            "exception": (r.get("exception_info") or {}).get("exception_type"),
            "effort": eff.get("effort"),
            "agent_s": round((parse(ae["finished_at"]) - parse(ae["started_at"])).total_seconds(), 2)
                       if ae.get("finished_at") and ae.get("started_at") else None,
            "dollars": eff.get("cost_usd"), "model_calls": eff.get("model_calls"),
            "tool_calls": eff.get("tool_calls"),
            "input": (eff.get("tokens") or {}).get("input"),
            "cache_read": (eff.get("tokens") or {}).get("cache_read"),
            "cache_write": (eff.get("tokens") or {}).get("cache_write"),
            "output": (eff.get("tokens") or {}).get("output"),
            "over_budget": eff.get("over_budget"),
            "build_commit": eff.get("build_commit"),
            "billed_models": ",".join(sorted((eff.get("by_model") or {}).keys())),
            # theseus-wf482's mark (bench-efficiency `mixed_model` / `fallback`): None on a record from before it.
            "mixed_model": eff.get("mixed_model"),
            "fallback": eff.get("fallback"),
            # theseus-tz23t, cell 8. `cn` is written only by the cn arm
            # (`efficiency.cn_record`); None everywhere else, which is "this is
            # not a cn trial", never "this cn trial was clean".
            "cn": eff.get("cn"),
            "gates": marks.get(r.get("trial_name"), ""),
            "started_mst": parse(r["started_at"]).astimezone(MST).strftime("%H:%M:%S")
                           if r.get("started_at") else None,
        })
    return out


def block(ts, label):
    done = [t for t in ts if t["agent_s"] is not None or t["exception"]]
    k = sum(t["reward"] for t in ts)
    p, lo, hi = wilson(k, len(ts))
    money = [t["dollars"] for t in ts if t["dollars"] is not None]
    secs = [t["agent_s"] for t in ts if t["agent_s"] is not None]
    calls = [t["model_calls"] for t in ts if t["model_calls"] is not None]
    return {
        "label": label, "trials": len(ts), "passed": k,
        "pass_rate": {"value": round(p, 6), "lo": round(lo, 6), "hi": round(hi, 6),
                      "k": k, "n": len(ts)},
        "usd_total": {"value": round(sum(money), 6)},
        "usd_per_trial": {"value": round(sum(money) / len(money), 6)} if money else None,
        "usd_per_solve": {"value": round(sum(money) / k, 6)} if k and money else None,
        "agent_s_median": {"value": round(st.median(secs), 2)} if secs else None,
        "model_calls_median": {"value": st.median(calls)} if calls else None,
        "exceptions": dict(Counter(t["exception"] for t in ts if t["exception"])),
        "billed_models": dict(Counter(t["billed_models"] for t in ts if t["billed_models"])),
        **cn_block(ts),
    }


def cn_block(ts):
    """theseus-tz23t. What the chokepoint did to a cn cell, or nothing at all
    when no trial carries a `cn` record (every non-cn cell).

    `mis_runs` is the number that matters: a trial whose `route.decided` rows
    carry `data_gate` had its cn model refused for what its session held, and
    the turn went to Claude -- so it measured the wrong model and is not a
    result. `unread` trials are the ones whose ledgers were missing, which is
    "unknown", never "clean". `jev_usd` is the Jev spend the cn arm cannot
    avoid (`tb.CN_EXTRA`), reported beside the model's dollars, never inside
    them."""
    cns = [t["cn"] for t in ts if t.get("cn")]
    if not cns:
        return {}
    reasons = Counter()
    for c in cns:
        reasons.update(c.get("reasons") or {})
    jev = [(c.get("jev") or {}).get("cost_usd") for c in cns]
    return {"cn": {
        "trials": len(cns),
        "mis_runs": sum(1 for c in cns if c.get("mis_run")),
        "off_model": sum(1 for c in cns if c.get("off_model")),
        "gated_turns": sum(c.get("gated") or 0 for c in cns),
        "reasons": dict(reasons),
        "unread": sum(1 for c in cns if not c.get("rows_read")),
        "jev_usd": round(sum(v for v in jev if isinstance(v, (int, float))), 6),
        "jev_calls": sum((c.get("jev") or {}).get("calls") or 0 for c in cns),
    }}


def main() -> int:
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    run_dir = sys.argv[1].rstrip("/")
    cell = os.path.basename(run_dir)
    if "--baseline" in sys.argv:
        bkey = sys.argv[sys.argv.index("--baseline") + 1]
        if bkey not in BASELINES:
            sys.exit(f"unknown --baseline {bkey!r}; have: {', '.join(sorted(BASELINES))}")
    elif "--no-baseline" in sys.argv:
        bkey = None
    elif any(a in cell for a in ARMLESS):
        sys.exit(
            f"{cell}: this arm has no published terminal-bench@2.0 row of its own, and the\n"
            f"default baseline is Theseus's -- a 'LIKE FOR LIKE' block against it would read as\n"
            f"this arm's own prior run. Say what you mean:\n"
            f"  --no-baseline              publish it as a first measurement (usual for cells 2-4)\n"
            f"  --baseline theseus-high-v1 a deliberate cross-arm comparison\n"
            f"  (keys: {', '.join(sorted(BASELINES))})")
    else:
        bkey = "theseus-high-v1"
    ts = trials(run_dir)
    if not ts:
        sys.exit("no finished trials yet")

    # --tasks <file>: restrict every block to a task list, one name per line.
    # The cross-arm matrix needs this. Two tasks (`qemu-startup`,
    # `qemu-alpine-ssh`) fail their apt install for any arm that installs
    # nodejs through apt -- Theseus, Claude Code and Codex do; OpenHands, Aider
    # and OpenClaw do not -- so an all-89 comparison charges Theseus two tasks
    # the other arms never attempt. Restricting every arm to the tasks they all
    # attempted is the only operation that compares like with like. Publish it
    # BESIDE the all-89 figures, never instead of them.
    if "--tasks" in sys.argv:
        want = {ln.strip() for ln in open(sys.argv[sys.argv.index("--tasks") + 1])
                if ln.strip() and not ln.startswith("#")}
        missing = want - {t["task"] for t in ts}
        ts = [t for t in ts if t["task"] in want]
        if not ts:
            sys.exit("--tasks selected no trials; check the task names")
        print(f"--tasks: {len(want)} task(s) requested, "
              f"{len({t['task'] for t in ts})} present, {len(ts)} trials kept"
              + (f"; NOT IN THIS RUN: {', '.join(sorted(missing))}" if missing else ""))

    replaced = []
    if "--swap" in sys.argv:
        regate = sys.argv[sys.argv.index("--swap") + 1].rstrip("/")
        spares = defaultdict(list)
        for t in trials(regate):
            if t["gates"]:
                sys.exit(f"{regate}: a regate trial is itself inside a gate window "
                         f"({t['trial']}); re-run it on a quiet host before swapping")
            spares[t["task"]].append(t)
        need = Counter(t["task"] for t in ts if t["gates"])
        short = {k: (v, len(spares.get(k, []))) for k, v in need.items()
                 if len(spares.get(k, [])) < v}
        if short:
            print("the regate cell is short of replicates; refusing to swap:")
            for task, (want, got) in sorted(short.items()):
                print(f"  {task}: need {want}, have {got}")
            sys.exit("re-run the regate cell before publishing a swapped dataset")
        kept = []
        for t in ts:
            if t["gates"]:
                sub = spares[t["task"]].pop(0)
                sub["replaces"] = t["trial"]
                replaced.append({"out": t, "in": sub["trial"]})
                kept.append(sub)
            else:
                kept.append(t)
        ts = kept
        print(f"swapped {len(replaced)} contaminated replicates for reruns from "
              f"{os.path.basename(regate)}\n")

    per_task = Counter(t["task"] for t in ts)
    ks = Counter(per_task.values())
    gated = [t for t in ts if t["gates"]]
    clean = [t for t in ts if not t["gates"]]
    # A trial whose agent never started is not evidence about the arm. Harbor
    # records it with reward 0 like any other failure, which silently charges
    # the arm for the container's `apt-get install` -- two of cell 1's first 66
    # died that way (exit 100 from apt, 27 s and 62 s, no agent metadata at
    # all), on top of the two setup TIMEOUTS that `--agent-setup-timeout-
    # multiplier 3` already fixed. Different fault, same consequence. The
    # signal is an exception with no agent execution recorded.
    setup_failed = [t for t in ts if t["exception"] and t["agent_s"] is None]
    ran = [t for t in ts if t not in setup_failed]

    data = {
        "report": f"2026-10-26-{cell}", "suite": "terminal-bench@2.0", "date": "2026-10-26",
        "cell": cell, "commit": sorted({t["build_commit"] for t in ts if t["build_commit"]}),
        "tasks": len(per_task), "trials": len(ts),
        "k_per_task": {str(k): v for k, v in sorted(ks.items())},
        "task_set": "full" if len(per_task) >= 89 else f"subset:{len(per_task)}",
        "summary": {"all": block(ts, "every trial"),
                    "clean": block(clean, "trials on a quiet host"),
                    "gate_contaminated": block(gated, "trials inside a WSL join-gate window"),
                    "agent_ran": block(ran, "trials whose agent actually started"),
                    "setup_failed": block(setup_failed, "trials whose agent never started")},
        "baseline": baseline(bkey),
        # Kept, never dropped: which contaminated replicate each rerun stands in
        # for, so the swap is auditable from the dataset alone.
        "replaced": [{"contaminated": r["out"]["trial"], "task": r["out"]["task"],
                      "contaminated_reward": r["out"]["reward"],
                      "contaminated_usd": r["out"]["dollars"],
                      "rerun": r["in"]} for r in replaced],
    }
    # A --tasks run is a SUBSET and must never land on the published dataset's
    # name: the cell's own `<cell>-dataset.json` is what gets committed, and a
    # peek at the 87-task column would otherwise silently replace it with a
    # file of the same name holding fewer trials. Separate name, by default.
    suffix = "-dataset-subset.json" if "--tasks" in sys.argv else "-dataset.json"
    out = Path(sys.argv[sys.argv.index("--json") + 1]) if "--json" in sys.argv \
        else Path(run_dir) / f"{cell}{suffix}"
    out.write_text(json.dumps(data, indent=1) + "\n")
    print(f"wrote {out}")

    if "--csv" in sys.argv:
        cpath = Path(run_dir) / (f"{cell}-trials-subset.csv" if "--tasks" in sys.argv
                                 else f"{cell}-trials.csv")
        cols = ["task", "trial", "reward", "exception", "effort", "agent_s", "dollars",
                "model_calls", "tool_calls", "input", "cache_read", "cache_write", "output",
                "over_budget", "billed_models", "gates", "started_mst"]
        with open(cpath, "w", newline="") as fh:
            w = csv.DictWriter(fh, fieldnames=cols, extrasaction="ignore")
            w.writeheader(); w.writerows(sorted(ts, key=lambda t: (t["task"], t["trial"])))
        print(f"wrote {cpath}")

    print(f"\n{cell}: {len(ts)} trials over {len(per_task)} tasks; k per task {dict(ks)}")
    for key in ("all", "agent_ran", "clean", "gate_contaminated", "setup_failed"):
        s = data["summary"][key]
        if not s["trials"]:
            continue
        pr = s["pass_rate"]
        print(f"  {s['label']:42s} {pr['k']:3d}/{pr['n']:3d} = {100*pr['value']:5.1f}% "
              f"[{100*pr['lo']:.1f}, {100*pr['hi']:.1f}]  "
              f"${s['usd_total']['value']:.2f}  "
              f"agent_s p50 {s['agent_s_median']['value'] if s['agent_s_median'] else '-'}  "
              f"calls p50 {s['model_calls_median']['value'] if s['model_calls_median'] else '-'}")
    b = data["baseline"]
    if b:
        pr = b.get("pass_rate", {})
        print(f"\n  baseline: {b['label']}")
        print(f"    {pr.get('k')}/{pr.get('n')} = {100*pr.get('value', 0):5.1f}% "
              f"[{100*pr.get('lo', 0):.1f}, {100*pr.get('hi', 0):.1f}]  "
              f"${b.get('usd_total', {}).get('value', 0):.2f}  "
              f"$/trial {b.get('usd_per_trial', {}).get('value', 0):.4f}  "
              f"agent_s p50 {b.get('agent_s_median', {}).get('value')}")
        group = clean if clean else ts
        own = b.get("own") or {}
        money = [t_["dollars"] for t_ in group if t_["dollars"] is not None]
        secs = [t_["agent_s"] for t_ in group if t_["agent_s"] is not None]
        if own.get("pass_agent_ran"):
            # Pass rate on ONE rule for both sides. The baseline's published
            # 73.0% is raw; its agent-ran rate is higher, and the cell's
            # agent-ran block must be read against that, not against the raw
            # figure. Both pairings are printed so neither can be quoted alone.
            ar = data["summary"]["agent_ran"]["pass_rate"]
            al = data["summary"]["all"]["pass_rate"]
            bk, bn = own["pass_agent_ran"]
            rk, rn = own["pass_raw"]
            print(f"\n    LIKE FOR LIKE (pass rate, one denominator rule on both sides):")
            print(f"      agent-ran  {100*ar['value']:.1f}% ({ar['k']}/{ar['n']}) vs "
                  f"{100*bk/bn:.1f}% ({bk}/{bn})  [baseline drops {own['no_agent']} no-agent trials]")
            print(f"      raw        {100*al['value']:.1f}% ({al['k']}/{al['n']}) vs "
                  f"{100*rk/rn:.1f}% ({rk}/{rn})")
        if money and own.get("usd_median"):
            print(f"\n    LIKE FOR LIKE (clean trials against the baseline's own recomputed figures):")
            print(f"      $/trial median {st.median(money):.4f} vs {own['usd_median']:.4f} "
                  f"= {st.median(money)/own['usd_median']:.2f}x")
            print(f"      $/trial mean   {st.mean(money):.4f} vs {own['usd_mean']:.4f} "
                  f"= {st.mean(money)/own['usd_mean']:.2f}x")
        if secs and own.get("agent_s_median"):
            print(f"      agent_s median {st.median(secs):.1f} vs {own['agent_s_median']:.1f} "
                  f"= {st.median(secs)/own['agent_s_median']:.2f}x")
    # theseus-wf482 is the catalog's refusal fallback from claude-sonnet-5-5 to
    # claude-sonnet-5, which once it fires carries the whole turn. Its signature
    # is `claude-sonnet-5` appearing in the bill -- NOT the absence of
    # `claude-sonnet-5-5`. Those are different things and conflating them
    # reports correct behaviour as a defect: on the routed arm Jev picks
    # `haikuhi` for routine coding, so a trial billed only claude-haiku-5-5 is
    # the arm working as designed, not a fallback.
    if setup_failed:
        print(f"\n  {len(setup_failed)} trial(s) never started an agent and are charged to the arm as\n"
              f"  reward 0 by Harbor; report them separately:")
        for t_ in setup_failed:
            print(f"    {t_['trial']:42s} {t_['exception']}  started {t_['started_mst']}  "
                  f"{t_['gates'] or 'clean'}")

    def models(t):
        return set(filter(None, t["billed_models"].split(",")))
    # A record that carries the mark says whether the fallback answered; an old one is inferred from the bill.
    def fell(t):
        return t["mixed_model"] if t["mixed_model"] is not None else "claude-sonnet-5" in models(t)
    fell_back = [t for t in ts if fell(t)]
    whole = [t for t in fell_back if "claude-sonnet-5-5" not in models(t)]
    routed_elsewhere = [t for t in ts if models(t) and "claude-sonnet-5-5" not in models(t)
                        and "claude-sonnet-5" not in models(t)]
    n = len(ts)
    marked = sum(1 for t in ts if t["mixed_model"] is not None)
    print(f"\n  theseus-wf482 watch: {len(fell_back)}/{n} trials ({100*len(fell_back)/n:.0f}%) "
          f"billed claude-sonnet-5, {len(whole)} of them for the whole trial "
          f"({marked} records carry the mark, {n - marked} inferred from the bill)")
    print(f"  routing (not a fallback): {len(routed_elsewhere)} trials billed neither sonnet "
          f"model -- Jev picked another profile, which is the arm working as designed")

    # theseus-tz23t: a cn cell's own watch. Printed only for a cn cell.
    cn = cn_block(ts).get("cn")
    if cn:
        print(f"\n  theseus-tz23t cn watch: {cn['mis_runs']}/{cn['trials']} trials are MIS-RUNS "
              f"(a `route.decided` row carried `data_gate`, so the chokepoint refused the cn "
              f"model and the turn went to Claude){'' if not cn['reasons'] else ': ' + ', '.join(f'{k} x{v}' for k, v in sorted(cn['reasons'].items()))}")
        print(f"  {cn['off_model']} trial(s) ran a turn on a model other than the arm's; "
              f"{cn['gated_turns']} turn(s) gated in all; {cn['unread']} trial(s) had no ledger "
              f"to read (UNKNOWN, not clean)")
        print(f"  Jev (the cn arm cannot be judge-free): {cn['jev_calls']} calls, "
              f"${cn['jev_usd']:.4f}, reported beside the model's dollars and not inside them")
        if cn["mis_runs"] or cn["unread"]:
            print("  A mis-run is not a result (theseus-9ymz0's owner). Exclude them from the "
                  "cell's pass rate, or rerun them, before publishing anything.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
