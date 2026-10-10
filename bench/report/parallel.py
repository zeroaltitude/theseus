#!/usr/bin/env python3
"""The parallel-calls report (theseus-2wxa): the async bench's Layer 1 and Layer 2 runs in, its report out.

    python3 bench/report/parallel.py --date 2026-10-11 \\
        --arm theseus-before=jobs/pc-theseus-before --arm theseus-d1hi=jobs/pc-theseus-d1hi \\
        --arm theseus-after=jobs/pc-theseus-after --arm claude-code=jobs/pc-claude --arm pi=jobs/pc-pi \\
        --arm opencode=jobs/pc-opencode \\
        --layer1 theseus-before=/tmp/l1/theseus-before.json --layer1 theseus-d1hi=/tmp/l1/theseus-d1hi.json \\
        --layer1 theseus-after=/tmp/l1/theseus-after.json --layer1 claude-code=/tmp/l1/claude-code.json \\
        --layer1 pi=/tmp/l1/pi.json --layer1 opencode=/tmp/l1/opencode.json

writes `docs/benchmarks/<date>-asyncbench-parallel-calls.md` (unless one stands there; `--force`), with its `.json`
(every number, the figures' specs, and the omnibus's row) and `.csv` (a row per trial), and the figures in light and
dark (`img/<report>/`). The report answers first, then: Layer 1's wall against N per harness; wall over the ideal by
family and arm; calls per response by arm; round trips and dollars per solved task; the controls' order violations;
the threats; and the projection to Terminal-Bench. The narrative's analysis is the maintainer's to write.

Arms are this report's keys (`ARMS`), each drawn in a registry colour (`charts.ARMS`) or a neutral, its label on
every mark: the registry has no slot for Pi or OpenCode, nor a third Theseus arm (docs/benchmarks/README.md).
Every trial is scored by bench/async/score.py. Standard library only.
"""

from __future__ import annotations

import argparse
import importlib.util
import json
import statistics
import sys
from pathlib import Path
from typing import Any

HERE = Path(__file__).resolve().parent
BENCH = HERE.parent


def _load(name: str, path: Path):
    if name in sys.modules:
        return sys.modules[name]
    spec = importlib.util.spec_from_file_location(name, path)
    mod = importlib.util.module_from_spec(spec)
    sys.modules[name] = mod
    spec.loader.exec_module(mod)
    return mod


draft = _load("bench_report_draft", HERE / "draft.py")
charts = draft.charts
stats = draft.stats
score = _load("bench_async_score", BENCH / "async" / "score.py")
families = sys.modules["families"]

SUITE, SLUG = "asyncbench", "parallel-calls"
# key -> (the colour's registry key, its label). Theseus as it ships today is Theseus's colour; the arm with the
# sentence takes the batching paragraph's slot; the harness change alone, the ablation between them, the gray.
ARMS: dict[str, tuple[str, str]] = {
    "theseus-before": ("theseus", "Theseus before"),
    "theseus-d1hi": ("context", "Theseus, calls grouped (d1hi)"),
    "theseus-after": ("theseus-batching", "Theseus, grouped + the sentence"),
    "claude-code": ("claude-code", "Claude Code"),
    "pi": ("other", "Pi"),
    "opencode": ("other", "OpenCode"),
    "oracle": ("oracle", "oracle"),
}
HARNESS_OF = {"theseus-before": "Theseus", "theseus-d1hi": "Theseus", "theseus-after": "Theseus",
              "claude-code": "Claude Code", "pi": "Pi", "opencode": "OpenCode"}
SHAPE_LABEL = {"programs": "Independent programs, then combine", "reads": "Independent reads and fetches",
               "writes": "Independent writes", "control": "Dependent controls", "mixed": "Mixed"}
# The design's finding (theseus-d1hi): on Terminal-Bench work, grouping a response's calls is worth about this share
# of tool time.
TB_SHARE = 0.002


def _kv(items: list[str], what: str) -> list[tuple[str, str]]:
    out = []
    for it in items:
        k, sep, v = it.partition("=")
        if not sep or k not in ARMS:
            raise SystemExit(f"--{what} {it!r}: want KEY=PATH, KEY one of {', '.join(ARMS)}")
        out.append((k, v))
    return out


def _median(xs: list[Any]) -> float | None:
    xs = [x for x in xs if isinstance(x, (int, float)) and not isinstance(x, bool)]
    return statistics.median(xs) if xs else None


