#!/usr/bin/env python3
"""The personality exam (PX, theseus-slvl.1): probe families with deterministic checks, a rubric judge, Claude Code
bare and with the same persona in its CLAUDE.md, distractor history, forced compactions, and a report.

    exam.py plan    --seed 7 --size smoke [--persona p.toml] [--profiles a=model,b=model]
    exam.py standin --out /tmp/px-standin                    the whole exam on a stand-in model, $0, oracle and wrong
    exam.py smoke   --arms theseus,claude-code-fair --out /tmp/px-smoke     ONE probe per arm, a real model
    exam.py run     --arms theseus,theseus-persona,claude-code,claude-code-fair --out /tmp/px --budget 10
    exam.py judge   --run /tmp/px                            the rubric judge on the character-bearing replies
    exam.py blind   --run /tmp/px [--answers FILE]           the 20-pair sheet, then its tally
    exam.py score   --run /tmp/px                            scores.json
    exam.py report  --run /tmp/px --date 2026-10-14 --slug first-run [--out DIR]

A run directory holds `plan.json`, `persona.json` (the fields and digest the arms were given), and one folder per
arm (`run.json`, `results.jsonl`, `raw/`, `judged.json` after `judge`). `README.md` is the guide.

Standard library only.
"""

from __future__ import annotations

import argparse
import json
import shutil
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent / "recall"))
sys.path.insert(0, str(HERE.parent / "report"))
sys.path.insert(0, str(HERE))

import blind as bl  # noqa: E402
import exam_drive as ed  # noqa: E402
import exam_score as es  # noqa: E402
import exam_standin as xs  # noqa: E402
import judge as jd  # noqa: E402
import persona as pe  # noqa: E402
import plan as pl  # noqa: E402
import report as rp  # noqa: E402
import xchecks as xc  # noqa: E402

DEFAULT_BUDGET = 10.0
JUDGE_BUDGET = 1.5


def add_common(ap: argparse.ArgumentParser) -> None:
    ap.add_argument("--persona", type=Path, default=None, help="a persona TOML (persona.example.toml); default: built in")
    ap.add_argument("--seed", type=int, default=7)
    ap.add_argument("--size", choices=tuple(pl.SIZES), default="smoke")
    ap.add_argument("--window", type=int, default=pl.DEFAULT_WINDOW,
                    help="the scratch context window the fills are sized to cross (forces the compactions)")
    ap.add_argument("--families", default="", help="comma list of families to run (default all)")
    ap.add_argument("--profiles", default="", help="routed profiles for the model-switch probes: name=model,name=model")
    ap.add_argument("--others", default="", help="names of other personas (the identity probe that asks if it is one)")


def add_run(ap: argparse.ArgumentParser) -> None:
    add_common(ap)
    ap.add_argument("--arms", default=",".join(ed.ARMS))
    ap.add_argument("--out", type=Path, required=True)
    ap.add_argument("--model", default="anthropic/claude-sonnet-5-5")
    ap.add_argument("--effort", default="medium", choices=("low", "medium", "high", "xhigh", "max"))
    ap.add_argument("--budget", type=float, default=DEFAULT_BUDGET, help="dollars, all arms together; a run stops at it")
    ap.add_argument("--turn-timeout", type=float, default=900.0)
    ap.add_argument("--bin-dir", default="target/release")
    ap.add_argument("--profile", default=str(HERE.parent / "theseus-bench.toml"), help="the base daemon config")
    ap.add_argument("--memory-arm", default="baseline")
    ap.add_argument("--max-loops", type=int, default=40)
    ap.add_argument("--spend-limit", type=float, default=50.0)
    ap.add_argument("--day-ceiling", type=float, default=None)
    ap.add_argument("--api-base", default=None, help="a stand-in model's address")
    ap.add_argument("--claude", default="claude")
    ap.add_argument("--claude-version", default=None, help="held to: the run refuses another claude")
    ap.add_argument("--claude-arg", action="append", default=[])
    ap.add_argument("--allow-overhead", action="store_true")


def load_persona(a) -> pe.Persona:
    return pe.load(a.persona, [x for x in a.others.split(",") if x])


def make_plan(a, per: pe.Persona) -> pl.Plan:
    fams = {x for x in a.families.split(",") if x} or None
    return pl.build(per, a.seed, a.size, a.window, profiles=list(ed.parse_profiles(a.profiles)), families=fams)


