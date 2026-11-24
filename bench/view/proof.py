#!/usr/bin/env python3
"""The view's end-to-end proof (issue 44): a scripted conversation suite on a
scratch daemon, and a per-turn cost table, with a stand-in model.

    python3 bench/view/proof.py --bin-dir target/debug --out /tmp/proof
    python3 bench/view/proof.py --bin-dir target/debug --out /tmp/proof --cases wake,fork

**What it runs.** Each case is one function, run on a scratch daemon of its
own (`scratch.py`) on an EMPTY store it makes itself, so nothing it reads was
written by a person: every word is the suite's own. The model is a local
stand-in (`Model` below, on bench/recall's `standin.py` frames) that answers
by script, counts what it is sent, and charges nothing; nothing reaches a
network. There is one context, the view: a turn is assembled no other way, so
each case runs once.

Each case ends with one line, `case <name>: ok|FAILED <why>`, and the suite
prints one table, a row a case: `ok`, `hand-back(<reasons>)` (a
`context.view_fallback` row, which no build since the cut writes), or
`FAILED`, each with the strategies of the case's compilations
(`context.compiled` rows). What a case asserts is what a person needs: an answer arrived, or the
turn ended in a plain failure. Counts and names only.

**The cost table** (`--cost-turns`, 30 by default): one place, short
messages, a tool call every seventh turn. Per turn, the stand-in's count of
the turn's first request (the compiler's rule, `tokens.py`) and the length of
its byte-identical prefix with the previous turn's first request (tools, then
system, then messages, in the request's own order, cache marks taken out),
which is what a provider's prompt cache could read.

**What it does not show.** The stand-in has no cache and no judgement: the
shared prefix is what a cache COULD read, not what one did, and no answer's
quality is measured. Two cases need a Discord surface the kit keeps off (a
`/new` mark: the terminal has none; a shared place: every local client is the
operator), so they say `FAILED not driven` until a fake Discord is part of
the kit. A retired key's start-up warning is on the daemon's stderr, which
the kit discards: the case shows that the daemon starts and answers, not the
warning's words.

Standard library only.
"""

from __future__ import annotations

import argparse
import json
import os
import signal
import sys
import threading
import time
import tomllib
from collections import Counter
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path[:0] = [str(HERE), str(HERE.parent / "recall")]

import drive  # noqa: E402
import scratch  # noqa: E402
import standin  # noqa: E402
import tokens as tk  # noqa: E402

REPLY = "Done."
# A writer's answer (a call that offers no tools: the ladder's `did` line and fold, a compaction's summary): a
# title line, then an account, which is the shape a fold must have (`ladder/fold.rs`, `parse`); a `did` line
# takes the first line.
WRITER = "Charted the buoys.\nThe person noted buoys on the chart, one a turn, and Theseus confirmed each."
# How long a case waits for the daemon to settle (every turn it started has ended or failed).
SETTLE = 90.0
# A gate job: it says it is up, then waits for its gate file (or its directory gone, or 300 s).
GATE = ("touch {d}/{g}.up; n=0; while [ ! -e {d}/{g} ] && [ -d {d} ] && [ $n -lt 6000 ]; "
        "do sleep 0.05; n=$((n+1)); done; echo gate {g} passed")
# The settings keys the cut retires (X-DECISIONS, map §6.8), at values today's code accepts.
RETIRED = {
    "context": {"kept_mb": 64, "ledger": True},
    "cache": {"keep_warm_hours": 24, "keep_warm_every_minutes": 55, "threads_keep_warm_hours": 4,
              "keep_warm_min_tokens": 20000, "stub_min_tokens": 20000, "stub_after_turns": 3},
    "memory": {"summary_profile": "bench"},
    "threads": {"resume_keep_turns": 3, "working_tokens": 20000},
    "profiles": {"bench": {"keep_warm_hours": 24}},
}


def too_long(counted: int) -> dict:
    """A provider's refusal for size, its maximum just under what it was sent, as Anthropic words it."""
    return {"type": "error", "error": {"type": "invalid_request_error",
                                     "message": f"prompt is too long: {counted} tokens > {counted - 1} maximum"}}


# ---- the stand-in model


def strip_marks(v):
    """`v` without its cache marks: where a mark sits does not change what a cache holds."""
    if isinstance(v, dict):
        return {k: strip_marks(x) for k, x in v.items() if k != "cache_control"}
    if isinstance(v, list):
        return [strip_marks(x) for x in v]
    return v


def request_bytes(req: dict) -> bytes:
    """A request in a provider cache's order (tools, system, messages), its marks taken out."""
    parts = [strip_marks(req.get("tools") or []), strip_marks(req.get("system") or [])]
    parts += [strip_marks(m) for m in req.get("messages") or []]
    return b"".join(json.dumps(p, sort_keys=True, separators=(",", ":")).encode() for p in parts)


def shared_prefix(a: bytes, b: bytes) -> int:
    """The length of the byte-identical prefix of `a` and `b`."""
    n = min(len(a), len(b))
    lo, hi = 0, n
    while lo < hi:  # the longest k with a[:k] == b[:k]
        mid = (lo + hi + 1) // 2
        if a[:mid] == b[:mid]:
            lo = mid
        else:
            hi = mid - 1
    return lo


def rounds(req: dict) -> int:
    """How many tool results this turn has carried so far: the trailing user messages that carry one, back to
    the last that carries none."""
    n = 0
    for m in reversed(req.get("messages") or []):
        if m.get("role") != "user":
            continue
        c = m.get("content")
        if isinstance(c, list) and any(b.get("type") == "tool_result" for b in c):
            n += 1
        else:
            break
    return n


def turn_text(req: dict) -> str:
    """The text of the last user message that carries no tool result: what opened this turn."""
    for m in reversed(req.get("messages") or []):
        if m.get("role") != "user":
            continue
        c = m.get("content")
        if isinstance(c, str):
            return c
        if any(b.get("type") == "tool_result" for b in c or []):
            continue
        return "\n".join(b.get("text", "") for b in c or [] if b.get("type") == "text")
    return ""


