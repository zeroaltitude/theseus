"""Part 5 of the release smoke: five scripted turns through the LIVE daemon.

The Discord test channel needs the owner's own hand to speak in it, so the
smoke drives the owner's daemon through its CLI (`theseus ask`, the owner from
a private place), which is the brief's named fallback: a greeting, a quick
fact, a tool call, a task start, and the task's answer. It only observes the
DM task flow (the task it starts posts as any task does) and changes nothing.

Each turn is timed from the ask to its reply, and its footer is the CLI's
status line (`[profile → provider/model · … · session ses_…]` on stderr). The
fifth turn waits for the task to end first (that wait is recorded apart, and is
not the reply's time), then asks what it reported; turns four and five count
only when a task really started and ended. Alarms are the session's and the
task's ledger rows of the kinds that mean something went wrong
(`ALARM_KINDS`), read only after the session's own `turn.ended` rows are found
(an unread ledger is not zero alarms). Dollars are the two sessions'
`provider.call` and `judge.call` rows.

On the live daemon a run leaves two sessions (the conversation and the task's),
the task's report wherever a CLI task reports, and three `judge.replay` rows
from the route part.

    python3 bench/smoke/dm.py --out FILE [--socket PATH] [--theseus PATH]

Standard library only.
"""

from __future__ import annotations

import argparse
import datetime
import json
import os
import re
import subprocess
import sys
import time
from pathlib import Path
from typing import Any

YEAR = str(datetime.date.today().year)
TURNS = [
    ("greeting", "Hello! This is the release smoke checking in; reply with one short sentence.", None),
    ("quick fact", "Quick fact, one word: what is the capital of France?", "paris"),
    ("tool call", "Run `uname -s` with a shell command and tell me exactly what it printed.", "linux"),
    ("task start", "Release smoke: please start a task that runs `date -u +%Y` and reports the year it printed. "
                   "Nothing else; it needs no answer from me.", None),
    ("task answer", "What year did that release-smoke task report?", YEAR),
]
FOOTER = re.compile(r"^\[.+ · session (ses_[0-9a-f]+).*\]$")
# The ledger's kinds that say a turn went wrong, read per session: there is no "notice" or "alarm" kind as such,
# and daemon-wide rows (a crash, the day's spend) never carry a session.
ALARM_KINDS = {"turn.failed", "turn.refused", "provider.error", "provider.refusal", "loop.cap_reached",
               "context.overflow", "execution.failed", "execution.interrupted", "action.failed",
               "action.outcome_unknown", "completion.duplicate", "place.warned", "privacy.warned", "content.unjudged"}
TASK_WAIT = "120s"


class Cli:
    def __init__(self, theseus: str, socket: str | None):
        self.base = [theseus] + (["--socket", socket] if socket else [])
        self.env = {k: v for k, v in os.environ.items() if k != "THESEUS_SESSION"}

    def run(self, *args: str, timeout: float = 120.0) -> subprocess.CompletedProcess:
        return subprocess.run([*self.base, *args], capture_output=True, text=True, timeout=timeout, env=self.env)

    def rows(self, session: str, kind: str | None = None) -> list[dict[str, Any]] | None:
        """The session's ledger rows (of `kind`); None when the ledger could not be read."""
        args = ["--json", "ledger", "-n", "1000", "-s", session] + (["-k", kind] if kind else [])
        r = self.run(*args, timeout=60)
        if r.returncode != 0:
            return None
        return json.loads(r.stdout).get("rows", [])


