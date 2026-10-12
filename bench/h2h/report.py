"""The head-to-head speed bench's report (theseus-7gir.13): a run.json in, its report out.

    python3 bench/h2h/report.py /tmp/h2h/run.json                # docs/benchmarks/<run's date>-head-to-head-speed.md
    python3 bench/h2h/report.py /tmp/h2h/run.json --out-dir /tmp/rep --force

It writes, as every benchmark report does (docs/benchmarks/README.md):

- `<name>.md`: the answer first (who is faster on each row, by how much, medians with their intervals), the table of
  rows, the figures, the method, the threats, and the commands to rerun;
- `<name>.json`: the numbers the table uses and every figure's spec;
- `img/<name>/*.svg`: the figures, drawn by bench/report/charts.py in the house palette, each a light and a dark file.

The name is `<date>-head-to-head-speed`, the date the run happened (the run's own, never today's). A row an arm has no
value for says "not measured": nothing is filled in. Lower is better on every row. An interval is a seeded bootstrap
of the median (95%) from five values up, else the range; a difference whose intervals overlap is said to be within
the noise. An existing report is kept unless --force.

Standard library only.
"""

from __future__ import annotations

import argparse
import importlib.util
import json
import math
import random
import sys
from pathlib import Path
from typing import Any

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
REPORT = HERE.parent / "report"


def _load(name: str, path: Path):
    if name in sys.modules:
        return sys.modules[name]
    spec = importlib.util.spec_from_file_location(name, path)
    mod = importlib.util.module_from_spec(spec)
    sys.modules[name] = mod
    spec.loader.exec_module(mod)
    return mod


charts = _load("bench_report_charts", REPORT / "charts.py")
stats = _load("stats_shim", REPORT / "stats.py")

SUITE = "head-to-head-speed"
ARMS = ["theseus", "claude-code"]
NAMES = {a: charts.ARMS[a][1] for a in ARMS}

# Every row, in the report's order: (row, kind) -> what it measures. Kinds a run has beyond these (T8's, task3's
# T4s) follow their row.
ROWS = {
    "T1": "Start to a ready prompt",
    "T2": "Typed prompt to the request leaving",
    "T3": "Model's first byte to the first text on screen",
    "T4": "Per tool call: the response's last byte to the next request",
    "T5": "Resume of a long session",
    "T6": "CPU per turn",
    "T7": "Idle, the surface open",
    "T8": "Under neighbour IO (a dd and fsync loop beside)",
    "task3": "A three-tool coding task, end to end",
    "first_request_bytes": "Bytes of the first request",
}
ORDER = list(ROWS)
KIND_ORDER = ["interactive", "one-shot", "read", "shell", "edit", "cpu", "rss"]
# How each unit is shown, and the word for the arm that has less of it.
UNITS = {"ms": ("ms", 1, "faster"), "bytes": ("kb", 1 / 1024, "smaller"), "pct": ("num", 1, "lighter"),
         "kb": ("mb", 1 / 1024, "lighter")}


def median_interval(xs: list[float], iters: int = 4000, seed: int = 7) -> dict[str, Any]:
    """The median with a seeded 95% bootstrap interval of it; under five values, the range."""
    xs = sorted(float(x) for x in xs)
    if not xs:
        return {"n": 0, "median": None, "lo": None, "hi": None, "method": "none"}
    if len(xs) < 5:
        return {"n": len(xs), "median": stats.median(xs), "lo": xs[0], "hi": xs[-1], "method": "range"}
    rng = random.Random(seed)
    meds = [stats.median([xs[rng.randrange(len(xs))] for _ in xs]) for _ in range(iters)]
    return {"n": len(xs), "median": stats.median(xs), "lo": stats.quantile(meds, 0.025),
            "hi": stats.quantile(meds, 0.975), "method": "bootstrap"}


def show(v: float | None, unit: str) -> str:
    if v is None:
        return "not measured"
    fmt, scale, _ = UNITS[unit]
    if unit == "pct":
        return f"{v:.2f}% of a core"
    return charts.fmt(v * scale, fmt)


def cell_text(s: dict[str, Any], unit: str) -> str:
    if s["median"] is None:
        return "not measured"
    word = "range" if s["method"] == "range" else "95% CI"
    lo, hi = show(s["lo"], unit), show(s["hi"], unit)
    return f"{show(s['median'], unit)} ({word} {lo} to {hi}; n {s['n']})"


