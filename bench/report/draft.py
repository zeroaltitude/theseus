"""The benchmark report's drafting tool (theseus-qla4): a run's outputs in, its report's tables, plots and skeleton out.

Every benchmark run ends in a report in `docs/benchmarks/` (bench/README.md, "Every run gets its report"). This tool
does the mechanical half: it reads what the run left, computes the numbers with their uncertainty (`stats.py`), and
writes, beside each other,

- `<report>.json`: the data file, the numbers the tables use and every figure's spec (`charts.py`);
- `<report>.csv`: one row per trial (or gate run, or probe), no transcripts;
- `img/<report>/*.svg`: the figures, each in a light and a dark file;
- `<report>.md`: the skeleton, every section of a report in order with its tables filled and its figures placed,
  and an italic prompt where the narrative goes. An existing `.md` is never overwritten (`--force` does).

The narrative (the answer, the question, the analysis, the threats) is the run's lane's to write. The report is named
`<YYYY-MM-DD>-<suite>[-<slug>]`, the date the run happened.

    # A Harbor run (Terminal-Bench, SWE-bench, the async families): one --arm per arm, a registry key from
    # charts.ARMS and its jobs (a job directory, a directory of jobs, or a quoted glob; commas for several).
    python3 bench/report/draft.py harbor --suite terminal-bench@2.0 --date 2026-10-04 --slug first-full-run \\
        --arm theseus=jobs/theseus-tb2 --arm claude-code=jobs/claude-tb2 --model anthropic/claude-sonnet-5-5

    # The gate's speed benches over a span of the bench history ($THESEUS_BENCH_HISTORY).
    python3 bench/report/draft.py history --since 2026-10-01 --branch main --date 2026-10-06

    # The recall bench, from score.py's output; the async bench, from its jobs through score.py's own reading.
    python3 bench/report/draft.py recall --scores /tmp/rc-report/scores.json --date 2026-10-07
    python3 bench/report/draft.py async --date 2026-10-07 jobs/async-theseus jobs/async-claude

    # Any report's figures again, from its data file (after editing a spec, or the renderer); the palette's figure.
    python3 bench/report/draft.py plot docs/benchmarks/<report>.json
    python3 bench/report/draft.py palette

Standard library only.
"""

from __future__ import annotations

import argparse
import csv
import glob
import importlib.util
import io
import json
import math
import os
import re
import sys
from collections import Counter, defaultdict
from datetime import datetime
from pathlib import Path
from typing import Any

HERE = Path(__file__).resolve().parent
BENCH = HERE.parent
DOCS = BENCH.parent / "docs" / "benchmarks"


def _load(name: str, path: Path):
    if name in sys.modules:
        return sys.modules[name]
    spec = importlib.util.spec_from_file_location(name, path)
    mod = importlib.util.module_from_spec(spec)
    sys.modules[name] = mod
    spec.loader.exec_module(mod)
    return mod


charts = _load("bench_report_charts", HERE / "charts.py")
stats = _load("stats_shim", HERE / "stats.py")


# ------------------------------------------------------------------ common

def report_name(date: str, suite: str, slug: str | None = None) -> str:
    if not re.fullmatch(r"\d{4}-\d{2}-\d{2}", date):
        raise ValueError(f"--date {date!r}: want YYYY-MM-DD, the day the run happened")
    short = re.sub(r"[^a-z0-9]+", "-", suite.lower().split("@")[0]).strip("-")
    name = f"{date}-{short}"
    if slug:
        name += "-" + re.sub(r"[^a-z0-9]+", "-", slug.lower()).strip("-")
    return name


ALIASES = {"claude": "claude-code", "claude_code": "claude-code", "claudecode": "claude-code"}


def arm_key(name: str) -> str:
    """A registry key for a name a run's files use (Harbor's agent name, the recall driver's arm label)."""
    n = (name or "").lower()
    if n in charts.ARMS or n in charts.NEUTRALS:
        return n
    if n in ALIASES:
        return ALIASES[n]
    for key in ("theseus-batching", "claude-code", "openclaw", "theseus"):
        if key in n or key.replace("-", "_") in n or key.replace("-", "") in n:
            return key
    if "claude" in n:
        return "claude-code"
    if "oracle" in n:
        return "oracle"
    return "other"


def label_of(key: str, labels: dict[str, str]) -> str:
    if key in labels:
        return labels[key]
    return charts.ARMS[key][1] if key in charts.ARMS else charts.NEUTRALS.get(key, ("", key))[1]


def interval(xs: list[float], n_small: int = 10) -> dict[str, Any]:
    """A mean with its 95% interval: a seeded bootstrap, or the range under `n_small` values."""
    xs = [float(x) for x in xs if x is not None]
    if not xs:
        return {"n": 0, "mean": None, "lo": None, "hi": None, "median": None, "method": "none"}
    if len(xs) < n_small:
        return {"n": len(xs), "mean": sum(xs) / len(xs), "lo": min(xs), "hi": max(xs), "median": stats.median(xs),
                "method": "range"}
    m, lo, hi = stats.bootstrap_mean(xs)
    return {"n": len(xs), "mean": m, "lo": lo, "hi": hi, "median": stats.median(xs), "method": "bootstrap"}


def rate(k: int, n: int) -> dict[str, Any]:
    p, lo, hi = stats.wilson(k, n)
    return {"k": k, "n": n, "rate": None if n == 0 else p, "lo": lo, "hi": hi}


def _r(v: Any, places: int = 4) -> Any:
    if isinstance(v, float):
        return None if math.isnan(v) else round(v, places)
    if isinstance(v, dict):
        return {k: _r(x, places) for k, x in v.items()}
    if isinstance(v, list):
        return [_r(x, places) for x in v]
    return v


def pct(x: float | None) -> str:
    return "–" if x is None else f"{x * 100:.1f}%"


def usd(x: float | None) -> str:
    return charts.fmt(x, "usd") if x is not None else "–"


