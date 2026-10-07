"""What one benchmark trial spent, by one method for every arm
(theseus-7gir.12).

A trial's record, `agent/efficiency.json` and the trial's
`context.metadata["efficiency"]`:

- `tokens`: the four classes, `input` (uncached), `cache_read`,
  `cache_write`, and `output`, summed over every model the trial called;
- `by_model`: the same per model, with its `cost_usd` and `calls` (a retry
  is a call, and a refusal's fallback bills a second model);
- `cost_usd`, `model_calls`, `tool_calls`, and `spend_from` (which file
  said so);
- `wall_s`: the sampler's window (Harbor's own agent execution is in the
  trial's result.json, which the report reads);
- `harness`, `work`, and (Theseus's job wrappers) `wrappers`: each
  `cpu_s` and `peak_rss_kb` (the largest summed RSS in a sample), with
  `peak_hwm_kb` (the largest single process's peak); null when the sampler
  did not run;
- `container`: the cgroup's CPU over the window and its `memory.peak`;
- `sampler`: its status (`ok`, `unavailable`, `failed`, `running` when it
  never finished, `missing` when it left nothing), why, its interval, its
  samples, and its own CPU.

Harbor's `AgentContext` keeps three counters, `n_input_tokens` (input, cache
read and cache write), `n_cache_tokens` (the read), and `n_output_tokens`,
so the write is lost there: this record keeps all four.

Every function here is pure over an arm's files, so the tests read fixture
turns, histories, and session logs, and the report reads an old job's files
by the same method. Standard library only.
"""

from __future__ import annotations

import json
from datetime import datetime
from pathlib import Path
from typing import Any, Iterable

SCHEMA = "bench-efficiency/1"
RECORD = "efficiency.json"
SAMPLER_SUMMARY = "sampler.json"
CLASSES = ("input", "cache_read", "cache_write", "output")

# Each arm's harness, as the sampler sees it: the executable names
# (`/proc/<pid>/comm`), and the first arguments that mark a job wrapper. A
# new arm (OpenClaw's own runtime) needs only its line.
ARMS: dict[str, dict[str, tuple[str, ...]]] = {
    "theseus": {"names": ("theseus", "theseusd"), "wrapper_args": ("job-wrapper", "job-sandbox")},
    "claude-code": {"names": ("claude",), "wrapper_args": ()},
    "pi": {"names": ("pi",), "wrapper_args": ()},
}

# Anthropic's usage keys, by class.
ANTHROPIC = {
    "input": "input_tokens",
    "cache_read": "cache_read_input_tokens",
    "cache_write": "cache_creation_input_tokens",
    "output": "output_tokens",
}
# Claude Code's `result` event's `modelUsage` keys, by class.
CLAUDE_MODEL_USAGE = {
    "input": "inputTokens",
    "cache_read": "cacheReadInputTokens",
    "cache_write": "cacheCreationInputTokens",
    "output": "outputTokens",
}


def tokens(usage: dict[str, Any] | None, keys: dict[str, str] = ANTHROPIC) -> dict[str, int]:
    """The four classes of a usage object, by its own key names."""
    u = usage or {}
    return {c: int(u.get(k) or 0) for c, k in keys.items()}


def _add(a: dict[str, int], b: dict[str, int]) -> dict[str, int]:
    return {c: a.get(c, 0) + b.get(c, 0) for c in CLASSES}


def _model(by_model: dict[str, dict[str, Any]], name: str | None) -> dict[str, Any]:
    return by_model.setdefault(name or "unknown",
                               {**dict.fromkeys(CLASSES, 0), "cost_usd": 0.0, "calls": 0})


def _priced(costs: Iterable[float | None]) -> float | None:
    """A sum of costs, or None when any is unknown: a partial sum would
    understate the total."""
    total = 0.0
    for c in costs:
        if c is None:
            return None
        total += c
    return round(total, 6)


def _spend(by_model: dict[str, dict[str, Any]], cost: float | None, source: str | None,
           model_calls: int | None, tool_calls: int | None) -> dict[str, Any]:
    total = dict.fromkeys(CLASSES, 0)
    for m in by_model.values():
        total = _add(total, m)
    return {
        "tokens": total,
        "by_model": by_model,
        "cost_usd": cost,
        "spend_from": source,
        "model_calls": model_calls,
        "tool_calls": tool_calls,
    }


# ------------------------------------------------------------------ Theseus


def _provider_spans(span: dict[str, Any] | None) -> int:
    """The model calls a turn's trace holds: its `provider` spans, a retry's
    and a refusal's fallback each one of its own."""
    if not isinstance(span, dict):
        return 0
    own = 1 if span.get("kind") == "provider" else 0
    return own + sum(_provider_spans(c) for c in span.get("children") or [])


