#!/usr/bin/env python3
"""The async bench's scorer (theseus-7gir.16), standard library only.

    python3 bench/async/score.py jobs/async-theseus jobs/async-claude --out /tmp/async

Reads each trial of each Harbor job directory: its `result.json`, the
verifier's copy of the ledger (`verifier/ledger.jsonl`) and its problems,
the agent's ATIF `trajectory.json`, the driver's report
(`agent/async-driver.json`), Theseus's model calls
(`agent/theseus-calls.json`), and, when there is one, bench-efficiency's
record (`agent/efficiency.json`, else result.json's `metadata["efficiency"]`),
which it reads and never requires. Writes
`scores.json` (a row per trial) and `report.md` (a row per arm and family):

- **success**: the verifier's reward;
- **wall / ideal**: the agent's wall over the ideal the ledger's drawn
  durations give (the longest step, the slow job, the injection's time for
  cancel, the service's bound for contention);
- **the wait tax**: the model calls, and their tokens, made inside the slow
  job's window (its first start to its last end, by the ledger's wall clock);
- **responsiveness**: the injection to the right answer's ledger line
  (ticket-count's end for interrupt, the migration's first stop for cancel);
- **orphans and duplicated effects**: steps that started and never ended,
  the migration's processes still alive at the check, and effects done twice;
- **harness CPU, its peak RSS, and work CPU**: from bench-efficiency's
  record (efficiency.json, else result.json's `metadata["efficiency"]`),
  when its sampler ran;
- **overlap** (theseus-2wxa): the ledger's sum of slow-step time over the
  slow phase's wall (the first slow start to the last slow end): 1 is one
  step at a time, N is N at once;
- **calls per response, and the share of responses with more than one
  call**: over the trajectory's agent steps that call a tool (each step one
  model response, in every arm's ATIF: Theseus's, Pi's from its converter,
  Claude Code's and OpenCode's from Harbor's), else OpenCode's own stream;
- **order violations**: in a family with order rules (`layer2.DEPS`: the
  controls, link and bisect), each dependent step that started before its
  prerequisite's end.

A trial whose injection could not reach its agent mid-run is "not
measurable", never a failure.
"""

from __future__ import annotations

import argparse
import json
import statistics
import sys
from collections import Counter
from datetime import datetime
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent / "tools"))
sys.path.insert(0, str(Path(__file__).resolve().parent))

import asyncbench as ab  # noqa: E402
import families as l2f  # noqa: E402
import layer2 as l2  # noqa: E402

NOT_MEASURABLE = "not measurable"
INJECTED = {"interrupt", "cancel"}
# The slow job each family's wait tax is measured over, by its tool.
SLOW = {"parallel": "digest", "wait-tax": "build-index", "interrupt": "train-model",
        "fanout": "ingest", "cancel": "migrate", "contention": "deposit"}


def slow_tools(family: str) -> set[str]:
    """The tools whose steps are the family's slow work: Layer 1's one slow
    job, Layer 2's every tool (theseus-2wxa)."""
    if family in l2f.BY_NAME:
        return set(l2f.BY_NAME[family].tools)
    return {SLOW[family]} if family in SLOW else set()


def _json(path: Path) -> Any:
    try:
        return json.loads(path.read_text())
    except (OSError, json.JSONDecodeError):
        return None


def _ts(s: str | None) -> float | None:
    if not s:
        return None
    try:
        return datetime.fromisoformat(s.replace("Z", "+00:00")).timestamp()
    except ValueError:
        return None


def trials(job_dirs: list[Path]) -> list[Path]:
    """Each trial directory: one that holds a result.json and a verifier or
    agent directory (the job's own result.json has neither)."""
    out = []
    for job in job_dirs:
        for d in sorted(p for p in job.iterdir() if p.is_dir()):
            if (d / "result.json").exists() and ((d / "agent").is_dir() or (d / "verifier").is_dir()):
                out.append(d)
    return out


# ---------------------------------------------------------------- the ledger's numbers


