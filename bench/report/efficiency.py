"""The efficiency report over Harbor jobs, one arm each (theseus-7gir.12).

    python3 bench/report/efficiency.py --arm theseus=jobs/eff-theseus \\
        --arm claude-code=jobs/eff-claude --out /tmp/eff

Each `--arm NAME=JOB_DIR` is one arm: every trial directory under the job
(each holds a `result.json`, Harbor's `TrialResult`) is read, with its
`agent/efficiency.json` (`bench/harbor/efficiency.py`). `--out` gets:

- `report.md`: per arm, the trials, solved, mean reward, dollars, solved per
  dollar, tokens per solved task by class, the cache-hit share (cache reads
  over all input), model and tool calls, harness CPU per tool call, peak
  harness RSS, and wall per trial; then the Pareto tables of score (mean
  reward) against dollars, tokens, and harness RAM, one point per arm, the
  front marked;
- `pareto-dollars.svg`, `pareto-tokens.svg`, `pareto-ram.svg`: the same as
  scatter plots, hand-written SVG;
- `trials.csv`: one row per trial.

A job from before the record reports what Harbor kept: its dollars and
three counters, the cache write from the trial's trajectory (or from the
arm's own files, read by the record's own functions), and CPU and RAM "not
sampled".

Standard library only.
"""

from __future__ import annotations

import argparse
import csv
import importlib.util
import math
import sys
from datetime import datetime
from pathlib import Path
from typing import Any
from xml.sax.saxutils import escape


def _record_module():
    """`bench/harbor/efficiency.py`, the record's functions, by its path:
    this file has its name too."""
    path = Path(__file__).resolve().parents[1] / "harbor" / "efficiency.py"
    spec = importlib.util.spec_from_file_location("bench_harbor_efficiency", path)
    mod = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = mod
    spec.loader.exec_module(mod)
    return mod


ef = _record_module()

CLASSES = ef.CLASSES
NOT_SAMPLED = "not sampled"


# ------------------------------------------------------------------ trials


def _when(s: str | None) -> datetime | None:
    if not s:
        return None
    try:
        return datetime.fromisoformat(s.replace("Z", "+00:00"))
    except ValueError:
        return None


def _wall(timing: dict[str, Any] | None) -> float | None:
    """Harbor's agent execution, in seconds."""
    t = timing or {}
    a, b = _when(t.get("started_at")), _when(t.get("finished_at"))
    return None if a is None or b is None else (b - a).total_seconds()


def _reward(verifier: dict[str, Any] | None) -> float | None:
    rewards = (verifier or {}).get("rewards") or {}
    if "reward" in rewards:
        return float(rewards["reward"])
    if len(rewards) == 1:
        return float(next(iter(rewards.values())))
    return None


def _rebuilt(agent: Path, harbor: dict[str, Any]) -> tuple[dict[str, Any], str]:
    """An old trial's record, from what it kept: the arm's own files, read by
    the record's functions, else its trajectory, else Harbor's three
    counters (the cache write lost)."""
    if (agent / "theseus-turn.json").exists() or (agent / "theseus-history.json").exists():
        return ef.theseus_record(agent), "theseus files"
    if (agent / "claude-code.txt").exists() or (agent / "sessions").is_dir():
        return ef.claude_code_record(agent), "claude code files"
    traj = ef.read_json(agent / "trajectory.json")
    if isinstance(traj, dict) and traj.get("steps"):
        return ef.record("unknown", ef.trajectory_spend(traj), None), "trajectory"
    n_in, n_cache = harbor.get("n_input_tokens"), harbor.get("n_cache_tokens")
    spend = {
        "tokens": None if n_in is None else {
            "input": None, "cache_read": n_cache or 0, "cache_write": None,
            "output": harbor.get("n_output_tokens") or 0,
            "input_and_write": n_in - (n_cache or 0)},
        "by_model": {}, "cost_usd": harbor.get("cost_usd"), "spend_from": "harbor",
        "model_calls": None, "tool_calls": None,
    }
    return ef.record("unknown", spend, None), "harbor counters"