def theseus_spend(turn: dict[str, Any] | None, history: dict[str, Any] | None) -> dict[str, Any]:
    """A Theseus trial's spend, from `theseus --json ask`'s result (`turn`)
    and `theseus --json history`.

    The totals are the turn's, which counts every call it settled; each
    answer in the history carries its model, usage, and cost, so the totals
    split by model, and what the answers do not account for (a call a stop
    cut, a failed call, settled at the kernel's estimate) goes to the turn's
    model. A turn cut short printed no result: then the history's answers
    are the spend, as `theseus_atif.spend` sums them."""
    nodes = (history or {}).get("nodes") or []
    tid = (turn or {}).get("turn_id")
    answers = [n for n in nodes if n.get("kind") == "assistant_message"
               and (tid is None or n.get("turn_id") in (None, tid))]
    by_model: dict[str, dict[str, Any]] = {}
    costs: list[float | None] = []
    for n in answers:
        d = n.get("detail") or {}
        m = _model(by_model, d.get("model"))
        m.update(_add(m, tokens(d.get("usage"))))
        m["calls"] += 1
        c = d.get("cost_usd")
        costs.append(c)
        m["cost_usd"] = None if c is None or m["cost_usd"] is None else round(m["cost_usd"] + c, 6)
    if turn:
        total = tokens(turn.get("usage"))
        left = {c: total[c] - sum(m[c] for m in by_model.values()) for c in CLASSES}
        cost = turn.get("cost_usd")
        answered = _priced(costs)
        cost_left = None if cost is None or answered is None else round(cost - answered, 6)
        if any(v > 0 for v in left.values()) or (cost_left or 0) > 0:
            m = _model(by_model, turn.get("model"))
            m.update(_add(m, {c: max(v, 0) for c, v in left.items()}))
            if m["cost_usd"] is not None:
                m["cost_usd"] = None if cost_left is None else round(m["cost_usd"] + max(cost_left, 0), 6)
        # The trace counts every call; without one, the answers, else the loops.
        calls = _provider_spans(turn.get("trace")) or len(answers) or int(turn.get("loops") or 0)
        tools = turn.get("tool_calls")
        if tools is None:
            tools = sum(1 for n in nodes if n.get("kind") == "tool_call")
        return _spend(by_model, cost, "turn", calls, int(tools))
    if history:
        tools = sum(1 for n in nodes if n.get("kind") == "tool_call")
        return _spend(by_model, _priced(costs) if costs else None, "history", len(answers), tools)
    return _spend({}, None, None, None, None)


# -------------------------------------------------------------- Claude Code


def result_event(stream: str | None) -> dict[str, Any] | None:
    """The last `{"type": "result", …}` line of Claude Code's stream-json
    output (Harbor tees it to `agent/claude-code.txt`), or None: a run that
    Harbor's timeout cancelled never printed one."""
    found = None
    for line in (stream or "").splitlines():
        line = line.strip()
        if not line.startswith("{"):
            continue
        try:
            event = json.loads(line)
        except json.JSONDecodeError:
            continue
        if isinstance(event, dict) and event.get("type") == "result":
            found = event
    return found


def session_messages(events: Iterable[dict[str, Any]]) -> dict[str, dict[str, Any]]:
    """Each model answer in Claude Code's session log, by message id, once.

    The log writes one line per content block, and each line repeats its
    message's usage (streaming updates it, so the last line's is the final
    one): a message is one call, with the last usage seen for it, its model,
    and its tool_use ids. Harbor's own converter does the same (its
    `last_usage_by_msg_id`, reported on the message's first step)."""
    out: dict[str, dict[str, Any]] = {}
    for i, e in enumerate(events):
        if e.get("type") != "assistant":
            continue
        msg = e.get("message")
        if not isinstance(msg, dict):
            continue
        mid = msg.get("id") or f"line-{i}"
        m = out.setdefault(mid, {"model": None, "usage": None, "tools": set()})
        m["model"] = msg.get("model") or m["model"]
        if msg.get("usage") is not None:
            m["usage"] = msg["usage"]
        for block in msg.get("content") or []:
            if isinstance(block, dict) and block.get("type") == "tool_use":
                m["tools"].add(block.get("id") or f"{mid}-{len(m['tools'])}")
    return out


def read_jsonl(paths: Iterable[Path]) -> list[dict[str, Any]]:
    events = []
    for p in paths:
        try:
            text = p.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue
        for line in text.splitlines():
            try:
                e = json.loads(line)
            except json.JSONDecodeError:
                continue
            if isinstance(e, dict):
                events.append(e)
    return events