def label(key: str) -> str:
    return ARMS[key][1]


# ------------------------------------------------------------------ the numbers

def trials_of(arms: list[tuple[str, str]]) -> list[dict[str, Any]]:
    """Every Layer 2 trial of every arm, scored, its arm the report's key."""
    out = []
    for key, jobs in arms:
        for job in draft._jobs(jobs.split(",")):
            for t in score.trials([job]):
                s = score.score(t, key)
                if s["family"] in families.BY_NAME:
                    out.append(s)
    return out


def summary(trials: list[dict[str, Any]], keys: list[str]) -> dict[str, Any]:
    """Each arm's numbers over its trials."""
    out = {}
    for k in keys:
        ts = [t for t in trials if t["arm"] == k]
        if not ts:
            continue
        solved = [t for t in ts if t["success"] == 1]
        indep = [t for t in ts if t["shape"] not in ("control",)]
        controls = [t for t in ts if t["shape"] == "control"]
        tooled = sum(t.get("tool_responses") or 0 for t in ts)
        calls = sum(t.get("tool_calls") or 0 for t in ts)
        multi = sum(t.get("multi_call_responses") or 0 for t in ts)
        rounds = [t.get("model_calls") or t.get("responses") for t in ts]
        cost = [t.get("cost_usd") for t in ts]
        out[k] = {
            "trials": len(ts),
            "solved": draft.rate(len(solved), len(ts)),
            "over_ideal_independent": draft.interval([t["over_ideal"] for t in indep if t["over_ideal"]]),
            "over_ideal_controls": draft.interval([t["over_ideal"] for t in controls if t["over_ideal"]]),
            "overlap_independent": _median([t["overlap"] for t in indep]),
            "calls_per_response": calls / tooled if tooled else None,
            "multi_call_share": draft.rate(multi, tooled) if tooled else None,
            "tool_responses": tooled,
            "round_trips": sum(r for r in rounds if r),
            "round_trips_per_solved": (sum(r for r in rounds if r) / len(solved)) if solved and any(rounds) else None,
            "usd": sum(c for c in cost if c is not None) if any(c is not None for c in cost) else None,
            "usd_per_solved": (sum(c for c in cost if c is not None) / len(solved))
            if solved and any(c is not None for c in cost) else None,
            "control_trials": len(controls),
            "control_trials_with_violations": sum(1 for t in controls if t.get("order_violations")),
            "order_violations": sum(t.get("order_violations") or 0 for t in ts),
        }
    return out


def by_family(trials: list[dict[str, Any]], keys: list[str]) -> list[dict[str, Any]]:
    rows = []
    for f in families.FAMILIES:
        for k in keys:
            ts = [t for t in trials if t["arm"] == k and t["family"] == f.name]
            if not ts:
                continue
            ratios = [t["over_ideal"] for t in ts if t["over_ideal"]]
            rows.append({"family": f.name, "shape": f.shape, "arm": k, "trials": len(ts),
                         "solved": sum(1 for t in ts if t["success"] == 1),
                         "over_ideal": _median(ratios),
                         "over_ideal_lo": min(ratios) if ratios else None,
                         "over_ideal_hi": max(ratios) if ratios else None,
                         "overlap": _median([t["overlap"] for t in ts]),
                         "order_violations": (sum(t.get("order_violations") or 0 for t in ts)
                                              if f.name in score.l2.DEPS else None)})
    return rows


def layer1(files: list[tuple[str, str]]) -> dict[str, Any]:
    """Each Layer 1 file (bench/async/layer1.py's output), by arm."""
    out = {}
    for key, path in files:
        try:
            d = json.loads(Path(path).read_text())
        except (OSError, ValueError) as e:
            d = {"ok": False, "why": f"no results: {e}", "counts": []}
        out[key] = {"harness": d.get("harness"), "ok": d.get("ok"), "why": d.get("why"),
                    "counts": [{k: c.get(k) for k in ("n", "ok", "why", "wall_s", "min_s", "max_s", "serial_s")}
                               for c in d.get("counts") or []]}
    return out


# ------------------------------------------------------------------ the figures

def legend_labels(keys: list[str]) -> dict[str, str]:
    """The legend's name for each colour the arms use: two arms may share the neutral `other`."""
    out: dict[str, list[str]] = {}
    for k in keys:
        out.setdefault(ARMS[k][0], []).append(label(k))
    return {c: ", ".join(v) for c, v in out.items()}