def ideal(family: str, rec: list[dict[str, Any]], agent_start: float | None) -> float | None:
    """The family's ideal wall, in seconds, from the ledger's drawn durations."""
    starts = [r for r in rec if r["kind"] == "start"]
    if family == "parallel":
        d = [r["duration"] for r in starts if r["tool"] == "digest"]
        return max(d) if d else None
    if family == "wait-tax":
        d = [r["duration"] for r in starts if r["tool"] == "build-index"]
        return d[0] if d else None
    if family == "interrupt":
        d = [r["duration"] for r in starts if r["tool"] == "train-model"]
        return d[0] if d else None
    if family == "fanout":
        # Each part's attempts in turn (a transient failure lasts half its
        # draw), the parts side by side.
        per: dict[str, float] = {}
        fails = {(r["step"], r["pid"]) for r in rec if r["kind"] == "fail" and r["tool"] == "ingest"}
        for r in starts:
            if r["tool"] == "ingest":
                per[r["step"]] = per.get(r["step"], 0.0) + r["duration"] * (
                    0.5 if (r["step"], r["pid"]) in fails else 1.0)
        return max(per.values()) if per else None
    if family == "cancel":
        inj = [r for r in rec if r["kind"] == "inject"]
        if inj and agent_start is not None:
            return max(0.0, inj[0]["wall"] - agent_start)
        return None
    if family == "contention":
        d = [r for r in starts if r["tool"] == "deposit"]
        if not d:
            return None
        per: dict[str, float] = {}
        for r in d:
            per[r["step"]] = per.get(r["step"], 0.0) + r["duration"]
        return max(sum(r["duration"] for r in d) / ab.DEPOSITS_AT_ONCE, max(per.values()))
    if family in l2.CHECKS:
        return l2.ideal(family, rec)
    return None


def window(family: str, rec: list[dict[str, Any]]) -> tuple[float, float] | None:
    """The slow job's window, by the ledger's wall clock: its first start to
    its last end (or stop, or fail); for cancel, the migration's start to
    the injection, the time it ran before it was asked to stop."""
    tools = slow_tools(family)
    starts = [r["wall"] for r in rec if r["kind"] == "start" and r["tool"] in tools]
    if not starts:
        return None
    if family == "cancel":
        inj = [r["wall"] for r in rec if r["kind"] == "inject"]
        ends = inj[:1]
    else:
        ends = [r["wall"] for r in rec if r["kind"] in ("end", "fail", "stopped") and r["tool"] in tools]
    if not ends:
        return None
    return min(starts), max(ends)


def responsiveness(family: str, rec: list[dict[str, Any]]) -> float | None:
    """Seconds from the injection to the right answer's ledger line."""
    inj = [r for r in rec if r["kind"] == "inject"]
    if not inj:
        return None
    t0 = inj[0]["mono"]
    if family == "interrupt":
        want = [r for r in rec if r["kind"] == "end" and r["tool"] == "ticket-count" and r["mono"] >= t0]
    elif family == "cancel":
        want = [r for r in rec if r["kind"] == "stopped" and r["tool"] == "migrate" and r["mono"] >= t0]
    else:
        return None
    return round(want[0]["mono"] - t0, 3) if want else None


def unfinished(rec: list[dict[str, Any]]) -> int:
    """Steps that started and have no end, fail, or stop by their process:
    killed, or still running at the check."""
    done = {(r["step"], r["pid"]) for r in rec if r["kind"] in ("end", "fail", "stopped")}
    return sum(1 for r in rec if r["kind"] == "start" and (r["step"], r["pid"]) not in done)


def duplicates(rec: list[dict[str, Any]]) -> int:
    """Effects done more than once: the same part ingested, the same payment
    posted, the migration's effect at all twice."""
    keys = Counter()
    for r in rec:
        if r["kind"] == "effect":
            keys[(r["tool"], r["step"], r.get("amount"))] += 1
    return sum(n - 1 for n in keys.values() if n > 1)


# ---------------------------------------------------------------- the model's calls


def calls(trial: Path) -> list[dict[str, Any]]:
    """Every model call of the trial, each with its wall time and tokens:
    Theseus's from its daemon's ledger (tasks' sessions included), else the
    ATIF trajectory's agent steps with metrics."""
    rows = (_json(trial / "agent/theseus-calls.json") or {}).get("rows") or []
    out = []
    for r in rows:
        if r.get("kind") != "provider.call":
            continue
        u = (r.get("data") or {}).get("usage") or {}
        out.append({"at": r["at_unix_ms"] / 1000,
                    "tokens": sum(u.get(k) or 0 for k in ("input_tokens", "output_tokens",
                                                          "cache_read_input_tokens",
                                                          "cache_creation_input_tokens"))})
    if out:
        return out
    traj = _json(trial / "agent/trajectory.json") or {}
    for s in traj.get("steps", []):
        m = s.get("metrics")
        at = _ts(s.get("timestamp"))
        if s.get("source") == "agent" and m and at is not None:
            out.append({"at": at, "tokens": (m.get("prompt_tokens") or 0) + (m.get("completion_tokens") or 0)})
    return out