def run_arms(a, plan: pl.Plan, per: pe.Persona, arms: list[str], out: Path) -> dict:
    out.mkdir(parents=True, exist_ok=True)
    (out / "plan.json").write_text(json.dumps(plan.to_json(), indent=1, sort_keys=True) + "\n")
    (out / "persona.json").write_text(json.dumps({**per.fields(), "digest": per.digest(), "source": per.source,
                                                  "persona_text": per.persona_text(), "card_text": per.card_text()},
                                                 indent=1, sort_keys=True) + "\n")
    a.profile_map = ed.parse_profiles(a.profiles)
    total = 0.0
    done = {}
    for name in arms:
        if name not in ed.ARM_CLASSES:
            raise SystemExit(f"exam: unknown arm {name!r} (one of {', '.join(ed.ARMS)})")
        left = a.budget - total
        if left <= 0:
            print(f"exam: the budget (${a.budget:.2f}) is spent: {name} not run", file=sys.stderr)
            continue
        # an even share of what is left, so the first arm cannot spend what the last needs
        share = left / (len(arms) - len(done))
        a_arm = argparse.Namespace(**{**vars(a), "budget": share})
        run = ed.RunDir(out / name, plan.window)
        arm = ed.ARM_CLASSES[name](a_arm, run, plan, per)
        t0 = time.monotonic()
        arm.run_plan()
        total += arm.spent
        done[name] = {"spent_usd": round(arm.spent, 6), "seconds": round(time.monotonic() - t0, 1)}
        print(f"exam: {name}: {run.meta.get('steps_done', 0)}/{len(plan.steps)} steps, ${arm.spent:.4f}, "
              f"compactions at {run.meta['compactions'] or 'none'}"
              f"{', stopped by the budget' if run.meta.get('stopped_by_budget') else ''}")
    return done


def cmd_plan(a) -> int:
    per = load_persona(a)
    plan = make_plan(a, per)
    print(pl.summary(plan))
    if a.estimate:
        e = pl.estimate_cost(plan, arms=a.estimate)
        print(f"estimate: ${e['per_arm_usd']:.2f} an arm on {e['model']} (about {e['compactions']} compactions in "
              f"{e['steps']} steps), ${e['arms_usd']:.2f} for {a.estimate} arms, before the judge (~$0.40)")
    return 0


def run_namespace(extra: list[str]) -> argparse.Namespace:
    ap = argparse.ArgumentParser()
    add_run(ap)
    return ap.parse_args(extra)


def cmd_run(a) -> int:
    per = load_persona(a)
    plan = make_plan(a, per)
    print(pl.summary(plan), file=sys.stderr)
    run_arms(a, plan, per, [x for x in a.arms.split(",") if x], a.out)
    return 0


def smoke_ids(plan: pl.Plan) -> set[str]:
    """One probe, the cheapest informative one: an identity probe with no ledger check, else the first probe."""
    for s in plan.probes():
        if s.family == "identity" and s.surface == "cli" and not s.profile and not xc.uses_ledger(s.checks):
            return {s.probe}
    return {plan.probes()[0].probe}


def cmd_smoke(a) -> int:
    a.size, a.families = "smoke", ""
    per = load_persona(a)
    full = make_plan(a, per)
    plan = full.only(smoke_ids(full))
    plan.window = a.window
    print(f"exam smoke: {len(plan.steps)} step(s), probe {plan.probes()[0].probe}", file=sys.stderr)
    run_arms(a, plan, per, [x for x in a.arms.split(",") if x], a.out)
    return 0


# ---- the judge, the blind sheet, the scores


def arm_dirs(run: Path) -> list[str]:
    return [x for x in ed.ARMS if (run / x / "results.jsonl").is_file()]


def load_plan(run: Path) -> dict:
    return json.loads((run / "plan.json").read_text())


def cmd_judge(a) -> int:
    run = a.run
    plan = load_plan(run)
    per = json.loads((run / "persona.json").read_text())
    j = jd.Judge(a.judge_model, a.judge_base, call=None)
    steps = {s["index"]: s for s in plan["steps"]}
    for arm in arm_dirs(run):
        res = ed.results_of(run / arm / "results.jsonl")
        meta = json.loads((run / arm / "run.json").read_text())
        given = bool(meta.get("has_persona"))
        out = {}
        for i, s in steps.items():
            r = res.get(i)
            if s["kind"] != "probe" or not s.get("judge") or not r or r.get("skipped") or not (r.get("reply") or "").strip():
                continue
            if j.spent > a.judge_budget:
                print(f"exam: the judge's budget (${a.judge_budget:.2f}) is spent", file=sys.stderr)
                break
            hist = [x["text"] for x in plan["steps"] if x["probe"] == s["probe"] and x["kind"] == "setup"]
            out[str(i)] = j.score(per["persona_text"], per["card_text"], hist, s["text"], r["reply"], given)
        (run / arm / "judged.json").write_text(json.dumps(out, indent=1, sort_keys=True) + "\n")
        print(f"exam: judged {len(out)} replies of {arm}")
    (run / "judge.json").write_text(json.dumps({"model": j.model, "calls": j.calls, "spent_usd": round(j.spent, 6)}) + "\n")
    return 0