def figures(summ: dict[str, Any], fam: list[dict[str, Any]], l1: dict[str, Any]) -> list[dict[str, Any]]:
    figs = []
    names = legend_labels(list(summ))
    if l1:
        panels = []
        ns = sorted({c["n"] for v in l1.values() for c in v["counts"]}) or [1, 2, 4, 8, 16]
        for harness in dict.fromkeys(HARNESS_OF[k] for k in l1):
            keys = [k for k in l1 if HARNESS_OF[k] == harness]
            series = [{"arm": "oracle", "label": "in turn: 2 s × N", "style": "line",
                       "points": [[n, 2 * n] for n in ns]}]
            why = []
            for k in keys:
                pts = [[c["n"], c["wall_s"]] for c in l1[k]["counts"] if c["ok"]]
                if pts:
                    series.append({"arm": ARMS[k][0], "label": label(k), "style": "both", "points": pts})
                if not l1[k]["ok"]:
                    why.append(f"{label(k)}: not measurable ({l1[k]['why']})" if not pts else
                               f"{label(k)}: some counts not measurable")
            panels.append({"title": harness + (f" ({'; '.join(why)})" if why else ""),
                           "y": {"label": "wall s", "format": "s", "min": 0},
                           "series": series, "limits": [{"y": 2, "label": "together: 2 s"}]})
        figs.append({"name": "layer1-wall", "form": "multiples", "columns": 2,
                     "title": "Layer 1: the wall of N calls of `sleep 2` in one response, by harness",
                     "question": "Does each harness run one response's independent calls together?",
                     "note": "The stand-in model (theseus-sim fake-model --rules); the CLI's wall from spawn to exit, "
                             "median of its runs.",
                     "x": {"label": "calls in the response (N)", "log": True, "format": "int", "min": 1,
                           "max": max(ns)}, "panels": panels})
    rows = []
    for shape in families.SHAPES:
        for k in summ:
            vals = [r for r in fam if r["arm"] == k and r["shape"] == shape and r["over_ideal"]]
            if vals:
                v = [r["over_ideal"] for r in vals]
                rows.append({"label": label(k), "arm": ARMS[k][0], "value": statistics.median(v), "lo": min(v),
                             "hi": max(v), "group": SHAPE_LABEL[shape], "tip": f"{len(vals)} families"})
    if rows:
        figs.append({"name": "over-ideal", "form": "intervals",
                     "title": "Layer 2: wall over the ideal, by shape and arm",
                     "question": "How close to the work's own critical path does each arm finish?",
                     "note": "Each dot the median of the shape's families' medians; the line their range.",
                     "x": {"label": "wall / ideal (1 is the critical path)", "format": "x", "min": 0},
                     "reference": [{"value": 1, "label": "the ideal"}], "rows": rows, "legend_labels": names})
    rows = [{"label": label(k), "arm": ARMS[k][0], "value": s["calls_per_response"],
             "tip": f"{s['tool_responses']} responses that call a tool"}
            for k, s in summ.items() if s["calls_per_response"] is not None]
    if rows:
        figs.append({"name": "calls-per-response", "form": "intervals",
                     "title": "Layer 2: tool calls per response, by arm",
                     "question": "How many calls does each arm put in one model response?",
                     "note": "Over the responses that call a tool, pooled over every Layer 2 trial.",
                     "x": {"label": "calls per response", "format": "num", "min": 0}, "rows": rows,
                     "legend_labels": names})
    rows = [{"label": label(k), "arm": ARMS[k][0], "value": s["multi_call_share"]["rate"],
             "lo": s["multi_call_share"]["lo"], "hi": s["multi_call_share"]["hi"],
             "tip": f"{s['multi_call_share']['k']} of {s['multi_call_share']['n']}"}
            for k, s in summ.items() if s["multi_call_share"] and s["multi_call_share"]["rate"] is not None]
    if rows:
        figs.append({"name": "multi-call-share", "form": "intervals",
                     "title": "Layer 2: responses with more than one call, by arm",
                     "question": "How often does each arm batch?",
                     "x": {"label": "share of tool-calling responses (95% Wilson)", "format": "pct", "min": 0,
                           "max": 1}, "rows": rows, "legend_labels": names})
    return figs


# ------------------------------------------------------------------ the text

def _x(v: float | None) -> str:
    return "–" if v is None else f"{v:.2f}×"


