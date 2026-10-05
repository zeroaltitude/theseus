"""The async bench's driver (theseus-7gir.16): what needs no Harbor.

- `Injection` and `injection_of`: a family's second message, from its
  task.toml (`[metadata.async.injection]`);
- `fire`: waits for the injection's trigger, the ledger's first record of
  a kind from a tool plus a delay, else a time after the agent started;
  writes the injection to the ledger with the trigger that fired; and hands
  the message to the arm's `deliver`;
- `Theseus`: the commands of a trial on a real daemon, and `settle`, the
  trial's end: no execution running, queued, or waiting on a job or a
  task, and no wake pending;
- `claude_stdin`: Harbor's Claude Code command, rewritten to read its input
  as stream-json from a FIFO the driver writes to.

`async_agents.py` holds the Harbor agents that use them. Standard library only,
so the tests (`test_driver.py`) run without Harbor.
"""

from __future__ import annotations

import asyncio
import json
import re
import shlex
import time
import tomllib
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Any, Awaitable, Callable, Protocol

# The tool library inside a task's image.
LIB = "/opt/async/lib/asyncbench.py"
# What the driver leaves in the trial's agent log directory.
REPORT = "async-driver.json"


class Env(Protocol):
    """What the driver needs of a Harbor environment."""

    async def exec(self, command: str, cwd: str | None = None, env: dict[str, str] | None = None,
                   timeout_sec: int | None = None) -> Any: ...


@dataclass(frozen=True)
class Injection:
    message: str
    after_tool: str
    after_kind: str
    delay_s: float
    at_s: float


def task_of(task_dir: Path) -> dict[str, Any]:
    """`[metadata.async]` of a task's task.toml, or {} for a task of no family."""
    try:
        cfg = tomllib.loads((task_dir / "task.toml").read_text())
    except OSError:
        return {}
    return cfg.get("metadata", {}).get("async", {})


def injection_of(task_dir: Path) -> Injection | None:
    inj = task_of(task_dir).get("injection")
    if not inj:
        return None
    return Injection(inj["message"], inj["after_tool"], inj["after_kind"],
                     float(inj["delay_s"]), float(inj["at_s"]))


async def fire(
    inj: Injection,
    env: Env,
    deliver: Callable[[str], Awaitable[Any]],
    *,
    started: float,
    lib: str = LIB,
    clock: Callable[[], float] = time.monotonic,
) -> dict[str, Any]:
    """The injection: wait for its trigger, record it in the ledger, deliver
    it. `started` is when the agent started, on `clock`. Returns what
    happened, for the trial's driver report."""
    left = inj.at_s - (clock() - started)
    trigger: dict[str, Any] = {"by": "time", "at_s": inj.at_s}
    if left > 0:
        r = await env.exec(
            f"python3 {shlex.quote(lib)} await --tool {shlex.quote(inj.after_tool)} "
            f"--kind {shlex.quote(inj.after_kind)} --timeout {left:.3f}",
            timeout_sec=int(left) + 30,
        )
        event = _json(r.stdout) if r.return_code == 0 else None
        if event:
            trigger = {"by": "event", "tool": inj.after_tool, "kind": inj.after_kind,
                       "seq": event["seq"], "event_mono": event["mono"], "delay_s": inj.delay_s}
            await asyncio.sleep(inj.delay_s)
        else:
            await asyncio.sleep(max(0.0, inj.at_s - (clock() - started)))
    r = await env.exec(
        f"python3 {shlex.quote(lib)} inject --message {shlex.quote(inj.message)} "
        f"--trigger {shlex.quote(json.dumps(trigger))}",
        timeout_sec=30,
    )
    record = _json(r.stdout)
    sent_at = clock()
    try:
        delivered = await deliver(inj.message)
        error = None
    except Exception as e:  # noqa: BLE001 - the report says why
        delivered, error = None, f"{type(e).__name__}: {e}"
    return {"injection": asdict(inj), "trigger": trigger, "record": record,
            "sent_after_s": round(sent_at - started, 3), "delivered": delivered, "error": error}


def _json(text: str | None) -> Any:
    try:
        return json.loads((text or "").strip().splitlines()[-1])
    except (IndexError, json.JSONDecodeError):
        return None


# ---------------------------------------------------------------- Theseus


# An execution is busy while it runs, is queued, has a call dispatched (a
# job its turn left running), has results its next turn will read, or waits
# on a due time, a job, or a child task.
BUSY_STATES = {"queued", "running"}
BUSY_WAITS = {"due_at", "actions", "execution"}