def cmd_blind(a) -> int:
    run = a.run
    plan = load_plan(run)
    d = run / "blind"
    d.mkdir(exist_ok=True)
    if a.answers:
        key = json.loads((d / "key.json").read_text())
        t = bl.tally(key, bl.parse_answers(a.answers.read_text(), key))
        (d / "tally.json").write_text(json.dumps(t, indent=1) + "\n")
        print(json.dumps(t))
        return 0
    for arm in (bl.A_ARM, bl.B_ARM):
        if not (run / arm / "results.jsonl").is_file():
            raise SystemExit(f"exam: the blind sheet needs the {arm} arm's results")
    sheet, key = bl.build(plan, ed.results_of(run / bl.A_ARM / "results.jsonl"),
                          ed.results_of(run / bl.B_ARM / "results.jsonl"), a.pairs, a.seed)
    (d / "sheet.md").write_text(sheet)
    (d / "key.json").write_text(json.dumps(key, indent=1, sort_keys=True) + "\n")
    print(f"exam: {len(key)} pairs in {d / 'sheet.md'}; answer in {d / 'answers.txt'} (one `07 a` a line)")
    return 0


def score_run(run: Path) -> dict:
    plan = load_plan(run)
    per = json.loads((run / "persona.json").read_text())
    pplan = pl.Plan(seed=plan["seed"], size=plan["size"], window=plan["window"], overhead=plan["overhead"],
                    persona=plan["persona"], persona_digest=plan["persona_digest"], profiles=plan["profiles"],
                    sessions=[pl.Session(**s) for s in plan["sessions"]], steps=[pl.Step(**s) for s in plan["steps"]])
    arms = {}
    for arm in arm_dirs(run):
        meta = json.loads((run / arm / "run.json").read_text())
        jf = run / arm / "judged.json"
        judged = {int(k): v for k, v in json.loads(jf.read_text()).items()} if jf.is_file() else {}
        cell = es.score_cell(pplan, ed.results_of(run / arm / "results.jsonl"), per["tz_names"],
                             meta.get("compactions", []), judged)
        arms[arm] = {"cell": cell, "run": {k: v for k, v in meta.items() if k not in ("compaction_rows",)}}
    first = next(iter(arms.values()))["run"] if arms else {}
    out = {"plan": {"digest": pplan.digest(), "seed": plan["seed"], "size": plan["size"], "window": plan["window"],
                    "steps": len(plan["steps"]), "probes": len(pplan.probes())},
           "run": {"model": first.get("model"), "effort": first.get("effort"), "persona": per,
                   "persona_digest": per["digest"], "persona_source": per.get("source"),
                   "budget_usd": first.get("budget_usd"), "persona_name": per["name"]},
           "arms": arms}
    out["run"]["persona"] = {k: per[k] for k in ("name", "owner_call", "tz")}
    jj = run / "judge.json"
    if jj.is_file():
        out["judge"] = json.loads(jj.read_text())
        out["run"]["judge_model"] = out["judge"]["model"]
    tl = run / "blind" / "tally.json"
    out["blind"] = json.loads(tl.read_text()) if tl.is_file() else None
    return out


def cmd_score(a) -> int:
    sc = score_run(a.run)
    (a.run / "scores.json").write_text(json.dumps(sc, indent=1, sort_keys=True) + "\n")
    for arm, v in sc["arms"].items():
        o = v["cell"]["overall"]
        print(f"exam: {arm}: {o['k']}/{o['n']} probes pass ({o['unscored']} unscored, {o['unreached']} unreached)")
    return 0


def cmd_report(a) -> int:
    path = a.run / "scores.json"
    if not path.is_file():
        cmd_score(a)
    for p in rp.make(json.loads(path.read_text()), a.date, a.slug, a.out, a.force):
        print(p)
    return 0


# ---- the stand-in run