def load_trial(d: Path) -> dict[str, Any] | None:
    """One trial: its reward, error, wall, and efficiency record."""
    result = ef.read_json(d / "result.json")
    if not isinstance(result, dict) or "task_name" not in result:
        return None
    harbor = result.get("agent_result") or {}
    agent = d / "agent"
    rec = ef.read_json(agent / ef.RECORD)
    source = "record"
    if not isinstance(rec, dict) or rec.get("schema") != ef.SCHEMA:
        rec = (harbor.get("metadata") or {}).get("efficiency")
    if not isinstance(rec, dict) or rec.get("schema") != ef.SCHEMA or "tokens" not in rec:
        rec, source = _rebuilt(agent, harbor)
        # Harbor's dollars are the job's own account of an old trial.
        if harbor.get("cost_usd") is not None:
            rec["cost_usd"] = harbor["cost_usd"]
    exc = result.get("exception_info") or {}
    reward = _reward(result.get("verifier_result"))
    sampled = (rec.get("sampler") or {}).get("status") in ("ok", "running") and rec.get("harness")
    return {
        "trial": result.get("trial_name") or d.name,
        "task": result["task_name"],
        "reward": reward,
        "solved": reward is not None and reward >= 1,
        "error": exc.get("exception_type"),
        "wall_s": _wall(result.get("agent_execution")),
        "record": rec,
        "record_from": source,
        "sampled": bool(sampled),
    }


def load_job(job: Path) -> list[dict[str, Any]]:
    trials = []
    for d in sorted(p for p in job.iterdir() if p.is_dir()):
        t = load_trial(d)
        if t is not None:
            trials.append(t)
    return trials


# -------------------------------------------------------------------- arms


def _mean(xs: list[float]) -> float | None:
    return sum(xs) / len(xs) if xs else None


def summarize(name: str, trials: list[dict[str, Any]]) -> dict[str, Any]:
    """An arm's numbers over its trials (worked in `test_report.py`)."""
    n = len(trials)
    solved = sum(1 for t in trials if t["solved"])
    recs = [t["record"] for t in trials]
    costs = [r.get("cost_usd") for r in recs]
    cost = sum(c for c in costs if c is not None)
    tok = dict.fromkeys(CLASSES, 0)
    writes_kept = True
    for r in recs:
        tk = r.get("tokens") or {}
        for c in CLASSES:
            if tk.get(c) is None:
                if c in ("input", "cache_write") and tk.get("input_and_write") is not None:
                    writes_kept = False
                continue
            tok[c] += tk[c]
        if tk.get("input_and_write") is not None:
            tok["input"] += tk["input_and_write"]  # the write is inside, unsplit
    all_input = tok["input"] + tok["cache_read"] + tok["cache_write"]
    total_tokens = all_input + tok["output"]
    sampled = [t for t in trials if t["sampled"]]
    h_cpu = sum(t["record"]["harness"].get("cpu_s") or 0 for t in sampled)
    s_tools = sum(t["record"].get("tool_calls") or 0 for t in sampled)
    peaks = [t["record"]["harness"].get("peak_rss_kb") or 0 for t in sampled]
    work = [t["record"]["work"].get("cpu_s") or 0 for t in sampled if t["record"].get("work")]
    calls = [r["model_calls"] for r in recs if r.get("model_calls") is not None]
    tools = [r["tool_calls"] for r in recs if r.get("tool_calls") is not None]
    walls = [t["wall_s"] for t in trials if t["wall_s"] is not None]
    return {
        "arm": name,
        "trials": n,
        "solved": solved,
        "mean_reward": sum(t["reward"] or 0 for t in trials) / n if n else None,
        "errors": sum(1 for t in trials if t["error"]),
        "cost_usd": round(cost, 6),
        "unpriced": sum(1 for c in costs if c is None),
        "cost_per_trial": cost / n if n else None,
        "solved_per_dollar": solved / cost if cost > 0 else None,
        "tokens": tok,
        "writes_kept": writes_kept,
        "tokens_per_trial": total_tokens / n if n else None,
        "tokens_per_solved": {c: tok[c] / solved for c in CLASSES} if solved else None,
        "cache_hit_share": tok["cache_read"] / all_input if all_input else None,
        "model_calls_per_trial": _mean(calls),
        "tool_calls_per_trial": _mean(tools),
        "sampled": len(sampled),
        "harness_cpu_ms_per_tool_call": h_cpu * 1000 / s_tools if sampled and s_tools else None,
        "harness_cpu_s_per_trial": h_cpu / len(sampled) if sampled else None,
        "work_cpu_s_per_trial": _mean(work),
        "peak_harness_rss_mb": max(peaks) / 1024 if peaks else None,
        "mean_peak_harness_rss_mb": _mean(peaks) / 1024 if peaks else None,
        "wall_s_per_trial": _mean(walls),
    }


