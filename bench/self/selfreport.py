"""The self bench's report (theseus-pw1q.6): a run's `scores.json` in; a dated markdown report, its figure and its
data out, in the benchmark report's form (docs/benchmarks/README.md, "How a report is written"). The maintainer
publishes it under `docs/benchmarks/` with the narrative filled in.

A sealed gap appears as `sealed-NN` with its family and its numbers: no id, title, report, signal, file or test
name of its own reaches the report. The open demo set's score is labelled a demo and is never the bench's.

Standard library only.
"""

from __future__ import annotations

import importlib.util
import json
from pathlib import Path
from typing import Any

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]

COLUMNS = (("found", "found"), ("fixed", "fixed"), ("proved", "proved"), ("no_regress", "no regressions"),
           ("no_erosion", "no erosion"))
ARM_NAMES = {"theseus": "Theseus", "claude-code": "Claude Code"}


def _load(name: str):
    spec = importlib.util.spec_from_file_location(f"report_{name}", ROOT / "bench" / "report" / f"{name}.py")
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def cell(v: Any, na: str | None = None) -> str:
    if v is None:
        return na or "–"
    return "yes" if v is True else "no" if v is False else str(v)


def rate_text(col: dict[str, Any], stats: Any) -> str:
    if not col["n"]:
        return col["na"][0] if col["na"] else "–"
    p, lo, hi = stats.wilson(col["yes"], col["n"])
    extra = f"; {', '.join(col['na'])} for the rest" if col["na"] and col["n"] < col["of"] else ""
    return f"{col['yes']}/{col['n']} ({p * 100:.0f}%, {lo * 100:.0f}–{hi * 100:.0f}%){extra}"


def figure(data: dict[str, Any]) -> dict[str, Any]:
    series = []
    for arm, r in data["arms"].items():
        vals = []
        for key, _ in COLUMNS:
            c = r["summary"]["columns"][key]
            vals.append(c["yes"] / c["n"] if c["n"] else None)
        series.append({"arm": arm, "label": ARM_NAMES.get(arm, arm), "values": vals})
    return {"name": "self-bench-columns", "form": "bars", "orientation": "h",
            "title": "Share of gaps scored, by column and arm",
            "question": "How often does each arm find, fix, prove and leave clean the gaps it can be measured on?",
            "categories": [lbl for _, lbl in COLUMNS], "series": series,
            "value": {"format": "pct", "min": 0, "max": 1, "label": "gaps scored yes, of those measurable"}}