def cmd_standin(a) -> int:
    """The whole exam end to end on the stand-in model, twice: once answering every probe with its oracle answer,
    once with its planted wrong one. Prints each arm's score; exits 1 if the oracle does not score 100% on the
    persona arm's reply checks or the wrong answers do not score 0 on them."""
    per = load_persona(a)
    plan = make_plan(a, per)
    print(pl.summary(plan), file=sys.stderr)
    bad = 0
    for mode in ("oracle", "wrong"):
        si = xs.ExamStandIn(xs.rules_for(plan.steps, mode), {s.oracle for s in plan.steps if s.oracle})
        a.api_base = si.base
        a.budget = 1e9
        sub = a.out / mode
        try:
            run_arms(a, plan, per, [x for x in a.arms.split(",") if x], sub)
            jargs = argparse.Namespace(run=sub, judge_model=jd.DEFAULT_MODEL, judge_base=si.base, judge_budget=JUDGE_BUDGET)
            cmd_judge(jargs)
        finally:
            si.close()
        sc = score_run(sub)
        (sub / "scores.json").write_text(json.dumps(sc, indent=1, sort_keys=True) + "\n")
        for arm, v in sc["arms"].items():
            reply_only = [p for p in v["cell"]["probes"] if p["ok"] is not None and
                          not any(ln[0].startswith("ledger") for ln in p["lines"])]
            k = sum(1 for p in reply_only if p["ok"])
            print(f"exam standin {mode}: {arm}: {k}/{len(reply_only)} reply-checked probes pass; overall "
                  f"{v['cell']['overall']['k']}/{v['cell']['overall']['n']}")
            want = len(reply_only) if mode == "oracle" else 0
            if k != want:
                bad += 1
                print(f"exam standin {mode}: {arm}: WANTED {want}", file=sys.stderr)
    if {"theseus-persona", "claude-code-fair"} <= set(arm_dirs(a.out / "oracle")):
        bad += standin_blind(a, plan)
    return 1 if bad else 0


def standin_blind(a, plan: pl.Plan) -> int:
    """The blind sheet on replies that differ: Theseus + persona's oracle answers against the fair arm's wrong ones,
    then an owner who always prefers the oracle: the tally must credit Theseus with every pair."""
    d = a.out / "blind"
    left = ed.results_of(a.out / "oracle" / bl.A_ARM / "results.jsonl")
    right = ed.results_of(a.out / "wrong" / bl.B_ARM / "results.jsonl")
    sheet, key = bl.build(load_plan(a.out / "oracle"), left, right, 20, a.seed)
    d.mkdir(exist_ok=True)
    (d / "sheet.md").write_text(sheet)
    (d / "key.json").write_text(json.dumps(key, indent=1, sort_keys=True) + "\n")
    ans = "".join(f"{k} {'a' if v['a'] == bl.A_ARM else 'b'}\n" for k, v in sorted(key.items()))
    (d / "answers.txt").write_text(ans)
    t = bl.tally(key, bl.parse_answers(ans, key))
    (d / "tally.json").write_text(json.dumps(t, indent=1) + "\n")
    print(f"exam standin blind: {t['pairs']} pairs, Theseus + persona picked {t['wins'][bl.A_ARM]} of {t['decided']}")
    return 0 if t["pairs"] >= 10 and t["wins"][bl.A_ARM] == t["decided"] == t["pairs"] else 1


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    sub = ap.add_subparsers(dest="cmd", required=True)
    p = sub.add_parser("plan")
    add_common(p)
    p.add_argument("--estimate", type=int, default=0, metavar="ARMS", help="a model of the dollars for this many arms")
    p = sub.add_parser("run")
    add_run(p)
    p = sub.add_parser("smoke")
    add_run(p)
    p = sub.add_parser("standin")
    add_run(p)
    for name in ("judge", "blind", "score", "report"):
        p = sub.add_parser(name)
        p.add_argument("--run", type=Path, required=True)
        if name == "judge":
            p.add_argument("--judge-model", default=jd.DEFAULT_MODEL)
            p.add_argument("--judge-base", default=jd.API)
            p.add_argument("--judge-budget", type=float, default=JUDGE_BUDGET)
        if name == "blind":
            p.add_argument("--answers", type=Path, default=None)
            p.add_argument("--pairs", type=int, default=20)
            p.add_argument("--seed", type=int, default=7)
        if name == "report":
            p.add_argument("--date", required=True)
            p.add_argument("--slug", default="")
            p.add_argument("--out", type=Path, default=rp.draft.DOCS)
            p.add_argument("--force", action="store_true")
    a = ap.parse_args(argv)
    return {"plan": cmd_plan, "run": cmd_run, "smoke": cmd_smoke, "standin": cmd_standin, "judge": cmd_judge,
            "blind": cmd_blind, "score": cmd_score, "report": cmd_report}[a.cmd](a)


if __name__ == "__main__":
    sys.exit(main())