def ci(d: dict[str, Any], f) -> str:
    if d.get("mean") is None:
        return "–"
    word = "range" if d["method"] == "range" else "95% CI"
    return f'{f(d["mean"])} ({word} {f(d["lo"])} to {f(d["hi"])}; n {d["n"]})'


def write_outputs(out: Path, name: str, data: dict[str, Any], rows: list[dict[str, Any]], skeleton: str,
                  force: bool = False) -> list[Path]:
    """The data file, the CSV, the figures, and the skeleton unless a report already stands there."""
    out.mkdir(parents=True, exist_ok=True)
    written = []
    jp = out / f"{name}.json"
    jp.write_text(json.dumps(_r(data), indent=1, ensure_ascii=False) + "\n")
    written.append(jp)
    if rows:
        cp = out / f"{name}.csv"
        cols = list(rows[0].keys())
        buf = io.StringIO()
        w = csv.DictWriter(buf, fieldnames=cols, extrasaction="ignore", lineterminator="\n")
        w.writeheader()
        for r in rows:
            w.writerow({k: _r(v) for k, v in r.items()})
        cp.write_text(buf.getvalue())
        written.append(cp)
    for spec in data.get("figures") or []:
        written += charts.write(spec, out / "img" / name)
    mp = out / f"{name}.md"
    if mp.exists() and not force:
        print(f"kept {mp}: a report stands there (--force rewrites it)", file=sys.stderr)
    else:
        mp.write_text(skeleton)
        written.append(mp)
    return written


def figures_md(name: str, figures: list[dict[str, Any]]) -> dict[str, str]:
    """Each figure's markdown block: the picture, then its numbered caption."""
    out = {}
    for spec in figures:
        out[spec["name"]] = (charts.picture(f"img/{name}", spec) + "\n\n"
                             f"*Figure #. {spec['question']} _(Answer it here, in one sentence.)_*\n")
    return out


def skeleton(title: str, glance: list[tuple[str, str]], sections: dict[str, str]) -> str:
    """A report's skeleton: every section the house form wants, in order (docs/benchmarks/README.md)."""
    lines = [f"# {title}", "",
             "> **The answer.** _One paragraph: the headline with its numbers and their intervals. Say what the run "
             "teaches that the tables don't._", "",
             "| | |", "|---|---|"]
    lines += [f"| {k} | {v} |" for k, v in glance]
    order = [
        ("The question", "_What the run asked, and why it mattered then._"),
        ("The setup", None),
        ("Results", None),
        ("Analysis", "_Where each arm wins and loses (task ids, never transcripts); cost, tokens and time against "
                     "success; what changed since the last comparable run._"),
        ("Threats to validity", "_Sample size, flaky tasks, spend parity, contamination, the harness's own bugs at "
                                "the time._"),
        ("What it cost", None),
        ("Reproduction", None),
        ("Data", None),
    ]
    for head, prompt in order:
        lines += ["", f"## {head}", ""]
        body = sections.get(head)
        if body:
            lines.append(body.rstrip())
        if prompt and head not in sections:
            lines.append(prompt)
        elif prompt:
            lines += ["", prompt]
    text = "\n".join(lines).rstrip() + "\n"
    n = iter(range(1, 1000))
    return re.sub(r"\*Figure #\.", lambda m: f"*Figure {next(n)}.", text)  # numbered in the order they appear


def _jobs(paths: list[str]) -> list[Path]:
    """Job directories from paths and quoted globs: a job holds trial directories (each with a result.json naming its
    task); a directory of jobs is opened one level."""
    out = []
    for p in paths:
        for q in sorted(glob.glob(os.path.expanduser(p))) or [p]:
            d = Path(q)
            if not d.is_dir():
                raise SystemExit(f"no job directory at {q}")
            if any(_is_trial(t) for t in d.iterdir() if t.is_dir()):
                out.append(d)
            else:
                out += [j for j in sorted(d.iterdir()) if j.is_dir() and any(_is_trial(t) for t in j.iterdir()
                                                                             if t.is_dir())]
    return out


def _is_trial(d: Path) -> bool:
    try:
        r = json.loads((d / "result.json").read_text())
    except (OSError, ValueError):
        return False
    return isinstance(r, dict) and "task_name" in r


# ------------------------------------------------------------------ harbor

ENDINGS = [
    ("ended by the agent", {None}),
    ("timeout", {"AgentTimeoutError"}),
    ("turn cut", {"TheseusTurnCutError"}),
    ("spend limit", {"TheseusSpendLimitError"}),
    ("refusal", {"AgentSafetyRefusalError"}),
    ("waits for approval", {"TheseusApprovalWaitError"}),
    ("rate limited", {"ApiRateLimitError"}),
    ("stopped", {"TheseusStoppedError"}),
]


def ending(exc: str | None) -> str:
    for name, kinds in ENDINGS:
        if exc in kinds:
            return name
    return "error (exit or harness)"


def _started(t: dict[str, Any], d: Path) -> str:
    try:
        r = json.loads((d / "result.json").read_text())
        return ((r.get("agent_execution") or {}).get("started_at") or r.get("started_at") or "")
    except (OSError, ValueError):
        return ""