def claude_code_spend(stream: str | None, events: list[dict[str, Any]],
                      trajectory: dict[str, Any] | None = None) -> dict[str, Any]:
    """A Claude Code trial's spend.

    Tokens and dollars come from the stream-json `result` event's
    `modelUsage` when the run printed one: it is Claude Code's own bill, per
    model, in all four classes, and it counts calls the session log does not
    hold (a background model's). Without it (a timeout) they come from the
    session log, each message once; without that, from Harbor's trajectory,
    whose steps keep the cache write in `metrics.extra`. Model and tool calls
    always come from the session log (a call is a message id), or the
    trajectory."""
    msgs = session_messages(events)
    steps = [s for s in (trajectory or {}).get("steps") or [] if s.get("source") == "agent"]
    if msgs:
        calls = len(msgs)
        tools = sum(len(m["tools"]) for m in msgs.values())
    else:
        calls = sum(1 for s in steps if s.get("metrics"))
        tools = sum(len(s.get("tool_calls") or []) for s in steps)
    per_model_calls: dict[str, int] = {}
    for m in msgs.values():
        per_model_calls[m["model"] or "unknown"] = per_model_calls.get(m["model"] or "unknown", 0) + 1

    result = result_event(stream)
    by_model: dict[str, dict[str, Any]] = {}
    if result and isinstance(result.get("modelUsage"), dict) and result["modelUsage"]:
        for name, u in result["modelUsage"].items():
            m = _model(by_model, name)
            m.update(tokens(u, CLAUDE_MODEL_USAGE))
            m["cost_usd"] = u.get("costUSD")
            m["calls"] = per_model_calls.get(name, 0)
        cost = result.get("total_cost_usd")
        if cost is None:
            cost = _priced(m["cost_usd"] for m in by_model.values())
        return _spend(by_model, cost, "result_event", calls, tools)
    if result and isinstance(result.get("usage"), dict):
        name = next((m["model"] for m in msgs.values() if m["model"]), None)
        m = _model(by_model, name)
        m.update(tokens(result["usage"]))
        m["cost_usd"] = result.get("total_cost_usd")
        m["calls"] = calls
        return _spend(by_model, result.get("total_cost_usd"), "result_event", calls, tools)
    final = (trajectory or {}).get("final_metrics") or {}
    if msgs:
        for msg in msgs.values():
            m = _model(by_model, msg["model"])
            m.update(_add(m, tokens(msg["usage"])))
            m["calls"] += 1
            m["cost_usd"] = None  # the log holds no prices
        return _spend(by_model, final.get("total_cost_usd"), "session_log", calls, tools)
    if steps:
        for s in steps:
            mt = s.get("metrics")
            if not mt:
                continue
            m = _model(by_model, s.get("model_name") or (trajectory.get("agent") or {}).get("model_name"))
            m.update(_add(m, trajectory_tokens(mt)))
            m["calls"] += 1
            c = mt.get("cost_usd")
            m["cost_usd"] = None if c is None or m["cost_usd"] is None else round(m["cost_usd"] + c, 6)
        return _spend(by_model, final.get("total_cost_usd"), "trajectory", calls, tools)
    return _spend({}, None, None, None, None)


def trajectory_tokens(metrics: dict[str, Any]) -> dict[str, int]:
    """An ATIF step's four classes: `prompt_tokens` is input, cache read and
    cache write together (Harbor's and Theseus's converters both write it
    so), `cached_tokens` the read, and `extra.cache_creation_input_tokens`
    the write."""
    read = int(metrics.get("cached_tokens") or 0)
    write = int((metrics.get("extra") or {}).get("cache_creation_input_tokens") or 0)
    prompt = int(metrics.get("prompt_tokens") or 0)
    return {"input": max(prompt - read - write, 0), "cache_read": read, "cache_write": write,
            "output": int(metrics.get("completion_tokens") or 0)}


def trajectory_spend(trajectory: dict[str, Any] | None) -> dict[str, Any]:
    """Any arm's spend from its ATIF trajectory alone: an old job's, where
    the trajectory is what Harbor kept beside its three counters."""
    return claude_code_spend(None, [], trajectory)


# ----------------------------------------------------------------------- Pi

# Pi's `Usage` keys (its message types), by class. Its `cost.total` is the
# dollars, priced from its own model catalog.
PI_USAGE = {"input": "input", "cache_read": "cacheRead", "cache_write": "cacheWrite", "output": "output"}


def pi_session_files(logs: Path) -> list[Path]:
    """Pi's session logs in a trial's agent directory: Harbor runs it with
    `--session-dir /logs/agent/pi/sessions`."""
    root = logs / "pi" / "sessions"
    return sorted(root.rglob("*.jsonl")) if root.is_dir() else []


def _pi_cost(usage: dict[str, Any] | None) -> float | None:
    total = ((usage or {}).get("cost") or {}).get("total")
    return None if total is None else float(total)