class Model:
    """A local stand-in for the Messages API. `script` maps a marker (a phrase of the turn's opening text) to
    a list of steps, one per call of the turn: `{"calls": [...]}` (a callable is called for them),
    `{"text": ...}` or `{"too_long": True}` (a 400 for size). A turn's text may quote other markers (a task's
    brief quotes the parent's words; a view's beside block, a task's card), so a turn's FIRST call takes the
    first marker in the script's order that has not opened a turn yet, and a later call of a turn takes the
    marker opened last among those it holds. A marker opens a turn ONCE: a later turn whose text holds only
    markers already opened (a wake's, a report's, a retry of the first call) is answered `REPLY`, as is
    anything no marker names. The step is
    counted here, call by call, not read from the request: a ring or a clearing that cut the turn's old results
    must not send the turn back to its first step. `seen` keeps each call's request, the marker it took and
    its step (`round`)."""

    def __init__(self, script: dict | None = None):
        self.script = script or {}
        self.seen: list[dict] = []
        self.opened: Counter = Counter()
        self.step: Counter = Counter()
        self.order: dict[str, int] = {}
        self.lock = threading.Lock()
        outer = self

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *a):
                pass

            def do_POST(self):
                n = int(self.headers.get("content-length", "0"))
                req = json.loads(self.rfile.read(n) or b"{}")
                status, events = outer.answer(req)
                if status != 200:
                    data = json.dumps(events).encode()
                    self.send_response(status)
                    self.send_header("content-type", "application/json")
                else:
                    data = "".join(f"event: {e['type']}\ndata: {json.dumps(e)}\n\n" for e in events).encode()
                    self.send_response(200)
                    self.send_header("content-type", "text/event-stream")
                self.send_header("content-length", str(len(data)))
                self.send_header("connection", "close")
                self.end_headers()
                self.wfile.write(data)

        self.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.base = f"http://127.0.0.1:{self.server.server_address[1]}"
        threading.Thread(target=self.server.serve_forever, daemon=True).start()

    def close(self) -> None:
        self.server.shutdown()
        self.server.server_close()

    def answer(self, req: dict):
        rates = tk.rates_of(req.get("model", standin.MODEL))
        counted = tk.census_of_request(req).tokens(rates)
        text, r = turn_text(req), rounds(req)
        held = [k for k in self.script if k in text] if req.get("tools") else []
        with self.lock:
            if r == 0:
                marker = next((k for k in held if not self.opened[k]), held[0] if held else None)
            else:
                marker = max(held, key=lambda k: self.order.get(k, -1), default=None)
            if marker is not None and r == 0:
                self.opened[marker] += 1
                self.order.setdefault(marker, len(self.order))
                self.step[marker] = 0
            elif marker is not None:
                self.step[marker] += 1
            k = self.step[marker] if marker is not None else 0
            again = marker is not None and r == 0 and self.opened[marker] > 1
            self.seen.append({"req": req, "counted": counted, "marker": marker, "round": k,
                              "tools": bool(req.get("tools")), "max_tokens": req.get("max_tokens")})
        if not req.get("tools"):  # a writer's call (a fold, a summary): it offers no tools
            return 200, standin.text_turn(WRITER, counted, rates)
        steps = [] if again else self.script.get(marker) or []
        step = steps[k] if k < len(steps) else {}
        if step.get("too_long"):
            return 400, too_long(counted)
        if "calls" in step:
            calls = step["calls"]() if callable(step["calls"]) else step["calls"]
            return 200, standin.calls_turn(calls, counted, rates)
        return 200, standin.text_turn(step.get("text") or REPLY, counted, rates)

    def calls_of(self, marker: str) -> list[dict]:
        with self.lock:
            return [s for s in self.seen if s["marker"] == marker]


# ---- the scratch daemon, with the case's settings


def merge(base: dict, extra: dict) -> dict:
    out = json.loads(json.dumps(base))
    for k, v in extra.items():
        out[k] = merge(out.get(k) or {}, v) if isinstance(v, dict) else v
    return out


def settings(projects: Path, extra: dict | None = None) -> dict:
    """The bench profile with the case's own settings; then the kit's `config`, so its rules (a local model
    only, a spend limit, nothing listening) are applied last and win."""
    base = merge(tomllib.loads(scratch.PROFILE.read_text()), extra or {})
    return merge(base, {"tools": {"projects_dir": str(projects)}})


class Daemon(scratch.Scratch):
    """A `Scratch` on an empty store whose config is the case's (`settings`), and which can be killed and
    started again on the same store (`kill`, then `start`)."""

    def __init__(self, bin_dir: Path, out: Path, model: Model, cfg: dict):
        super().__init__(bin_dir, out, api_base=model.base, model=standin.MODEL)
        try:
            safe = scratch.config(cfg, model.base, standin.MODEL)
            self.cfg.write_text(drive.toml_dumps(safe))
        except BaseException:
            self.stop()
            raise

    def kill(self) -> None:
        """SIGKILL the daemon alone (its jobs stay, as after a crash), and reap it."""
        if self.proc is not None:
            os.kill(self.proc.pid, signal.SIGKILL)
            self.proc.wait(scratch.STOP_WAIT)
            self.proc = None


# ---- the daemon's answers, as counts


def rpc(d: Daemon, method: str, params: dict, timeout: float = 120.0):
    v = d.json("rpc", method, json.dumps(params), timeout=timeout)
    return v.get("result", v) if isinstance(v, dict) else v


def rows(d: Daemon, kind: str) -> list[dict]:
    return scratch.ledger_rows(d, kind)


def open_session(d: Daemon, label: str, cwd: Path) -> str:
    cwd.mkdir(parents=True, exist_ok=True)
    v = rpc(d, "session.open", {"label": label, "dir": str(cwd.resolve())})
    return v.get("session_id") or v["id"]