def harbor(args: argparse.Namespace) -> int:
    eff = _load("efficiency_report", HERE / "efficiency.py")
    labels = dict(x.split("=", 1) for x in args.label or [])
    arms: dict[str, list[dict[str, Any]]] = {}
    sources: dict[str, list[str]] = {}
    for spec in args.arm:
        key, sep, paths = spec.partition("=")
        if not sep or key not in charts.ARMS:
            raise SystemExit(f"--arm {spec}: want KEY=JOBS with KEY one of {', '.join(charts.ARMS)}")
        jobs = _jobs(paths.split(","))
        trials = []
        for job in jobs:
            for d in sorted(p for p in job.iterdir() if p.is_dir()):
                t = eff.load_trial(d)
                if t is not None:
                    t["started"] = _started(t, d)
                    trials.append(t)
        arms.setdefault(key, []).extend(trials)
        sources.setdefault(key, []).extend(str(j) for j in jobs)
    keys = list(arms)
    name = report_name(args.date, args.suite, args.slug)
    # attempts: per arm and task, in the order the trials started
    by_task: dict[str, dict[str, list[dict[str, Any]]]] = {k: defaultdict(list) for k in keys}
    for k in keys:
        for t in sorted(arms[k], key=lambda t: (t["task"], t["started"], t["trial"])):
            by_task[k][t["task"]].append(t)
    tasks = sorted({t for k in keys for t in by_task[k]})
    attempts = max((len(v) for k in keys for v in by_task[k].values()), default=0)
    rows_csv, summary = [], {}
    for k in keys:
        ts = arms[k]
        solved = sum(1 for t in ts if t["solved"])
        task_rates = [sum(1 for t in by_task[k][x] if t["solved"]) / len(by_task[k][x]) for x in by_task[k]]
        ends = Counter(ending(t["error"]) for t in ts)
        s = eff.summarize(k, ts)
        summary[k] = {
            "label": label_of(k, labels), "trials": len(ts), "tasks": len(by_task[k]),
            "pass": rate(solved, len(ts)),
            "solved_every_attempt": sum(1 for x in by_task[k] if all(t["solved"] for t in by_task[k][x])),
            "solved_any_attempt": sum(1 for x in by_task[k] if any(t["solved"] for t in by_task[k][x])),
            "task_mean_rate": interval(task_rates),
            "cost_usd": s["cost_usd"], "unpriced": s["unpriced"],
            "cost_per_trial": interval([t["record"].get("cost_usd") for t in ts]),
            "agent_min_per_trial": interval([t["wall_s"] / 60 for t in ts if t["wall_s"] is not None]),
            "model_calls_per_trial": s["model_calls_per_trial"], "tool_calls_per_trial": s["tool_calls_per_trial"],
            "tokens": s["tokens"], "cache_hit_share": s["cache_hit_share"], "endings": dict(ends),
            "sampled": s["sampled"], "harness_cpu_ms_per_tool_call": s["harness_cpu_ms_per_tool_call"],
            "peak_harness_rss_mb": s["peak_harness_rss_mb"],
        }
        for x in sorted(by_task[k]):
            for i, t in enumerate(by_task[k][x], 1):
                rec, tk = t["record"], t["record"].get("tokens") or {}
                rows_csv.append({"arm": k, "task": x, "attempt": i, "trial": t["trial"], "reward": t["reward"],
                                 "ending": ending(t["error"]), "exception": t["error"] or "",
                                 "agent_s": t["wall_s"], "cost_usd": rec.get("cost_usd"),
                                 "model_calls": rec.get("model_calls"), "tool_calls": rec.get("tool_calls"),
                                 **{c: tk.get(c) for c in ("input", "cache_read", "cache_write", "output")}})
    # paired against the first arm, by task and attempt
    ref = keys[0]
    paired = {}
    for k in keys[1:]:
        b = c = both = neither = 0
        for x in tasks:
            for a1, a2 in zip(by_task[ref].get(x, []), by_task[k].get(x, [])):
                s1, s2 = a1["solved"], a2["solved"]
                b += s1 and not s2
                c += s2 and not s1
                both += s1 and s2
                neither += not s1 and not s2
        only_ref = sorted(x for x in tasks if any(t["solved"] for t in by_task[ref].get(x, []))
                          and not any(t["solved"] for t in by_task[k].get(x, [])))
        only_k = sorted(x for x in tasks if any(t["solved"] for t in by_task[k].get(x, []))
                        and not any(t["solved"] for t in by_task[ref].get(x, [])))
        paired[k] = {"against": ref, "only_ref": b, "only_arm": c, "both": both, "neither": neither,
                     "p_mcnemar": stats.mcnemar_exact(b, c), "tasks_only_ref": only_ref, "tasks_only_arm": only_k}
    figures = harbor_figures(keys, summary, by_task, tasks, attempts, len(tasks))
    data = {"report": name, "suite": args.suite, "date": args.date, "model": args.model, "commit": args.commit,
            "kind": "harbor", "arms": [{"key": k, "label": summary[k]["label"]} for k in keys],
            "summary": summary, "paired": paired, "figures": figures}
    sk = harbor_skeleton(name, args, keys, summary, paired, figures, attempts, len(tasks), sources)
    for p in write_outputs(Path(args.out), name, data, rows_csv, sk, args.force):
        print(p)
    return 0


