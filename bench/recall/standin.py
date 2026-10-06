"""A stand-in for the Messages API, for the recall bench's offline smoke
(theseus-523y): `theseus-sim fake-model --rules`'s scripted answers, but each
call reports the input tokens a provider would count, the request's estimate
by the compiler's rule (`tokens.py`), where theseus-sim's reports 40.

Theseus trusts the provider's count: from a compilation's second call on,
the count of the last request is `counted`, and only what was written since
is estimated. At 40 a call, a turn's history costs nothing, its newest read
alone decides the ring, and a mark that fails live passes offline. With the
request's own estimate counted, the stand-in's turns fill the context as a
model's do, and the ring, a compaction or an overage come where they would.

Rules, as theseus-sim's (`when`, then `calls` or `text`): the first rule
whose `when` the turn's last user text holds, before any recall notes (they
quote earlier turns), asks for its calls, or answers its text; a call that carries a tool result is answered with `reply`
(REPLY_BYTES of text, a model's reply after its work), as is a turn no rule
matches.

Standard library only.
"""

from __future__ import annotations

import itertools
import json
import math
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

import tokens as tk

MODEL = "claude-sonnet-5-5"
# theseus-core's task_graph/view.rs, `HEAD`: the harness's view, not the user's.
TASK_GRAPH_HEAD = "[The task graph in this conversation's scope:"
# theseus-core's recall/render.rs, the recall notes' preamble: they quote
# earlier turns, so a rule is matched against the person's words before it.
RECALL_HEAD = "[Recalled by the harness:"
_ids = itertools.count()
SUMMARY = "Summary: the session ran the workspace's scripts, read its files and build logs, and answered questions."


def last_user_text(req: dict) -> str:
    for m in reversed(req.get("messages", [])):
        if m.get("role") != "user":
            continue
        c = m.get("content")
        if isinstance(c, str):
            return c
        return "\n".join(b.get("text", "") for b in c or [] if b.get("type") == "text"
                         and not b.get("text", "").startswith(TASK_GRAPH_HEAD))
    return ""


def carries_tool_result(req: dict) -> bool:
    msgs = req.get("messages") or []
    c = msgs[-1].get("content") if msgs else None
    return isinstance(c, list) and any(b.get("type") == "tool_result" for b in c)


class StandIn:
    """The server, on 127.0.0.1 at an ephemeral port, in a thread of its own:
    `base` is its address; `counts` each call's reported input tokens."""

    def __init__(self, rules: list[dict], reply: str):
        self.rules, self.reply = rules, reply
        self.counts: list[int] = []
        self.calls: list[tuple[str, str | None]] = []  # each call's last user text, and the rule it took
        outer = self

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *a):  # quiet
                pass

            def do_POST(self):
                n = int(self.headers.get("content-length", "0"))
                req = json.loads(self.rfile.read(n) or b"{}")
                body = "".join(f"event: {e['type']}\ndata: {json.dumps(e)}\n\n" for e in outer.answer(req))
                data = body.encode()
                self.send_response(200)
                self.send_header("content-type", "text/event-stream")
                self.send_header("cache-control", "no-cache")
                self.send_header("content-length", str(len(data)))
                self.send_header("connection", "close")
                self.end_headers()
                self.wfile.write(data)

        self.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.base = f"http://127.0.0.1:{self.server.server_address[1]}"
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()

    def close(self) -> None:
        self.server.shutdown()
        self.server.server_close()

    def answer(self, req: dict) -> list[dict]:
        rates = tk.rates_of(req.get("model", MODEL))
        counted = tk.census_of_request(req).tokens(rates)
        self.counts.append(counted)
        if not req.get("tools"):
            # A compaction's summary call: it offers no tools.
            return text_turn(SUMMARY, counted, rates)
        if not carries_tool_result(req):
            text = last_user_text(req).split(RECALL_HEAD, 1)[0]
            for r in self.rules:
                if r["when"] in text:
                    self.calls.append((text, r["when"]))
                    if r.get("calls"):
                        return calls_turn(r["calls"], counted, rates)
                    return text_turn(r.get("text") or "Done.", counted, rates)
            self.calls.append((text, None))
        return text_turn(self.reply, counted, rates)


def _start(input_tokens: int) -> dict:
    return {"type": "message_start", "message": {
        "id": f"msg_standin_{next(_ids)}", "type": "message", "role": "assistant", "model": MODEL, "content": [],
        "usage": {"input_tokens": input_tokens, "output_tokens": 1, "cache_read_input_tokens": 0,
                  "cache_creation_input_tokens": 0}}}


def _end(stop: str, output_tokens: int) -> list[dict]:
    return [{"type": "message_delta", "delta": {"stop_reason": stop}, "usage": {"output_tokens": output_tokens}},
            {"type": "message_stop"}]


def text_turn(text: str, counted: int, rates) -> list[dict]:
    return [_start(counted),
            {"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}},
            {"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": text}},
            {"type": "content_block_stop", "index": 0},
            *_end("end_turn", max(1, math.ceil(len(text.encode()) / rates[1])))]


def calls_turn(calls: list[dict], counted: int, rates) -> list[dict]:
    n = next(_ids)
    out = [_start(counted)]
    size = 0
    for i, c in enumerate(calls):
        args = json.dumps(c["input"])
        size += len(c["name"]) + len(args)
        out += [{"type": "content_block_start", "index": i, "content_block": {
                    "type": "tool_use", "id": f"toolu_standin_{n}_{i}", "name": c["name"], "input": {}}},
                {"type": "content_block_delta", "index": i, "delta": {"type": "input_json_delta",
                                                                      "partial_json": args}},
                {"type": "content_block_stop", "index": i}]
    return out + _end("tool_use", max(1, math.ceil(size / rates[0])))