def turn(cli: Cli, n: int, name: str, prompt: str, want: str | None, session: str | None) -> dict[str, Any]:
    args = ["--no-stream", "ask"] + (["-s", session] if session else []) + [prompt]
    t0 = time.monotonic()
    try:
        r = cli.run(*args, timeout=180)
        out, err, code = r.stdout, r.stderr, r.returncode
    except subprocess.TimeoutExpired:
        out, err, code = "", "", None
    seconds = round(time.monotonic() - t0, 2)
    footer = next((m for m in (FOOTER.match(x.strip()) for x in err.splitlines()) if m), None)
    calls = re.search(r"· (\d+) tool call", err)
    expected = code == 0 and bool(out.strip()) and (want is None or want in out.lower())
    if name in ("tool call", "task start"):
        expected = expected and bool(calls)
    return {"turn": n, "name": name, "seconds": seconds, "exit": code, "footer": bool(footer),
            "session": footer.group(1) if footer else session, "expected": expected,
            "tool_calls": int(calls.group(1)) if calls else 0, "reply": out.strip()[:300],
            "harness_lines": [x for x in err.splitlines() if x.strip() and not FOOTER.match(x.strip())][:5]}


def dollars(rows: list[dict[str, Any]]) -> float:
    total = 0.0
    for r in rows:
        d = r.get("data") or {}
        if r["kind"] == "provider.call":
            total += float(d.get("cost_usd") or 0)
        elif r["kind"] == "judge.call":
            total += float(d.get("cost_micros") or 0) / 1e6
    return total


def run(cli: Cli) -> dict[str, Any]:
    replies, session, task_session, task_ended, notes = [], None, None, False, []
    for n, (name, prompt, want) in enumerate(TURNS):
        if name == "task answer" and session:
            created = cli.rows(session, "task.created") or []
            task_session = (created[-1].get("data") or {}).get("session") if created else None
            if task_session:
                t0 = time.monotonic()
                w = cli.run("--json", "wait", task_session, "--until", "terminal", "--timeout", TASK_WAIT, timeout=180)
                task_ended = w.returncode == 0
                notes.append(f"the task {'ended' if task_ended else 'did not end'} in "
                             f"{time.monotonic() - t0:.0f} s (wait exit {w.returncode})")
            else:
                notes.append("no task.created row on the session: the task never started")
            # The task's start and its answer count only when a task really started and ended: a model that ran
            # `date` itself, or knew the year, would otherwise pass both.
            if replies:
                replies[-1]["expected"] = replies[-1]["expected"] and bool(task_session)
        r = turn(cli, n, name, prompt, want, session)
        if name == "task answer":
            r["expected"] = r["expected"] and task_ended
        session = session or r["session"]
        replies.append(r)
        if r["exit"] is None or not r["session"]:
            notes.append(f"turn {n} ({name}) got no reply; the rest are not sent")
            break
    sessions = [s for s in (session, task_session) if s]
    read = {s: cli.rows(s) for s in sessions}
    rows = [row for got in read.values() for row in (got or [])]
    alarms = [{"kind": r["kind"], "session": r["session_id"]} for r in rows if r["kind"] in ALARM_KINDS]
    turns_ended = sum(1 for r in (read.get(session) or []) if r["kind"] == "turn.ended")
    return {"replies": replies, "alarms": len(alarms), "alarm_rows": alarms, "sessions": sessions,
            "ledger_unread": [s for s, got in read.items() if got is None], "turns_ended": turns_ended,
            "cost_usd": round(dollars(rows), 4), "notes": notes}


def main(argv: list[str]) -> int:
    ap = argparse.ArgumentParser(prog="dm.py", description=__doc__.split("\n\n")[0])
    ap.add_argument("--out", required=True, type=Path)
    ap.add_argument("--socket", default=None, help="the daemon's socket (default: the CLI's own, the live one)")
    ap.add_argument("--theseus", default="theseus", help="the CLI to drive it with")
    a = ap.parse_args(argv)
    rec = run(Cli(a.theseus, a.socket))
    a.out.write_text(json.dumps(rec, indent=1) + "\n")
    print(f"dm: {len(rec['replies'])} replies, {rec['alarms']} alarms, ${rec['cost_usd']:.4f}"
          + (f"; {'; '.join(rec['notes'])}" if rec["notes"] else ""))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