def harbor_figures(keys: list[str], summary: dict[str, Any], by_task: dict[str, Any], tasks: list[str],
                   attempts: int, n_tasks: int) -> list[dict[str, Any]]:
    lab = {k: summary[k]["label"] for k in keys}
    figs: list[dict[str, Any]] = [{
        "name": "pass-rates", "form": "intervals", "title": "Pass rate by arm, with 95% Wilson intervals",
        "question": "Which arm solves the most trials, and do the intervals separate?",
        "note": f"Each arm's trials: {n_tasks} tasks × up to {attempts} attempts; a trial passes when its tests give "
                "reward 1.",
        "x": {"label": "trials passed", "format": "pct", "min": 0, "max": 1},
        "rows": [{"label": lab[k], "arm": k, "value": summary[k]["pass"]["rate"], "lo": summary[k]["pass"]["lo"],
                  "hi": summary[k]["pass"]["hi"], "tip": f'{summary[k]["pass"]["k"]} of {summary[k]["pass"]["n"]}'}
                 for k in keys if summary[k]["pass"]["n"]],
    }]
    cats = [e for e, _ in ENDINGS[1:]] + ["error (exit or harness)"]
    cats = [c for c in cats if any(summary[k]["endings"].get(c) for k in keys)]
    if cats:
        figs.append({
            "name": "endings", "form": "bars", "title": "Trials that did not end by the agent's own choice",
            "question": "How does each arm's run end when it does not end on its own, and which ending costs most?",
            "categories": cats,
            "series": [{"arm": k, "label": lab[k], "values": [summary[k]["endings"].get(c, 0) for c in cats]}
                       for k in keys],
            "value": {"label": "trials", "format": "int", "min": 0}})
    figs.append({
        "name": "cost-per-trial", "form": "strip", "title": "Dollars per trial",
        "question": "How are each arm's dollars spread over its trials, and is the mean carried by a few?",
        "x": {"label": "dollars per trial (log scale)", "format": "usd", "log": True},
        "groups": [{"arm": k, "label": lab[k],
                    "values": [t["record"].get("cost_usd") for x in tasks for t in by_task[k].get(x, [])],
                    } for k in keys]})
    figs.append({
        "name": "pareto-dollars", "form": "scatter", "front": "min-x-max-y",
        "title": "Pass rate against dollars per trial",
        "question": "Which arms buy the most passes for the least money?",
        "x": {"label": "mean dollars per trial (95% CI)", "format": "usd", "min": 0},
        "y": {"label": "trials passed (95% Wilson)", "format": "pct"},
        "points": [{"arm": k, "label": lab[k], "x": summary[k]["cost_per_trial"]["mean"],
                    "xlo": summary[k]["cost_per_trial"]["lo"], "xhi": summary[k]["cost_per_trial"]["hi"],
                    "y": summary[k]["pass"]["rate"], "ylo": summary[k]["pass"]["lo"],
                    "yhi": summary[k]["pass"]["hi"]}
                   for k in keys if summary[k]["cost_per_trial"]["mean"] is not None and summary[k]["pass"]["n"]]})
    cols = [{"label": f"{lab[k][:1] if len(keys) > 1 else ''}{i}".strip() or str(i), "arm": k}
            for k in keys for i in range(1, attempts + 1)]
    letters = {k: (chr(ord("A") + j) if len(keys) > 1 else "") for j, k in enumerate(keys)}
    for c in cols:
        k = c["arm"]
        c["label"] = f'{letters[k]}{c["label"][-1]}' if letters[k] else c["label"]
    rows = []
    for x in tasks:
        cells, tips = "", []
        for k in keys:
            ts = by_task[k].get(x, [])
            for i in range(attempts):
                if i >= len(ts) or ts[i]["reward"] is None and not ts[i]["error"]:
                    cells += "-"
                    tips.append("no result")
                    continue
                t = ts[i]
                cells += "P" if t["solved"] else ("E" if t["error"] else "F")
                cost = t["record"].get("cost_usd")
                tips.append(f'{ending(t["error"])}, {usd(cost) if cost is not None else "cost unknown"}')
        solved = cells.count("P")
        rows.append({"label": x, "cells": cells, "note": f"{solved}/{len(cells)}", "tips": tips})
    rows.sort(key=lambda r: (-r["cells"].count("P"), r["label"]))
    figs.append({"name": "outcomes", "form": "matrix", "title": "Every trial's outcome, task by task",
                 "question": "Where do the arms differ, task by task, and which tasks no arm solves?",
                 "note": "Columns: each arm's attempts, in the order they ran. Rows: most solved first.",
                 "columns": cols, "rows": rows})
    return figs


def harbor_skeleton(name: str, args: argparse.Namespace, keys: list[str], summary: dict[str, Any],
                    paired: dict[str, Any], figures: list[dict[str, Any]], attempts: int, n_tasks: int,
                    sources: dict[str, list[str]]) -> str:
    lab = {k: summary[k]["label"] for k in keys}
    figs = figures_md(name, figures)
    total = sum(summary[k]["cost_usd"] for k in keys)
    glance = [("Suite", f"`{args.suite}`, {n_tasks} tasks"), ("Arms", "; ".join(lab[k] for k in keys)),
              ("Model", f"`{args.model}`" if args.model else "_the model_"),
              ("Tasks × attempts", f"{n_tasks} × {attempts}"),
              ("Date, commit", f"{args.date}, `{args.commit}`" if args.commit else args.date),
              ("Cost", usd(total)), ("Data", f"[`{name}.json`]({name}.json), [`{name}.csv`]({name}.csv)")]
    head = "| | " + " | ".join(lab[k] for k in keys) + " |\n|---|" + "---|" * len(keys) + "\n"

    def row(label: str, f) -> str:
        return f"| {label} | " + " | ".join(f(summary[k]) for k in keys) + " |\n"

    t = head
    t += row("Trials passed (95% Wilson)", lambda s: f'{s["pass"]["k"]}/{s["pass"]["n"]} = {pct(s["pass"]["rate"])} '
                                                    f'[{pct(s["pass"]["lo"])}, {pct(s["pass"]["hi"])}]')
    t += row("Tasks solved in every attempt / in any", lambda s: f'{s["solved_every_attempt"]} / {s["solved_any_attempt"]}'
                                                            f' of {s["tasks"]}')
    t += row("Dollars per trial, mean", lambda s: ci(s["cost_per_trial"], usd))
    t += row("Dollars, total", lambda s: usd(s["cost_usd"]) + (f' ({s["unpriced"]} unpriced)' if s["unpriced"] else ""))
    t += row("Agent minutes per trial, mean", lambda s: ci(s["agent_min_per_trial"], lambda v: f"{v:.1f}"))
    t += row("Model calls per trial", lambda s: "–" if s["model_calls_per_trial"] is None
             else f'{s["model_calls_per_trial"]:.1f}')
    t += row("Input read from the cache", lambda s: pct(s["cache_hit_share"]))
    for e, _ in ENDINGS[1:] + [("error (exit or harness)", None)]:
        if any(summary[k]["endings"].get(e) for k in keys):
            t += row(f"Trials ending in: {e}", lambda s, e=e: str(s["endings"].get(e, 0)))
    p = ""
    for k, d in paired.items():
        p += (f"- **{lab[k]} against {lab[d['against']]}**, paired by task and attempt: {d['only_ref']} pairs passed "
              f"only by {lab[d['against']]}, {d['only_arm']} only by {lab[k]}, {d['both']} by both, {d['neither']} "
              f"by neither; exact McNemar p = {d['p_mcnemar']:.3g}. Tasks solved only by {lab[d['against']]} "
              f"(in any attempt): {', '.join(d['tasks_only_ref']) or 'none'}. Only by {lab[k]}: "
              f"{', '.join(d['tasks_only_arm']) or 'none'}.\n")
    results = t + "\n" + "\n".join(figs[f["name"]] for f in figures if f["name"] in ("pass-rates", "pareto-dollars"))
    analysis = ((p + "\n") if p else "") + "\n".join(
        figs[f["name"]] for f in figures if f["name"] in ("endings", "cost-per-trial", "outcomes"))
    setup = "\n".join(
        [f"- **{lab[k]}** (`{k}`): _its harness and version, its options and limits._ Jobs: {len(sources[k])} "
         "Harbor job directories." for k in keys] +
        ["- **Model:** " + (f"`{args.model}`" if args.model else "_the model_") + " for every arm.",
         f"- **Dataset:** `{args.suite}`, {n_tasks} tasks, {attempts} attempts each.",
         "- **Limits:** _attempts, the task's agent timeout, the spend and turn caps (`THESEUS_BENCH_*`; Claude Code's "
         "`max_budget_usd`, `max_turns`)._",
         "- **Machine:** _the host, its cores and memory, and how many trials ran at once (`-n`)._",
         f"- **Date and commit:** {args.date}" + (f", Theseus at `{args.commit}`." if args.commit else ".")])
    arms_cmd = " ".join(f"--arm {k}=<jobs>" for k in keys)
    repro = ("```bash\n"
             "export PYTHONPATH=$PWD/bench/harbor HARBOR_TELEMETRY=0\n"
             + "".join(f"# {lab[k]}: _its harbor run line, from bench/README.md_\n" for k in keys)
             + f"python3 bench/report/draft.py harbor --suite {args.suite} --date {args.date}"
             + (f" --slug {args.slug}" if args.slug else "") + f" {arms_cmd}"
             + (f" --model {args.model}" if args.model else "") + "\n```\n")
    data = (f"[`{name}.json`]({name}.json) holds the arms' numbers, the paired comparisons and every figure's spec "
            f"(`python3 bench/report/draft.py plot docs/benchmarks/{name}.json` redraws them); "
            f"[`{name}.csv`]({name}.csv) has one row per trial: arm, task, attempt, reward, ending, agent seconds, "
            "dollars, calls and tokens by class. The trials' own files (transcripts included) are not published.")
    cost = "\n".join(f"- {lab[k]}: {usd(summary[k]['cost_usd'])}" for k in keys) + f"\n- **In all: {usd(total)}.**"
    return skeleton(f"{args.suite}: _what the run asked_ ({args.date})", glance,
                    {"The setup": setup, "Results": results, "Analysis": analysis, "What it cost": cost,
                     "Reproduction": repro, "Data": data})


