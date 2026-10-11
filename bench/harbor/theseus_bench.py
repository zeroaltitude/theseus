"""What the Harbor adapter does that needs no Harbor (theseus-n88g.3).

- `profile`: the bench profile (`bench/theseus-bench.toml`) with the lines a
  trial sets (the model, the limits, the task's working directory);
- `run_script` and `stop_script`: the shell a trial's container runs, with
  the harness sampler around the turn (`sampler.py`, theseus-7gir.12);
  `daemon_script`, the async bench's trial on a daemon of its own
  (`bench/async/`);
- `ENDS`: what each of `theseus ask`'s exit codes says (theseus-n88g.2,
  `theseus ask --help`).

Standard library only, so the tests (`test_bench.py`) run without Harbor.
"""

from __future__ import annotations

import json
import re
import shlex
import tomllib
from pathlib import Path
from typing import Any

import efficiency as ef
import sampler as smp

PROFILE = Path(__file__).resolve().parent.parent / "theseus-bench.toml"
# The routed arm's profile (theseus-eo3h): the plain one with the judge and
# the routing table on, and the Jev key as a second secret.
PROFILE_ROUTED = Path(__file__).resolve().parent.parent / "theseus-bench-routed.toml"
# The profiles the routed arm names, each given the trial's loop cap.
ROUTED_PROFILES = ("sonnet", "opus", "fable", "haiku", "haikuhi")
# The environment variable that holds the Jev key (the profile's
# `jev_api_key = "env:TYPESAFE_API_KEY"`): passed into the container for the
# routed arm only.
JEV_KEY_ENV = "TYPESAFE_API_KEY"
# The most one trial may spend, by default (`THESEUS_BENCH_SPEND_LIMIT`). The
# kernel reserves a call's worst case before it runs, and the profiles cap no
# output (128,000 tokens): one Opus 5.5 call reserves about $2.6 and one Fable
# 5.1 call about $6.5, so the plain arm's $2 refuses the first call the router
# sends to either (stop reason `budget`, exit 5). The routed arm's default
# clears Fable's reserve with room for what the trial has spent (review of
# theseus-eo3h; the number is the owner's).
SPEND_LIMIT_USD = 2.0
ROUTED_SPEND_LIMIT_USD = 20.0

# `theseus ask`'s exit codes, by how the turn ended (theseus-n88g.2).
ENDS = {
    0: "done",
    1: "failed",
    2: "usage",
    3: "unreachable",
    5: "spend_limit",
    6: "waiting",
    7: "refused",
    8: "cut",
    9: "stopped",
    130: "signalled",
    143: "signalled",
}

# The files a trial's run leaves in its agent log directory.
TURN = "theseus-turn.json"
HISTORY = "theseus-history.json"
EXIT = "theseus-exit.txt"
LOG = "theseus.log"
DONE = "theseus-done"
# The routed arm's two reads of the ledger, taken after the history: Jev's
# judgments (`judge.call`, with their cost) and where each turn ran
# (`route.decided`).
# How long `ask` follows what its turn left for later (theseus-mqxk; the CLI's
# default is 30 minutes): 0, so a trial ends with its turn, as every run before
# the follow did, and a rerun compares with them (theseus-mqxk's review). A run
# that wants late results back sets THESEUS_BENCH_FOLLOW_FOR to the task's
# agent timeout less a margin (Harbor's SIGTERM at the timeout ends the follow).
FOLLOW_FOR = "0"
FOLLOW_FOR_ENV = "THESEUS_BENCH_FOLLOW_FOR"
JUDGE = "theseus-judge.json"
ROUTES = "theseus-routes.json"


def ended(code: int | None) -> str:
    """`done`, `failed`, `spend_limit`, …: what an exit code says."""
    return "unknown" if code is None else ENDS.get(code, "unknown")


def settings(
    model: str,
    workdir: str,
    *,
    max_loops: int = 200,
    spend_limit_usd: float = 2.0,
    proc_sync_secs: int = 900,
    system: str | None = None,
    api_base: str | None = None,
    routed: bool = False,
) -> dict[tuple[str, str], Any]:
    """The `(table, key)` values a trial sets in the profile: the lines it
    marks "set by the adapter", and the optional system text and API base."""
    s: dict[tuple[str, str], Any] = {
        ("model", "model"): model,
        ("profiles.bench", "model"): model,
        ("model", "max_loops"): max_loops,
        ("profiles.bench", "max_loops"): max_loops,
        ("kernel", "spend_limit_usd"): float(spend_limit_usd),
        ("tools", "projects_dir"): workdir,
        ("tools", "proc_sync_secs"): proc_sync_secs,
    }
    if system:
        s[("profiles.bench", "system")] = system
    if api_base:
        s[("model", "api_base")] = api_base
    if routed:
        # The profiles the router may pick run as many loops as `bench` does.
        for name in ROUTED_PROFILES:
            s[(f"profiles.{name}", "max_loops")] = max_loops
    return s


