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
- `KillInjection`, `kill_of` and `fire_kill`: the kill family's injection
  (theseus-7ajc): at its trigger the harness's own process(es) get SIGKILL
  inside the trial's container, `kill` is in the ledger first, and after a
  settle the harness is restarted as its documentation says (`Arm`, one per
  arm: `theseus_arm`, `claude_arm`, `pi_arm`, `opencode_arm`);
- `claude_stdin`: Harbor's Claude Code command, rewritten to read its input
  as stream-json from a FIFO the driver writes to.
- `pi_stdin`: Harbor's Pi command, rewritten to run Pi in its RPC mode,
  reading its commands from a FIFO the driver writes to.

`async_agents.py` holds the Harbor agents that use them. Standard library only,
so the tests (`test_driver.py`) run without Harbor; it reads bench/harbor's
`theseus_bench` and `sampler`, so that directory is on the path, as it is
for the agents.
"""

from __future__ import annotations

import asyncio
import json
import random
import re
import shlex
import time
import tomllib
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Any, Awaitable, Callable, Protocol

import efficiency as ef
import sampler as smp
import theseus_bench as tb

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
    """A family's second message; None for a family with none, and for the
    kill family's injection (`kill_of`)."""
    inj = task_of(task_dir).get("injection")
    if not inj or inj.get("kind", "message") != "message":
        return None
    return Injection(inj["message"], inj["after_tool"], inj["after_kind"],
                     float(inj["delay_s"]), float(inj["at_s"]))


async def _await_trigger(
    env: Env, tool: str, kind: str, delay_s: float, at_s: float, *, started: float, lib: str,
    clock: Callable[[], float],
) -> dict[str, Any]:
    """The ledger's first `kind` record of `tool` plus `delay_s`, else
    `at_s` after the agent started: the trigger that fired, as the ledger
    records it."""
    left = at_s - (clock() - started)
    trigger: dict[str, Any] = {"by": "time", "at_s": at_s}
    if left > 0:
        r = await env.exec(
            f"python3 {shlex.quote(lib)} await --tool {shlex.quote(tool)} "
            f"--kind {shlex.quote(kind)} --timeout {left:.3f}",
            timeout_sec=int(left) + 30,
        )
        event = _json(r.stdout) if r.return_code == 0 else None
        if event:
            trigger = {"by": "event", "tool": tool, "kind": kind,
                       "seq": event["seq"], "event_mono": event["mono"], "delay_s": delay_s}
            await asyncio.sleep(delay_s)
        else:
            await asyncio.sleep(max(0.0, at_s - (clock() - started)))
    return trigger


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
    trigger = await _await_trigger(env, inj.after_tool, inj.after_kind, inj.delay_s, inj.at_s,
                                   started=started, lib=lib, clock=clock)
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


# ---------------------------------------------------------------- the kill injection (theseus-7ajc)

# What the restarted harness is told where its CLI needs a message to
# continue (Claude Code, Pi and OpenCode take a prompt with their continue
# flag; Theseus takes none, its daemon resumes the interrupted turn itself).
# One text for every arm; it never says what the task is or how to run it.
RESUME_PROMPT = "Continue."


@dataclass(frozen=True)
class KillInjection:
    """`[metadata.async.injection]` with `kind = "kill"`. `mode` is "random"
    (a time drawn from `seed` in [`window_lo_s`, `window_hi_s`] after the
    agent started, a window of the family's ideal wall) or "mid-step" (the
    ledger's first `after_kind` of `after_tool` plus `delay_s`, so the kill
    lands in a running sub-step; `at_s` is its fallback time). `settle_s` is
    the wait between the kill and the restart."""

    seed: int
    mode: str
    window_lo_s: float
    window_hi_s: float
    after_tool: str
    after_kind: str
    delay_s: float
    at_s: float
    settle_s: float = 2.0


def kill_of(task_dir: Path) -> KillInjection | None:
    inj = task_of(task_dir).get("injection")
    if not inj or inj.get("kind") != "kill":
        return None
    return KillInjection(int(inj["seed"]), inj["mode"], float(inj["window_lo_s"]), float(inj["window_hi_s"]),
                         inj["after_tool"], inj["after_kind"], float(inj["delay_s"]), float(inj["at_s"]),
                         float(inj.get("settle_s", 2.0)))