# ------------------------------------------------------------------ history

PHASES = {  # the bench history's metric prefixes: display name, unit format
    "cold": ("cold start", "ms"), "vault": ("vault start", "ms"), "shutdown": ("clean shutdown", "ms"),
    "inflight": ("the in-flight post", "ms"), "kill": ("SIGKILL then restart", "ms"), "swap": ("binary swap", "ms"),
    "restore": ("restore", "ms"), "seed": ("the push's seed", "ms"), "cancel": ("cancel round trip", "ms"),
    "turn_plain": ("plain turn", "ms"), "turn_tool": ("tool-call turn", "ms"), "turn_long": ("long turn", "ms"),
    "frames_plain": ("plain turn's frames", "int"), "frames_tool": ("tool-call turn's frames", "int"),
    "idle_frames": ("idle frames", "int"), "decodes_long": ("long turn's decodes", "int"),
    "idle_cpu": ("idle CPU", "num"), "idle_wakeups": ("idle wakeups", "int"),
    "rss_start": ("RSS at start", "mb"), "rss_burst": ("RSS after a burst", "mb"), "rss_long": ("RSS, long turn", "mb"),
    "rss_idle": ("RSS idle", "mb"), "rss_tender": ("the tender's RSS", "mb"), "rss_active": ("RSS active", "mb"),
    "size_theseusd": ("theseusd's size", "mb"), "size_theseus": ("theseus's size", "mb"),
    "size_theseus_tui": ("theseus-tui's size", "mb"),
}
GROUPS = [
    ("lifecycle", "The lifecycle bench's p95 by phase, against each phase's budget",
     ["cold", "vault", "shutdown", "inflight", "kill", "swap", "restore", "seed", "cancel"]),
    ("turn", "The turn bench's wall times", ["turn_plain", "turn_tool", "turn_long"]),
    ("frames", "Frames and decodes per turn (counts: the budget allows no slack)",
     ["frames_plain", "frames_tool", "idle_frames", "decodes_long"]),
    ("memory", "Resident memory", ["rss_start", "rss_burst", "rss_long", "rss_idle", "rss_tender", "rss_active"]),
]


def read_history(path: Path) -> list[dict[str, Any]]:
    """Every row of the bench history, each under the header above it (a new phase adds a header block)."""
    out, head = [], None
    with path.open(newline="") as fh:
        for rec in csv.reader(fh):
            if not rec:
                continue
            if rec[0] == "time":
                head = rec
                continue
            if head is None:
                continue
            row = dict(zip(head, rec))
            parsed: dict[str, Any] = {"time": row.get("time"), "label": row.get("label", ""),
                                      "load1": _num(row.get("load1")),
                                      "passed": row.get("passed") == "true",
                                      "allowance": _num(row.get("allowance"))}
            for col, val in row.items():
                if col.endswith(("_p50", "_p95", "_limit")):
                    parsed[col] = _num(val)
            out.append(parsed)
    return out


def _num(v: Any) -> float | None:
    try:
        return float(v) if v not in (None, "") else None
    except ValueError:
        return None


