"""A Theseus session's history as an ATIF trajectory (theseus-n88g.3).

`theseus --json history` prints a session's nodes in order: the user's
message, each model answer (its text, its tool calls, its usage and cost),
each call's gate decision, and each call's result. Harbor reads a trial's
`agent/trajectory.json` in ATIF, the Agent Trajectory Interchange Format, for
its viewer and its per-model usage. `trajectory` turns the one into the
other with the standard library only, so the tests run without Harbor.

The steps: the user's message first, then one agent step per model answer,
with its text, its tool calls (each with the gate's decision in `extra`),
its tokens and dollars, and the results of its calls as its observation. A
result whose call is not in the history, and any other node with text (a
wake, a task's report), is a system step of its own. A model call with no
answer (one a stop cut, or one that failed) is in the turn's result but in
no node: a last agent step, marked `unanswered`, holds its tokens and cost,
so the trajectory's totals are the turn's.

Every model answer's node carries its own cost, written in the frame that
settled the call, so a trial cut off before its turn returned (the process
killed) still has the spend of every call that finished: `spend` sums them.
"""

from __future__ import annotations

from datetime import datetime, timezone
from typing import Any

SCHEMA_VERSION = "ATIF-v1.7"


def trajectory(
    history: dict[str, Any],
    *,
    version: str,
    model_name: str | None = None,
    turn: dict[str, Any] | None = None,
) -> dict[str, Any]:
    """The ATIF trajectory of `history` (`theseus --json history`), as a
    JSON-ready dict. `turn` is `theseus --json ask`'s result, when the turn
    returned one: its stop reason, loops, and tool calls go in `extra`."""
    session = history.get("session") or {}
    steps: list[dict[str, Any]] = []
    calls: dict[str, dict[str, Any]] = {}
    callers: dict[str, dict[str, Any]] = {}
    for node in history.get("nodes") or []:
        kind = node.get("kind")
        detail = node.get("detail") or {}
        text = node.get("text") or ""
        if kind == "user_message":
            steps.append(_step("user", text, node))
        elif kind == "assistant_message":
            step = _answer(node, detail, text)
            for call in step.get("tool_calls", []):
                calls[call["tool_call_id"]] = call
                callers[call["tool_call_id"]] = step
            steps.append(step)
        elif kind == "tool_call":
            call = calls.get(detail.get("tool_use_id"))
            if call is not None:
                decision = detail.get("decision") or {}
                gate = {
                    "tool": detail.get("tool"),
                    "posture": decision.get("posture"),
                    "reason": decision.get("reason"),
                }
                call["extra"] = {k: v for k, v in gate.items() if v is not None}
        elif kind == "tool_result":
            call_id = detail.get("tool_use_id")
            result: dict[str, Any] = {"source_call_id": call_id, "content": text}
            kept = ("tool", "status", "is_error", "duration_ms", "truncated", "late")
            extra = {k: detail[k] for k in kept if detail.get(k) is not None}
            if extra:
                result["extra"] = extra
            step = callers.get(call_id)
            if step is None:
                # ATIF ties a result's source_call_id to a call in its own
                # step: this one's call is in no step, so its id is extra.
                tool = detail.get("tool") or "a call"
                step = _step("system", f"the result of {tool} from an earlier turn", node)
                steps.append(step)
                del result["source_call_id"]
                result.setdefault("extra", {})["tool_use_id"] = call_id
            step.setdefault("observation", {"results": []})["results"].append(result)
        elif text:
            steps.append(_step("system", text, node))
    unanswered = _unanswered(history, turn) if turn else None
    if unanswered:
        steps.append(unanswered)
    if not steps:
        # ATIF needs one step; a session with none has no user message yet.
        steps.append({"step_id": 1, "source": "system", "message": "the session has no history"})
    for i, step in enumerate(steps, start=1):
        step["step_id"] = i
    out: dict[str, Any] = {
        "schema_version": SCHEMA_VERSION,
        "session_id": session.get("session_id") or "unknown",
        "agent": {"name": "theseus", "version": version or "unknown"},
        "steps": steps,
        "final_metrics": _final(steps),
    }
    if model_name:
        out["agent"]["model_name"] = model_name
    kept = ("turn_id", "stop_reason", "loops", "tool_calls", "elapsed_ms")
    extra = {k: turn[k] for k in kept if turn and turn.get(k) is not None}
    if extra:
        out["extra"] = extra
    return out


USAGE_KEYS = (
    "input_tokens",
    "cache_read_input_tokens",
    "cache_creation_input_tokens",
    "output_tokens",
)