def kill_time(k: KillInjection, salt: str = "") -> float:
    """A random-mode kill's time, seconds after the agent started: drawn
    from the seed (and `salt`, which a caller may use to tell attempts
    apart), so the same trial is killed at the same moment."""
    return round(random.Random(f"{k.seed}:{salt}").uniform(k.window_lo_s, k.window_hi_s), 3)


@dataclass(frozen=True)
class Arm:
    """How an arm is killed and continued, in its container.

    `find` prints the JSON list of the harness's processes (`asyncbench.py
    procs`); `restart` starts the continuation detached, its pid in
    `pidfile`, or is None for an arm that cannot continue a session: scored
    "not resumable", never a crash. `docs` names where the arm's own
    documentation says to continue."""

    name: str
    find: str
    restart: str | None
    pidfile: str | None
    docs: str


def procs_command(*selectors: str, lib: str = LIB) -> str:
    return f"python3 {shlex.quote(lib)} procs " + " ".join(selectors)


def detached(inner: str, pidfile: str, log: str) -> str:
    """`inner` started in the background, its pid in `pidfile`, its output
    appended to `log`; the command returns at once."""
    return (f"nohup sh -c {shlex.quote(inner)} >> {shlex.quote(log)} 2>&1 < /dev/null & "
            f"echo $! > {shlex.quote(pidfile)}")


def alive_command(pidfile: str) -> str:
    """Exit 0 while the restarted harness runs."""
    f = shlex.quote(pidfile)
    return f'p=$(cat {f} 2>/dev/null); [ -n "$p" ] && kill -0 "$p" 2>/dev/null'


# Where the CLIs' processes are found: the CLI itself, never the shell that
# runs it (a kill of the root of the CLI's tree; its children are left).
_SHELLS = "--not-comm bash,sh,dash,tee,timeout,stdbuf,grep,sleep"


def theseus_arm(bin_dir: str, state: str, logs: str, session: str, lib: str = LIB) -> Arm:
    """Theseus: kill `theseusd` (by its pid file) and the CLI client (comm
    `theseus`: the arm's own asks; the driver's calls run as `async-driver`
    and its job wrappers as `theseusd job-wrapper`, which keep their own
    comm and are not killed here: the wrappers outlive the daemon by
    design). Restart as `theseusd` serves it: start `theseusd` again on the
    same state, which on startup requeues the interrupted turn and resumes
    it with its unfinished calls answered "unknown" (the spec's execution
    model; crates/theseus-kernel's startup, toolrun/resume.rs), then
    reattach the CLI as the daemon-mode script and `Theseus.settle` do:
    `theseus wait SESSION --until settled`. The continuation is the wait; no
    message is sent."""
    b, st, lg = shlex.quote(bin_dir), shlex.quote(state), shlex.quote(logs)
    sock = f"{st}/theseus.sock"
    pidfile = f"{state}/restart.pid"
    start = (
        f"export THESEUS_SOCKET={sock}; detach=$(command -v setsid || true); "
        f"$detach {b}/theseusd < /dev/null > /dev/null 2>> {lg}/theseus.log & echo $! > {st}/theseusd.pid; "
        f"i=0; until {b}/theseus --json health > /dev/null 2>&1; do i=$((i+1)); "
        f"if [ $i -ge 300 ]; then echo 'theseus: the daemon did not answer in 30 s' >&2; exit 3; fi; sleep 0.1; done; "
        + detached(f"{b}/theseus --json wait {shlex.quote(session)} --until settled", pidfile, f"{logs}/theseus.log")
    )
    return Arm("theseus",
               procs_command(f"--pidfile {st}/theseusd.pid", "--comm theseus", lib=lib),
               start, pidfile,
               "docs/spec: the execution model (a running turn is requeued as interrupted on restart)")


def claude_parts(command: str) -> tuple[str, str] | None:
    """Harbor's Claude Code run: what precedes the CLI (its PATH, nvm) and
    its flags, for the continuation to keep."""
    m = CLAUDE_RUN.search(command)
    return (command[: m.start()], m["flags"]) if m else None