def history(args: argparse.Namespace) -> int:
    path = Path(os.path.expanduser(args.csv or os.environ.get("THESEUS_BENCH_HISTORY")
                                   or "~/.cache/theseus/bench-history.csv"))
    rows = read_history(path)
    if args.branch:
        rows = [r for r in rows if r["label"].strip('"').split(" ")[0] == args.branch]
    if args.since:
        rows = [r for r in rows if (r["time"] or "") >= args.since]
    if args.until:
        rows = [r for r in rows if (r["time"] or "") < args.until]
    if not rows:
        raise SystemExit("no bench-history rows in that span")
    name = report_name(args.date, "gate-bench", args.slug)
    phases = sorted({c[:-4] for r in rows for c in r if c.endswith("_p95") and r[c] is not None},
                    key=lambda p: list(PHASES).index(p) if p in PHASES else 99)
    summ, figures = {}, []
    for p in phases:
        p95 = [r[f"{p}_p95"] for r in rows if r.get(f"{p}_p95") is not None]
        p50 = [r[f"{p}_p50"] for r in rows if r.get(f"{p}_p50") is not None]
        lim = [r[f"{p}_limit"] for r in rows if r.get(f"{p}_limit") is not None]
        last = next((r for r in reversed(rows) if r.get(f"{p}_p95") is not None), None)
        summ[p] = {"name": PHASES.get(p, (p, "num"))[0], "rows": len(p95),
                   "p50_median": stats.median(p50) if p50 else None,
                   "p95_median": stats.median(p95) if p95 else None,
                   "p95_q90": stats.quantile(p95, 0.9) if p95 else None,
                   "p95_max": max(p95) if p95 else None, "limit_last": lim[-1] if lim else None,
                   "last": {"time": last["time"], "label": last["label"], "p50": last.get(f"{p}_p50"),
                            "p95": last.get(f"{p}_p95")} if last else None}
    for key, title, members in GROUPS:
        ps = [p for p in members if p in phases]
        if not ps:
            continue
        panels = []
        for p in ps:
            pts = [[r["time"], r[f"{p}_p95"]] for r in rows if r.get(f"{p}_p95") is not None]
            limits = sorted({r[f"{p}_limit"] for r in rows if r.get(f"{p}_limit") is not None})
            unit = PHASES.get(p, (p, "num"))[1]
            # A latency's failed runs sit 10x above its band: a log axis keeps both, and the budget, readable.
            panel = {"title": f"{PHASES.get(p, (p, ''))[0]}, p95",
                     "y": {"format": unit, "log": True} if unit == "ms" else {"format": unit, "min": 0},
                     "series": [{"arm": "theseus", "label": "p95", "points": pts, "style": "line"}],
                     "marks": [{"x": r["time"], "y": r[f"{p}_p95"], "status": "critical", "label": "",
                                "tip": f'{r["label"]} (load {r["load1"]})'}
                               for r in rows if not r["passed"] and r.get(f"{p}_p95") is not None
                               and r.get(f"{p}_limit") and r[f"{p}_p95"] > r[f"{p}_limit"]]}
            if limits:
                panel["limits"] = [{"y": limits[-1], "label": f"budget {charts.fmt(limits[-1], PHASES.get(p, (p, 'num'))[1])}"}]
            panels.append(panel)
        figures.append({"name": key, "form": "multiples", "title": title + (" (log scale)" if key in ("lifecycle", "turn") else ""),
                        "columns": 2 if len(panels) > 1 else 1,
                        "question": f"Has each {key} number held its budget across the span, and which joins moved it?",
                        "x": {"type": "time"}, "panels": panels,
                        "mark_labels": {"critical": "over its budget in a failed run"}})
    loads = [r["load1"] for r in rows if r["load1"] is not None]
    misses = [r for r in rows if not r["passed"]]
    data = {"report": name, "suite": "gate-bench", "date": args.date, "kind": "history",
            "span": {"from": rows[0]["time"], "to": rows[-1]["time"], "rows": len(rows), "branch": args.branch},
            "rows_failed": len(misses), "load1": interval(loads), "phases": summ, "figures": figures}
    csv_rows = [{"time": r["time"], "label": r["label"], "load1": r["load1"], "passed": r["passed"],
                 "allowance": r["allowance"], **{f"{p}_p95": r.get(f"{p}_p95") for p in phases}} for r in rows]
    figs = figures_md(name, figures)
    t = "| Phase | Budget | p50, median | p95, median | p95, 90th pct | p95, worst | Latest p95 |\n|---|---|---|---|---|---|---|\n"
    for p in phases:
        s, u = summ[p], PHASES.get(p, (p, "num"))[1]
        t += (f'| {s["name"]} | {charts.fmt(s["limit_last"], u) if s["limit_last"] else "–"} | '
              f'{charts.fmt(s["p50_median"], u)} | {charts.fmt(s["p95_median"], u)} | {charts.fmt(s["p95_q90"], u)} | '
              f'{charts.fmt(s["p95_max"], u)} | {charts.fmt(s["last"]["p95"], u) if s["last"] else "–"} |\n')
    glance = [("Suite", "the gate's speed benches (`theseus-sim bench`: lifecycle, turn, idle, size)"),
              ("Span", f'{rows[0]["time"]} to {rows[-1]["time"]}, {len(rows)} runs'
                       + (f" on `{args.branch}`" if args.branch else "")),
              ("Runs that failed (a miss before its rerun, or a red gate)", str(len(misses))),
              ("Data", f"[`{name}.json`]({name}.json), [`{name}.csv`]({name}.csv)")]
    repro = ("```bash\n# Each gate appends its runs to the bench history ($THESEUS_BENCH_HISTORY):\n"
             "target/debug/theseus-sim bench history --last 20\n"
             f"python3 bench/report/draft.py history --date {args.date}"
             + (f" --branch {args.branch}" if args.branch else "") + (f" --since {args.since}" if args.since else "")
             + "\n```\n")
    sk = skeleton(f"The gate's speed benches: _what the span asked_ ({args.date})", glance,
                  {"The setup": "- _The bench's build (debug), its runs per phase, the store it starts on, the "
                                "machine, the busy allowance and settle()._",
                   "Results": t + "\n" + "\n".join(figs.values()),
                   "What it cost": "Nothing beyond the gates' own time: no model is called.",
                   "Reproduction": repro,
                   "Data": f"[`{name}.json`]({name}.json) holds each phase's summary and the figures' specs; "
                           f"[`{name}.csv`]({name}.csv) one row per bench run (time, label, load, passed, each p95)."})
    for p in write_outputs(Path(args.out), name, data, csv_rows, sk, args.force):
        print(p)
    return 0