def _unanswered(history: dict[str, Any], turn: dict[str, Any]) -> dict[str, Any] | None:
    """What the turn's result counts that no answer in the history does: a
    model call that a stop cut (a timeout's), or one that failed, which the
    kernel settled at its own estimate. One agent step holds those tokens and
    that cost, so the trajectory's totals are the turn's, as Harbor's are."""
    tid = turn.get("turn_id")
    answers = [
        n.get("detail") or {}
        for n in history.get("nodes") or []
        if n.get("kind") == "assistant_message" and (tid is None or n.get("turn_id") == tid)
    ]
    usage = turn.get("usage") or {}
    left = {
        k: (usage.get(k) or 0) - sum((a.get("usage") or {}).get(k) or 0 for a in answers)
        for k in USAGE_KEYS
    }
    cost = turn.get("cost_usd")
    cost_left = None
    if cost is not None:
        cost_left = round(cost - sum(a.get("cost_usd") or 0 for a in answers), 6)
    if all(v <= 0 for v in left.values()) and not (cost_left and cost_left > 0):
        return None
    detail: dict[str, Any] = {"usage": {k: max(v, 0) for k, v in left.items()}}
    if cost_left is not None:
        detail["cost_usd"] = max(cost_left, 0.0)
    step: dict[str, Any] = {
        "step_id": 0,
        "source": "agent",
        "message": "",
        "metrics": _metrics(detail),
        "extra": {
            "unanswered": "model calls with no answer in the history (cut by a stop, or "
            "failed): their tokens and cost as the turn settled them"
        },
    }
    if turn.get("model"):
        step["model_name"] = turn["model"]
    return step


def spend(history: dict[str, Any]) -> dict[str, Any]:
    """What the session's model calls cost and used, summed over the answers
    in `history`: `cost_usd` (`None` when a call had no price), and the
    tokens as Harbor counts them (cache reads and writes are input, and the
    reads are also counted as cached)."""
    steps = [_answer(n, n.get("detail") or {}, "") for n in history.get("nodes") or []
             if n.get("kind") == "assistant_message"]
    final = _final(steps)
    return {
        "cost_usd": final.get("total_cost_usd"),
        "input_tokens": final["total_prompt_tokens"],
        "cache_tokens": final["total_cached_tokens"],
        "output_tokens": final["total_completion_tokens"],
        "calls": len(steps),
    }


def _step(source: str, message: str, node: dict[str, Any]) -> dict[str, Any]:
    step: dict[str, Any] = {"step_id": 0, "source": source, "message": message}
    ms = node.get("at_unix_ms")
    if isinstance(ms, (int, float)):
        when = datetime.fromtimestamp(ms / 1000, tz=timezone.utc)
        step["timestamp"] = when.isoformat(timespec="milliseconds").replace("+00:00", "Z")
    return step


def _answer(node: dict[str, Any], detail: dict[str, Any], text: str) -> dict[str, Any]:
    """One model answer's agent step: its text, tool calls, and metrics."""
    step = _step("agent", text, node)
    if detail.get("model"):
        step["model_name"] = detail["model"]
    tool_calls = []
    for c in detail.get("tool_calls") or []:
        args = c.get("input")
        tool_calls.append({
            "tool_call_id": c.get("id") or "",
            "function_name": c.get("name") or "",
            "arguments": args if isinstance(args, dict) else {"input": args},
        })
    if tool_calls:
        step["tool_calls"] = tool_calls
    metrics = _metrics(detail)
    if metrics:
        step["metrics"] = metrics
    kept = ("provider", "stop_reason", "request_id")
    extra = {k: detail[k] for k in kept if detail.get(k)}
    if node.get("loop_index") is not None:
        extra["loop"] = node["loop_index"]
    if extra:
        step["extra"] = extra
    return step


def _metrics(detail: dict[str, Any]) -> dict[str, Any] | None:
    usage = detail.get("usage") or {}
    if not usage and detail.get("cost_usd") is None:
        return None
    read = usage.get("cache_read_input_tokens") or 0
    written = usage.get("cache_creation_input_tokens") or 0
    metrics: dict[str, Any] = {
        "prompt_tokens": (usage.get("input_tokens") or 0) + read + written,
        "completion_tokens": usage.get("output_tokens") or 0,
        "cached_tokens": read,
    }
    if detail.get("cost_usd") is not None:
        metrics["cost_usd"] = detail["cost_usd"]
    if written:
        metrics["extra"] = {"cache_creation_input_tokens": written}
    return metrics


def _final(steps: list[dict[str, Any]]) -> dict[str, Any]:
    priced = [s["metrics"] for s in steps if s.get("metrics")]
    final: dict[str, Any] = {
        "total_prompt_tokens": sum(m["prompt_tokens"] for m in priced),
        "total_completion_tokens": sum(m["completion_tokens"] for m in priced),
        "total_cached_tokens": sum(m["cached_tokens"] for m in priced),
        "total_steps": len(steps),
    }
    # As Harbor sums usage: a total with an unpriced call in it is no total.
    if priced and all(m.get("cost_usd") is not None for m in priced):
        final["total_cost_usd"] = round(sum(m["cost_usd"] for m in priced), 6)
    return final