def claude_arm(flags: str, log: str, prefix: str = "") -> Arm:
    """Claude Code: kill the `claude` process (the root of the CLI's tree,
    not its shell or `tee`); restart with `claude --continue --print`, which
    its CLI reference gives as "continue the most recent conversation in the
    current directory" (`claude -c -p "query"`; `claude --help`: `-c,
    --continue`), the flags of the first run kept."""
    pidfile = "/tmp/async-claude-restart.pid"
    inner = (f"{prefix}claude {flags}--continue --print {shlex.quote(RESUME_PROMPT)} 2>&1 | tee -a {shlex.quote(log)}")
    return Arm("claude-code",
               procs_command(r"--cmd 'claude\b.*--print'", _SHELLS, "--root"),
               detached(inner, pidfile, "/dev/null"), pidfile,
               "Claude Code CLI reference: `claude -c -p`, `--continue`")


def pi_parts(command: str) -> tuple[str, str] | None:
    m = PI_RUN.search(command)
    return (command[: m.start()], m["flags"]) if m else None


def pi_arm(flags: str, log: str, prefix: str = "") -> Arm:
    """Pi: kill the `pi` process (RPC mode's root); restart with `pi --print
    --mode json --continue`, "continue the most recent session" (`pi
    --help`: `-c, --continue`), on the same `--session-dir`, flags kept."""
    pidfile = "/tmp/async-pi-restart.pid"
    inner = (f"{prefix}pi --print --mode json {flags}--continue {shlex.quote(RESUME_PROMPT)} "
             f"2>&1 </dev/null | tee -a {shlex.quote(log)}")
    return Arm("pi", procs_command(r"--cmd '\bpi\b.*--mode rpc'", _SHELLS, "--root"),
               detached(inner, pidfile, "/dev/null"), pidfile, "Pi CLI: `pi --continue`")


OPENCODE_RUN = re.compile(r"opencode (?P<model>--model=\S+) run (?P<flags>.*?)-- ")


def opencode_parts(command: str) -> tuple[str, str, str] | None:
    """What precedes `opencode`, its `--model=` and its `run` flags."""
    m = OPENCODE_RUN.search(command)
    return (command[: m.start()], m["model"], m["flags"]) if m else None


def opencode_arm(model: str, flags: str, log: str, prefix: str = "") -> Arm:
    """OpenCode: kill the `opencode run` process; restart with `opencode run
    --continue`, "continue the last session" (OpenCode's CLI docs: `opencode
    run --continue`, `-c`), model and flags kept."""
    pidfile = "/tmp/async-opencode-restart.pid"
    inner = (f"{prefix}opencode {model} run {flags}--continue -- {shlex.quote(RESUME_PROMPT)} "
             f"2>&1 | tee -a {shlex.quote(log)}")
    return Arm("opencode", procs_command(r"--cmd 'opencode\b.*\brun\b'", _SHELLS, "--root"),
               detached(inner, pidfile, "/dev/null"), pidfile, "OpenCode CLI: `opencode run --continue`")