def answer(summ: dict[str, Any], l1: dict[str, Any]) -> str:
    """The answer, from the numbers: what each Theseus arm did against the others."""
    parts = []
    th = [k for k in ("theseus-before", "theseus-d1hi", "theseus-after") if k in summ]
    if th:
        n = sum(1 for f in families.FAMILIES if f.shape != "control")
        parts.append(f"On Layer 2's {n} families with independent steps, wall over the ideal (median) was " +
                     ", ".join(f"{label(k)} {_x(summ[k]['over_ideal_independent']['median'])}" for k in th))
        others = [k for k in ("claude-code", "pi", "opencode") if k in summ]
        if others:
            parts[-1] += "; " + ", ".join(f"{label(k)} {_x(summ[k]['over_ideal_independent']['median'])}"
                                          for k in others)
        parts[-1] += "."
        shares = [f"{label(k)} {draft.pct(summ[k]['multi_call_share']['rate'])}" for k in summ
                  if k in HARNESS_OF and summ[k]["multi_call_share"]]
        if shares:
            parts.append("Responses that called more than one tool: " + ", ".join(shares) + ".")
        bad = {k: s["control_trials_with_violations"] for k, s in summ.items() if s["control_trials"]}
        if bad:
            parts.append("Control trials with an order violation: " +
                         ", ".join(f"{label(k)} {n} of {summ[k]['control_trials']}" for k, n in bad.items()) + ".")
    at8 = [(k, next((c["wall_s"] for c in v["counts"] if c["n"] == 8 and c["ok"]), None)) for k, v in l1.items()]
    if at8:
        parts.append("Layer 1, eight calls of `sleep 2` in one response: " +
                     ", ".join(f"{label(k)} {w:.1f} s" if w is not None else f"{label(k)} not measurable"
                               for k, w in at8) + " (together: 2 s; in turn: 16 s).")
    parts.append(f"Projected to Terminal-Bench, where the design found grouping worth about {TB_SHARE:.1%} of tool "
                 "time, the gain is small there whatever it is here.")
    return " ".join(parts)


def tables(summ: dict[str, Any], fam: list[dict[str, Any]], l1: dict[str, Any]) -> dict[str, str]:
    out = {}
    lines = ["| Arm | Trials | Solved | Wall / ideal, independent | Wall / ideal, controls | Overlap, independent "
             "| Calls / response | Multi-call share | Round trips / solved | $ / solved |", "|" + "---|" * 10]
    for k, s in summ.items():
        lines.append(f"| {label(k)} | {s['trials']} | {s['solved']['k']} ({draft.pct(s['solved']['rate'])}) | "
                     f"{draft.ci(s['over_ideal_independent'], _x)} | {draft.ci(s['over_ideal_controls'], _x)} | "
                     f"{charts.fmt(s['overlap_independent'], 'num') if s['overlap_independent'] else '–'} | "
                     f"{charts.fmt(s['calls_per_response'], 'num') if s['calls_per_response'] else '–'} | "
                     f"{draft.pct((s['multi_call_share'] or {}).get('rate'))} | "
                     f"{charts.fmt(s['round_trips_per_solved'], 'num') if s['round_trips_per_solved'] else '–'} | "
                     f"{draft.usd(s['usd_per_solved'])} |")
    out["arms"] = "\n".join(lines) + "\n"
    keys = list(summ)
    lines = ["| Family | Shape | " + " | ".join(label(k) for k in keys) + " |", "|---|---|" + "---|" * len(keys)]
    for f in families.FAMILIES:
        cells = []
        for k in keys:
            r = next((r for r in fam if r["family"] == f.name and r["arm"] == k), None)
            cells.append("–" if r is None else f"{_x(r['over_ideal'])} ({r['solved']}/{r['trials']})")
        lines.append(f"| `{f.name}` | {f.shape} | " + " | ".join(cells) + " |")
    out["families"] = "\n".join(lines) + "\n"
    lines = ["| Family | " + " | ".join(label(k) for k in keys) + " |", "|---|" + "---|" * len(keys)]
    for f in families.FAMILIES:
        if f.name not in score.l2.DEPS:
            continue
        cells = []
        for k in keys:
            r = next((r for r in fam if r["family"] == f.name and r["arm"] == k), None)
            cells.append("–" if r is None else f"{r['order_violations']} ({r['solved']}/{r['trials']} solved)")
        lines.append(f"| `{f.name}`{' (control)' if f.shape == 'control' else ''} | " + " | ".join(cells) + " |")
    out["violations"] = "\n".join(lines) + "\n"
    if l1:
        ns = sorted({c["n"] for v in l1.values() for c in v["counts"]})
        lines = ["| Harness | " + " | ".join(f"N={n}" for n in ns) + " | |", "|---|" + "---|" * (len(ns) + 1)]
        for k, v in l1.items():
            cells = []
            for n in ns:
                c = next((c for c in v["counts"] if c["n"] == n), None)
                cells.append("–" if c is None else (f"{c['wall_s']:.2f} s" if c["ok"] else "not measurable"))
            lines.append(f"| {label(k)} | " + " | ".join(cells) + f" | {'' if v['ok'] else 'why: ' + str(v['why'])} |")
        out["layer1"] = "\n".join(lines) + "\n"
    return out


