"""A Pi session log as an ATIF trajectory (theseus-jp9p).

Pi writes each session as JSONL (its `session-format.md`, version 3): a
header, then entries in a tree by `id` and `parentId`. A `message` entry holds
one message: the system prompt and its tools (`system`), the user's text, a
model answer (`assistant`: its text, thinking, and `toolCall` blocks, its
model, `usage` with `cost.total`, and `stopReason`), and each call's result
(`toolResult`). A `compaction` or `branch_summary` entry is a summary, with
the usage of the call that wrote it.

Harbor reads a trial's `agent/trajectory.json` in ATIF for its viewer and its
per-model usage, and Harbor's own Pi agent writes none. `trajectory` turns the
log into one, with the standard library only, so the tests run without
Harbor:

- the user's message is a user step;
- each answer is an agent step, with its text, its thinking as
  `reasoning_content`, its tool calls, its tokens and dollars, and the
  results of its calls as its observation (a result whose call is in no
  step is a system step of its own);
- each summary (a compaction, a branch summary) or usage entry with a usage
  is an agent step of its own, its call's metrics, `extra.kind` naming it;
- a tool result's own usage (a tool's nested model work) is an agent step
  marked `extra.nested`, so the trajectory's totals are the session's;
- the system prompt is not a step: it is in the log, and it is Pi's.

Each entry is read once, by its id, in the file's order (the branch that ran;
`--print` makes no other).
"""

from __future__ import annotations

from typing import Any, Iterable

SCHEMA_VERSION = "ATIF-v1.7"


def _text(content: Any, kind: str = "text") -> str:
    if isinstance(content, str):
        return content if kind == "text" else ""
    out = []
    for b in content or []:
        if isinstance(b, dict) and b.get("type") == kind:
            out.append(b.get(kind) or "")
    return "\n".join(t for t in out if t)


def _metrics(usage: dict[str, Any] | None) -> dict[str, Any] | None:
    """An ATIF step's metrics from Pi's usage: `prompt_tokens` is input,
    cache read and cache write together, as Harbor's converters write it,
    `cached_tokens` the read, and the write in `extra`."""
    if not isinstance(usage, dict):
        return None
    read = int(usage.get("cacheRead") or 0)
    written = int(usage.get("cacheWrite") or 0)
    metrics: dict[str, Any] = {
        "prompt_tokens": int(usage.get("input") or 0) + read + written,
        "completion_tokens": int(usage.get("output") or 0),
        "cached_tokens": read,
    }
    cost = (usage.get("cost") or {}).get("total")
    if cost is not None:
        metrics["cost_usd"] = float(cost)
    if written:
        metrics["extra"] = {"cache_creation_input_tokens": written}
    return metrics


def _step(source: str, message: str, entry: dict[str, Any]) -> dict[str, Any]:
    step: dict[str, Any] = {"step_id": 0, "source": source, "message": message}
    if isinstance(entry.get("timestamp"), str):
        step["timestamp"] = entry["timestamp"]
    return step


def _answer(entry: dict[str, Any], msg: dict[str, Any]) -> dict[str, Any]:
    step = _step("agent", _text(msg.get("content")), entry)
    model = msg.get("responseModel") or msg.get("model")
    if model:
        step["model_name"] = model
    thinking = _text(msg.get("content"), "thinking")
    if thinking:
        step["reasoning_content"] = thinking
    calls = []
    for b in msg.get("content") or []:
        if isinstance(b, dict) and b.get("type") == "toolCall":
            args = b.get("arguments")
            calls.append({
                "tool_call_id": b.get("id") or "",
                "function_name": b.get("name") or "",
                "arguments": args if isinstance(args, dict) else {"input": args},
            })
    if calls:
        step["tool_calls"] = calls
    metrics = _metrics(msg.get("usage"))
    if metrics:
        step["metrics"] = metrics
    step["llm_call_count"] = 1
    extra = {k: msg[v] for k, v in (("stop_reason", "stopReason"), ("error", "errorMessage"),
                                     ("provider", "provider")) if msg.get(v)}
    if extra:
        step["extra"] = extra
    return step


def trajectory(entries: Iterable[dict[str, Any]], *, version: str,
               model_name: str | None = None) -> dict[str, Any]:
    """The ATIF trajectory of Pi's session log (its entries, in order), as a
    JSON-ready dict."""
    steps: list[dict[str, Any]] = []
    callers: dict[str, dict[str, Any]] = {}
    session_id = None
    seen: set[str] = set()
    current = model_name
    for e in entries:
        t = e.get("type")
        if t == "session":
            session_id = session_id or e.get("id")
            continue
        eid = e.get("id")
        if eid is not None:
            if eid in seen:
                continue
            seen.add(eid)
        if t == "model_change":
            current = e.get("modelId") or current
        elif t == "message":
            msg = e.get("message") or {}
            role = msg.get("role")
            if role == "user":
                steps.append(_step("user", _text(msg.get("content")), e))
            elif role == "assistant":
                step = _answer(e, msg)
                for c in step.get("tool_calls", []):
                    callers[c["tool_call_id"]] = step
                steps.append(step)
            elif role == "toolResult":
                cid = msg.get("toolCallId")
                result: dict[str, Any] = {"source_call_id": cid, "content": _text(msg.get("content"))}
                extra = {k: msg[k] for k in ("toolName", "isError") if msg.get(k) is not None}
                if extra:
                    result["extra"] = extra
                step = callers.get(cid)
                if step is None:
                    # ATIF ties a result to a call in its own step.
                    step = _step("system", f"the result of {msg.get('toolName') or 'a call'}", e)
                    steps.append(step)
                    del result["source_call_id"]
                    result.setdefault("extra", {})["tool_call_id"] = cid
                step.setdefault("observation", {"results": []})["results"].append(result)
                if isinstance(msg.get("usage"), dict):
                    nested = _step("agent", "", e)
                    nested["metrics"] = _metrics(msg["usage"])
                    if current:
                        nested["model_name"] = current
                    nested["extra"] = {"nested": msg.get("toolName") or "a tool"}
                    steps.append(nested)
            elif role in ("bashExecution", "custom", "branchSummary", "compactionSummary"):
                text = msg.get("output") or msg.get("summary") or _text(msg.get("content"))
                if text:
                    steps.append(_step("system", text, e))
        elif t in ("compaction", "branch_summary", "usage"):
            if isinstance(e.get("usage"), dict):
                step = _step("agent", e.get("summary") or "", e)
                step["metrics"] = _metrics(e["usage"])
                if e.get("model") or current:
                    step["model_name"] = e.get("model") or current
                step["llm_call_count"] = 1
                step["extra"] = {"kind": e.get("kind") if t == "usage" else t}
                steps.append(step)
            elif e.get("summary"):
                steps.append(_step("system", e["summary"], e))
        elif t == "custom_message":
            text = _text(e.get("content"))
            if text:
                steps.append(_step("system", text, e))
    if not steps:
        steps.append({"step_id": 1, "source": "system", "message": "the session has no history"})
    for i, step in enumerate(steps, start=1):
        step["step_id"] = i
    out: dict[str, Any] = {
        "schema_version": SCHEMA_VERSION,
        "session_id": session_id or "unknown",
        "agent": {"name": "pi", "version": version or "unknown"},
        "steps": steps,
        "final_metrics": _final(steps),
    }
    if model_name:
        out["agent"]["model_name"] = model_name
    return out


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