def pi_calls(entries: Iterable[dict[str, Any]]) -> list[dict[str, Any]]:
    """Every model call Pi's session log records, in order, each entry once
    (by its id): `kind`, `model`, `usage`, `tools` (its tool-call ids),
    `stop` and `error`.

    - `answer`: an assistant message, one request, its retries' failures
      included (a failed one is persisted with `stopReason: "error"`);
    - `compaction`, `branch_summary`, `usage`: a summary's call, or usage
      Pi bills outside the conversation (a cache warm), each one call, on
      the session's model at that point when the entry names none;
    - `nested`: a tool result's own `usage` (a tool's model work): its
      tokens and dollars, with no count of the calls behind them.

    Every entry carries its own usage and cost, written when the call
    settled, so a run cut short (a timeout) has the spend of every call
    that finished."""
    out: list[dict[str, Any]] = []
    seen: set[str] = set()
    current = None
    for e in entries:
        eid = e.get("id")
        if eid is not None:
            if eid in seen:
                continue
            seen.add(eid)
        t = e.get("type")
        if t == "model_change":
            current = e.get("modelId") or current
        elif t == "message":
            msg = e.get("message")
            if not isinstance(msg, dict):
                continue
            if msg.get("role") == "assistant":
                model = msg.get("responseModel") or msg.get("model")
                current = msg.get("model") or current
                tools = [b.get("id") for b in msg.get("content") or []
                         if isinstance(b, dict) and b.get("type") == "toolCall"]
                out.append({"kind": "answer", "model": model, "usage": msg.get("usage"),
                            "tools": tools, "stop": msg.get("stopReason"),
                            "error": msg.get("errorMessage")})
            elif msg.get("role") == "toolResult" and isinstance(msg.get("usage"), dict):
                out.append({"kind": "nested", "model": current, "usage": msg["usage"], "tools": [],
                            "stop": None, "error": None})
        elif t in ("compaction", "branch_summary", "usage") and isinstance(e.get("usage"), dict):
            out.append({"kind": t, "model": e.get("model") or current, "usage": e["usage"], "tools": [],
                        "stop": None, "error": None})
    return out


def pi_stream_entries(stream: str | None) -> list[dict[str, Any]]:
    """Pi's `--mode json` stream (Harbor tees it to `agent/pi.txt`, its
    `message_update` lines left out) as session entries: each `message_end`
    a message, and each finished `compaction_end` a compaction. The
    fallback when no session log was kept."""
    out: list[dict[str, Any]] = []
    for e in read_jsonl_text(stream):
        if e.get("type") == "message_end" and isinstance(e.get("message"), dict):
            out.append({"type": "message", "message": e["message"]})
        elif e.get("type") == "compaction_end" and isinstance(e.get("result"), dict):
            usage = e["result"].get("usage")
            if isinstance(usage, dict):
                out.append({"type": "compaction", "usage": usage})
    return out


def read_jsonl_text(text: str | None) -> list[dict[str, Any]]:
    events = []
    for line in (text or "").splitlines():
        line = line.strip()
        if not line.startswith("{"):
            continue
        try:
            e = json.loads(line)
        except json.JSONDecodeError:
            continue
        if isinstance(e, dict):
            events.append(e)
    return events


def pi_spend(entries: list[dict[str, Any]], stream: str | None = None,
             trajectory: dict[str, Any] | None = None) -> dict[str, Any]:
    """A Pi trial's spend: its session log's calls (`pi_calls`), each with
    its own usage and dollars; without a log, the stream's finished
    messages; without either, Harbor's trajectory. Model calls are the
    answers and the out-of-conversation calls; a tool's nested model work
    adds tokens and dollars but no call."""
    calls, source = pi_calls(entries), "session_log"
    if not calls:
        calls, source = pi_calls(pi_stream_entries(stream)), "stream"
    if not calls:
        if trajectory and trajectory.get("steps"):
            return trajectory_spend(trajectory)
        return _spend({}, None, None, None, None)
    by_model: dict[str, dict[str, Any]] = {}
    for c in calls:
        m = _model(by_model, c["model"])
        m.update(_add(m, tokens(c["usage"], PI_USAGE)))
        if c["kind"] != "nested":
            m["calls"] += 1
        cost = _pi_cost(c["usage"])
        m["cost_usd"] = None if cost is None or m["cost_usd"] is None else round(m["cost_usd"] + cost, 6)
    n = sum(1 for c in calls if c["kind"] != "nested")
    tools = sum(len(c["tools"]) for c in calls)
    return _spend(by_model, _priced(_pi_cost(c["usage"]) for c in calls), source, n, tools)