def profile_effort(text: str | None = None, path: Path | None = None) -> str | None:
    """The reasoning effort the bench profile asks for, `[profiles.bench]
    effort` (theseus-n6p5): the same one the other arms ask for
    (`measure.EFFORT`)."""
    parsed = tomllib.loads((path or PROFILE).read_text() if text is None else text)
    return parsed.get("profiles", {}).get("bench", {}).get("effort")


def profile(text: str, values: dict[tuple[str, str], Any]) -> str:
    """`text`, a TOML profile, with each `(table, key)` of `values` set: the
    line that holds the key is replaced, a missing key goes at the top of its
    table, and a missing table is added at the end. The result is parsed, and
    each value checked, before it is returned."""
    lines = text.splitlines()
    table, found = "", set()
    for i, line in enumerate(lines):
        head = _header(line)
        if head is not None:
            table = head
            continue
        m = re.match(r"\s*([A-Za-z0-9_-]+)\s*=", line)
        if m and (table, m.group(1)) in values:
            lines[i] = f"{m.group(1)} = {_toml(values[(table, m.group(1))])}"
            found.add((table, m.group(1)))
    for t, k in values:
        if (t, k) in found:
            continue
        entry = f"{k} = {_toml(values[(t, k)])}"
        at = next((i for i, line in enumerate(lines) if _header(line) == t), None)
        if at is None:
            lines += ["", f"[{t}]", entry]
        else:
            lines.insert(at + 1, entry)
    result = "\n".join(lines) + "\n"
    parsed = tomllib.loads(result)
    for (t, k), v in values.items():
        got = parsed
        for part in t.split("."):
            got = got.get(part, {})
        if got.get(k) != v:
            raise ValueError(f"the profile's [{t}] {k} is {got.get(k)!r}, not {v!r}")
    return result


def run_script(bin_dir: str, state: str, logs: str, sampler: str | None = None,
               sample_ms: int = smp.INTERVAL_MS, routed: bool = False,
               follow_for: str = FOLLOW_FOR) -> str:
    """The trial's one command, run as the task's user, with the instruction
    in `THESEUS_BENCH_INSTRUCTION` (unset before anything else starts).

    `theseus --spawn theseusd --json ask --follow-for <follow_for> -` runs the
    turn in the background (following what it left for later for at most
    `follow_for`, `FOLLOW_FOR`'s 0 by default), its pid kept for
    `stop_script`; then its exit code is written, the
    session's history is read from the store (the trajectory, and the spend
    of a turn cut short), and the done marker is written. A failure's last
    `theseus:` line goes to stderr, where Harbor reads a failed command's
    cause, and the command exits with `ask`'s code.

    With `sampler` (its path in the container), the harness sampler
    (`sampler.py`, theseus-7gir.12) starts before the turn and stops after
    the history is read, before the done marker; it never fails the run.

    With `routed` (theseus-eo3h), the history is followed by two reads of the
    ledger, `JUDGE` (the `judge.call` rows: Jev's calls and their cost) and
    `ROUTES` (the `route.decided` rows: which profile each turn ran on), at
    most 1000 rows each. A failed read leaves its file empty and fails
    nothing."""
    b, s, lg = (shlex.quote(p) for p in (bin_dir, state, logs))
    spawn = f"{b}/theseus --spawn {b}/theseusd --json"
    arm = ef.ARMS["theseus"]
    start = stop = ""
    if sampler:
        start = smp.start_script(sampler, logs, state, arm["names"], arm["wrapper_args"],
                                 sample_ms) + "; "
        stop = smp.stop_script(logs, state) + "; "
    ledger = ""
    if routed:
        ledger = "".join(
            f"{spawn} ledger -n 1000 -k {kind} > {lg}/{out} 2>> {lg}/{LOG}; "
            for kind, out in (("judge.call", JUDGE), ("route.decided", ROUTES))
        )
    return (
        'instruction="$THESEUS_BENCH_INSTRUCTION"; unset THESEUS_BENCH_INSTRUCTION; '
        f"rm -f {lg}/{DONE}; "
        f"{start}"
        f'printf "%s" "$instruction" | {spawn} ask --follow-for {shlex.quote(follow_for)} - '
        f"> {lg}/{TURN} 2> {lg}/{LOG} & "
        f"echo $! > {s}/ask.pid; wait $!; rc=$?; "
        f"echo $rc > {lg}/{EXIT}; "
        f"{spawn} history > {lg}/{HISTORY} 2>> {lg}/{LOG}; "
        f"{ledger}"
        f"{stop}"
        f"touch {lg}/{DONE}; "
        f"if [ $rc -ne 0 ]; then grep '^theseus: ' {lg}/{LOG} | tail -n 1 >&2; fi; "
        "exit $rc"
    )