def wait_tax(family: str, rec: list[dict[str, Any]], model_calls: list[dict[str, Any]]) -> dict[str, Any] | None:
    w = window(family, rec)
    if w is None:
        return None
    inside = [c for c in model_calls if w[0] <= c["at"] <= w[1]]
    return {"calls": len(inside), "tokens": sum(c["tokens"] for c in inside),
            "window_s": round(w[1] - w[0], 3)}


def responses(trial: Path) -> list[int] | None:
    """Each model response's tool calls, in order: the ATIF trajectory's
    agent steps (one step a response in every arm's converter), with
    Theseus's task sessions' answers after them (`theseus-history-<task>.json`:
    its trajectory is the conversation's, as Claude Code's holds its
    subagents' sidechains); else OpenCode's own stream (`agent/opencode.txt`:
    the `tool_use` events between a `step_start` and its `step_finish`).
    None when the trial left neither."""
    traj = _json(trial / "agent/trajectory.json")
    steps = [s for s in (traj or {}).get("steps") or [] if isinstance(s, dict) and s.get("source") == "agent"]
    if steps:
        out = [len(s.get("tool_calls") or []) for s in steps]
        for h in sorted((trial / "agent").glob("theseus-history-*.json")):
            out += [len(((n.get("detail") or {}).get("tool_calls")) or [])
                    for n in (_json(h) or {}).get("nodes") or [] if n.get("kind") == "assistant_message"]
        return out
    try:
        text = (trial / "agent/opencode.txt").read_text(encoding="utf-8", errors="replace")
    except OSError:
        return None
    out: list[int] = []
    n = None
    for line in text.splitlines():
        try:
            e = json.loads(line)
        except json.JSONDecodeError:
            continue
        kind = e.get("type") if isinstance(e, dict) else None
        if kind == "step_start":
            n = 0
        elif kind == "tool_use" and n is not None:
            n += 1
        elif kind == "step_finish" and n is not None:
            out.append(n)
            n = None
    return out or None


def batching(counts: list[int] | None) -> dict[str, Any]:
    """Calls per response over the responses that call a tool, and the share
    of them with more than one call; a final answer, which calls none, is
    neither."""
    tooled = [n for n in counts or [] if n > 0]
    return {"responses": len(counts) if counts is not None else None,
            "tool_responses": len(tooled) if counts is not None else None,
            "tool_calls": sum(tooled) if counts is not None else None,
            "calls_per_response": round(sum(tooled) / len(tooled), 3) if tooled else None,
            "multi_call_responses": sum(1 for n in tooled if n > 1) if counts is not None else None,
            "multi_call_share": round(sum(1 for n in tooled if n > 1) / len(tooled), 3) if tooled else None,
            "max_calls": max(tooled) if tooled else None}


# bench-efficiency's record (bench/harbor/efficiency.py's SCHEMA).
EFFICIENCY_SCHEMA = "bench-efficiency/1"
# A sampler status whose classes are numbers (bench/report's rule): `running`
# never wrote its last summary, but its samples stand.
SAMPLED = ("ok", "running")


def efficiency(trial: Path, result: dict[str, Any] | None = None) -> dict[str, Any]:
    """The harness's CPU seconds and peak RSS (MB), and its work's CPU
    seconds, from bench-efficiency's record: `agent/efficiency.json`, else
    result.json's `metadata["efficiency"]`. Each is None unless the record's
    sampler ran (`ok` or `running`): an unsampled trial has no numbers, not
    zeros."""
    none = {"harness_cpu_s": None, "harness_peak_rss_mb": None, "work_cpu_s": None}
    e = _json(trial / "agent/efficiency.json")
    if not isinstance(e, dict) or e.get("schema") != EFFICIENCY_SCHEMA:
        meta = ((result or {}).get("agent_result") or {}).get("metadata") or {}
        e = meta.get("efficiency")
    if not isinstance(e, dict) or e.get("schema") != EFFICIENCY_SCHEMA:
        return none
    harness = e.get("harness")
    if (e.get("sampler") or {}).get("status") not in SAMPLED or not isinstance(harness, dict):
        return none
    work = e.get("work") or {}

    def num(v: Any) -> float | None:
        return v if isinstance(v, (int, float)) and not isinstance(v, bool) else None

    rss = num(harness.get("peak_rss_kb"))
    return {"harness_cpu_s": num(harness.get("cpu_s")),
            "harness_peak_rss_mb": round(rss / 1024, 3) if rss is not None else None,
            "work_cpu_s": num(work.get("cpu_s"))}