# ------------------------------------------------------------------ recall

def recall(args: argparse.Namespace) -> int:
    sc = json.loads(Path(args.scores).read_text())
    name = report_name(args.date, "recall", args.slug)
    arms = list(sc["arms"])
    keys = {a: arm_key(a) for a in arms}
    figures = []
    rows_fig = []
    for sal in ("incidental", "central"):
        for a in arms:
            for b in sc["arms"][a].get("curves", {}).get(sal, []):
                if not b["n"]:
                    continue
                p, lo, hi = stats.wilson(b["correct"], b["n"])
                rows_fig.append({"group": f"{sal} facts", "label": f'{b["bucket"]} · {a}', "arm": keys[a],
                                 "value": p, "lo": lo, "hi": hi,
                                 "tip": f'{b["correct"]} of {b["n"]}; {b["turns_since"]:.0f} turns since'
                                 if b.get("turns_since") is not None else f'{b["correct"]} of {b["n"]}'})
    if rows_fig:
        figures.append({"name": "curve", "form": "intervals", "title": "Recall by distance from the fact",
                        "question": "How fast does recall fall as the fact recedes, for each arm and salience?",
                        "x": {"label": "probes answered right (95% Wilson)", "format": "pct", "min": 0, "max": 1},
                        "rows": rows_fig, "note": "Buckets nearest first; scored probes only (undelivered and "
                                                  "failed turns are counted apart, never as misses)."})
    acc = [{"label": a, "arm": keys[a], "value": s["recall_accuracy"]} for a, s in sc["arms"].items()
           if s.get("recall_accuracy") is not None]
    t = "| | " + " | ".join(arms) + " |\n|---|" + "---|" * len(arms) + "\n"
    for label, f in (("Probes, scored / all", lambda s: f'{s["scored"]} / {s["probes"]}'),
                     ("Recall accuracy", lambda s: pct(s.get("recall_accuracy"))),
                     ("Abstention accuracy", lambda s: pct(s.get("abstention_accuracy"))),
                     ("Confident-wrong", lambda s: str(s.get("confident_wrong"))),
                     ("Stale (of superseded)", lambda s: f'{s.get("stale")} of {s.get("stale_of")}'),
                     ("Half-life, incidental (turns)", lambda s: str(_r(s["half_life"]["incidental"]["turns"], 1))),
                     ("Dollars per probe", lambda s: usd(s.get("cost_usd_per_probe"))),
                     ("Undelivered / failed", lambda s: f'{s["undelivered"]} / {s["failed"]}')):
        t += f"| {label} | " + " | ".join(f(sc["arms"][a]) for a in arms) + " |\n"
    data = {"report": name, "suite": "recall", "date": args.date, "kind": "recall", "digest": sc.get("digest"),
            "stale_rule": sc.get("stale_rule"), "arms": [{"key": keys[a], "label": a} for a in arms],
            "summary": {a: {k: v for k, v in s.items() if k not in ("curves", "abstention_curves")}
                        for a, s in sc["arms"].items()},
            "curves": {a: s.get("curves") for a, s in sc["arms"].items()}, "figures": figures}
    rows = [{k: p.get(k) for k in ("arm", "id", "kind", "salience", "bucket", "planned", "status", "correct",
                                   "turns_since", "tokens_since", "cost_usd", "latency_ms")}
            for p in sc.get("probes", [])]
    figs = figures_md(name, figures)
    glance = [("Suite", "the incidental-recall bench (`bench/recall`)"), ("Arms", "; ".join(arms)),
              ("Progression", f'digest `{sc.get("digest")}`'), ("Scoring", f'stale rule `{sc.get("stale_rule")}`'),
              ("Data", f"[`{name}.json`]({name}.json), [`{name}.csv`]({name}.csv)")]
    sk = skeleton(f"Recall: _what the run asked_ ({args.date})", glance,
                  {"Results": t + "\n" + "\n".join(figs.values()),
                   "Reproduction": "```bash\npython3 bench/recall/generate.py --seed <seed> --size <size> --out <prog>\n"
                                   "python3 bench/recall/drive.py --arm <arm> ... --progression <prog> --out <run>\n"
                                   "python3 bench/recall/score.py <runs...> --out <scores>\n"
                                   f"python3 bench/report/draft.py recall --scores <scores>/scores.json --date {args.date}\n```\n",
                   "Data": f"[`{name}.json`]({name}.json): each arm's summary and curve, and the figures' specs; "
                           f"[`{name}.csv`]({name}.csv): one row per probe, no transcript."})
    for p in write_outputs(Path(args.out), name, data, rows, sk, args.force):
        print(p)
    return 0


# ------------------------------------------------------------------ async