def busy(e: dict[str, Any]) -> bool:
    return (
        e.get("state") in BUSY_STATES
        or e.get("outstanding", 0) > 0
        or e.get("queued_results", 0) > 0
        or (e.get("waiting_on") or {}).get("on") in BUSY_WAITS
    )


@dataclass
class Theseus:
    """A trial's daemon, reached as `theseus` with its socket in the
    environment (THESEUS_SOCKET, which the daemon-mode script exports)."""

    bin_dir: str
    state: str
    logs: str

    @property
    def cli(self) -> str:
        return (f"THESEUS_SOCKET={shlex.quote(self.state)}/theseus.sock "
                f"{shlex.quote(self.bin_dir)}/theseus --json")

    def session_command(self, wait_s: int = 30) -> str:
        """Prints the trial's session once the daemon-mode script opened it."""
        f = shlex.quote(f"{self.state}/session")
        return (f"i=0; while [ ! -s {f} ] && [ $i -lt {wait_s * 10} ]; do sleep 0.1; i=$((i+1)); done; "
                f"cat {f}")

    def ask_command(self, session: str, n: int) -> str:
        """The `n`th message on the session, from ASYNC_MESSAGE: queued
        behind a turn that runs, never refused (main's `turn.submit` waits
        for admission)."""
        out = shlex.quote(f"{self.logs}/theseus-turn-{n}.json")
        return (f'printf "%s" "$ASYNC_MESSAGE" | {self.cli} ask -s {shlex.quote(session)} - '
                f"> {out} 2>> {shlex.quote(self.logs)}/theseus.log; echo $?")

    def finish_script(self, session: str) -> str:
        """After the trial: every session stopped (its turn and its jobs),
        the conversation's history, every model call in the ledger (tasks
        have sessions of their own), the tasks, health (its cgroup phase
        says whether jobs got a cgroup), then a clean stop of the daemon."""
        lg, s = shlex.quote(self.logs), shlex.quote(session)
        sock = shlex.quote(f"{self.state}/theseus.sock")
        pid = shlex.quote(f"{self.state}/theseusd.pid")
        return (
            f"{self.cli} executions > {lg}/theseus-executions.json 2>> {lg}/theseus.log; "
            f"for x in $(tr ',' '\\n' < {lg}/theseus-executions.json "
            f"| sed -n 's/.*\"session_id\": *\"\\([^\"]*\\)\".*/\\1/p' | sort -u); "
            f"do {self.cli} stop \"$x\" > /dev/null 2>&1; done; "
            f"{self.cli} history {s} > {lg}/theseus-history.json 2>> {lg}/theseus.log; "
            f"{self.cli} ledger -n 1000 -k provider.call > {lg}/theseus-calls.json 2>> {lg}/theseus.log; "
            f"{self.cli} tasks > {lg}/theseus-tasks.json 2>> {lg}/theseus.log; "
            f"{self.cli} health > {lg}/theseus-health.json 2>> {lg}/theseus.log; "
            f"{self.cli} shutdown > /dev/null 2>> {lg}/theseus.log; "
            f"p=$(cat {pid} 2>/dev/null); i=0; "
            f'while [ -n "$p" ] && kill -0 "$p" 2>/dev/null && [ $i -lt 100 ]; do sleep 0.1; i=$((i+1)); done; '
            f'if [ -n "$p" ] && kill -0 "$p" 2>/dev/null; then echo "theseusd did not stop" >&2; kill -TERM "$p"; fi; '
            f"rm -f {sock}; exit 0"
        )

    async def settle(self, env: Env, deadline: float, clock: Callable[[], float] = time.monotonic) -> str:
        """Wait until nothing is busy and no wake is pending: `settled`, or
        `timeout` at `deadline` (on `clock`). The daemon owns each wait
        (`theseus wait`, after the position the last one saw), so nothing
        polls; a pending wake is slept until it is due."""
        after: dict[str, int] = {}
        rounds = 0
        while (left := deadline - clock()) > 0:
            rounds += 1
            execs = _json((await env.exec(f"{self.cli} executions", timeout_sec=30)).stdout) or {}
            wakes = _json((await env.exec(f"{self.cli} wakes", timeout_sec=30)).stdout) or {}
            live = [e for e in execs.get("executions", []) if busy(e)]
            pending = wakes.get("wakes", [])
            if not live and not pending:
                return "settled"
            if not live:
                due = min(w.get("due_at_ms", 0) for w in pending) / 1000 - time.time()
                await asyncio.sleep(max(0.5, min(due + 0.5, left)))
                continue
            session = live[0]["session_id"]
            pos = f" --after {after[session]}" if session in after else ""
            r = await env.exec(
                f"{self.cli} wait {shlex.quote(session)} --until settled{pos} --timeout {max(1, int(left))}s",
                timeout_sec=int(left) + 30,
            )
            got = _json(r.stdout) or {}
            position = (got.get("execution") or {}).get("position")
            if position is not None:
                after[session] = position
        return "timeout"


