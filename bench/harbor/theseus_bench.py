"""What the Harbor adapter does that needs no Harbor (theseus-n88g.3).

- `profile`: the bench profile (`bench/theseus-bench.toml`) with the lines a
  trial sets (the model, the limits, the task's working directory);
- `run_script` and `stop_script`: the shell a trial's container runs, with
  the harness sampler around the turn (`sampler.py`, theseus-7gir.12);
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
    return s


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
               sample_ms: int = smp.INTERVAL_MS) -> str:
    """The trial's one command, run as the task's user, with the instruction
    in `THESEUS_BENCH_INSTRUCTION` (unset before anything else starts).

    `theseus --spawn theseusd --json ask -` runs the turn in the background,
    its pid kept for `stop_script`; then its exit code is written, the
    session's history is read from the store (the trajectory, and the spend
    of a turn cut short), and the done marker is written. A failure's last
    `theseus:` line goes to stderr, where Harbor reads a failed command's
    cause, and the command exits with `ask`'s code.

    With `sampler` (its path in the container), the harness sampler
    (`sampler.py`, theseus-7gir.12) starts before the turn and stops after
    the history is read, before the done marker; it never fails the run."""
    b, s, lg = (shlex.quote(p) for p in (bin_dir, state, logs))
    spawn = f"{b}/theseus --spawn {b}/theseusd --json"
    arm = ef.ARMS["theseus"]
    start = stop = ""
    if sampler:
        start = smp.start_script(sampler, logs, state, arm["names"], arm["wrapper_args"],
                                 sample_ms) + "; "
        stop = smp.stop_script(logs, state) + "; "
    return (
        'instruction="$THESEUS_BENCH_INSTRUCTION"; unset THESEUS_BENCH_INSTRUCTION; '
        f"rm -f {lg}/{DONE}; "
        f"{start}"
        f'printf "%s" "$instruction" | {spawn} ask - > {lg}/{TURN} 2> {lg}/{LOG} & '
        f"echo $! > {s}/ask.pid; wait $!; rc=$?; "
        f"echo $rc > {lg}/{EXIT}; "
        f"{spawn} history > {lg}/{HISTORY} 2>> {lg}/{LOG}; "
        f"{stop}"
        f"touch {lg}/{DONE}; "
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