# ------------------------------------------------------------------ pareto

# Each plot: its file, its x measure (lower is better), its axis label.
PARETO = (
    ("dollars", "cost_per_trial", "dollars per trial", "pareto-dollars.svg"),
    ("tokens", "tokens_per_trial", "tokens per trial (all classes)", "pareto-tokens.svg"),
    ("ram", "mean_peak_harness_rss_mb", "peak harness RSS per trial, MB (mean)", "pareto-ram.svg"),
)


def front(points: list[tuple[str, float, float]]) -> set[str]:
    """The arms no other arm dominates: none has at least their score at no
    more cost, and better in one. Points are (arm, cost, score)."""
    out = set()
    for name, x, y in points:
        dominated = any(
            (x2 <= x and y2 >= y) and (x2 < x or y2 > y)
            for n2, x2, y2 in points if n2 != name
        )
        if not dominated:
            out.add(name)
    return out


def _nice(hi: float) -> tuple[float, float]:
    """An axis's top and step: 1, 2, or 5 times a power of ten, about five
    ticks."""
    if hi <= 0:
        return 1.0, 0.2
    raw = hi / 5
    mag = 10 ** math.floor(math.log10(raw))
    step = next(m * mag for m in (1, 2, 5, 10) if m * mag >= raw)
    return step * math.ceil(hi / step), step


def _fmt(v: float, step: float) -> str:
    if step >= 1:
        return f"{v:,.0f}"
    places = max(0, -math.floor(math.log10(step)))
    return f"{v:.{places}f}"


def svg(title: str, xlabel: str, points: list[tuple[str, float, float]], on_front: set[str]) -> str:
    """A scatter of score against a cost, one labeled point per arm, the
    front joined by a line. Hand-written, no plotting package."""
    w, h, left, right, top, bottom = 640, 420, 70, 30, 50, 60
    pw, ph = w - left - right, h - top - bottom
    xmax, xstep = _nice(max((p[1] for p in points), default=1) * 1.1)
    ymax, ystep = _nice(max((p[2] for p in points), default=1) * 1.1)
    ymax = min(ymax, 1.0) if all(p[2] <= 1 for p in points) else ymax
    if ymax <= 0:
        ymax, ystep = 1.0, 0.2

    def px(x: float) -> float:
        return left + pw * x / xmax

    def py(y: float) -> float:
        return top + ph * (1 - y / ymax)

    out = [
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" '
        f'viewBox="0 0 {w} {h}" font-family="sans-serif" font-size="12">',
        f'<rect width="{w}" height="{h}" fill="#ffffff"/>',
        f'<text x="{w / 2}" y="24" text-anchor="middle" font-size="15" fill="#222">{escape(title)}</text>',
    ]
    i = 0
    while i * xstep <= xmax + 1e-12:
        v = i * xstep
        out.append(f'<line x1="{px(v):.1f}" y1="{top}" x2="{px(v):.1f}" y2="{top + ph}" stroke="#eee"/>')
        out.append(f'<text x="{px(v):.1f}" y="{top + ph + 16}" text-anchor="middle" fill="#555">'
                   f'{_fmt(v, xstep)}</text>')
        i += 1
    i = 0
    while i * ystep <= ymax + 1e-12:
        v = i * ystep
        out.append(f'<line x1="{left}" y1="{py(v):.1f}" x2="{left + pw}" y2="{py(v):.1f}" stroke="#eee"/>')
        out.append(f'<text x="{left - 8}" y="{py(v) + 4:.1f}" text-anchor="end" fill="#555">'
                   f'{_fmt(v, ystep)}</text>')
        i += 1
    out.append(f'<rect x="{left}" y="{top}" width="{pw}" height="{ph}" fill="none" stroke="#999"/>')
    out.append(f'<text x="{left + pw / 2}" y="{h - 16}" text-anchor="middle" fill="#222">{escape(xlabel)}</text>')
    out.append(f'<text x="18" y="{top + ph / 2}" text-anchor="middle" fill="#222" '
               f'transform="rotate(-90 18 {top + ph / 2})">score (mean reward)</text>')
    on = sorted((p for p in points if p[0] in on_front), key=lambda p: p[1])
    if len(on) > 1:
        path = " ".join(f"{px(x):.1f},{py(y):.1f}" for _, x, y in on)
        out.append(f'<polyline points="{path}" fill="none" stroke="#3b6fb6" stroke-width="1.5" '
                   f'stroke-dasharray="4 3"/>')
    for name, x, y in points:
        fill = "#3b6fb6" if name in on_front else "#9aa4b1"
        out.append(f'<circle cx="{px(x):.1f}" cy="{py(y):.1f}" r="6" fill="{fill}"/>')
        out.append(f'<text x="{px(x) + 9:.1f}" y="{py(y) - 8:.1f}" fill="#222">{escape(name)}</text>')
    out.append(f'<text x="{left + pw}" y="{top - 8}" text-anchor="end" fill="#3b6fb6" font-size="11">'
               'filled blue: on the front (no arm scores more for less)</text>')
    out.append("</svg>")
    return "\n".join(out) + "\n"


