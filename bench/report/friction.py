"""Tool friction and cost per solve, from Harbor's ATIF trajectories (theseus-w052 5).

Every report carries two tables beside its pass rates, `draft.py` prints them:

**T4, tool friction, per arm.** Read from each trial's `agent/trajectory.json`: the
steps with `source == "agent"`, their `tool_calls`, and each call's
`observation.results` entry, matched by `source_call_id`.

- *a tool-level error*: a result flagged as an error (`extra.status == "error"`, or
  `extra.is_error` / `extra.tool_result_is_error` true, or Pi's `extra.isError`) whose text does not start
  with `[exit code` or `Exit code`: those are a command's own failure (the tool did
  its work and the command failed), not the tool's. A trial counts once however
  many it has;
- *a malformed input*: a call whose input was not valid JSON for the tool's schema
  (the tool refused it before running), counted by the tool;
- *an invented name*: a call to a tool the arm does not offer, or with a field the
  tool's schema does not have.

The last two are kinds of tool-level error, told apart by what each harness says
when it refuses the call (`MALFORMED`, `INVENTED`: the wording is data, one line
per harness, so a new one is a line). A harness that words its refusal some other
way is counted a tool-level error and nothing finer: the table says "tool-level
errors" first and the two kinds as parts of it. A trial with no trajectory is left
out of the shares and counted apart.

**T5, cost and calls per solve.** Over the (task, attempt) pairs every arm solved
(a pair is the same task's same-numbered attempt, as the paired comparison reads
them): per arm, the mean and the median dollars, the mean model calls, the mean
agent seconds, and output tokens per call (the pairs' output tokens over their
calls). Comparing arms over what all of them solved keeps an arm that solves the
easy tasks cheaply from looking cheaper than one that also solves the hard ones.

Standard library only.
"""

from __future__ import annotations

import json
import re
import statistics
from collections import Counter
from pathlib import Path
from typing import Any

# A command's own failure: the tool ran, the program failed.
COMMAND_EXIT = ("[exit code", "Exit code")
# What each harness says when it refuses a call before running it. Theseus: its
# `invalid_json` result is `{"INVALID_JSON": …}`, an invalid input is "Invalid
# input: …" (a serde error: "unknown field `x`, expected one of …" for a field the
# schema lacks), an unknown tool "Unknown tool `x`. Available: …". Claude Code:
# "<tool_use_error>InputValidationError: …" (an extra field: "An unexpected
# parameter `x` was provided"), "Error: No such tool available: x". Pi:
# "Validation failed for tool "x":" (an extra field: "must NOT have additional
# properties"), "Tool x not found".
INVENTED = (
    re.compile(r"^Unknown tool `", re.I),
    re.compile(r"No such tool available", re.I),
    re.compile(r"\bTool \S+ not found", re.I),
    re.compile(r"unknown field `", re.I),
    re.compile(r"unexpected parameter", re.I),
    re.compile(r"must NOT have additional properties|unexpected propert", re.I),
)
MALFORMED = (
    re.compile(r"INVALID_JSON"),
    re.compile(r"^Invalid input:", re.I),
    re.compile(r"InputValidationError", re.I),
    re.compile(r"Validation failed for tool", re.I),
)


def _text(content: Any) -> str:
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        return " ".join(p.get("text", "") for p in content if isinstance(p, dict))
    return ""


def is_error(result: dict[str, Any]) -> bool:
    """A result flagged as an error by its harness."""
    x = result.get("extra") or {}
    # `isError` is Pi's own key (`pi_atif.py` keeps its session log's `toolName` and `isError`).
    return (x.get("status") == "error" or bool(x.get("is_error")) or bool(x.get("tool_result_is_error"))
            or bool(x.get("isError")))


def kind(text: str) -> str | None:
    """`invented`, `malformed` or `error` (a tool-level error told no finer) for an
    error result's text; None for a command's own failure."""
    if re.sub(r"^\s*(<tool_use_error>)?\s*", "", text).startswith(COMMAND_EXIT):
        return None
    if any(p.search(text) for p in INVENTED):
        return "invented"
    if any(p.search(text) for p in MALFORMED):
        return "malformed"
    return "error"


def scan(trajectory: dict[str, Any]) -> dict[str, Any]:
    """One trial's friction: its calls, its tool-level errors, and the malformed
    inputs and invented names among them, each counted by tool."""
    calls = tool_errors = 0
    malformed: Counter[str] = Counter()
    invented: Counter[str] = Counter()
    for step in trajectory.get("steps") or []:
        if step.get("source") != "agent":
            continue
        names = {c.get("tool_call_id"): c.get("function_name") or "?" for c in step.get("tool_calls") or []}
        calls += len(names)
        for r in (step.get("observation") or {}).get("results") or []:
            if not is_error(r):
                continue
            k = kind(_text(r.get("content")))
            if k is None:
                continue
            tool_errors += 1
            tool = names.get(r.get("source_call_id")) or "?"
            if k == "malformed":
                malformed[tool] += 1
            elif k == "invented":
                invented[tool] += 1
    return {"calls": calls, "tool_errors": tool_errors, "malformed": dict(malformed), "invented": dict(invented)}


def scan_dir(trial: Path) -> dict[str, Any] | None:
    """`scan` of a trial directory's `agent/trajectory.json`; None without one."""
    try:
        t = json.loads((trial / "agent" / "trajectory.json").read_text())
    except (OSError, ValueError):
        return None
    return scan(t) if isinstance(t, dict) else None