async def fire_kill(
    kill: KillInjection,
    arm: Arm | Callable[[], Arm],
    env: Env,
    *,
    started: float,
    lib: str = LIB,
    clock: Callable[[], float] = time.monotonic,
    sleep: Callable[[float], Awaitable[Any]] = asyncio.sleep,
    salt: str = "",
    restart_env: dict[str, str] | Callable[[], dict[str, str]] | None = None,
    cwd: str | None = None,
) -> dict[str, Any]:
    """The kill injection: wait for the trigger, find the harness's
    processes, write `kill` to the ledger (the trigger and the pids), send
    SIGKILL to those pids and no others, wait the settle, restart the
    harness by its arm's command, and write `restart` to the ledger. Never
    raises: an arm with nothing to kill gets an `error` (not measurable), one
    with no way to continue `resumable: false`. Everything runs through
    `env.exec`, inside the trial's container."""
    q = shlex.quote
    if kill.mode == "random":
        at = kill_time(kill, salt)
        left = at - (clock() - started)
        if left > 0:
            await sleep(left)
        trigger: dict[str, Any] = {"by": "seed", "seed": kill.seed, "mode": "random", "at_s": at}
    else:
        trigger = await _await_trigger(env, kill.after_tool, kill.after_kind, kill.delay_s, kill.at_s,
                                       started=started, lib=lib, clock=clock)
        trigger["mode"] = "mid-step"
        trigger["seed"] = kill.seed
    # An arm that learns its command's flags only when its CLI starts is
    # given as a function, read now.
    arm = arm() if callable(arm) else arm
    report: dict[str, Any] = {"arm": arm.name, "trigger": trigger, "docs": arm.docs, "pidfile": arm.pidfile}
    found = _json((await env.exec(arm.find, timeout_sec=30)).stdout) or []
    pids = [p for p in found if isinstance(p, dict) and int(p.get("pid", 0)) > 1]
    if not pids:
        report["error"] = "no harness process to kill: not measurable"
        report["found"] = found
        report["find"] = arm.find
        return report
    r = await env.exec(
        f"python3 {q(lib)} kill-record --trigger {q(json.dumps(trigger))} --pids {q(json.dumps(pids))} "
        f"--settle {kill.settle_s}", timeout_sec=30)
    report["record"] = _json(r.stdout)
    report["killed"] = pids
    report["killed_after_s"] = round(clock() - started, 3)
    # Only the pids found, and only inside this container: `env.exec` runs there.
    await env.exec("kill -KILL " + " ".join(str(int(p["pid"])) for p in pids) + " 2>/dev/null; exit 0",
                   timeout_sec=30)
    await sleep(kill.settle_s)
    resumable = arm.restart is not None
    note = None if resumable else "not resumable: the arm has no way to continue a session"
    cmd_args = f"--resumable {'true' if resumable else 'false'}" + (
        f" --command {q(arm.restart)}" if arm.restart else "") + (f" --note {q(note)}" if note else "")
    if arm.restart:
        renv = restart_env() if callable(restart_env) else restart_env
        rr = await env.exec(arm.restart, env=renv, cwd=cwd, timeout_sec=120)
        report["restart_exit"] = rr.return_code
        if rr.return_code != 0:
            report["error"] = f"the restart failed (exit {rr.return_code}): {(rr.stderr or '').strip()[-300:]}"
    else:
        report["error"] = note
    report["resumable"] = resumable
    r = await env.exec(f"python3 {q(lib)} restart-record {cmd_args}", timeout_sec=30)
    report["restart_record"] = _json(r.stdout)
    return report