# ------------------------------------------------------------------ report


def _n(v: Any, fmt: str = "{:,.0f}", none: str = "–") -> str:
    return none if v is None else fmt.format(v)


def markdown(arms: list[dict[str, Any]], sources: list[tuple[str, str]]) -> str:
    lines = ["# Efficiency report", ""]
    lines.append("Arms: " + "; ".join(f"**{n}** (`{d}`)" for n, d in sources) + ".")
    lines.append("")
    head = "| | " + " | ".join(a["arm"] for a in arms) + " |"
    lines += [head, "|---|" + "---|" * len(arms)]

    def row(label: str, f) -> None:
        lines.append(f"| {label} | " + " | ".join(f(a) for a in arms) + " |")

    def sampled(f):
        return lambda a: f(a) if a["sampled"] else NOT_SAMPLED

    row("Trials", lambda a: str(a["trials"]))
    row("Solved", lambda a: f"{a['solved']}/{a['trials']}")
    row("Mean reward", lambda a: _n(a["mean_reward"], "{:.3f}"))
    row("Trials with an error", lambda a: str(a["errors"]))
    row("Cost, total", lambda a: f"${a['cost_usd']:.2f}" + (f" ({a['unpriced']} unpriced)" if a["unpriced"] else ""))
    row("Cost per trial", lambda a: _n(a["cost_per_trial"], "${:.3f}"))
    row("Solved per dollar", lambda a: _n(a["solved_per_dollar"], "{:.2f}"))
    row("Tokens per trial", lambda a: _n(a["tokens_per_trial"]))
    row("Input read from the cache", lambda a: _n(a["cache_hit_share"] and a["cache_hit_share"] * 100, "{:.1f}%"))
    row("Model calls per trial", lambda a: _n(a["model_calls_per_trial"], "{:.1f}"))
    row("Tool calls per trial", lambda a: _n(a["tool_calls_per_trial"], "{:.1f}"))
    row("Agent time per trial", lambda a: _n(a["wall_s_per_trial"] and a["wall_s_per_trial"] / 60, "{:.1f} min"))
    row("Trials sampled", lambda a: f"{a['sampled']}/{a['trials']}")
    row("Harness CPU per tool call", sampled(lambda a: _n(a["harness_cpu_ms_per_tool_call"], "{:.1f} ms")))
    row("Harness CPU per trial", sampled(lambda a: _n(a["harness_cpu_s_per_trial"], "{:.2f} s")))
    row("Work CPU per trial", sampled(lambda a: _n(a["work_cpu_s_per_trial"], "{:.1f} s")))
    row("Peak harness RSS, largest", sampled(lambda a: _n(a["peak_harness_rss_mb"], "{:.1f} MB")))
    row("Peak harness RSS, mean", sampled(lambda a: _n(a["mean_peak_harness_rss_mb"], "{:.1f} MB")))
    lines += ["", "## Tokens per solved task, by class", ""]
    lines += ["| | " + " | ".join(a["arm"] for a in arms) + " |", "|---|" + "---|" * len(arms)]
    for c in CLASSES:
        def cell(a, c=c):
            if a["tokens_per_solved"] is None:
                return "–"
            if c in ("input", "cache_write") and not a["writes_kept"]:
                return "not kept" if c == "cache_write" else _n(a["tokens_per_solved"][c]) + " (with writes)"
            return _n(a["tokens_per_solved"][c])
        lines.append(f"| {c.replace('_', ' ')} | " + " | ".join(cell(a) for a in arms) + " |")
    for key, measure, label, file in PARETO:
        pts = [(a["arm"], a[measure], a["mean_reward"]) for a in arms
               if a[measure] is not None and a["mean_reward"] is not None]
        left_out = [a["arm"] for a in arms if a[measure] is None]
        on = front(pts)
        lines += ["", f"## Score against {label}", ""]
        lines += [f"| Arm | Score | {label[0].upper() + label[1:]} | On the front |", "|---|---|---|---|"]
        for name, x, y in sorted(pts, key=lambda p: p[1]):
            shown = f"${x:.3f}" if key == "dollars" else (f"{x:.1f}" if key == "ram" else f"{x:,.0f}")
            lines.append(f"| {name} | {y:.3f} | {shown} | {'yes' if name in on else ''} |")
        if left_out:
            lines.append("")
            lines.append(f"Left out ({NOT_SAMPLED if key == 'ram' else 'no measure'}): " + ", ".join(left_out) + ".")
        lines += ["", f"![score against {label}]({file})"]
    lines += ["", "## What the numbers are", "",
              "- **Solved**: trials whose tests gave reward 1. **Mean reward** is the mean over every trial, "
              "a trial with no reward counting 0; it is the score of the Pareto tables.",
              "- **Cost** is each trial's own (`agent/efficiency.json`; an old trial's, Harbor's `cost_usd`).",
              "- **Tokens** are by class: input (uncached), cache read, cache write, output, every model "
              "included. **Input read from the cache** is cache reads over all input (input, read, write).",
              "- **Harness** is the agent's own processes (by executable name), **work** what they ran; "
              "CPU and RSS come from the sampler (`bench/harbor/sampler.py`). An arm with no sampled trial "
              f"says \"{NOT_SAMPLED}\". Harness CPU per tool call is over sampled trials only.",
              "- **On the front**: no other arm has at least its score at no more cost, and better in one.",
              "- **Agent time** is Harbor's agent execution, from each trial's `result.json`.",
              ""]
    return "\n".join(lines)