def async_(args: argparse.Namespace) -> int:
    score = _load("bench_async_score", BENCH / "async" / "score.py")
    trials = [score.score(t) for t in score.trials([Path(j) for j in args.jobs])]
    if not trials:
        raise SystemExit("no trials in those jobs")
    name = report_name(args.date, "async", args.slug)
    table = score.rows(trials)
    arms = sorted({t["arm"] for t in trials})
    keys = {a: arm_key(a) for a in arms}
    fams = sorted({t["family"] for t in trials})
    figures = []
    succ = []
    for a in arms:
        m = [t for t in trials if t["arm"] == a and t["measurable"]]
        k = sum(1 for t in m if t["success"] == 1)
        p, lo, hi = stats.wilson(k, len(m))
        if m:
            succ.append({"label": a, "arm": keys[a], "value": p, "lo": lo, "hi": hi, "tip": f"{k} of {len(m)}"})
    if succ:
        figures.append({"name": "success", "form": "intervals", "title": "Trials that succeeded, by arm",
                        "question": "Which arm does the simultaneous work right?",
                        "x": {"label": "measurable trials with reward 1 (95% Wilson)", "format": "pct", "min": 0,
                              "max": 1}, "rows": succ})
    series = [{"arm": keys[a], "label": a,
               "values": [next((r["over_ideal"] for r in table if r["arm"] == a and r["family"] == f), None)
                          for f in fams]} for a in arms if keys[a] != "oracle"]
    if any(v is not None for s in series for v in s["values"]):
        figures.append({"name": "over-ideal", "form": "bars", "title": "Wall time over the ideal, by family",
                        "question": "How much longer than the work's own critical path does each arm take?",
                        "categories": fams, "series": series,
                        "value": {"label": "wall / ideal (median)", "format": "x", "min": 0}})
    cols = ["arm", "family", "trials", "success", "over_ideal", "wall_s", "ideal_s", "wait_tax_calls",
            "responsiveness_s", "orphans", "duplicated_effects", "cost_usd"]
    t = "| " + " | ".join(cols) + " |\n|" + "---|" * len(cols) + "\n"
    for r in table:
        t += "| " + " | ".join(score._cell(r.get(c)) for c in cols) + " |\n"
    data = {"report": name, "suite": "async", "date": args.date, "kind": "async",
            "arms": [{"key": keys[a], "label": a} for a in arms], "rows": table, "figures": figures}
    figs = figures_md(name, figures)
    glance = [("Suite", "the async bench (`bench/async`)"), ("Arms", "; ".join(arms)),
              ("Families", ", ".join(fams)), ("Trials", str(len(trials))),
              ("Data", f"[`{name}.json`]({name}.json), [`{name}.csv`]({name}.csv)")]
    sk = skeleton(f"Async: _what the run asked_ ({args.date})", glance,
                  {"Results": t + "\n" + "\n".join(figs.values()),
                   "Reproduction": "```bash\n# the harbor run lines of bench/async/README.md, then:\n"
                                   "python3 bench/async/score.py <jobs...> --out <scores>\n"
                                   f"python3 bench/report/draft.py async --date {args.date} <jobs...>\n```\n",
                   "Data": f"[`{name}.json`]({name}.json): the table and the figures' specs; "
                           f"[`{name}.csv`]({name}.csv): one row per trial."})
    rows = [{k: v for k, v in t.items() if not isinstance(v, (dict, list))} for t in trials]
    for p in write_outputs(Path(args.out), name, data, rows, sk, args.force):
        print(p)
    return 0


# ------------------------------------------------------------------ plot

def plot(summary_path: Path, img_dir: Path | None = None,
         only: list[str] | None = None) -> list[tuple[dict[str, Any], list[Path]]]:
    data = json.loads(summary_path.read_text())
    img_dir = img_dir or summary_path.parent / "img" / summary_path.stem
    out = []
    names = set()
    for spec in data.get("figures") or []:
        if spec["name"] in names:
            raise ValueError(f"two figures named {spec['name']!r}")
        names.add(spec["name"])
        if only and spec["name"] not in only:
            continue
        out.append((spec, charts.write(spec, img_dir)))
    return out


def main(argv: list[str]) -> int:
    ap = argparse.ArgumentParser(description="Draft a benchmark report's tables, plots and skeleton.")
    sub = ap.add_subparsers(dest="cmd", required=True)

    def common(p: argparse.ArgumentParser, slug: bool = True) -> None:
        p.add_argument("--date", required=True, help="YYYY-MM-DD, the day the run happened")
        if slug:
            p.add_argument("--slug", help="a short name after the suite's")
        p.add_argument("--out", default=str(DOCS), help="where the report goes (default docs/benchmarks)")
        p.add_argument("--force", action="store_true", help="rewrite an existing report's .md")

    p = sub.add_parser("harbor", help="a Harbor run: one --arm per arm")
    common(p)
    p.add_argument("--suite", required=True, help="the dataset, as Harbor names it (terminal-bench@2.0)")
    p.add_argument("--arm", action="append", required=True, metavar="KEY=JOBS")
    p.add_argument("--label", action="append", metavar="KEY=TEXT", help="an arm's display name")
    p.add_argument("--model")
    p.add_argument("--commit", help="Theseus's commit, as `theseus --version` says")
    p = sub.add_parser("history", help="the gate's speed benches over a span")
    common(p)
    p.add_argument("--csv", help="the bench history (default $THESEUS_BENCH_HISTORY)")
    p.add_argument("--branch", help="only rows of this branch (main)")
    p.add_argument("--since", help="ISO date or time, inclusive")
    p.add_argument("--until", help="ISO date or time, exclusive")
    p = sub.add_parser("recall", help="the recall bench, from score.py's scores.json")
    common(p)
    p.add_argument("--scores", required=True)
    p = sub.add_parser("async", help="the async bench, from its Harbor jobs")
    common(p)
    p.add_argument("jobs", nargs="+")
    p = sub.add_parser("palette", help="draw the house palette's figure (docs/benchmarks/img/palette*.svg)")
    p.add_argument("--out", default=str(DOCS / "img"))
    p = sub.add_parser("plot", help="render a report's figures from its data file")
    p.add_argument("summary", type=Path)
    p.add_argument("--img", type=Path, help="where the SVGs go (default: img/<report>/ beside the data file)")
    p.add_argument("--only", nargs="*", help="only these figures")
    a = ap.parse_args(argv)
    if a.cmd == "plot":
        img = a.img or a.summary.parent / "img" / a.summary.stem
        done = plot(a.summary, img, a.only)
        rel = os.path.relpath(img, a.summary.parent)
        for i, (spec, paths) in enumerate(done, 1):
            print(f"<!-- figure {i}: {spec['name']} -> {', '.join(str(p) for p in paths)} -->")
            print(charts.picture(rel, spec))
            print()
        return 0
    if a.cmd == "palette":
        out = Path(a.out)
        out.mkdir(parents=True, exist_ok=True)
        for mode in charts.MODES:
            target = out / ("palette-dark.svg" if mode == "dark" else "palette.svg")
            target.write_text(charts.palette_svg(mode))
            print(target)
        return 0
    return {"harbor": harbor, "history": history, "recall": recall, "async": async_}[a.cmd](a)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