# ---------------------------------------------------------------- a trial


def score(trial: Path, arm: str | None = None) -> dict[str, Any]:
    """One trial's row; `arm` names its arm where Harbor's agent name does
    not tell arms apart (the three Theseus arms are all `theseus-async`)."""
    result = _json(trial / "result.json") or {}
    rec = ab.read(trial / "verifier/ledger.jsonl")
    problems = _json(trial / "verifier/problems.json") or {}
    report = _json(trial / "agent/async-driver.json") or {}
    family = problems.get("family") or report.get("family") or result.get("task_name", "")
    arm = arm or (result.get("agent_info") or {}).get("name") or "unknown"
    ex = result.get("agent_execution") or {}
    t0, t1 = _ts(ex.get("started_at")), _ts(ex.get("finished_at"))
    wall = round(t1 - t0, 3) if t0 is not None and t1 is not None else None
    rewards = ((result.get("verifier_result") or {}).get("rewards") or {})
    reward = rewards.get("reward")
    inj = report.get("injection")
    measurable = family not in INJECTED or arm == "oracle" or bool(inj and not inj.get("error"))
    best = ideal(family, rec, t0)
    agent = result.get("agent_result") or {}
    alive = sum(1 for p in problems.get("problems", []) if "still run" in p)
    ov = l2.overlap(rec, slow_tools(family))
    model_calls = calls(trial)
    return {
        "trial": trial.name,
        "job": trial.parent.name,
        "arm": arm,
        "family": family,
        "measurable": measurable,
        "success": reward if measurable else NOT_MEASURABLE,
        "ledger_ok": not ab.verify_chain(rec) if rec else None,
        "wall_s": wall,
        "ideal_s": round(best, 3) if best is not None else None,
        "over_ideal": round(wall / best, 3) if wall and best else None,
        "wait_tax": wait_tax(family, rec, model_calls),
        "responsiveness_s": (responsiveness(family, rec) if measurable else NOT_MEASURABLE),
        "trigger": (inj or {}).get("trigger"),
        "unfinished_steps": unfinished(rec),
        "orphans_alive": alive,
        "duplicated_effects": duplicates(rec),
        "cost_usd": agent.get("cost_usd"),
        "tokens": (agent.get("n_input_tokens") or 0) + (agent.get("n_output_tokens") or 0),
        "exception": (result.get("exception_info") or {}).get("exception_type"),
        **efficiency(trial, result),
        "layer": 2 if family in l2f.BY_NAME else 1,
        "shape": l2f.BY_NAME[family].shape if family in l2f.BY_NAME else None,
        "overlap": ov["overlap"] if ov else None,
        "slow_busy_s": ov["busy_s"] if ov else None,
        "slow_phase_s": ov["phase_s"] if ov else None,
        "model_calls": len(model_calls) or None,
        **batching(responses(trial)),
        "order_violations": len(l2.order_violations(family, rec)) if family in l2.DEPS else None,
    }


# ---------------------------------------------------------------- the report


def _median(xs: list[Any]) -> Any:
    xs = [x for x in xs if isinstance(x, (int, float)) and not isinstance(x, bool)]
    return round(statistics.median(xs), 3) if xs else None


def _cell(v: Any) -> str:
    if v is None:
        return "–"
    if isinstance(v, float):
        return f"{v:.3g}" if abs(v) < 1000 else f"{v:.0f}"
    return str(v)


