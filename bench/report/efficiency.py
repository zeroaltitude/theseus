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
- `pareto-dollars.svg`, `pareto-tokens.svg`, `pareto-ram.svg` (and each one's
  `-dark.svg`): the same as scatter plots, drawn by `charts.py`;
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
import sys
from datetime import datetime
from pathlib import Path
from typing import Any


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
    if (agent / "pi.txt").exists() or (agent / "pi" / "sessions").is_dir():
        return ef.pi_record(agent), "pi files"
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


# What Pi's own record says of a run Harbor recorded no exception for: its
# print mode exits 0 when the provider fails, so the last answer's
# `stopReason` is where the failure is (theseus-bpeg), and the trial is given
# an error of its own, by that stop.
PI_END_ERRORS = {"error": "PiProviderError", "aborted": "PiAbortedError"}


def _error(exc: dict[str, Any], rec: dict[str, Any]) -> str | None:
    """The trial's error: Harbor's exception, or, when it recorded none, the
    record's `end` (Pi's last answer failed or was aborted)."""
    if exc.get("exception_type"):
        return exc["exception_type"]
    end = rec.get("end")
    return PI_END_ERRORS.get(end.get("stop_reason")) if isinstance(end, dict) else None


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
        # A verifier's own timeout is the grader's, never the agent's failure.
        "grader_timeout": exc.get("exception_type") in ef.GRADER_TIMEOUT_ERRORS,
        "error": None if exc.get("exception_type") in ef.GRADER_TIMEOUT_ERRORS else _error(exc, rec),
        "over_budget": ef.over_budget(rec),
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
    # An arm whose harness enforces no caps (Pi) records the others' and
    # whether each trial passed them; the rest have no `limits`.
    limits = [r["limits"] for r in recs if isinstance(r.get("limits"), dict)]
    past = sum(1 for x in limits if x.get("over_budget") or x.get("over_turns")) if limits else None
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
        "past_caps": past,
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


def _charts_module():
    """`charts.py` beside this file, the house renderer, by its path."""
    path = Path(__file__).resolve().parent / "charts.py"
    if "bench_report_charts" in sys.modules:
        return sys.modules["bench_report_charts"]
    spec = importlib.util.spec_from_file_location("bench_report_charts", path)
    mod = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = mod
    spec.loader.exec_module(mod)
    return mod


charts = _charts_module()

# Each plot's x axis format, by its measure.
_X_FORMAT = {"cost_per_trial": "usd", "tokens_per_trial": "int", "mean_peak_harness_rss_mb": "num"}


def pareto_spec(name: str, title: str, xlabel: str, points: list[tuple[str, float, float]],
                x_format: str = "num") -> dict[str, Any]:
    """The scatter's spec for `charts.py`: one labelled point per arm, the front ringed and joined. An arm whose
    name is a registry key (`theseus`, `claude-code`, ...) wears its house colour; any other arm (`pi`, say) the
    neutral one, its name in the legend."""
    others = [n for n, _, _ in points if n not in charts.ARMS]
    return {
        "name": name, "form": "scatter", "title": title, "front": "min-x-max-y",
        "question": f"Which arms score the most for the least {xlabel}?",
        "x": {"label": xlabel, "format": x_format, "min": 0},
        "y": {"label": "score (mean reward)", "format": "num", "min": 0,
              "max": 1.0 if all(p[2] <= 1 for p in points) else None},
        "points": [{"arm": n if n in charts.ARMS else "other", "label": n, "x": x, "y": y} for n, x, y in points],
        **({"legend_labels": {"other": ", ".join(others)}} if others else {}),
    }


def svg(title: str, xlabel: str, points: list[tuple[str, float, float]], on_front: set[str],
        x_format: str = "num", mode: str = "light") -> str:
    """A scatter of score against a cost, one labelled point per arm, the front ringed and joined (the house
    renderer; `on_front` is the front this module computed, the same rule as the renderer's)."""
    spec = pareto_spec("pareto", title, xlabel, points, x_format)
    drawn = {points[i][0] for i in charts.pareto_front([(x, y) for _, x, y in points])}
    if drawn != set(on_front):
        raise ValueError(f"front mismatch: {sorted(drawn)} against {sorted(on_front)}")
    return charts.render(spec, mode)


# ------------------------------------------------------------------ report


def _n(v: Any, fmt: str = "{:,.0f}", none: str = "–") -> str:
    return none if v is None else fmt.format(v)


def _dollars(v: float) -> str:
    """Cents, as the published tables have them; four places under a dime,
    so a small run does not read as free."""
    return f"${v:.2f}" if v >= 0.1 or v == 0 else f"${v:.4f}"


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
    row("Cost, total", lambda a: _dollars(a["cost_usd"]) + (f" ({a['unpriced']} unpriced)" if a["unpriced"] else ""))
    row("Cost per trial", lambda a: _n(a["cost_per_trial"], "${:.3f}"))
    row("Solved per dollar", lambda a: _n(a["solved_per_dollar"], "{:.2f}"))
    row("Tokens per trial", lambda a: _n(a["tokens_per_trial"]))
    row("Input read from the cache", lambda a: _n(a["cache_hit_share"] and a["cache_hit_share"] * 100, "{:.1f}%"))
    row("Model calls per trial", lambda a: _n(a["model_calls_per_trial"], "{:.1f}"))
    row("Tool calls per trial", lambda a: _n(a["tool_calls_per_trial"], "{:.1f}"))
    if any(a.get("past_caps") is not None for a in arms):
        row("Trials past the others' caps (not enforced)",
            lambda a: "–" if a.get("past_caps") is None else f"{a['past_caps']}/{a['trials']}")
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
    if any(a.get("past_caps") is not None for a in arms):
        lines.insert(-1, "- **Past the others' caps**: an arm with no spend or turn cap of its own (Pi) runs "
                         "unbounded; its trials whose dollars passed `max_budget_usd`, or whose answers passed "
                         "`max_turns`, as the capped arms ran, are counted here and kept in its score.")
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
        for mode in charts.MODES:
            target = out / (file if mode == "light" else file.replace(".svg", "-dark.svg"))
            target.write_text(svg(f"Score against {label}", label, pts, front(pts), _X_FORMAT[measure], mode))
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
        print(f"{s['arm']}: {s['solved']}/{s['trials']} solved, {_dollars(s['cost_usd'])}")
    print(f"wrote {a.out / 'report.md'}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