async def wait_restarted(env: Env, kill_report: dict[str, Any], deadline: float,
                         clock: Callable[[], float] = time.monotonic,
                         sleep: Callable[[float], Awaitable[Any]] = asyncio.sleep) -> str:
    """Wait until the restarted harness has exited, or `deadline` (on
    `clock`): `ended` or `timeout`; `none` for an arm that was not
    restarted. The driver's own poll, every 2 s, as the Claude arm's."""
    pidfile = kill_report.get("pidfile")
    if not pidfile or not kill_report.get("resumable") or kill_report.get("restart_exit"):
        return "none"
    while clock() < deadline:
        if (await env.exec(alive_command(pidfile), timeout_sec=30)).return_code != 0:
            return "ended"
        await sleep(2)
    return "timeout"


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

    def _cli(self, exe: str) -> str:
        return f"THESEUS_SOCKET={shlex.quote(self.state)}/theseus.sock {shlex.quote(exe)} --json"

    @property
    def cli(self) -> str:
        """The driver's own calls (settle's polls, the finish's reads and
        stops): through `<state>/async-driver`, the daemon script's link to
        `theseus` (`theseus_bench.POLL`), so the sampler counts them outside
        the harness."""
        return self._cli(f"{self.state}/{tb.POLL}")

    @property
    def ask_cli(self) -> str:
        """The arm's own client, the asks: `theseus`, harness by name."""
        return self._cli(f"{self.bin_dir}/theseus")

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
        return (f'printf "%s" "$ASYNC_MESSAGE" | {self.ask_cli} ask -s {shlex.quote(session)} - '
                f"> {out} 2>> {shlex.quote(self.logs)}/theseus.log; echo $?")

    def finish_script(self, session: str, sampler: str | None = None) -> str:
        """After the trial: every session stopped (its turn and its jobs),
        the conversation's history, the tasks and each task session's
        history (`efficiency.task_history_file`), every model call in the
        ledger and every call a stop cut (tasks have sessions of their own),
        health (its cgroup phase says whether jobs got a cgroup), then a
        clean stop of the daemon. Each ledger read is the newest
        `efficiency.LEDGER_CAP` rows: the record says when one is full.
        With `sampler` (`daemon_script` started it), the harness sampler is
        stopped last, after the daemon's stop, so its whole life is sampled."""
        lg, s = shlex.quote(self.logs), shlex.quote(session)
        sock = shlex.quote(f"{self.state}/theseus.sock")
        pid = shlex.quote(f"{self.state}/theseusd.pid")
        return (
            f"{self.cli} sessions --state > {lg}/theseus-executions.json 2>> {lg}/theseus.log; "
            f"for x in $(tr ',' '\\n' < {lg}/theseus-executions.json "
            f"| sed -n 's/.*\"session_id\": *\"\\([^\"]*\\)\".*/\\1/p' | sort -u); "
            f"do {self.cli} stop \"$x\" > /dev/null 2>&1; done; "
            f"{self.cli} history {s} > {lg}/theseus-history.json 2>> {lg}/theseus.log; "
            f"{self.cli} tasks > {lg}/theseus-tasks.json 2>> {lg}/theseus.log; "
            f"for t in $(tr ',' '\\n' < {lg}/theseus-tasks.json "
            f"| sed -n 's/.*\"task_id\": *\"\\([^\"]*\\)\".*/\\1/p' | grep -E '^[A-Za-z0-9_-]+$' | sort -u); "
            f"do {self.cli} history \"$t\" > {lg}/theseus-history-\"$t\".json 2>> {lg}/theseus.log; done; "
            f"{self.cli} ledger -n {ef.LEDGER_CAP} -k provider.call > {lg}/theseus-calls.json 2>> {lg}/theseus.log; "
            f"{self.cli} ledger -n {ef.LEDGER_CAP} -k provider.cut > {lg}/theseus-cuts.json 2>> {lg}/theseus.log; "
            f"{self.cli} health > {lg}/theseus-health.json 2>> {lg}/theseus.log; "
            f"{self.cli} shutdown > /dev/null 2>> {lg}/theseus.log; "
            f"p=$(cat {pid} 2>/dev/null); i=0; "
            f'while [ -n "$p" ] && kill -0 "$p" 2>/dev/null && [ $i -lt 100 ]; do sleep 0.1; i=$((i+1)); done; '
            f'if [ -n "$p" ] && kill -0 "$p" 2>/dev/null; then echo "theseusd did not stop" >&2; kill -TERM "$p"; fi; '
            f"rm -f {sock}; "
            + (f"{self.stop_sampler_script()}; " if sampler else "")
            + "exit 0"
        )

    def stop_sampler_script(self) -> str:
        """The harness sampler stopped, its last summary written: the
        finish's last step, or the trial's end when no session opened."""
        return smp.stop_script(self.logs, self.state)

    async def settle(self, env: Env, deadline: float, clock: Callable[[], float] = time.monotonic) -> str:
        """Wait until nothing is busy and no wake is pending: `settled`, or
        `timeout` at `deadline` (on `clock`). The daemon owns each wait
        (`theseus wait`, after the position the last one saw), so nothing
        polls; a pending wake is slept until it is due."""
        after: dict[str, int] = {}
        rounds = 0
        while (left := deadline - clock()) > 0:
            rounds += 1
            execs = _json((await env.exec(f"{self.cli} sessions --state", timeout_sec=30)).stdout) or {}
            wakes = _json((await env.exec(f"{self.cli} wake", timeout_sec=30)).stdout) or {}
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
    (`theseus ledger -k provider.call`), tasks' sessions included, and every
    `provider.cut` row (a call a stop cut, at the kernel's estimate of its
    input, output, and dollars), counted apart as `cut_calls`."""
    s = {"input_tokens": 0, "cache_tokens": 0, "output_tokens": 0, "cost_usd": 0.0, "calls": 0,
         "cut_calls": 0}
    for r in rows:
        data = r.get("data") or {}
        if r.get("kind") == "provider.cut":
            s["input_tokens"] += data.get("input_tokens") or 0
            s["output_tokens"] += data.get("output_tokens") or 0
            s["cost_usd"] += data.get("cost_usd") or 0.0
            s["cut_calls"] += 1
            continue
        if r.get("kind") != "provider.call":
            continue
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


def log_command(log: str) -> str:
    """Where the CLI's stream is: `LAST TOTAL`, the line of its last result
    event (0 for none) and its lines. A message is answered once a result
    lands after the line count read when it was sent, whether the CLI took
    it mid-turn or as a turn of its own."""
    f = shlex.quote(log)
    # Not anchored: Claude Code writes the result's `type` after other keys.
    return (f"r=$(grep -n '\"type\":\"result\"' {f} 2>/dev/null | tail -n 1 | cut -d: -f1); "
            f'n=$(wc -l < {f} 2>/dev/null); echo "${{r:-0}} ${{n:-0}}"')


def log_state(stdout: str | None) -> tuple[int, int]:
    try:
        last, total = (stdout or "").split()
        return int(last), int(total)
    except ValueError:
        return 0, 0


# ---------------------------------------------------------------------- Pi

# Harbor 0.23's Pi run (pi.py's run): the instruction, shell-quoted, as
# `pi --print --mode json`'s last argument, its stream filtered and teed.
PI_RUN = re.compile(
    r"pi --print --mode json (?P<flags>.*?)(?P<instruction>(?:'(?:[^']|'\"'\"')*')+|[^\s']+) "
    r"2>&1 </dev/null \| (?P<filter>grep -v '\"type\":\"message_update\"' \| stdbuf -oL )?tee (?P<log>\S+)$"
)
PI_FIRST = "ASYNC_PI_FIRST"


def rpc_prompt(text: str, steer: bool = False) -> str:
    """One `prompt` command of Pi's RPC mode (its rpc-commands.md). With
    `steer`, a prompt that comes while Pi runs is delivered after the
    current answer's tool calls, before its next model call; while Pi is
    idle it starts a run (Pi 1.0.4: `disposition: started`)."""
    cmd: dict[str, Any] = {"type": "prompt", "message": text}
    if steer:
        cmd["streamingBehavior"] = "steer"
    return json.dumps(cmd, ensure_ascii=False)


def pi_stdin(command: str, env: dict[str, str] | None, fifo: str) -> tuple[str, dict[str, str]] | None:
    """Harbor's Pi run with Pi in RPC mode (`--mode rpc`, JSONL commands on
    stdin, its session events on stdout, as JSON mode's) reading from
    `fifo`, and the instruction as its first `prompt`; None for any other
    command. A holder keeps the FIFO open for writing, so Pi reads on until
    `close_command` kills it: closing its stdin is RPC mode's orderly
    shutdown."""
    m = PI_RUN.search(command)
    if not m:
        return None
    instruction = shlex.split(m["instruction"])[0]
    f = shlex.quote(fifo)
    run = (
        f'rm -f {f} {f}.holder; mkfifo {f}; '
        # Pi's stream is read as it runs, so its filter writes each line as
        # it comes (Harbor's own buffers it, read only at the end).
        f"pi --mode rpc {m['flags']}< {f} 2>&1 | {'stdbuf -oL ' + m['filter'] if m['filter'] else ''}"
        f"tee {m['log']} & cpid=$!; "
        f"sleep 2147483647 > {f} & echo $! > {f}.holder; "
        f'printf "%s\\n" "${PI_FIRST}" > {f}; unset {PI_FIRST}; '
        f'wait $cpid; rc=$?; kill "$(cat {f}.holder)" 2>/dev/null; rm -f {f} {f}.holder; exit $rc'
    )
    return command[: m.start()] + run, {**(env or {}), PI_FIRST: rpc_prompt(instruction)}


def pi_send_command(fifo: str, text: str) -> str:
    """A message to the running Pi: one steering `prompt` into its FIFO,
    refused (exit 3) when Pi is gone."""
    f = shlex.quote(fifo)
    return (f"[ -p {f} ] && [ -e {f}.holder ] || exit 3; "
            f"printf '%s\\n' {shlex.quote(rpc_prompt(text, steer=True))} > {f}")


def pi_log_command(log: str) -> str:
    """Where Pi's stream is: `LAST TOTAL`, the line of its last
    `agent_settled` (Pi will do no more on its own) and its lines, as
    `log_command` reads Claude Code's results."""
    f = shlex.quote(log)
    return (f"r=$(grep -n '\"type\":\"agent_settled\"' {f} 2>/dev/null | tail -n 1 | cut -d: -f1); "
            f'n=$(wc -l < {f} 2>/dev/null); echo "${{r:-0}} ${{n:-0}}"')