def rows(scores: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """One row per arm and family: medians over its trials."""
    groups: dict[tuple[str, str], list[dict[str, Any]]] = {}
    for s in scores:
        groups.setdefault((s["arm"], s["family"]), []).append(s)
    out = []
    for (arm, family), g in sorted(groups.items()):
        m = [s for s in g if s["measurable"]]
        taxes = [s["wait_tax"] for s in m if s["wait_tax"]]
        out.append({
            "arm": arm,
            "family": family,
            "trials": len(g),
            "success": (f"{sum(1 for s in m if s['success'] == 1)}/{len(m)}" if m else NOT_MEASURABLE),
            "over_ideal": _median([s["over_ideal"] for s in m]),
            "wall_s": _median([s["wall_s"] for s in m]),
            "ideal_s": _median([s["ideal_s"] for s in m]),
            "wait_tax_calls": _median([t["calls"] for t in taxes]),
            "wait_tax_tokens": _median([t["tokens"] for t in taxes]),
            "responsiveness_s": (_median([s["responsiveness_s"] for s in m]) if m else NOT_MEASURABLE)
            if family in INJECTED else None,
            "orphans": sum(s["unfinished_steps"] + s["orphans_alive"] for s in m),
            "duplicated_effects": sum(s["duplicated_effects"] for s in m),
            "harness_cpu_s": _median([s["harness_cpu_s"] for s in m]),
            "harness_peak_rss_mb": _median([s["harness_peak_rss_mb"] for s in m]),
            "work_cpu_s": _median([s["work_cpu_s"] for s in m]),
            "cost_usd": _median([s["cost_usd"] for s in m]),
            "overlap": _median([s["overlap"] for s in m]),
            **_pooled(m),
            "order_violations": (sum(s["order_violations"] or 0 for s in m)
                                 if family in l2.DEPS else None),
        })
    return out


def _pooled(trials: list[dict[str, Any]]) -> dict[str, Any]:
    """Calls per response and the multi-call share, pooled over the trials'
    tool-calling responses (a trial with many responses weighs more)."""
    tooled = sum(s.get("tool_responses") or 0 for s in trials)
    calls_ = sum(s.get("tool_calls") or 0 for s in trials)
    multi = sum(s.get("multi_call_responses") or 0 for s in trials)
    return {"calls_per_response": round(calls_ / tooled, 3) if tooled else None,
            "multi_call_share": round(multi / tooled, 3) if tooled else None}


COLUMNS = [
    ("arm", "Arm"), ("family", "Family"), ("trials", "Trials"), ("success", "Success"),
    ("over_ideal", "Wall / ideal"), ("wall_s", "Wall s"), ("ideal_s", "Ideal s"),
    ("wait_tax_calls", "Wait-tax calls"), ("wait_tax_tokens", "Wait-tax tokens"),
    ("responsiveness_s", "Responsiveness s"), ("orphans", "Orphans"),
    ("duplicated_effects", "Dup. effects"), ("harness_cpu_s", "Harness CPU s"),
    ("harness_peak_rss_mb", "Harness peak RSS MB"), ("work_cpu_s", "Work CPU s"),
    ("cost_usd", "Cost $"), ("overlap", "Overlap"), ("calls_per_response", "Calls / response"),
    ("multi_call_share", "Multi-call share"), ("order_violations", "Order violations"),
]


def markdown(table: list[dict[str, Any]], jobs: list[str]) -> str:
    lines = [
        "# The async bench",
        "",
        f"Jobs: {', '.join(jobs)}. Medians over each arm's trials of a family; orphans and duplicated "
        "effects are totals. \"Not measurable\": the arm's CLI could not be reached mid-run, so the "
        "injection never arrived. Harness CPU, its peak RSS, and the work's CPU come from bench-efficiency's "
        "record, from trials whose sampler ran; \"–\" where none did.",
        "",
        "| " + " | ".join(h for _, h in COLUMNS) + " |",
        "|" + "|".join("---" for _ in COLUMNS) + "|",
    ]
    for r in table:
        lines.append("| " + " | ".join(_cell(r[k]) for k, _ in COLUMNS) + " |")
    return "\n".join(lines) + "\n"


def labelled(jobs: list[str]) -> list[tuple[str | None, Path]]:
    """Job arguments, each a directory or `LABEL=DIR` (theseus-2wxa): the
    label names the arm of every trial in it."""
    out = []
    for j in jobs:
        label, sep, path = j.partition("=")
        out.append((label, Path(path)) if sep and not Path(j).exists() else (None, Path(j)))
    return out


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("jobs", nargs="+", help="a job directory, or LABEL=DIR to name its arm")
    ap.add_argument("--out", type=Path, required=True)
    a = ap.parse_args(argv)
    jobs = labelled(a.jobs)
    scores = [score(t, label) for label, job in jobs for t in trials([job])]
    a.out.mkdir(parents=True, exist_ok=True)
    (a.out / "scores.json").write_text(json.dumps(scores, indent=2))
    table = rows(scores)
    (a.out / "report.md").write_text(markdown(table, [j.name for _, j in jobs]))
    print(f"{len(scores)} trials, {len(table)} rows: {a.out / 'report.md'}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