def submit(d: Daemon, input: str, timeout: float = 180.0, **params) -> dict:
    return rpc(d, "turn.submit", {"input": input, "author": "proof", **params}, timeout=timeout)


def answered(v) -> bool:
    return isinstance(v, dict) and bool(v.get("output"))


def turns(d: Daemon, cut: frozenset = frozenset()) -> dict:
    """The case's turns: started, ended, failed, continuations, and those still open. A turn in `cut` (one a
    kill of the daemon ended) is not open: no row can say it ended."""
    started = rows(d, "turn.started")
    done = {r.get("turn_id") for r in rows(d, "turn.ended")}
    failed = {r.get("turn_id") for r in rows(d, "turn.failed")}
    return {"started": len(started), "ended": len(done - {None}), "failed": len(rows(d, "turn.failed")),
            "continuations": sum(1 for r in started if (r.get("data") or {}).get("continuation")),
            "open": sum(1 for r in started if r.get("turn_id") not in done | failed | cut)}


def open_turns(d: Daemon) -> frozenset:
    started = {r.get("turn_id") for r in rows(d, "turn.started")}
    return frozenset(started - {r.get("turn_id") for r in rows(d, "turn.ended") + rows(d, "turn.failed")})


def wait_for(what, timeout: float = SETTLE, every: float = 0.25):
    """`what()`'s first truthy value, or None after `timeout`."""
    t0 = time.monotonic()
    while time.monotonic() - t0 < timeout:
        v = what()
        if v:
            return v
        time.sleep(every)
    return None


def settle(d: Daemon, at_least: int = 1, timeout: float = SETTLE, cut: frozenset = frozenset()) -> dict:
    """The case's turns once at least `at_least` have started and none is open (read twice, a second apart,
    so a turn that a job's end is about to start is not missed); the last reading when it never settles."""
    last = {}

    def quiet():
        nonlocal last
        a = turns(d, cut)
        last = a
        if a["started"] < at_least or a["open"]:
            return None
        time.sleep(1.0)
        b = turns(d, cut)
        last = b
        return b if b == a else None

    return wait_for(quiet, timeout) or last


def in_background(f, *a, **kw) -> tuple[threading.Thread, dict]:
    box: dict = {}

    def run():
        try:
            box["value"] = f(*a, **kw)
        except BaseException as e:  # noqa: BLE001  (a refused or cut call is a case's fact, not the suite's end)
            box["error"] = type(e).__name__
    t = threading.Thread(target=run, daemon=True)
    t.start()
    return t, box


def gate_call(gates: Path, g: str) -> list[dict]:
    return [{"name": "proc_run", "input": {"argv": ["sh", "-c", GATE.format(d=gates, g=g)], "timeout_secs": 600}}]


def observe(d: Daemon) -> dict:
    """What the ledger says of the case, counts and names only."""
    fallbacks = Counter((r.get("data") or {}).get("why") or "unnamed" for r in rows(d, "context.view_fallback"))
    failures = Counter((r.get("data") or {}).get("class") or "unnamed" for r in rows(d, "turn.failed"))
    strategies = Counter((r.get("data") or {}).get("strategy") or "unnamed" for r in rows(d, "context.compiled"))
    return {"fallback": dict(fallbacks), "strategies": dict(strategies), "turns": turns(d),
            "failure_classes": dict(failures), "refused": len(rows(d, "turn.refused")),
            "compacted": len(rows(d, "context.compacted")), "overage": len(rows(d, "context.overage"))}


# ---- the cases. Each gets a `Case` and returns (ok, why).


class Case:
    def __init__(self, d: Daemon, model: Model, root: Path):
        self.d, self.model, self.root = d, model, root
        self.gates = root / "projects" / "gates"
        self.gates.mkdir(parents=True, exist_ok=True)
        self.state: dict = {}

    def session(self, label: str) -> str:
        return open_session(self.d, label, self.root / "projects" / label)

    def up(self, g: str) -> bool:
        return bool(wait_for(lambda: (self.gates / f"{g}.up").exists(), 60))

    def release(self, g: str) -> None:
        (self.gates / g).write_text("")


def all_ended(t: dict, at_least: int) -> str | None:
    """Why the turns are not what a person needs, or None: every turn started ended (an answer or a plain
    failure), none is left open, and at least `at_least` ran."""
    if t["started"] < at_least:
        return f"{t['started']} turns ran, {at_least} expected"
    if t["open"]:
        return f"{t['open']} turns never ended"
    return None


def message_and_follow_up(c: Case):
    s = c.session("talk")
    a = submit(c.d, "Tell me about the harbour tides.", session_id=s)
    b = submit(c.d, "And when is the next high tide?", session_id=s)
    if not (answered(a) and answered(b)):
        return False, "a message had no answer"
    return True, ""


def steer(c: Case):
    c.model.script = {"Survey the reef": [{"calls": gate_call(c.gates, "steer")}, {"text": "Reef surveyed."}]}
    s = c.session("reef")
    t, box = in_background(submit, c.d, "Survey the reef, slowly.", session_id=s)
    if not c.up("steer"):
        return False, "the turn's job never started"
    t2, box2 = in_background(submit, c.d, "Also count the gulls.", session_id=s)
    steered = wait_for(lambda: rows(c.d, "turn.steered"), 30)
    c.release("steer")
    t.join(180)
    t2.join(180)
    if not steered:
        return False, "the second message was not steered into the turn"
    if not answered(box.get("value")):
        return False, "the steered turn had no answer"
    return True, ""


def tool_loop(c: Case):
    ls = {"calls": [{"name": "fs_list", "input": {"path": "."}}]}
    c.model.script = {"List the workspace": [ls, ls, ls, {"text": "Listed three times."}]}
    s = c.session("loop")
    v = submit(c.d, "List the workspace three times.", session_id=s)
    calls = len(c.model.calls_of("List the workspace"))
    if not answered(v):
        return False, "no answer"
    if calls < 4:
        return False, f"{calls} model calls in the loop, 4 expected"
    return True, ""