def pi_end(entries: list[dict[str, Any]]) -> dict[str, Any]:
    """How Pi's run ended, from its last answer: its `stopReason` (`stop`,
    `length`, `error`, `aborted`, …) and error. Pi's print mode exits 0 on a
    provider's error, so this is where a failed run says so. `answers` counts
    every answer; `turns` those that are not a failed request (`stopReason`
    `error`: Pi persists each failed request it retries as an answer), which
    is what Claude Code's `--max-turns` counts."""
    answers = [c for c in pi_calls(entries) if c["kind"] == "answer"]
    last = answers[-1] if answers else {}
    return {"stop_reason": last.get("stop"), "error": last.get("error"), "answers": len(answers),
            "turns": sum(1 for a in answers if a["stop"] != "error")}


def pi_limits(spend: dict[str, Any], answers: int | None, max_budget_usd: float | None,
              max_turns: int | None, turns: int | None = None) -> dict[str, Any]:
    """The other arms' caps, held against a Pi trial that had none: Pi has
    no spend cap and no turn cap, so the trial ran unbounded, and the record
    says whether it passed the caps the others ran under (`max_budget_usd`,
    against its dollars; `max_turns`, against its `turns`, its answers less
    the failed requests it retried, as Claude Code's `--max-turns` counts
    them; `answers` is every answer, a retry's failure included). None where
    a side is unknown."""
    cost = spend.get("cost_usd")
    return {
        "enforced": False,
        "max_budget_usd": max_budget_usd,
        "max_turns": max_turns,
        "answers": answers,
        "turns": turns,
        "over_budget": None if max_budget_usd is None or cost is None else cost > max_budget_usd,
        "over_turns": None if max_turns is None or turns is None else turns > max_turns,
    }


def pi_effort_ran(entries: list[dict[str, Any]]) -> dict[str, Any]:
    """The thinking level Pi's session log says ran, beside the one the trial
    asked for (`record["effort"]`): each `thinking_level_change`'s level in
    order, and the `providerThinkingLevel` its answers went out with."""
    changes: list[str] = []
    answers: set[str] = set()
    for e in entries:
        if e.get("type") == "thinking_level_change" and e.get("thinkingLevel") is not None:
            if not changes or changes[-1] != e["thinkingLevel"]:
                changes.append(e["thinkingLevel"])
        msg = e.get("message")
        if e.get("type") == "message" and isinstance(msg, dict) and msg.get("role") == "assistant":
            if msg.get("providerThinkingLevel") is not None:
                answers.add(str(msg["providerThinkingLevel"]))
    return {"changes": changes, "answers": sorted(answers)}


# ------------------------------------------------------------------ machine


def _cls(c: dict[str, Any] | None) -> dict[str, Any] | None:
    if not c:
        return None
    return {k: c.get(k) for k in ("cpu_s", "peak_rss_kb", "peak_hwm_kb", "processes")}


def machine(summary: dict[str, Any] | None) -> dict[str, Any]:
    """The `harness`, `work`, `wrappers`, `container`, and `sampler` parts
    of a record, from the sampler's summary (`sampler.json`).

    The work's CPU is the container's (its cgroup's, where that read) less
    the harness's, the wrappers', and the outside class's, so a command
    shorter than an interval still counts. The sampler's own CPU is not taken
    off again: it classes its own pid as outside, so the outside class holds
    it from its first sample (`sampler.cpu_s` is recorded beside, not
    subtracted). `cpu_s_sampled` is what the samples alone saw, and
    `cpu_from` says which `cpu_s` is."""
    if not summary:
        return {"harness": None, "work": None, "wrappers": None, "container": None,
                "sampler": {"status": "missing", "reason": f"no {SAMPLER_SUMMARY}"}}
    status = summary.get("status") or "failed"
    sampler = {
        "status": status,
        "reason": summary.get("reason"),
        "interval_ms": summary.get("interval_ms"),
        "samples": summary.get("samples"),
        "cpu_s": (summary.get("sampler") or {}).get("cpu_s"),
        "core_share": (summary.get("sampler") or {}).get("core_share"),
    }
    if status == "running" and not sampler["reason"]:
        sampler["reason"] = "the sampler never wrote its final summary (stopped from outside)"
    classes = summary.get("classes")
    if status not in ("ok", "running") or not classes:
        return {"harness": None, "work": None, "wrappers": None, "container": None,
                "sampler": sampler}
    harness, wrappers, work = (_cls(classes.get(c)) for c in ("harness", "wrapper", "work"))
    outside = classes.get("outside") or {}
    cg = summary.get("cgroup") or {}
    work = dict(work or {}, cpu_s_sampled=(work or {}).get("cpu_s"), cpu_from="samples")
    if cg.get("cpu_s") is not None:
        rest = (cg["cpu_s"] - (harness or {}).get("cpu_s", 0) - (wrappers or {}).get("cpu_s", 0)
                - outside.get("cpu_s", 0))
        work["cpu_s"] = round(max(rest, 0.0), 3)
        work["cpu_from"] = "cgroup"
    container = None
    if cg:
        container = {"cpu_s": cg.get("cpu_s"), "memory_peak_kb": cg.get("memory_peak_kb")}
    return {"harness": harness, "work": work, "wrappers": wrappers, "container": container,
            "sampler": sampler}