def write_csv(path: Path, arms: dict[str, list[dict[str, Any]]]) -> None:
    cols = ["arm", "trial", "task", "reward", "error", "wall_s", "cost_usd", "model_calls",
            "tool_calls", *CLASSES, "harness_cpu_s", "harness_peak_rss_kb", "work_cpu_s",
            "sampler", "record_from"]
    with path.open("w", newline="") as f:
        w = csv.writer(f)
        w.writerow(cols)
        for name, trials in arms.items():
            for t in trials:
                r = t["record"]
                tk = r.get("tokens") or {}
                h, wk = r.get("harness") or {}, r.get("work") or {}
                w.writerow([name, t["trial"], t["task"], t["reward"], t["error"] or "",
                            t["wall_s"], r.get("cost_usd"), r.get("model_calls"), r.get("tool_calls"),
                            *(tk.get(c) for c in CLASSES), h.get("cpu_s"), h.get("peak_rss_kb"),
                            wk.get("cpu_s"), (r.get("sampler") or {}).get("status"), t["record_from"]])


def report(arms: list[tuple[str, Path]], out: Path) -> dict[str, Any]:
    out.mkdir(parents=True, exist_ok=True)
    trials = {name: load_job(job) for name, job in arms}
    summaries = [summarize(name, trials[name]) for name, _ in arms]
    (out / "report.md").write_text(markdown(summaries, [(n, str(j)) for n, j in arms]))
    for key, measure, label, file in PARETO:
        pts = [(a["arm"], a[measure], a["mean_reward"]) for a in summaries
               if a[measure] is not None and a["mean_reward"] is not None]
        (out / file).write_text(svg(f"Score against {label}", label, pts, front(pts)))
    write_csv(out / "trials.csv", trials)
    return {"arms": summaries}


def main(argv: list[str]) -> int:
    ap = argparse.ArgumentParser(description="The efficiency report over Harbor jobs.")
    ap.add_argument("--arm", action="append", required=True, metavar="NAME=JOB_DIR")
    ap.add_argument("--out", required=True, type=Path)
    a = ap.parse_args(argv)
    arms = []
    for spec in a.arm:
        name, sep, job = spec.partition("=")
        if not sep or not name or not Path(job).is_dir():
            ap.error(f"--arm {spec}: want NAME=JOB_DIR, a job directory")
        arms.append((name, Path(job)))
    if len({n for n, _ in arms}) != len(arms):
        ap.error("each --arm needs its own name")
    result = report(arms, a.out)
    for s in result["arms"]:
        print(f"{s['arm']}: {s['solved']}/{s['trials']} solved, ${s['cost_usd']:.2f}")
    print(f"wrote {a.out / 'report.md'}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
