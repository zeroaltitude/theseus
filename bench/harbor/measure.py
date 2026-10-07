"""What the three Harbor arms share in a task's container (theseus-n6p5).

Standard library only, so the tests run without Harbor.

- `EFFORT`: the reasoning effort every arm asks for, `medium`. Claude Code and
  Pi send it by their own defaults, so Theseus's bench profile
  (`theseus-bench.toml`) says it too, and Claude Code's and Pi's adapters set it
  where Harbor's leave it to a host's environment. An `--ak` still wins, as an
  ablation.
- `stop_agent_script`, `stop_agent`: at the task's agent timeout Harbor
  cancels `run`, and its Docker environment ends only its `docker compose
  exec` client (theseus-sgpx). The agent runs on in the container the
  verifier shares, spending and changing files unrecorded. This stops it: a
  SIGTERM to the arm's processes by name (`/proc/<pid>/comm`) and to
  everything they started, a SIGKILL to what is left after a short grace.
  Plain sh over `/proc`, since a task's image may lack pkill. Theseus's own
  arm stops its turn through `theseus_bench.stop_script`.
- `version_script`, `record_version`: the agent's version as the container
  reads it (`get_version_command`), written to the trial's logs, where the
  record names it (`efficiency.stamp`), so a pin that did not take shows.
"""

from __future__ import annotations

import shlex
from typing import Any

EFFORT = "medium"
# How long a SIGTERMed agent gets before the SIGKILL, in seconds.
GRACE_S = 3
# Where the container's version read goes, in the trial's agent directory.
VERSION_FILE = "version.txt"


def stop_agent_script(names: tuple[str, ...] | list[str], grace_s: int = GRACE_S) -> str:
    """The POSIX shell that stops an agent: every process whose `comm` is one
    of `names`, and every descendant of those (found before any signal, so a
    child the first SIGTERM orphans is still the agent's), gets a SIGTERM; one
    still running after `grace_s` seconds (a zombie is not running) gets a
    SIGKILL. This shell and a process of no name given are never signalled. No
    agent running is no error."""
    listed = " ".join(shlex.quote(n) for n in names)
    ticks = max(0, int(grace_s)) * 10
    return (
        f'names="{listed}"; me=$$; pairs=""; roots=""; '
        'for d in /proc/[0-9]*; do p=${d#/proc/}; [ "$p" = "$me" ] && continue; '
        'st=$(cat "$d/stat" 2>/dev/null) || continue; c=$(cat "$d/comm" 2>/dev/null) || continue; '
        "rest=${st##*) }; set -- $rest; pairs=\"$pairs $p:$2\"; "
        'for n in $names; do [ "$c" = "$n" ] && roots="$roots $p"; done; done; '
        'tree="$roots"; new="$roots"; '
        'while [ -n "$new" ]; do next=""; for q in $pairs; do k=${q%%:*}; pp=${q#*:}; '
        'for r in $new; do [ "$pp" = "$r" ] && [ "$k" != "$me" ] && next="$next $k"; done; done; '
        'tree="$tree $next"; new="$next"; done; '
        'if [ -n "$tree" ]; then kill -TERM $tree 2>/dev/null; i=0; alive=""; '
        f"while [ $i -lt {ticks} ]; do alive=\"\"; "
        'for p in $tree; do st=$(cat /proc/$p/stat 2>/dev/null) || continue; rest=${st##*) }; '
        'case "$rest" in Z*) ;; *) alive="$alive $p";; esac; done; '
        '[ -n "$alive" ] || break; sleep 0.1 2>/dev/null || sleep 1; i=$((i+1)); done; '
        '[ -n "$alive" ] && kill -KILL $alive 2>/dev/null; fi; true'
    )


async def stop_agent(environment: Any, names: tuple[str, ...] | list[str],
                     grace_s: int = GRACE_S) -> None:
    """Run `stop_agent_script` in the task's container, as root (the agent's
    user is not always the one that can signal what it started). Never the
    trial's failure: the timeout it answers is the cause."""
    try:
        await environment.exec(command=stop_agent_script(names, grace_s), user="root",
                               timeout_sec=grace_s + 30)
    except Exception:  # noqa: BLE001
        pass


def version_script(command: str, logs: str) -> str:
    """The shell that writes `command`'s stdout, a version read, to the
    trial's logs: nothing is written when it fails, and it never fails."""
    return (f"out=$( {command} 2>/dev/null ) && printf '%s\\n' \"$out\" > "
            f"{shlex.quote(logs + '/' + VERSION_FILE)}; true")


async def record_version(environment: Any, command: str | None, logs: str) -> None:
    """Read the agent's version in the container and keep it in the logs."""
    if not command:
        return
    try:
        await environment.exec(command=version_script(command, logs), timeout_sec=60)
    except Exception:  # noqa: BLE001
        pass