THREATS = """- **Sleep-based durations.** Every slow step sleeps a drawn time, so the walls measure scheduling, not work: a
  harness that runs real work together would also contend for CPU and disk, which these steps never do. The ideal is
  the critical path of those draws, a floor no arm can beat.
- **One model.** Every arm drives Claude Sonnet 5.5 at effort medium; whether a response holds several calls is the
  model's choice as much as the harness's, and another model may batch more or less.
- **Two attempts.** Each family has two trials an arm, so a family's number is two draws; the shapes' and the arms'
  pooled numbers carry the weight.
- **Each arm's own tools.** The harnesses call the same programs through their own shell tools; how a harness reads a
  long output (the notes, the logs) differs, and so do its round trips.
- **Layer 1 is a stand-in.** A harness that sends side requests, or reads the stand-in's answers differently, may be
  timed with work that is not the calls' own; a count whose runs failed reads "not measurable", never a number.
"""


def omnibus(name: str, date: str, summ: dict[str, Any], head: str) -> dict[str, Any]:
    """The omnibus's rows for this report: its line in "Every report", the index's row, and the data row."""
    day = f"{['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'][int(date[5:7]) - 1]} " \
          f"{int(date[8:])}"
    title = "Async: parallel tool calls, Theseus before and after grouping them, against Claude Code, Pi and OpenCode"
    usd = sum(s["usd"] or 0 for s in summ.values())
    return {
        "every_report_row": f"| Speed | [{title}]({name}.md) | {day} |",
        "index_row": f"| {date} | `{SUITE}` | " + " · ".join(label(k) for k in summ) + f" | {head} | [{title}]({name}.md) |",
        "data": {"report": f"{name}.md", "question": "Speed", "date": date, "usd_total": round(usd, 4),
                 "arms": {k: {"label": label(k), "over_ideal_independent": s["over_ideal_independent"]["median"],
                              "calls_per_response": s["calls_per_response"],
                              "multi_call_share": (s["multi_call_share"] or {}).get("rate"),
                              "solved": s["solved"]["k"], "trials": s["trials"]} for k, s in summ.items()}},
    }