def t4(scans: list[dict[str, Any] | None]) -> dict[str, Any]:
    """An arm's T4 over its trials' scans (None: no trajectory)."""
    got = [s for s in scans if s is not None]
    n = len(got)
    bad = [s for s in got if s["tool_errors"]]
    mal, inv = Counter(), Counter()
    for s in got:
        mal.update(s["malformed"])
        inv.update(s["invented"])
    return {
        "trials": len(scans), "with_trajectory": n,
        "trials_with_tool_error": len(bad), "share_tool_error": len(bad) / n if n else None,
        "tool_errors": sum(s["tool_errors"] for s in got),
        "malformed": sum(mal.values()), "malformed_by_tool": dict(mal.most_common()),
        "trials_with_malformed": sum(1 for s in got if s["malformed"]),
        "share_malformed": sum(1 for s in got if s["malformed"]) / n if n else None,
        "invented": sum(inv.values()), "invented_by_tool": dict(inv.most_common()),
        "trials_with_invented": sum(1 for s in got if s["invented"]),
        "share_invented": sum(1 for s in got if s["invented"]) / n if n else None,
    }


def _mean(xs: list[float]) -> float | None:
    return sum(xs) / len(xs) if xs else None


def t5(arms: dict[str, list[list[dict[str, Any]]]]) -> dict[str, Any]:
    """T5 over `arms`: for each arm, its attempts per task in order (a list of
    trials per task, in the order `draft.py` pairs them: `[task][attempt]`), each a
    loaded trial (`efficiency.load_trial`). Only the (task, attempt) pairs every
    arm solved count."""
    keys = list(arms)
    index = {k: {t[0]["task"]: t for t in arms[k] if t} for k in keys}
    pairs = []
    tasks = sorted(set.intersection(*(set(index[k]) for k in keys))) if keys else []
    for x in tasks:
        for i in range(min(len(index[k][x]) for k in keys)):
            if all(index[k][x][i]["solved"] for k in keys):
                pairs.append((x, i + 1))
    out: dict[str, Any] = {"pairs": [{"task": x, "attempt": i} for x, i in pairs], "arms": {}}
    for k in keys:
        ts = [index[k][x][i - 1] for x, i in pairs]
        recs = [t["record"] for t in ts]
        costs = [r.get("cost_usd") for r in recs if r.get("cost_usd") is not None]
        calls = [r.get("model_calls") for r in recs if r.get("model_calls") is not None]
        secs = [t["wall_s"] for t in ts if t["wall_s"] is not None]
        both = [(r.get("model_calls"), (r.get("tokens") or {}).get("output")) for r in recs]
        both = [(c, o) for c, o in both if c and o is not None]
        out["arms"][k] = {
            "pairs": len(ts),
            "cost_mean": _mean(costs), "cost_median": statistics.median(costs) if costs else None,
            "calls_mean": _mean(calls), "agent_s_mean": _mean(secs),
            "output_per_call": sum(o for _, o in both) / sum(c for c, _ in both) if both else None,
        }
    return out


def t4_table(arms: dict[str, dict[str, Any]], labels: dict[str, str]) -> str:
    """T4 as a Markdown table, one column per arm."""
    keys = list(arms)
    pct = lambda x: "–" if x is None else f"{100 * x:.1f}%"  # noqa: E731

    def row(name: str, f) -> str:
        return f"| {name} | " + " | ".join(f(arms[k]) for k in keys) + " |\n"

    def by(d: dict[str, int]) -> str:
        return ", ".join(f"{t} {n}" for t, n in d.items()) or "none"

    t = "| | " + " | ".join(labels[k] for k in keys) + " |\n|---|" + "---|" * len(keys) + "\n"
    t += row("Trials with a trajectory", lambda a: f'{a["with_trajectory"]} of {a["trials"]}')
    t += row("Trials with at least one tool-level error",
             lambda a: f'{a["trials_with_tool_error"]} ({pct(a["share_tool_error"])})')
    t += row("Tool-level errors, in all", lambda a: str(a["tool_errors"]))
    t += row("Malformed inputs (trials, share)", lambda a: f'{a["malformed"]} ({a["trials_with_malformed"]}, '
                                                          f'{pct(a["share_malformed"])})')
    t += row("Malformed inputs, by tool", lambda a: by(a["malformed_by_tool"]))
    t += row("Invented names (trials, share)", lambda a: f'{a["invented"]} ({a["trials_with_invented"]}, '
                                                        f'{pct(a["share_invented"])})')
    t += row("Invented names, by tool", lambda a: by(a["invented_by_tool"]))
    return t


def t5_table(t: dict[str, Any], labels: dict[str, str]) -> str:
    """T5 as a Markdown table, one column per arm."""
    keys = list(t["arms"])
    num = lambda v, f: "–" if v is None else f.format(v)  # noqa: E731

    def row(name: str, f) -> str:
        return f"| {name} | " + " | ".join(f(t["arms"][k]) for k in keys) + " |\n"

    s = "| | " + " | ".join(labels[k] for k in keys) + " |\n|---|" + "---|" * len(keys) + "\n"
    s += row("Pairs every arm solved", lambda a: str(a["pairs"]))
    s += row("Dollars per solve, mean", lambda a: num(a["cost_mean"], "${:.3f}"))
    s += row("Dollars per solve, median", lambda a: num(a["cost_median"], "${:.3f}"))
    s += row("Model calls per solve, mean", lambda a: num(a["calls_mean"], "{:.1f}"))
    s += row("Agent seconds per solve, mean", lambda a: num(a["agent_s_mean"], "{:.0f}"))
    s += row("Output tokens per call", lambda a: num(a["output_per_call"], "{:.0f}"))
    return s