def cells(run: dict[str, Any]) -> list[dict[str, Any]]:
    """Every (row, kind) the run has or should have, with each arm's summary."""
    groups: dict[tuple[str, str], dict[str, Any]] = {}
    for s in run.get("samples") or []:
        g = groups.setdefault((s["row"], s["kind"]), {"unit": s.get("unit", "ms"), "values": {}})
        g["values"].setdefault(s["arm"], []).append(s["value"])
    for row in run.get("rows") or []:  # a row asked for but with no sample at all still gets its line
        if row in ROWS and not any(r == row for r, _ in groups):
            groups[(row, "-")] = {"unit": "bytes" if row == "first_request_bytes" else "ms", "values": {}}

    def key(rk: tuple[str, str]) -> tuple[int, int, str]:
        row, kind = rk
        base = kind.split(" ")[-1] if row == "T8" else kind
        return (ORDER.index(row) if row in ORDER else 99,
                KIND_ORDER.index(base) if base in KIND_ORDER else 50, kind)

    out = []
    for (row, kind) in sorted(groups, key=key):
        g = groups[(row, kind)]
        summ = {a: median_interval(g["values"].get(a, [])) for a in ARMS}
        out.append({"row": row, "kind": kind, "title": ROWS.get(row, row), "unit": g["unit"], "arms": summ,
                    "verdict": verdict(summ, g["unit"])})
    return out


def verdict(summ: dict[str, dict[str, Any]], unit: str) -> dict[str, Any]:
    """Which arm has less, by how much, and whether the intervals keep them apart."""
    a, b = (summ[x] for x in ARMS)
    if a["median"] is None or b["median"] is None:
        missing = [NAMES[x] for x in ARMS if summ[x]["median"] is None]
        return {"winner": None, "missing": missing}
    if a["median"] == b["median"]:
        return {"winner": "tie", "missing": []}
    win, lose = (ARMS[0], ARMS[1]) if a["median"] < b["median"] else (ARMS[1], ARMS[0])
    w, l_ = summ[win], summ[lose]
    clear = w["hi"] < l_["lo"]
    ratio = l_["median"] / w["median"] if w["median"] > 0 else None
    return {"winner": win, "by": l_["median"] - w["median"], "ratio": ratio, "clear": clear, "missing": []}


def _lower(title: str) -> str:
    """A title inside a sentence: its first letter lower-cased, unless it starts an acronym (CPU, IO)."""
    return title if title[1:2].isupper() else title[:1].lower() + title[1:]


def _brief(s: dict[str, Any], unit: str) -> str:
    return f"{show(s['median'], unit)} [{show(s['lo'], unit)}, {show(s['hi'], unit)}]"


def sentence(c: dict[str, Any]) -> str:
    """One row's verdict in words: who has less, by how much, both medians with their intervals."""
    v, unit = c["verdict"], c["unit"]
    what = f"{c['row']} ({_lower(c['title'])}" + (f", {c['kind']})" if c["kind"] != "-" else ")")
    if v["winner"] is None:
        return f"{what}: {', '.join(v['missing'])} not measured."
    if v["winner"] == "tie":
        return f"{what}: the same median, {show(c['arms'][ARMS[0]]['median'], unit)}."
    win, lose = v["winner"], [a for a in ARMS if a != v["winner"]][0]
    ratio = f", {v['ratio']:.1f}×" if v.get("ratio") else ""
    word = "lighter" if c["row"] == "T6" else UNITS[unit][2]
    tail = "" if v["clear"] else "; the intervals overlap, so within the noise"
    return (f"{what}: {NAMES[win]} is {word} by {show(v['by'], unit)}{ratio} "
            f"({_brief(c['arms'][win], unit)} against {_brief(c['arms'][lose], unit)}{tail}).")


def figures(cs: list[dict[str, Any]], name: str) -> list[dict[str, Any]]:
    """One intervals figure per kind of quantity: latency, CPU per turn, bytes, idle CPU, idle memory."""
    plans = [
        ("latency", "Latency, median and 95% interval, per row and arm (lower is faster)",
         lambda c: c["unit"] == "ms" and c["row"] not in ("T6",), "ms", "milliseconds (log scale)", True),
        ("cpu-per-turn", "CPU per turn, median and 95% interval (lower is lighter)",
         lambda c: c["row"] == "T6", "ms", "CPU milliseconds per turn", False),
        ("first-request-bytes", "Bytes of the first request (lower is smaller)",
         lambda c: c["unit"] == "bytes", "kb", "kilobytes", False),
        ("idle-cpu", "Idle CPU, the surface open and nothing asked",
         lambda c: c["unit"] == "pct", "num", "percent of one core", False),
        ("idle-memory", "Idle memory (RSS), the surface open and nothing asked",
         lambda c: c["unit"] == "kb", "mb", "resident memory", False),
    ]
    out = []
    for fname, title, pick, fmt, label, log in plans:
        rows = []
        for c in filter(pick, cs):
            scale = UNITS[c["unit"]][1]
            for arm in ARMS:
                s = c["arms"][arm]
                if s["median"] is None:
                    continue
                rows.append({"label": NAMES[arm], "arm": arm, "value": s["median"] * scale, "lo": s["lo"] * scale,
                             "hi": s["hi"] * scale, "group": f"{c['row']} · {c['kind']}",
                             "tip": f"n {s['n']}, {s['method']}"})
        if not rows:
            continue
        log = log and all(r["lo"] > 0 for r in rows)
        out.append({"name": fname, "form": "intervals", "title": title,
                    "question": f"Which harness takes less on each row: {title.split(',')[0].lower()}?",
                    "x": {"label": label if log else label.replace(" (log scale)", ""), "format": fmt, "log": log},
                    "rows": rows, "note": f"One stand-in model for both arms; {name}."})
    return out