def build(args: argparse.Namespace) -> tuple[str, dict[str, Any], list[dict[str, Any]], str]:
    arms = _kv(args.arm or [], "arm")
    l1_files = _kv(args.layer1 or [], "layer1")
    trials = trials_of(arms)
    keys = list(dict.fromkeys(k for k, _ in arms if any(t["arm"] == k for t in trials)))
    summ = summary(trials, keys)
    fam = by_family(trials, keys)
    l1 = layer1(l1_files)
    name = draft.report_name(args.date, SUITE, SLUG)
    figs = figures(summ, fam, l1)
    fm = draft.figures_md(name, figs)
    tab = tables(summ, fam, l1)
    head = answer(summ, l1)
    data = {"report": name, "suite": SUITE, "date": args.date, "kind": "async-parallel-calls",
            "arms": [{"key": k, "label": label(k), "colour": ARMS[k][0]} for k in keys],
            "summary": summ, "families": fam, "layer1": l1, "tb_share": TB_SHARE, "figures": figs,
            "omnibus": omnibus(name, args.date, summ, head)}
    glance = [("Suite", "the async bench's Layer 1 and Layer 2 (`bench/async`, theseus-2wxa)"),
              ("Arms", "; ".join(label(k) for k in keys) or "–"),
              ("Model", "Claude Sonnet 5.5 at effort medium; $2 and 200 calls a trial; two attempts"),
              ("Families", f"{len(families.FAMILIES)}: " + ", ".join(
                  f"{sum(1 for f in families.FAMILIES if f.shape == s)} {s}" for s in families.SHAPES)),
              ("Trials", str(len(trials))),
              ("Data", f"[`{name}.json`]({name}.json), [`{name}.csv`]({name}.csv)")]
    results = ""
    if "layer1" in tab:
        results += "### Layer 1: N calls in one response\n\n" + tab["layer1"] + "\n" + fm.get("layer1-wall", "") + "\n"
    results += "### Layer 2: by arm\n\n" + tab["arms"] + "\n"
    for f in ("over-ideal", "calls-per-response", "multi-call-share"):
        results += fm.get(f, "") + "\n"
    results += "### Wall over the ideal, by family and arm\n\nEach cell the median of its trials (solved/trials).\n\n" \
               + tab["families"] + "\n"
    results += "### The controls' order violations\n\nA dependent step started before its prerequisite ended: reward 0 " \
               "whatever the answer. Each cell the violations of its trials.\n\n" + tab["violations"]
    sections = {
        "The question": "Theseus now groups the calls of one response by class and runs each group together "
                        "(theseus-d1hi), and tells the model so in one sentence (theseus-da46). Does it finish "
                        "chores with independent slow steps nearer their critical path, without breaking the ones "
                        "whose steps depend on each other, and how does it compare with Claude Code, Pi and "
                        "OpenCode on the same tasks?",
        "The setup": "Layer 1: `theseus-sim fake-model --rules` asks each harness for N calls of `sleep 2` in one "
                     "response (N in 1, 2, 4, 8, 16), and the wall is timed around its CLI (`bench/async/layer1.py`). "
                     f"Layer 2: {len(families.FAMILIES)} Harbor tasks whose slow sub-steps sleep a drawn 10 to 45 s, "
                     "graded by script from what the task left and a hash-chained ledger of every step; a fifth are "
                     "controls whose dependent steps punish batching (bench/async/README.md).",
        "Results": results,
        "Threats to validity": THREATS,
        "Analysis": "### The projection to Terminal-Bench\n\nThe design's finding: on Terminal-Bench work, grouping a "
                    f"response's calls is worth about {TB_SHARE:.1%} of tool time, since its responses seldom hold "
                    "independent slow calls. Layer 2's chores are built to hold them, so its gain is a ceiling, not "
                    "a forecast. _The analysis: where each arm wins and loses, by family._",
        "What it cost": "| Arm | Model spend |\n|---|---|\n" + "".join(
            f"| {label(k)} | {draft.usd(s['usd'])} |\n" for k, s in summ.items()),
        "Reproduction": "```bash\n# bench/async/README.md, \"The parallel-calls run\" and \"Layer 1\", then:\n"
                        f"python3 bench/report/parallel.py --date {args.date} --arm KEY=JOBS ... --layer1 KEY=FILE ...\n"
                        "```\n",
        "Data": f"[`{name}.json`]({name}.json): every number, the figures' specs and the omnibus's row; "
                f"[`{name}.csv`]({name}.csv): one row per trial.",
    }
    md = draft.skeleton(f"Async: parallel tool calls ({args.date})", glance, sections)
    md = md.replace(
        "> **The answer.** _One paragraph: the headline with its numbers and their intervals. Say what the run "
        "teaches that the tables don't._", f"> **The answer.** {head}")
    rows = [{k: v for k, v in t.items() if not isinstance(v, (dict, list))} for t in trials]
    return name, data, rows, md


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--date", required=True, help="YYYY-MM-DD, the day the run happened")
    ap.add_argument("--arm", action="append", metavar="KEY=JOBS", help=f"an arm's jobs; KEY one of {', '.join(ARMS)}")
    ap.add_argument("--layer1", action="append", metavar="KEY=FILE", help="an arm's bench/async/layer1.py results")
    ap.add_argument("--out", default=str(draft.DOCS), help="where the report goes (default docs/benchmarks)")
    ap.add_argument("--force", action="store_true", help="rewrite an existing report's .md")
    a = ap.parse_args(argv)
    if not a.arm and not a.layer1:
        raise SystemExit("nothing to report: give --arm KEY=JOBS or --layer1 KEY=FILE")
    name, data, rows, md = build(a)
    for p in draft.write_outputs(Path(a.out), name, data, rows, md, a.force):
        print(p)
    print("omnibus:", data["omnibus"]["every_report_row"])
    return 0


if __name__ == "__main__":
    sys.exit(main())