def continuation(c: Case):
    job = [{"name": "proc_run", "input": {"argv": ["sh", "-c", "sleep 6; echo the job is done"]}}]
    c.model.script = {"Run the long job": [{"calls": job}, {"text": "Started."}]}
    s = c.session("job")
    v = submit(c.d, "Run the long job.", session_id=s)
    t = settle(c.d, at_least=2)
    if not answered(v):
        return False, "the first turn had no answer"
    why = all_ended(t, 2)
    if why:
        return False, why
    if t["continuations"] < 1:
        return False, "no continuation turn ran for the job's end"
    return True, ""


def restart(c: Case):
    c.model.script = {"Run the held job": [{"calls": gate_call(c.gates, "held")}, {"text": "Held job done."}]}
    s = c.session("held")
    t, box = in_background(submit, c.d, "Run the held job.", session_id=s)
    if not c.up("held"):
        return False, "the turn's job never started"
    cut = open_turns(c.d)
    c.d.kill()
    t.join(60)
    c.d.start()
    wait_for(lambda: turns(c.d)["started"] >= 2, 60)
    c.release("held")
    tt = settle(c.d, at_least=2, cut=cut)
    c.state["cut_turns"] = len(cut)
    why = all_ended(tt, 2)
    if why:
        return False, why
    if tt["continuations"] < 1:
        return False, "no turn resumed the cut one"
    return True, ""


def wake(c: Case):
    c.model.script = {"Wake me soon": [{"calls": [{"name": "wake_at", "input": {"after": "2s",
                                                                                   "note": "check the tide"}}]},
                                       {"text": "I will look again soon."}]}
    s = c.session("wake")
    v = submit(c.d, "Wake me soon.", session_id=s)
    fired = wait_for(lambda: rows(c.d, "wake.fired"), 60)
    t = settle(c.d, at_least=2)
    if not answered(v):
        return False, "the first turn had no answer"
    if not fired:
        return False, "the wake never fired"
    return (False, why) if (why := all_ended(t, 2)) else (True, "")


QUAY = "Please start a quay task to survey the north quay."


def start_task(c: Case) -> tuple[str | None, str]:
    """The parent asks for a task; the task's turn answers its brief and reports. Its id, or why not."""
    task = {"name": "task_create", "input": {
        "brief": "Survey the north quay.", "wake_parent": True,
        "arrangement": {"pieces": [{"quote": QUAY, "role": "objective"}]}}}
    work = c.state.get("task_work") or [{"text": "Quay surveyed: four berths."}]
    c.model.script = {"Survey the north quay": work, **c.model.script,
                      "Please start a quay task": [{"calls": [task]}, {"text": "Started the quay survey."}]}
    s = c.session("quay")
    c.state["parent"] = s
    v = submit(c.d, QUAY, session_id=s)
    if not answered(v):
        return None, "the parent's turn had no answer"
    if not wait_for(lambda: rows(c.d, "task.ended"), 90):
        return None, "the task never ended"
    tasks = rpc(c.d, "task.list", {}).get("tasks") or []
    if not tasks:
        return None, "no task listed"
    return tasks[0]["task_id"], ""


def report(c: Case):
    tid, why = start_task(c)
    if tid is None:
        return False, why
    t = settle(c.d, at_least=3)
    return (False, f"{why} (parent, task, report)") if (why := all_ended(t, 3)) else (True, "")


def task(c: Case):
    """A task's brief is its first turn; its job, which outlives the turn, ends and starts its second (a task's
    session takes no message of the owner's: the daemon refuses one)."""
    job = [{"name": "proc_run", "input": {"argv": ["sh", "-c", "sleep 5; echo the berths are counted"]}}]
    c.state["task_work"] = [{"calls": job}, {"text": "Counting the berths."}]
    tid, why = start_task(c)
    if tid is None:
        return False, why
    t = settle(c.d, at_least=4)
    mine = [r for r in rows(c.d, "turn.started") if r.get("session_id") == tid]
    c.state["task_turns"] = len(mine)
    if len(mine) < 2:
        return False, f"the task ran {len(mine)} turn(s), 2 expected"
    return (False, why) if (why := all_ended(t, 4)) else (True, "")


def check(c: Case):
    tid, why = start_task(c)
    if tid is None:
        return False, why
    settle(c.d, at_least=3)
    chk = {"name": "task_create", "input": {"brief": "Check the quay survey.", "check_of": tid}}
    c.model.script = {"Check the quay survey": [{"text": "The survey holds: four berths."}], **c.model.script,
                      "Check the survey": [{"calls": [chk]}, {"text": "A check is running."}]}
    v = submit(c.d, "Check the survey.", session_id=c.state["parent"])
    opened = wait_for(lambda: rows(c.d, "task.check_opened"), 60)
    ended = wait_for(lambda: len(rows(c.d, "task.ended")) >= 2, 90)
    t = settle(c.d, at_least=5)
    if not answered(v):
        return False, "the asking turn had no answer"
    if not opened:
        return False, f"no check opened ({len(rows(c.d, 'task.check_refused'))} refused)"
    if not ended:
        return False, "the check never ended"
    return (False, why) if (why := all_ended(t, 5)) else (True, "")