def _r(v: Any) -> Any:
    if isinstance(v, float):
        return None if math.isnan(v) else round(v, 4)
    if isinstance(v, dict):
        return {k: _r(x) for k, x in v.items()}
    if isinstance(v, list):
        return [_r(x) for x in v]
    return v


def markdown(run: dict[str, Any], cs: list[dict[str, Any]], figs: list[dict[str, Any]], name: str) -> str:
    m, si, arms = run.get("machine") or {}, run.get("standin") or {}, run.get("arms") or {}
    both = [c for c in cs if c["verdict"]["winner"] not in (None,)]
    wins = {a: sum(1 for c in both if c["verdict"]["winner"] == a and c["verdict"].get("clear")) for a in ARMS}
    noise = len(both) - sum(wins.values())
    lead = (f"Over {len(both)} rows measured on both arms, Theseus is clearly ahead on {wins['theseus']} and Claude "
            f"Code on {wins['claude-code']}; {noise} {'is' if noise == 1 else 'are'} within the noise. Medians, "
            "each with its 95% interval in brackets; lower is better. " if both else
            "Only one arm was measured, so this run compares nothing: it records one arm's numbers. ")
    if run.get("theseus_only"):
        why = (arms.get("claude-code") or {}).get("why") or "not run"
        lead += f"Claude Code was not run ({why}). "
    answer = lead + " ".join(sentence(c) for c in cs)
    lines = [f"# Head-to-head speed: Theseus and Claude Code on one stand-in model ({run.get('date')})", "",
             f"> **The answer.** {answer}", ""]
    lines += ["## Results", "",
              "| Row | What | Kind | Theseus | Claude Code | Difference |", "|---|---|---|---|---|---|"]
    for c in cs:
        v = c["verdict"]
        diff = ("–" if v["winner"] is None else "the same" if v["winner"] == "tie" else
                f"{NAMES[v['winner']]} by {show(v['by'], c['unit'])}" + ("" if v["clear"] else " (noise)"))
        lines.append(f"| {c['row']} | {c['title']} | {c['kind']} | {cell_text(c['arms']['theseus'], c['unit'])} | "
                     f"{cell_text(c['arms']['claude-code'], c['unit'])} | {diff} |")
    lines += ["", "Lower is better on every row. An interval is a seeded bootstrap of the median (95%) from five "
              "values up, else the range; \"(noise)\" marks a difference whose intervals overlap."]
    fails = run.get("failures") or []
    if fails:
        lines += ["", f"{len(fails)} attempt{'' if len(fails) == 1 else 's'} gave no value (a marker that never showed, a request never logged); "
                      "they are left out, never filled in: " +
                  ", ".join(sorted({f"{f['row']} {f['kind']} ({NAMES.get(f['arm'], f['arm'])})" for f in fails})) + "."]
    if figs:
        lines += ["", "## Figures", ""]
        for i, f in enumerate(figs, 1):
            lines += [charts.picture(f"img/{name}", f), "", f"*Figure {i}. {f['question']}*", ""]
    ver = run.get("versions") or {}
    lines += ["## Method", "",
              f"Both arms ran on one machine against one stand-in model (`theseus-sim fake-model`), which answers "
              f"every request after {si.get('ttfb_ms')} ms to its first byte, in {si.get('chunks')} chunks "
              f"{si.get('chunk_ms')} ms apart, and logs each request's arrival, first byte and last byte on "
              "CLOCK_MONOTONIC, the bench's own clock. The model's time is the same for both, so what differs is the "
              "harness. Both ran the same work in their own tools (read `notes.txt`, edit a line, run `cat` or "
              "`wc -l`), with every tool open: Theseus's policy open, Claude Code with its permissions skipped.",
              "",
              f"Theseus {ver.get('theseus') or '(version not recorded)'} ran as a scratch daemon of a release build per "
              "run (its own state dir, socket and config; memory, the judge, the index, the language servers, "
              "Discord and the web UI off), its surface the terminal UI (`theseus-tui`) and its one-shot "
              f"`theseus ask`. Claude Code {ver.get('claude-code') or '(not run)'} ran in a scratch home per run, "
              "onboarded, its project trusted, with updates, telemetry, error reports and nonessential traffic off; "
              "interactive with `--dangerously-skip-permissions`, one-shot with `-p`. T1 for Theseus is the TUI's "
              "start with its daemon serving, as Theseus runs (the daemon is a service)" + _daemon_text(run) + ".",
              "",
              "The interactive surfaces ran on a 120×40 pseudo-terminal, typed at by the bench; every read of the "
              "screen was stamped as it arrived, and a row's end is the first appearance of its marker (each reply "
              "is `<word>-<marker> … end-<marker>`). The arms alternated run by run, ABBA by pairs, "
              f"{run.get('runs')} runs each. T5's session was {run.get('long_turns')} turns long; T7 sampled "
              f"`/proc` for {run.get('idle_secs')} s; T8 repeated T1 to T4 beside a loop writing 64 MiB with fsync.",
              "",
              "## Threats to validity", "",
              f"- **One machine**: {m.get('nproc')} CPUs ({m.get('cpu') or 'model not recorded'}), kernel "
              f"{m.get('kernel')}. Another machine moves every number; the comparison holds only on this one.",
              f"- **Load during the run**: load averages {_load_text(m.get('loadavg_before'))} before and "
              f"{_load_text(m.get('loadavg_after'))} after. Other work on the machine lands in both arms' numbers.",
              "- **The stand-in's timing**: a real model's first byte varies by seconds; the stand-in's is fixed, so "
              "these rows are the harness's own cost and not a user's whole wait. Its streaming (chunk count and "
              "spacing) shapes T3 and the turn's end.",
              "- **Markers on a screen**: a surface that draws a word in pieces, or redraws it, is stamped when the "
              "word first stands whole in its output; a terminal's own paint time is not counted.",
              "- **The TUI is not Claude Code's REPL**: Theseus's interactive surface is a board of sessions; its T1 "
              "counts the board with the session on it, and opening the session is not timed.",
              "- **T5's one-shot resume adds a turn each run**, so its session grows by one turn a run.",
              "", "## Reproduction", "",
              "```bash",
              "scripts/build.sh --profile release-thin",
              run.get("command") or "python3 bench/h2h/run.py --bin-dir target/release-thin --runs 10 --out /tmp/h2h",
              f"python3 bench/h2h/report.py {Path(run.get('_path') or '/tmp/h2h/run.json')}",
              "```", "",
              "## Data", "",
              f"`{name}.json` holds every cell's numbers and each figure's spec; the raw samples stay in the run's "
              "`run.json`, off the repository.", ""]
    return "\n".join(lines)