# ------------------------------------------------------------------- record


def record(arm: str, spend: dict[str, Any], summary: dict[str, Any] | None,
           wall_s: float | None = None) -> dict[str, Any]:
    """One trial's record: its spend, and what its harness and work cost.
    `wall_s` is the window when the sampler has none (a turn's own time)."""
    m = machine(summary)
    wall_from = None
    if summary and summary.get("wall_s") is not None and m["sampler"]["status"] in ("ok", "running"):
        wall_s, wall_from = summary["wall_s"], "sampler"
    elif wall_s is not None:
        wall_from = "agent"
    return {
        "schema": SCHEMA,
        "arm": arm,
        **spend,
        "wall_s": wall_s,
        "wall_from": wall_from,
        **m,
    }


def read_json(path: Path) -> Any:
    try:
        text = path.read_text().strip()
        return json.loads(text) if text else None
    except (OSError, json.JSONDecodeError):
        return None


def theseus_record(logs: Path, turn_file: str = "theseus-turn.json",
                   history_file: str = "theseus-history.json") -> dict[str, Any]:
    """A Theseus trial's record, from its agent directory."""
    turn = read_json(logs / turn_file)
    history = read_json(logs / history_file)
    elapsed = (turn or {}).get("elapsed_ms")
    return record("theseus", theseus_spend(turn if isinstance(turn, dict) else None,
                                           history if isinstance(history, dict) else None),
                  read_json(logs / SAMPLER_SUMMARY),
                  wall_s=None if elapsed is None else elapsed / 1000)


def claude_code_record(logs: Path) -> dict[str, Any]:
    """A Claude Code trial's record, from its agent directory: the stream
    (`claude-code.txt`), the session log (`sessions/projects/**/*.jsonl`,
    subagents' included), and Harbor's `trajectory.json`."""
    try:
        stream = (logs / "claude-code.txt").read_text(encoding="utf-8", errors="replace")
    except OSError:
        stream = None
    root = logs / "sessions" / "projects"
    files = sorted(root.rglob("*.jsonl")) if root.is_dir() else []
    trajectory = read_json(logs / "trajectory.json")
    result = result_event(stream)
    wall = None if not result or result.get("duration_ms") is None else result["duration_ms"] / 1000
    return record("claude-code",
                  claude_code_spend(stream, read_jsonl(files),
                                    trajectory if isinstance(trajectory, dict) else None),
                  read_json(logs / SAMPLER_SUMMARY), wall_s=wall)


def _iso(s: Any) -> datetime | None:
    if not isinstance(s, str):
        return None
    try:
        return datetime.fromisoformat(s.replace("Z", "+00:00"))
    except ValueError:
        return None


def pi_wall(entries: list[dict[str, Any]]) -> float | None:
    """The span of Pi's session log, from its header to its last entry: the
    run's own time, when the sampler has none."""
    times = [t for t in (_iso(e.get("timestamp")) for e in entries) if t is not None]
    return round((max(times) - min(times)).total_seconds(), 3) if len(times) > 1 else None


def pi_record(logs: Path, max_budget_usd: float | None = None,
              max_turns: int | None = None) -> dict[str, Any]:
    """A Pi trial's record, from its agent directory: the session log
    (`pi/sessions/*.jsonl`), the stream (`pi.txt`), and the trajectory. It
    adds `end` (`pi_end`) and `limits` (`pi_limits`), the caps the other
    arms ran under, which Pi does not enforce."""
    entries = read_jsonl(pi_session_files(logs))
    try:
        stream = (logs / "pi.txt").read_text(encoding="utf-8", errors="replace")
    except OSError:
        stream = None
    trajectory = read_json(logs / "trajectory.json")
    spend = pi_spend(entries, stream, trajectory if isinstance(trajectory, dict) else None)
    end = pi_end(entries or pi_stream_entries(stream))
    rec = record("pi", spend, read_json(logs / SAMPLER_SUMMARY), wall_s=pi_wall(entries))
    rec["end"] = end
    counted = spend["spend_from"] in ("session_log", "stream")
    rec["limits"] = pi_limits(spend, end["answers"] if counted else None, max_budget_usd, max_turns,
                              end["turns"] if counted else None)
    rec["effort_ran"] = pi_effort_ran(entries) if entries else None
    return rec