# The async bench's driver reaches the daemon through a link of this name
# beside the state, so its polls' `comm` is not a harness name (`ARMS`).
POLL = "async-driver"


def daemon_script(bin_dir: str, state: str, logs: str, sampler: str | None = None,
                  sample_ms: int = smp.INTERVAL_MS) -> str:
    """The async bench's trial (theseus-7gir.16): a real daemon for the
    whole trial, not one turn on stdio, so a job, a task, or a wake that
    outlives the turn is still read. The daemon starts in a session of its
    own on `<state>/theseus.sock` (its pid in `theseusd.pid`), the
    conversation's session is opened (its id in `session`, where the driver
    reads it to send its messages), and the instruction is its first turn;
    the command exits with that `ask`'s code, as `run_script`'s does, and
    leaves the daemon running for the driver's settle and finish.

    `<state>/async-driver` (`POLL`) is a link to `theseus` for the driver's
    own calls, so the sampler sees them as outside the harness. With
    `sampler` (its path in the container), the harness sampler starts before
    the daemon, in a session of its own so that it outlives this command, as
    `run_script` runs it (theseusd's job wrappers apart); the driver's finish
    stops it after the daemon's clean stop (`driver.Theseus.finish_script`)."""
    b, s, lg = (shlex.quote(p) for p in (bin_dir, state, logs))
    cli = f"{b}/theseus --json"
    start = ""
    if sampler:
        arm = ef.ARMS["theseus"]
        start = "$detach sh -c " + shlex.quote(
            smp.start_script(sampler, logs, state, arm["names"], arm["wrapper_args"], sample_ms)) + "; "
    return (
        'instruction="$THESEUS_BENCH_INSTRUCTION"; unset THESEUS_BENCH_INSTRUCTION; '
        f"export THESEUS_SOCKET={s}/theseus.sock; rm -f {s}/session {lg}/{DONE}; "
        f"ln -sf {b}/theseus {s}/{POLL}; "
        "detach=$(command -v setsid || true); "
        f"{start}"
        f"$detach {b}/theseusd < /dev/null > /dev/null 2>> {lg}/{LOG} & echo $! > {s}/theseusd.pid; "
        f"i=0; until {cli} health > /dev/null 2>&1; do i=$((i+1)); "
        f"if [ $i -ge 100 ]; then echo 'theseus: the daemon did not answer in 10 s' >&2; exit 3; fi; "
        "sleep 0.1; done; "
        f"ses=$({b}/theseus sessions open --label bench 2>> {lg}/{LOG}) || exit 3; "
        f'echo "$ses" > {s}/session; '
        f'printf "%s" "$instruction" | {cli} ask -s "$ses" - > {lg}/{TURN} 2>> {lg}/{LOG}; rc=$?; '
        f"echo $rc > {lg}/{EXIT}; "
        f"if [ $rc -ne 0 ]; then grep '^theseus: ' {lg}/{LOG} | tail -n 1 >&2; fi; "
        "exit $rc"
    )


def stop_script(state: str, logs: str, wait_secs: int = 20) -> str:
    """After Harbor's timeout: a SIGTERM to the turn's `theseus`, which
    stops the turn as `/stop` does and its daemon cleanly (exit 9), then a
    wait for the run's own end (its history read, the sampler stopped, the
    done marker). A second SIGTERM, if the first is not done in `wait_secs`,
    ends it at once (143), and the daemon still stops cleanly. A sampler the
    run's end did not stop is stopped last."""
    s, lg = shlex.quote(state), shlex.quote(logs)
    wait = (
        f"i=0; while [ ! -e {lg}/{DONE} ] && [ $i -lt {int(wait_secs)} ]; "
        "do sleep 1; i=$((i+1)); done"
    )
    return (
        f'pid=$(cat {s}/ask.pid 2>/dev/null); if [ -n "$pid" ]; then '
        f'kill -TERM "$pid" 2>/dev/null; {wait}; '
        f'[ -e {lg}/{DONE} ] || kill -TERM "$pid" 2>/dev/null; {wait}; fi; '
        f"{smp.stop_script(logs, state)}; exit 0"
    )


def _header(line: str) -> str | None:
    m = re.match(r"\s*\[([A-Za-z0-9_.\-]+)\]\s*(#.*)?$", line)
    return m.group(1) if m else None


def _toml(v: Any) -> str:
    if isinstance(v, bool):
        return "true" if v else "false"
    if isinstance(v, (int, float)):
        return repr(v)
    if isinstance(v, str):
        # A JSON string is a TOML basic string: the same escapes.
        return json.dumps(v, ensure_ascii=False)
    if isinstance(v, list):
        return "[" + ", ".join(_toml(x) for x in v) + "]"
    raise TypeError(f"no TOML for {v!r}")