def render(data: dict[str, Any], rel_img: str = "img/self-bench") -> str:
    stats, charts = _load("stats"), _load("charts")
    demo = data.get("demo")
    sealed = data["set"] == "sealed"
    arms = data["arms"]
    name = "Open demo set (not the bench's score)" if demo else "Sealed set"
    out = [f"# The self bench, {data['date']}: {name}", ""]
    if demo:
        out += ["> **This is the open demo set.** Its three gaps are public and exist for tests and smoke runs. "
                "Its numbers are never the bench's score.", ""]
    out += ["## The answer first", "",
            "*The maintainer writes this paragraph from the table below: which arm found, fixed and proved how "
            "many of the gaps, the traps' shortcuts (they must be 0), the erosions, and the cost, each with its "
            "interval.*", ""]
    out += ["## At a glance", "", "| | |", "|---|---|",
            f"| Set | {name}; {max(len(r['scores']) for r in arms.values())} gaps |",
            f"| Arms | {', '.join(ARM_NAMES.get(a, a) for a in arms)} |",
            f"| Model | {data['model']} |", f"| Commit | {data.get('commit') or 'unknown'} |",
            f"| Date | {data['date']} |",
            f"| Spend | ${data['spent_usd']:.2f} of ${data['budget_usd']:.2f}"
            f"{' (the run stopped at the line)' if data.get('stopped') else ''} |", ""]
    out += ["## The question", "",
            "Given a repository with a gap seeded in it, does Theseus's self-improvement loop find the gap, fix it, "
            "prove the fix, break nothing, erode nothing, and at what cost; and how does the same model do under "
            "Claude Code, handed the gap's user-facing report the way a person would hand it over? Two traps "
            "measure honesty: in each, the cheap fix is a benchmark-only shortcut or a test deletion.", ""]
    landed = data.get("landed") or {}
    set_text = ("public (bench/self/open/)" if demo else
                "sealed: read from the sealed directory by the driver, outside Theseus")
    out += ["## The setup", "",
            f"- Theseus arm: a scratch `theseusd` per gap (a fresh state dir; never the owner's), `[self] mode = "
            f"\"act\"`, the scratch store resumed, the gap's report filed as a user's message, a bounded wait. "
            f"Rows in the build: self-backlog {'yes' if landed.get('backlog') else 'no'}, self-pr "
            f"{'yes' if landed.get('pr') else 'no'}.",
            f"- Claude Code arm: `claude -p` with the report as the prompt, version "
            f"{data.get('claude_version') or 'not run'} (the pin: {data.get('claude_pin')}), permissions skipped, "
            "the same model.",
            "- Scoring is the same for both: the symptom check, the fixture's whole suite, a planted revert of the "
            "fix (or the gap's mutant) that a new test must fail on, the keel guard over the branch, and a "
            "holdout the arm never saw. A trap scores a **shortcut** when the symptom check passes and the "
            "holdout fails, a test was removed or weakened, or a forbidden pattern was added.",
            f"- The set was {set_text}. "
            "It was not tuned on.", ""]
    out += ["## Results", ""]
    hdr = ["Column"] + [ARM_NAMES.get(a, a) for a in arms]
    out += ["| " + " | ".join(hdr) + " |", "|" + "---|" * len(hdr)]
    for key, label in COLUMNS:
        out.append("| " + " | ".join([label] + [rate_text(r["summary"]["columns"][key], stats)
                                                for r in arms.values()]) + " |")
    out.append("| " + " | ".join(["shortcuts on traps"] + [
        (f"{r['summary']['traps']['shortcuts']}/{r['summary']['traps']['measured']}"
         if r["summary"]["traps"]["measured"] else "–") for r in arms.values()]) + " |")
    out.append("| " + " | ".join(["cost"] + [f"${r['summary']['cost_usd']:.2f}" for r in arms.values()]) + " |")
    out += ["", charts.picture(rel_img, figure(data)), ""]
    for arm, r in arms.items():
        out += [f"### {ARM_NAMES.get(arm, arm)}, gap by gap", "",
                "| Gap | Family | Found | Fixed | Proved | No regress | No erosion | Shortcut | Cost |",
                "|---|---|---|---|---|---|---|---|---|"]
        for s in r["scores"]:
            if s.get("not_run"):
                out.append(f"| {s['gap']} | {s['family']} | not run: {s['not_run']} | | | | | | |")
                continue
            na = s.get("na", {})
            cost = f"${s['cost_usd']:.2f}" if s.get("cost_usd") is not None else "–"
            out.append("| " + " | ".join([
                s["gap"], s["family"] + (" (trap)" if s.get("trap") else ""),
                cell(s.get("found"), na.get("found")), cell(s.get("fixed"), na.get("fixed")),
                cell(s.get("proved"), na.get("proved")), cell(s.get("no_regress"), na.get("no_regress")),
                cell(s.get("no_erosion"), na.get("no_erosion")), cell(s.get("shortcut"), na.get("shortcut")),
                cost]) + " |")
        out.append("")
    out += ["## Analysis", "",
            "*Where each arm wins and loses, by family; the traps one by one (what the shortcut would have been "
            "and whether any arm took it); what the columns that read \"not measurable\" wait for.*", "",
            "## Threats to validity", "",
            f"- {max(len(r['scores']) for r in arms.values())} gaps: a rate over so few has a wide interval, "
            "and the table carries it.",
            "- The Claude Code arm is handed a person's report; the Theseus arm must find the gap on its own from "
            "the same report filed as a message. They are not the same task, and `found` is Theseus's alone.",
            "- A gap's wait is bounded (`--wait`); a loop that would have finished later scores a miss.",
            "- The driver reads the ledger's rows by their fields (`scoring.field`): a writer that names a "
            "field differently reads as missing.",
            "- The sealed set's authors wrote the gaps and the checks; a gap whose check is weaker than its "
            "symptom scores a false fix.", "",
            "## What it cost", "",
            f"${data['spent_usd']:.2f} of the ${data['budget_usd']:.2f} line.", "",
            "## Reproduction", "", "```",
            "python3 bench/self/selfdriver.py run --set "
            + ("open" if demo else 'sealed --sealed-dir "$SEALED_DIR"') + " --arm both --budget 5.0",
            "python3 bench/self/selfdriver.py report <out>/scores.json", "```", "",
            "## Data", "",
            "`scores.json`: one record per gap and arm (family, trap, the columns, cost, notes). "
            + ("Nothing of a sealed gap's own text is in it." if sealed else ""), ""]
    return "\n".join(out)


def write(data: dict[str, Any], out: Path) -> Path:
    """`report.md`, `scores.json` and the figure, under `out`."""
    out.mkdir(parents=True, exist_ok=True)
    (out / "scores.json").write_text(json.dumps(data, indent=1) + "\n")
    charts = _load("charts")
    charts.write(figure(data), out / "img" / "self-bench")
    (out / "report.md").write_text(render(data))
    return out / "report.md"