def version_read(logs: Path, parse: Any = None) -> str | None:
    """The agent's version as its container read it (`measure.record_version`
    wrote `version.txt`), run through the adapter's own `parse_version`."""
    try:
        text = (logs / "version.txt").read_text(encoding="utf-8", errors="replace").strip()
    except OSError:
        return None
    if not text:
        return None
    try:
        return (parse(text) if parse else text) or None
    except Exception:  # noqa: BLE001
        return text


def stamp(logs: Path, rec: dict[str, Any], effort: str | None, version_asked: str | None = None,
          parse: Any = None) -> dict[str, Any]:
    """A trial's record with what its arm asked for and ran, the same keys on
    every arm: `effort`, the reasoning effort asked for (`measure.EFFORT`
    unless an `--ak` says another); `version`, the agent's version as the
    container read it, None when it could not; `version_asked`, the pin or the
    `--ak version=` the install was given, None for Harbor's own latest. A pin
    that did not take is a `version` that differs from `version_asked`."""
    rec["effort"] = effort
    rec["version"] = version_read(logs, parse)
    rec["version_asked"] = version_asked
    return rec


def write(logs: Path, rec: dict[str, Any]) -> Path:
    path = logs / RECORD
    path.write_text(json.dumps(rec, indent=2, sort_keys=True) + "\n")
    return path


def metadata(existing: dict[str, Any] | None, rec: dict[str, Any]) -> dict[str, Any]:
    """A context's metadata with the record in it, as `efficiency`."""
    return {**(existing or {}), "efficiency": rec}


# ------------------------------------------------------- the async bench's


# `ledger.tail` returns at most this many rows a read: a read that returns
# this many may have left older rows out.
LEDGER_CAP = 1000


def _tool_calls(history: dict[str, Any] | None) -> int:
    return sum(1 for n in (history or {}).get("nodes") or [] if n.get("kind") == "tool_call")


def ledger_spend(rows: Iterable[dict[str, Any]], history: dict[str, Any] | None = None,
                 task_histories: dict[str, dict[str, Any] | None] | None = None) -> dict[str, Any]:
    """A Theseus trial's spend from its daemon's ledger (theseus-z5ty,
    theseus-eq1a): each `provider.call` row (`theseus ledger -k
    provider.call`, every session's, a task's included) is one model call,
    with its `model`, its `usage` in Anthropic's four keys, and its
    `cost_usd`, the catalog's price of it. The calls are the rows; the
    dollars their sum, None when a row has no price.

    A call a `/stop` cut is a `provider.cut` row (`ledger -k provider.cut`):
    the kernel's estimate of its input and output tokens and its dollars,
    which it books as spent. Each is summed into the tokens and the dollars
    beside the calls, and counted apart: `cut_calls` (not in `model_calls`),
    and `cut_cost_usd`, the estimated dollars, apart from `billed_usd`, the
    priced calls'. `cost_usd` is the two together, what the trial spent.
    A failed call (a `provider.error` row) has neither usage nor cost, and
    stays out.

    Tool calls are no ledger kind: they are the `tool_call` nodes of
    `history` (the conversation) and of each task session's history in
    `task_histories` (by task id, None for one that could not be read).
    `tool_calls_from` says which: `conversation and tasks` when every task's
    history read (none, when the trial had no task), else `conversation`,
    the conversation's alone (and so when `task_histories` is None: the
    tasks were not read)."""
    by_model: dict[str, dict[str, Any]] = {}
    costs: list[float | None] = []
    cuts: list[float | None] = []
    for r in rows:
        kind = r.get("kind")
        d = r.get("data") or {}
        if kind == "provider.call":
            m = _model(by_model, d.get("model"))
            m.update(_add(m, tokens(d.get("usage"))))
            m["calls"] += 1
            c = d.get("cost_usd")
            costs.append(c)
        elif kind == "provider.cut":
            m = _model(by_model, d.get("model"))
            m.update(_add(m, {"input": int(d.get("input_tokens") or 0),
                              "output": int(d.get("output_tokens") or 0)}))
            c = d.get("cost_usd")
            cuts.append(c)
        else:
            continue
        m["cost_usd"] = None if c is None or m["cost_usd"] is None else round(m["cost_usd"] + c, 6)
    billed = _priced(costs) if costs else None
    cut = _priced(cuts) if cuts else None
    total = _priced(costs + cuts) if costs or cuts else None
    tools = _tool_calls(history)
    tools_from = "conversation"
    if task_histories is not None and all(h is not None for h in task_histories.values()):
        tools += sum(_tool_calls(h) for h in task_histories.values())
        tools_from = "conversation and tasks"
    return {**_spend(by_model, total, "ledger", len(costs), tools),
            "billed_usd": billed, "cut_calls": len(cuts), "cut_cost_usd": cut,
            "tool_calls_from": tools_from}