def spend(rows: list[dict[str, Any]]) -> dict[str, Any]:
    """A trial's spend: every `provider.call` row of its daemon's ledger
    (`theseus ledger -k provider.call`), tasks' sessions included."""
    s = {"input_tokens": 0, "cache_tokens": 0, "output_tokens": 0, "cost_usd": 0.0, "calls": 0}
    for r in rows:
        if r.get("kind") != "provider.call":
            continue
        data = r.get("data") or {}
        u = data.get("usage") or {}
        read = u.get("cache_read_input_tokens") or 0
        s["input_tokens"] += (u.get("input_tokens") or 0) + read + (u.get("cache_creation_input_tokens") or 0)
        s["cache_tokens"] += read
        s["output_tokens"] += u.get("output_tokens") or 0
        s["cost_usd"] += data.get("cost_usd") or 0.0
        s["calls"] += 1
    s["cost_usd"] = round(s["cost_usd"], 6)
    return s


# ---------------------------------------------------------------- Claude Code

# Harbor 0.23's Claude Code run: the instruction piped into `claude --print`.
CLAUDE_RUN = re.compile(
    r'printf "%s" "\$(?P<var>\w+)" \| claude (?P<flags>.*?)--print 2>&1 \| tee (?P<log>\S+)$'
)
FIRST = "ASYNC_CLAUDE_FIRST"


def user_line(text: str) -> str:
    """One user message in the CLI's stream-json input."""
    return json.dumps({"type": "user", "message": {"role": "user", "content": text},
                       "parent_tool_use_id": None}, ensure_ascii=False)


def claude_stdin(command: str, env: dict[str, str] | None, fifo: str) -> tuple[str, dict[str, str]] | None:
    """Harbor's run command with stdin from `fifo` in stream-json, and the
    instruction as its first line; None for any other command. A holder
    keeps the FIFO open for writing, so the CLI reads on until `close`
    kills it; the driver's messages are lines written to the FIFO."""
    m = CLAUDE_RUN.search(command)
    if not m or env is None:
        return None
    instruction = next((v for k, v in env.items() if k.startswith("HARBOR_CLAUDE_CODE_INSTRUCTION_")), None)
    if instruction is None:
        return None
    f = shlex.quote(fifo)
    run = (
        f'rm -f {f} {f}.holder; mkfifo {f}; '
        f"claude {m['flags']}--input-format=stream-json --print < {f} 2>&1 | tee {m['log']} & cpid=$!; "
        f"sleep 2147483647 > {f} & echo $! > {f}.holder; "
        f'printf "%s\\n" "${FIRST}" > {f}; unset {FIRST}; '
        f'wait $cpid; rc=$?; kill "$(cat {f}.holder)" 2>/dev/null; rm -f {f} {f}.holder; exit $rc'
    )
    return command[: m.start()] + run, {**env, FIRST: user_line(instruction)}


def send_command(fifo: str, text: str) -> str:
    """A message to the running CLI: one line into its FIFO, refused (exit 3)
    when the CLI is gone."""
    f = shlex.quote(fifo)
    return f"[ -p {f} ] && [ -e {f}.holder ] || exit 3; printf '%s\\n' {shlex.quote(user_line(text))} > {f}"


def close_command(fifo: str) -> str:
    """The end of the CLI's input: its holder killed, so it reads EOF."""
    f = shlex.quote(fifo)
    return f'[ -e {f}.holder ] && kill "$(cat {f}.holder)" 2>/dev/null; exit 0'


def results_command(log: str) -> str:
    """How many turns the CLI has finished: its stream's result events."""
    return f"grep -c '^{{\"type\":\"result\"' {shlex.quote(log)} 2>/dev/null || true"