def fork(c: Case):
    s = c.session("fork")
    for line in ("The buoy is red.", "The buoy is now green.", "The buoy is blue at last."):
        if not answered(submit(c.d, line, session_id=s)):
            return False, "a turn of the source had no answer"
    nodes = scratch.history_nodes(c.d, s)
    at = next((n.get("position") for n in nodes[len(nodes) // 2:] if isinstance(n.get("position"), int)), None)
    if at is None:
        return False, "no position to fork at"
    f = rpc(c.d, "session.fork", {"session": s, "at": str(at)})
    v = submit(c.d, "Which colour did we settle on?", session_id=f["session_id"])
    if not answered(v):
        return False, "the fork's first turn had no answer"
    return True, ""


BIG = "yes 'synthetic log line from the proof suite' | head -c 28000"


def over_budget(c: Case):
    big = {"calls": [{"name": "proc_run", "input": {"argv": ["sh", "-c", BIG]}}]}
    c.model.script = {"Read the big logs": [big] * 6 + [{"text": "Read them all."}]}
    s = c.session("budget")
    try:
        v = submit(c.d, "Read the big logs.", session_id=s)
    except scratch.Failed:
        v = None
    t = settle(c.d)
    most = max((x["counted"] for x in c.model.calls_of("Read the big logs")), default=0)
    c.state["most_counted"] = most
    if why := all_ended(t, 1):
        return False, why
    if not answered(v) and not t["failed"]:
        return False, "neither an answer nor a plain failure"
    return True, "" if answered(v) else "a plain failure"


def provider_refusal(c: Case):
    c.model.script = {"Refuse this for size": [{"too_long": True}]}
    s = c.session("refusal")
    try:
        v = submit(c.d, "Refuse this for size.", session_id=s)
    except scratch.Failed:
        v = None
    t = settle(c.d)
    c.state["calls"] = len(c.model.calls_of("Refuse this for size"))
    if why := all_ended(t, 1):
        return False, why
    if not answered(v) and not t["failed"]:
        return False, "neither an answer nor a plain failure"
    return True, "" if answered(v) else "a plain failure"


def new_mark(c: Case):
    return False, "not driven: /new is the chat's (the terminal has none) and the kit keeps Discord off"


def shared_place(c: Case):
    return False, "not driven: every local client is the operator; a second person needs a Discord place"


def leaf_keys(d: dict, pre: str = "") -> list[str]:
    out = []
    for k, v in d.items():
        out += leaf_keys(v, f"{pre}{k}.") if isinstance(v, dict) else [f"{pre}{k}"]
    return out


def string_values(v) -> list[str]:
    """Every string VALUE in `v` (never a key: a summary's field names are not a warning)."""
    if isinstance(v, dict):
        return [s for x in v.values() for s in string_values(x)]
    if isinstance(v, list):
        return [s for x in v for s in string_values(x)]
    return [v] if isinstance(v, str) else []


def named(key: str, said: str) -> bool:
    """Whether `said` names the retired `key`: its dotted name, or its own name when that cannot be a plain
    word (`kept_mb`, never `ledger`, which health says of other things)."""
    leaf = key.rsplit(".", 1)[-1]
    return key in said or ("_" in leaf and leaf in said)


def retired_keys(c: Case):
    """The daemon starts on a settings file naming every retired key, and answers. Its start-up warning goes to
    stderr, which the kit discards; what CAN be read is `health`: how many of the retired keys a string in it
    names (`retired_named_in_health`, of `retired_keys`). None today: health has no field for a config's
    warnings."""
    s = c.session("retired")
    if not answered(submit(c.d, "Say hello.", session_id=s)):
        return False, "the daemon started but did not answer"
    said = " ".join(string_values(c.d.json("health")))
    keys = leaf_keys(RETIRED)
    c.state["retired_keys"] = len(keys)
    c.state["retired_named_in_health"] = sum(1 for k in keys if named(k, said))
    if c.state["retired_named_in_health"] < len(keys):
        return True, f"health names {c.state['retired_named_in_health']} of {len(keys)} retired keys"
    return True, ""


# name -> (function, the case's own settings)
CASES = {
    "message_and_follow_up": (message_and_follow_up, {}),
    "steer": (steer, {}),
    "tool_loop": (tool_loop, {}),
    "continuation": (continuation, {"tools": {"proc_sync_secs": 2}}),
    "restart_mid_turn": (restart, {"tools": {"proc_sync_secs": 120}}),
    # The terminal defers the reminders family behind the provider's tool search, which a stand-in cannot run.
    "wake": (wake, {"tools": {"defer": {"enabled": False}}}),
    "report": (report, {}),
    "task": (task, {"tools": {"proc_sync_secs": 2}}),
    "check": (check, {}),
    "fork": (fork, {}),
    "over_budget": (over_budget, {"context": {"working_window_tokens": 40000},
                                  "model": {"max_output_tokens": 2000},
                                  "profiles": {"bench": {"max_output_tokens": 2000}}}),
    "provider_refusal": (provider_refusal, {}),
    "new_mark": (new_mark, {}),
    "shared_place": (shared_place, {}),
    "retired_keys": (retired_keys, RETIRED),
}
NOT_DRIVEN = {"new_mark", "shared_place"}


def tool_results(model: Model, limit: int = 240) -> list[str]:
    """Each tool result the stand-in was sent, once, cut to `limit` characters: the harness's words for a
    failing case (`--debug`). The store is the suite's own, so nothing here is a person's."""
    out: list[str] = []
    for s in model.seen:
        for m in s["req"].get("messages") or []:
            for b in m.get("content") if isinstance(m.get("content"), list) else []:
                if b.get("type") == "tool_result":
                    c = b.get("content")
                    text = c if isinstance(c, str) else " ".join(x.get("text", "") for x in c or [])
                    line = ("error: " if b.get("is_error") else "") + " ".join(text.split())[:limit]
                    if line not in out:
                        out.append(line)
    return out


DEBUG = False


def run_case(name: str, bin_dir: Path, out: Path) -> dict:
    """One case, on a daemon of its own; its verdict and what the ledger says."""
    f, extra = CASES[name]
    res: dict = {"case": name}
    if name in NOT_DRIVEN:
        ok, why = f(None)
        res.update(ok=ok, why=why, not_driven=True)
        return res
    root = out / name
    model = Model()
    try:
        d = Daemon(bin_dir, root, model, settings(root / "projects", extra))
        with d:
            c = Case(d, model, root)
            try:
                ok, why = f(c)
            except scratch.Failed as e:
                ok, why = False, str(e)
            res.update(ok=ok, why=why, **observe(d))
            res["state"] = {k: v for k, v in c.state.items() if isinstance(v, int)}
            res["model_calls"] = len(model.seen)
            if DEBUG and not ok:
                res["debug"] = {"tool_results": tool_results(model),
                                "tools_offered": sorted({t.get("name", "") for s in model.seen[:1]
                                                         for t in s["req"].get("tools") or []})}
    except scratch.Failed as e:
        res.update(ok=False, why=str(e))
    finally:
        model.close()
    return res


def verdict_line(r: dict) -> str:
    return f"case {r['case']}: ok" + (f" ({r['why']})" if r.get("why") else "") if r["ok"] \
        else f"case {r['case']}: FAILED {r['why']}"


def cell(r: dict) -> str:
    """One table cell: ok / hand-back(reasons) / FAILED, with the case's compilations by strategy."""
    if r.get("not_driven"):
        return "not driven"
    head = "FAILED" if not r["ok"] else (
        f"hand-back({', '.join(f'{k} x{n}' for k, n in sorted(r['fallback'].items()))})"
        if r.get("fallback") else "ok")
    strat = ", ".join(f"{k} {n}" for k, n in sorted((r.get("strategies") or {}).items()))
    return f"{head} [{strat}]" if strat else head


def table(results: list[dict]) -> str:
    by = {r["case"]: r for r in results}
    lines = ["| case | result |", "|---|---|"]
    lines += [f"| {name} | {cell(by[name])} |" for name in CASES if name in by]
    return "\n".join(lines)


# ---- the cost table


COST_TOOL_EVERY = 7
# How long a turn's writer calls (the ladder's, made after the turn ends) are waited for: until none has come
# for QUIET seconds, at most WRITER_WAIT.
QUIET = 1.5
WRITER_WAIT = 30.0
# The ladder writer's two works, by the output cap each asks (`ladder/writer.rs`, `Work::max_tokens`).
WRITER_KINDS = {200: "did", 1200: "fold"}


def cost_line(i: int) -> str:
    return f"Turn {i}: note buoy number {i} on the chart."


def short_conversation(n: int) -> tuple[list[str], dict]:
    """`n` short messages, a `fs_list` call every seventh."""
    script = {cost_line(i): [{"calls": [{"name": "fs_list", "input": {"path": "."}}]}, {"text": "Charted."}]
              for i in range(1, n + 1) if i % COST_TOOL_EVERY == 0}
    return [cost_line(i) for i in range(1, n + 1)], script


TOOL_RESULT_CHARS = 6000


def tool_line(i: int) -> str:
    return f"Turn {i}: read the three tide logs."


def tool_heavy(n: int) -> tuple[list[str], dict]:
    """`n` turns, each with three tool results of about TOOL_RESULT_CHARS characters, then a short answer."""
    def three(i: int) -> list[dict]:
        return [{"name": "proc_run", "input": {"argv": ["sh", "-c", f"yes 'synthetic tide log {i}.{k} from the proof "
                                                                    f"suite' | head -c {TOOL_RESULT_CHARS}"]}}
                for k in range(3)]
    script = {tool_line(i): [{"calls": three(i)}, {"text": "Read the three logs."}] for i in range(1, n + 1)}
    return [tool_line(i) for i in range(1, n + 1)], script


def writer_quiet(model: Model, timeout: float = WRITER_WAIT) -> None:
    """Wait until the stand-in has been sent nothing for QUIET seconds (the ladder writes after a turn ends)."""
    t0 = time.monotonic()
    last, at = len(model.seen), time.monotonic()
    while time.monotonic() - t0 < timeout:
        time.sleep(0.25)
        n = len(model.seen)
        if n != last:
            last, at = n, time.monotonic()
        elif time.monotonic() - at >= QUIET:
            return


def extra_calls(mine: list[dict], line: str, steps: int) -> list[dict]:
    """The tool-offering calls in a turn's span that the turn's script does not account for: any whose
    opening text is not this turn's, and the turn's own beyond its `steps` (one for a turn with no script)."""
    tools = [x for x in mine if x["tools"]]
    own = [x for x in tools if line in turn_text(x["req"])]
    return [x for x in tools if line not in turn_text(x["req"])] + own[steps:]


def call_probe(x: dict, chars: int = 200) -> dict:
    """What a call is, for `--debug`: its system text's first `chars` characters, its tool names, its opening
    text's first `chars`, its step, its output cap and its tokens. The suite's own synthetic words."""
    req = x["req"]
    system = req.get("system")
    text = system if isinstance(system, str) else " ".join(b.get("text", "") for b in system or [])
    return {"system": " ".join(text.split())[:chars], "tools": [t.get("name", "") for t in req.get("tools") or []],
            "opening": " ".join(turn_text(req).split())[:chars], "round": x["round"],
            "max_tokens": x.get("max_tokens"), "tokens": x["counted"]}


def cost_run(bin_dir: Path, out: Path, n: int, name: str = "cost",
             conversation=short_conversation, extra: dict | None = None) -> dict:
    """A conversation in one place. Per turn: its first request's tokens and the prefix it shares with the
    previous turn's first request; the model calls the turn made; the calls made OFF the turn (a call that
    offers no tools, between this turn's start and the next's: the ladder's `did` lines and folds, a
    compaction's summary), with their tokens; its follow-up requests (a tool turn's later steps); and the
    turn's compilations by strategy. `all_calls` and `all_tokens` count EVERY request the stand-in was sent,
    each once: a turn's span (`all_*` of its row), and any outside every turn's span (`outside_*`)."""
    lines, script = conversation(n)
    model = Model(script)
    root = out / name
    per: list[dict] = []
    spans: set[int] = set()
    try:
        with Daemon(bin_dir, root, model, settings(root / "projects", extra)) as d:
            s = open_session(d, name, root / "projects" / name)
            for i, line in enumerate(lines, 1):
                before = len(model.seen)
                try:
                    v = submit(d, line, session_id=s)
                except scratch.Failed:
                    v = None
                writer_quiet(model)
                with model.lock:
                    mine = model.seen[before:]
                spans.update(range(before, before + len(mine)))
                first = [x for x in mine if x["tools"] and x["round"] == 0 and line in turn_text(x["req"])]
                off = [x for x in mine if not x["tools"]]
                kinds = Counter(WRITER_KINDS.get(x.get("max_tokens"), "other") for x in off)
                extra = extra_calls(mine, line, len(script.get(line) or [{}]))
                odd = {id(x) for x in extra} | ({id(first[0])} if first else set())
                follow = [x for x in mine if x["tools"] and id(x) not in odd]
                per.append({"turn": i, "answered": answered(v), "req": first[0] if first else None,
                            "calls": sum(1 for x in mine if x["tools"]), "off_calls": len(off),
                            "off_tokens": sum(x["counted"] for x in off), "off_kinds": dict(kinds),
                            "extra_calls": len(extra), "extra_tokens": sum(x["counted"] for x in extra),
                            "follow_calls": len(follow), "follow_tokens": sum(x["counted"] for x in follow),
                            "all_calls": len(mine), "all_tokens": sum(x["counted"] for x in mine),
                            **({"extra": [call_probe(x) for x in extra]} if DEBUG and extra else {})})
            started = [r for r in rows(d, "turn.started") if r.get("session_id") == s]
            by_turn: dict = {}
            for r in rows(d, "context.compiled"):
                by_turn.setdefault(r.get("turn_id"), Counter())[(r.get("data") or {}).get("strategy") or "unnamed"] += 1
            seen = observe(d)
    finally:
        model.close()
    with model.lock:
        every = list(model.seen)
    outside = [x for k, x in enumerate(every) if k not in spans]
    for p, r in zip(per, started):
        p["strategies"] = dict(by_turn.get(r.get("turn_id")) or {})
    table_rows, prev = [], None
    for p in per:
        req = p.pop("req")
        if req is None:
            table_rows.append({**p, "tokens": None, "shared_tokens": None})
            prev = None
            continue
        b = request_bytes(req["req"])
        shared = shared_prefix(prev, b) if prev is not None else 0
        tokens = req["counted"]
        table_rows.append({**p, "tokens": tokens, "bytes": len(b), "shared_bytes": shared,
                           "shared_tokens": round(tokens * shared / len(b)) if b else 0})
        prev = b
    total = sum(r["tokens"] or 0 for r in table_rows)
    shared = sum(r["shared_tokens"] or 0 for r in table_rows)
    return {"case": name, "rows": table_rows, "total_tokens": total, "shared_tokens": shared,
            "unshared_tokens": total - shared, "off_calls": sum(r["off_calls"] for r in table_rows),
            "off_tokens": sum(r["off_tokens"] for r in table_rows),
            "off_kinds": dict(sum((Counter(r["off_kinds"]) for r in table_rows), Counter())),
            "extra_calls": sum(r["extra_calls"] for r in table_rows),
            "extra_tokens": sum(r["extra_tokens"] for r in table_rows),
            "follow_calls": sum(r["follow_calls"] for r in table_rows),
            "follow_tokens": sum(r["follow_tokens"] for r in table_rows),
            "outside_calls": len(outside), "outside_tokens": sum(x["counted"] for x in outside),
            "all_calls": len(every), "all_tokens": sum(x["counted"] for x in every),
            "unanswered": sum(1 for r in table_rows if not r["answered"]),
            "strategies": seen["strategies"], "fallback": seen["fallback"]}


def cost_table(r: dict) -> str:
    """Per turn, its FIRST request's tokens and the prefix that request shares with the turn before's; then
    every request of the run (`all_line`)."""
    lines = ["| turn | first request | shared prefix |", "|---|---|---|"]
    for x in r["rows"]:
        lines.append(f"| {x['turn']} | {x['tokens'] if x['tokens'] is not None else '-'} | "
                     f"{x['shared_tokens'] if x['shared_tokens'] is not None else '-'} |")
    lines.append(f"| total | {r['total_tokens']} | {r['shared_tokens']} |")
    lines.append(f"| not shared | {r['unshared_tokens']} | |")
    lines.append(all_line(r))
    lines.append(f"compilations {r['strategies']}, hand-backs {r['fallback'] or 0}")
    return "\n".join(lines)


def strat(d: dict) -> str:
    return ",".join(f"{k}{'x' + str(n) if n > 1 else ''}" for k, n in sorted(d.items())) or "-"


def long_table(r: dict) -> str:
    """A long cost case: per turn, tokens, not shared, the turn's calls, the calls off the turn (with their
    kinds and tokens), and the compilations; then the totals."""
    lines = [f"### {r['case']}", "", "| turn | tokens | not shared | calls | off-turn | compiled |",
             "|---|---|---|---|---|---|"]
    for x in r["rows"]:
        unshared = x["tokens"] - x["shared_tokens"] if x["tokens"] is not None else None
        kinds = " ".join(f"{k}{n}" for k, n in sorted(x["off_kinds"].items()))
        cells = [str(x["tokens"]) if x["tokens"] is not None else "-",
                 str(unshared) if unshared is not None else "-", str(x["calls"]),
                 f"{x['off_calls']} ({kinds}) {x['off_tokens']}t" if x["off_calls"] else "0",
                 strat(x.get("strategies") or {})]
        lines.append(f"| {x['turn']} | " + " | ".join(cells) + " |")
    lines.append("")
    lines.append(f"first requests {r['total_tokens']} tokens, not shared {r['unshared_tokens']}")
    lines.append(f"{all_line(r)}; unanswered {r['unanswered']}, compilations {r['strategies']}, hand-backs "
                 f"{r['fallback'] or 0}")
    if r["case"] in COLD:
        lines += ["", cold_line(r)]
    return "\n".join(lines)


def all_tokens(r: dict) -> int:
    """Every token of every request the stand-in was sent in the run, each request counted once."""
    return r.get("all_tokens", 0)


def all_line(r: dict) -> str:
    """Every request of the run, and its parts: each turn's first request, a tool turn's follow-ups, the calls
    off the turns, the extra calls, and any outside every turn's span."""
    firsts = sum(1 for x in r.get("rows") or [] if x.get("tokens") is not None)
    return (f"all requests {r.get('all_calls', 0)} calls {all_tokens(r)} tokens: first {firsts} calls "
            f"{r['total_tokens']} tokens, follow-up {r.get('follow_calls', 0)} calls {r.get('follow_tokens', 0)} "
            f"tokens, off-turn {r.get('off_calls', 0)} calls {r.get('off_tokens', 0)} tokens "
            f"{r.get('off_kinds', {})}, extra {r.get('extra_calls', 0)} calls {r.get('extra_tokens', 0)} tokens, "
            f"outside a turn {r.get('outside_calls', 0)} calls {r.get('outside_tokens', 0)} tokens")


# A cold start: the first turn after the provider's cache has gone is written whole. Sonnet 5.5's cache-write
# prices from the design (§16): $2.50 a million tokens for five minutes, $4 for an hour.
COLD = {"cold_start_61"}
WRITE_5M, WRITE_1H = 2.50, 4.00


def cold_line(r: dict) -> str:
    """The last turn's whole request, as the first turn after a lapse writes it."""
    x = r["rows"][-1]
    t = x["tokens"] or 0
    return (f"cold start, turn {x['turn']} after a lapse (its whole request): {t} tokens "
            f"(${t * WRITE_5M / 1e6:.3f} at the 5-minute write, ${t * WRITE_1H / 1e6:.3f} at the 1-hour write; "
            f"{x['tokens'] - x['shared_tokens'] if x['tokens'] else '-'} not shared while cached)")


def expect_cut(results: list[dict], costs: list[dict]) -> list[str]:
    """What keeps the cut from being accepted: a driven case that is not ok, any hand-back
    (`context.view_fallback` row), any compilation whose strategy is not `view`, in a case or a cost run."""
    bad = []
    for r in results:
        if r.get("not_driven"):
            continue
        cell = r["case"]
        if not r["ok"]:
            bad.append(f"{cell}: FAILED {r.get('why')}")
        if r.get("fallback"):
            bad.append(f"{cell}: hand-back {r['fallback']}")
        if set(r.get("strategies") or {}) - {"view"}:
            bad.append(f"{cell}: compilations {r['strategies']}")
    for c in costs:
        cell = f"cost {c.get('case', 'cost')}"
        if c.get("fallback"):
            bad.append(f"{cell}: hand-back {c['fallback']}")
        if set(c.get("strategies") or {}) - {"view"}:
            bad.append(f"{cell}: compilations {c['strategies']}")
    return bad


# name -> (the conversation, its turns, the case's own settings)
LONG = {
    "long_120": (short_conversation, 120, {}),
    "tool_heavy_20": (tool_heavy, 20, {}),
    # 60 turns, then the 61st: its whole request is what the first turn after a lapse writes.
    "cold_start_61": (short_conversation, 61, {}),
}
LONG_NOT_DRIVEN = {
    "new_break": "skipped: the terminal has no /new (it is the chat's), and the kit keeps Discord off",
    "day_change": "skipped: the daemon reads the machine's clock, and the kit cannot move it",
}


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--bin-dir", default="target/debug")
    ap.add_argument("--out", required=True, help="an empty directory of the suite's own")
    ap.add_argument("--cases", default="all", help="names, comma-separated, or all, or none")
    ap.add_argument("--cost-turns", type=int, default=30, help="0: no cost table")
    ap.add_argument("--cost-long", action="store_true",
                    help="the long cost cases: 120 short turns in one place, 20 tool-heavy turns, and a cold start "
                         "at turn 61")
    ap.add_argument("--expect-cut", action="store_true",
                    help="the cut's acceptance: exit nonzero unless every driven case is ok, no hand-back exists "
                         "and every compilation is the view's")
    ap.add_argument("--debug", action="store_true",
                    help="a failing case's tool results and offered tools in proof.json (the suite's own words)")
    a = ap.parse_args(argv)
    global DEBUG
    DEBUG = a.debug
    out = scratch.check_out(Path(a.out))
    out.mkdir(parents=True, exist_ok=True)
    names = [] if a.cases == "none" else list(CASES) if a.cases == "all" else a.cases.split(",")
    unknown = [n for n in names if n not in CASES]
    if unknown:
        ap.error(f"unknown cases: {unknown}")
    results = []
    for name in names:
        r = run_case(name, Path(a.bin_dir), out)
        results.append(r)
        print(verdict_line(r), flush=True)
    report: dict = {"results": results}
    costs: list[dict] = []
    if names:
        print("\n" + table(results))
    if a.cost_turns:
        run = cost_run(Path(a.bin_dir), out, a.cost_turns)
        report["cost"] = run
        costs.append(run)
        print("\n" + cost_table(run))
    if a.cost_long:
        report["cost_long"] = {}
        for name, (conv, n, extra) in LONG.items():
            run = cost_run(Path(a.bin_dir), out, n, name, conv, extra)
            report["cost_long"][name] = run
            costs.append(run)
            print("\n" + long_table(run), flush=True)
        for name, why in LONG_NOT_DRIVEN.items():
            print(f"\n### {name}\n\n{why}")
    if a.expect_cut:
        bad = expect_cut(results, costs)
        report["expect_cut"] = bad
        print("\n## The cut's acceptance (--expect-cut)\n")
        print("\n".join(f"- {b}" for b in bad) if bad else "accepted: every driven case ok, no hand-back, "
              "every compilation the view's")
    (out / "proof.json").write_text(json.dumps(report, indent=1, sort_keys=True) + "\n")
    if a.expect_cut and report["expect_cut"]:
        return 2
    return 0 if all(r["ok"] or r.get("not_driven") for r in results) else 1


if __name__ == "__main__":
    sys.exit(main())