def task_ids(tasks: Any) -> list[str] | None:
    """The task sessions' ids in `theseus --json tasks`' answer, or None when
    it did not read."""
    if not isinstance(tasks, dict) or not isinstance(tasks.get("tasks"), list):
        return None
    return [t["task_id"] for t in tasks["tasks"] if isinstance(t, dict) and t.get("task_id")]


def task_history_file(task_id: str) -> str:
    """Where the finish leaves a task session's history (bench/async's
    `driver.Theseus.finish_script`)."""
    return f"theseus-history-{task_id}.json"


def theseus_ledger_record(logs: Path, calls_file: str = "theseus-calls.json",
                          history_file: str = "theseus-history.json",
                          wall_s: float | None = None, cuts_file: str = "theseus-cuts.json",
                          tasks_file: str = "theseus-tasks.json") -> dict[str, Any]:
    """The async bench's Theseus trial (bench/async/): its spend from the
    daemon's ledger (`ledger_spend` over `calls_file`'s and `cuts_file`'s
    rows, with the conversation's and the tasks' histories), the sampler's
    summary, and `wall_s`, the trial's own wall, when the sampler has none.
    It replaces the record `theseus_record` makes of the first ask's turn
    alone.

    `truncated` is true when a read returned `LEDGER_CAP` rows: the read is
    the newest that many, so older calls (the trial's first) may be missing
    from the record, and its numbers are a floor. Until the CLI reads the
    ledger in pages, such a trial's spend is not whole."""
    reads = [read_json(logs / f) for f in (calls_file, cuts_file)]
    pages = [r.get("rows") if isinstance(r, dict) else None for r in reads]
    history = read_json(logs / history_file)
    ids = task_ids(read_json(logs / tasks_file))
    tasks = None
    if ids is not None:
        tasks = {}
        for t in ids:
            h = read_json(logs / task_history_file(t))
            tasks[t] = h if isinstance(h, dict) and isinstance(h.get("nodes"), list) else None
    spend = ledger_spend([r for p in pages for r in (p or [])],
                         history if isinstance(history, dict) else None, tasks)
    rec = record("theseus", spend, read_json(logs / SAMPLER_SUMMARY), wall_s=wall_s)
    rec["truncated"] = any(len(p or []) >= LEDGER_CAP for p in pages)
    return rec


def result_events(stream: str | None) -> list[dict[str, Any]]:
    """Every `{"type": "result", …}` line of Claude Code's stream, in order.
    In stream-json input the CLI writes one per turn; a message taken
    mid-turn joins the running turn and adds none."""
    out = []
    for line in (stream or "").splitlines():
        one = result_event(line)
        if one is not None:
            out.append(one)
    return out


def _summed_results(results: list[dict[str, Any]]) -> dict[str, Any]:
    """One result whose `modelUsage` and `total_cost_usd` are the sums of
    `results`': the bill when each result is its own turn's."""
    usage: dict[str, dict[str, Any]] = {}
    for r in results:
        for name, u in (r.get("modelUsage") or {}).items():
            m = usage.setdefault(name, {})
            for k in (*CLAUDE_MODEL_USAGE.values(), "costUSD"):
                if u.get(k) is not None:
                    m[k] = round((m.get(k) or 0) + u[k], 6)
    costs = [r.get("total_cost_usd") for r in results]
    return {"type": "result", "modelUsage": usage,
            "total_cost_usd": None if any(c is None for c in costs) else round(sum(costs), 6)}


def claude_code_async_record(logs: Path, per_turn: bool = False) -> dict[str, Any]:
    """The async bench's Claude Code trial (bench/async/): the measured
    arm's record (`claude_code_record`), with `result_events`, the results
    its stream holds (one per turn; the driver's second message adds one
    when the CLI answers it as a turn of its own).

    Each result's `modelUsage` and `total_cost_usd` are read as the
    session's so far, so the last result is the trial's bill: Claude Code
    keeps its cost and model usage for the process, not for a turn. If a
    live two-message trial shows them per turn, `per_turn=True` sums every
    result's instead (`_summed_results`)."""
    rec = claude_code_record(logs)
    try:
        stream = (logs / "claude-code.txt").read_text(encoding="utf-8", errors="replace")
    except OSError:
        stream = None
    results = result_events(stream)
    if per_turn and len(results) > 1:
        root = logs / "sessions" / "projects"
        files = sorted(root.rglob("*.jsonl")) if root.is_dir() else []
        trajectory = read_json(logs / "trajectory.json")
        spend = claude_code_spend(json.dumps(_summed_results(results)), read_jsonl(files),
                                  trajectory if isinstance(trajectory, dict) else None)
        rec.update(spend)
    rec["result_events"] = len(results)
    rec["result_reading"] = "per_turn" if per_turn else "session"
    return rec
