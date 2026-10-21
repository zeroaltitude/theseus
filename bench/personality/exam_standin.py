"""The exam's stand-in model ($0): the recall bench's server (`bench/recall/standin.py`: the request's own token
count, the same SSE a provider sends) with the exam's rules. Every probe's last message has an answer: the ORACLE's
(a reply that satisfies every one of the probe's checks) or the WRONG one (that fails them), and, for a probe that
wants a wake, the tool call that sets it. A message with no rule gets the default reply, `ok.`.

Also the judge's stand-in: a request whose system prompt carries `judge.MARK` is answered with the rubric's JSON,
all fives for a reply that is an oracle answer and all ones for any other, so the judge's request, its parse and
its cost are exercised without a model.

Standard library only.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent / "recall"))
sys.path.insert(0, str(HERE))

import standin as base  # noqa: E402  (bench/recall/standin.py)
import tokens as tk  # noqa: E402

JUDGE_MARK = "THESEUS-PERSONALITY-JUDGE"
DEFAULT_REPLY = "ok."
SEARCH_TOOL = "tool_search_tool_bm25"


def rules_for(steps, mode: str) -> list[dict]:
    """`mode` is `oracle` or `wrong`. The longest `when` first, so a rule never shadows a longer one."""
    if mode not in ("oracle", "wrong"):
        raise ValueError(mode)
    rules: dict[str, dict] = {}
    for s in steps:
        if s.kind != "probe":
            continue
        calls = s.oracle_calls if mode == "oracle" else s.wrong_calls
        rules[s.text] = {"when": s.text, "text": s.oracle if mode == "oracle" else s.wrong, "calls": calls}
    return sorted(rules.values(), key=lambda r: -len(r["when"]))


def person_texts(req: dict) -> list[str]:
    """The user messages that have words in them, newest first (a request that carries a tool result ends in a
    message of result blocks)."""
    out = []
    for m in reversed(req.get("messages", [])):
        if m.get("role") != "user":
            continue
        c = m.get("content")
        text = c if isinstance(c, str) else "\n".join(
            b.get("text", "") for b in c or [] if b.get("type") == "text"
            and not b.get("text", "").startswith(base.TASK_GRAPH_HEAD))
        if text.strip():
            out.append(text.split(base.RECALL_HEAD, 1)[0])
    return out


def searched(req: dict, calls: list[dict], counted: int, rates) -> list[dict]:
    """The rule's calls as a model makes them: a tool the request defers (`defer_loading`) is not callable unless a
    search found it first, so the turn opens with the provider's tool search, the references it found, and then the
    calls (the daemon answers an unsearched call "not run", and no wake is set)."""
    hidden = {t.get("name") for t in req.get("tools") or [] if t.get("defer_loading")}
    wanted = [c["name"] for c in calls if c["name"] in hidden]
    events = base.calls_turn(calls, counted, rates)
    if not wanted:
        return events
    sid = f"srvtoolu_standin_{next(base._ids)}"
    shift = 2
    out = [events[0]]
    out += [{"type": "content_block_start", "index": 0, "content_block": {
                "type": "server_tool_use", "id": sid, "name": SEARCH_TOOL, "input": {}}},
            {"type": "content_block_delta", "index": 0, "delta": {
                "type": "input_json_delta", "partial_json": json.dumps({"query": " ".join(wanted)})}},
            {"type": "content_block_stop", "index": 0},
            {"type": "content_block_start", "index": 1, "content_block": {
                "type": "tool_search_tool_result", "tool_use_id": sid, "content": {
                    "type": "tool_search_tool_search_result",
                    "tool_references": [{"type": "tool_reference", "tool_name": n} for n in wanted]}}},
            {"type": "content_block_stop", "index": 1}]
    for e in events[1:]:
        if "index" in e:
            e = {**e, "index": e["index"] + shift}
        out.append(e)
    return out


def person_text(req: dict) -> str:
    t = person_texts(req)
    return t[0] if t else ""


class ExamStandIn(base.StandIn):
    def __init__(self, rules: list[dict], oracle_replies: set[str] | None = None):
        super().__init__(rules, DEFAULT_REPLY)
        self.oracle_replies = {r.strip() for r in (oracle_replies or set())}
        self.judge_calls = 0

    def rule_for(self, text: str) -> dict | None:
        for r in self.rules:
            if r["when"] in text:
                return r
        return None

    def answer(self, req: dict) -> list[dict]:
        rates = tk.rates_of(req.get("model", base.MODEL))
        counted = tk.census_of_request(req).tokens(rates)
        self.counts.append(counted)
        system = req.get("system")
        flat = system if isinstance(system, str) else json.dumps(system or "")
        if JUDGE_MARK in flat:
            return self.judge(req, counted, rates)
        if not req.get("tools"):
            return base.text_turn(base.SUMMARY, counted, rates)
        text = person_text(req).split(base.RECALL_HEAD, 1)[0]
        rule = self.rule_for(text)
        if rule is None and base.carries_tool_result(req):
            # the harness's own blocks (the pending wakes) follow the person's words in a later message
            rule = next((r for t in person_texts(req) if (r := self.rule_for(t))), None)
        self.calls.append((text, rule["when"] if rule else None))
        if rule is None:
            return base.text_turn(self.reply, counted, rates)
        if not base.carries_tool_result(req) and rule.get("calls"):
            return searched(req, rule["calls"], counted, rates)
        return base.text_turn(rule["text"] or self.reply, counted, rates)

    def judge(self, req: dict, counted: int, rates) -> list[dict]:
        self.judge_calls += 1
        msgs = req.get("messages") or []
        c = msgs[-1].get("content") if msgs else ""
        body = c if isinstance(c, str) else "\n".join(b.get("text", "") for b in c or [])
        reply = body.split("REPLY:\n", 1)[-1].split("\nEND REPLY", 1)[0].strip()
        score = 5 if reply in self.oracle_replies else 1
        out = {"scores": {d: score for d in ("voice", "candour", "register", "warmth", "initiative")},
               "flattery": score == 1, "notes": "stand-in"}
        return base.text_turn(json.dumps(out), counted, rates)