def _daemon_text(run: dict[str, Any]) -> str:
    xs = run.get("daemon_start_ms") or []
    if not xs:
        return ""
    s = median_interval(xs)
    return f"; the scratch daemon's own start to its first health answer, not counted in T1, was {cell_text(s, 'ms')}"


def _load_text(v: Any) -> str:
    return " / ".join(f"{x:.2f}" for x in v) if isinstance(v, list) and v else "not recorded"


def build(run: dict[str, Any], out_dir: Path, date: str | None = None, force: bool = False) -> list[Path]:
    date = date or run.get("date")
    if not date:
        raise ValueError("the run has no date; pass --date YYYY-MM-DD, the day it ran")
    name = f"{date}-{SUITE}"
    cs = cells(run)
    figs = figures(cs, name)
    out_dir.mkdir(parents=True, exist_ok=True)
    written = []
    data = {"name": name, "date": date, "issue": run.get("issue"), "machine": run.get("machine"),
            "versions": run.get("versions"), "standin": run.get("standin"), "runs": run.get("runs"),
            "cells": cs, "figures": figs}
    jp = out_dir / f"{name}.json"
    jp.write_text(json.dumps(_r(data), indent=1, ensure_ascii=False) + "\n")
    written.append(jp)
    for spec in figs:
        written += charts.write(spec, out_dir / "img" / name)
    mp = out_dir / f"{name}.md"
    if mp.exists() and not force:
        print(f"kept {mp}: a report stands there (--force rewrites it)", file=sys.stderr)
    else:
        mp.write_text(markdown(run, cs, figs, name))
        written.append(mp)
    return written


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description="The head-to-head speed bench's report, from a run.json.")
    ap.add_argument("run", type=Path)
    ap.add_argument("--out-dir", type=Path, default=REPO / "docs" / "benchmarks")
    ap.add_argument("--date", help="YYYY-MM-DD (default: the run's own date)")
    ap.add_argument("--force", action="store_true", help="rewrite an existing report")
    a = ap.parse_args(argv)
    run = json.loads(a.run.read_text())
    run["_path"] = str(a.run)
    for p in build(run, a.out_dir, a.date, a.force):
        print(p)
    return 0


if __name__ == "__main__":
    sys.exit(main())
