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


# Where the arm keeps the container's process list from before Harbor's run,
# in its state directory (`snapshot_script`).
BASELINE_FILE = "pids.before"


def snapshot_script(path: str) -> str:
    """The POSIX shell that records the container's processes into `path`, one
    `pid:starttime` a line (a recycled pid is another process), before Harbor's
    run starts the agent. The arm runs it after the sampler's start, so the
    sampler is in the list and the stop leaves it to its own stop. It never
    fails: a list that could not be written is no list, and the stop then
    finds the agent by name."""
    q = shlex.quote(path)
    return (
        f'{{ for d in /proc/[0-9]*; do p=${{d#/proc/}}; st=$(cat "$d/stat" 2>/dev/null) || continue; '
        'rest=${st##*) }; set -- $rest; shift 19; printf "%s:%s\\n" "$p" "$1"; done; } '
        f'> {q}.tmp 2>/dev/null && mv -f {q}.tmp {q} 2>/dev/null; true'
    )


def stop_agent_script(names: tuple[str, ...] | list[str], grace_s: int = GRACE_S,
                      baseline: str | None = None) -> str:
    """The POSIX shell that stops an agent, by what the run started.

    With `baseline` (the file `snapshot_script` wrote before the run), the
    agent is whatever runs in the container that the list does not hold, bar
    this shell, its ancestors and its own children: a command a tool's shell
    backgrounded through a subshell that exited (reparented to PID 1, so no
    tree from the agent's name reaches it) is the run's as much as the CLI is,
    and so is what a SIGTERM-ignoring process forks during the grace
    (theseus-8xp0). So: SIGSTOP every such process, and read `/proc` again
    until a pass finds no new one (a stopped process forks no more); SIGTERM
    them and SIGCONT, so a handler runs; wait up to `grace_s` seconds for them
    to go (a zombie is gone); then stop what is left again (the SIGCONT let it
    run) and the new ones it forked meanwhile, to a fixpoint again, and
    SIGKILL them all. No process of the run is no error.

    With no `baseline`, or one that cannot be read, the old way: every process
    whose `comm` is one of `names`, and every descendant of those as one read
    of `/proc` finds them, gets a SIGTERM, and a SIGKILL after the grace.
    This shell and a process of no name given are never signalled."""
    listed = " ".join(shlex.quote(n) for n in names)
    ticks = max(0, int(grace_s)) * 10
    by_name = (
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


    if baseline is None:
        return by_name
    b = shlex.quote(baseline)
    # `scan_new` sets $fresh to the pids of the run's processes not yet stopped: in no baseline line (pid and start
    # time), not this shell, its ancestors or its descendants, not a zombie, not in $stopped.
    return (
        f'names="{listed}"; base={b}; me=$$; '
        'if [ ! -s "$base" ]; then ' + by_name.split("; ", 1)[1] + "; exit 0; fi; "
        # this shell's ancestors, kept apart from the run
        'keep=" $me "; a=$me; while [ "$a" -gt 0 ] 2>/dev/null; do st=$(cat /proc/$a/stat 2>/dev/null) || break; '
        'rest=${st##*) }; set -- $rest; a=$2; [ "$a" -gt 0 ] 2>/dev/null && keep="$keep$a "; done; '
        'basetxt=" "; while read -r l; do basetxt="$basetxt$l "; done < "$base"; '
        'fresh=""; scan_new() { fresh=""; cand=""; '
        'for d in /proc/[0-9]*; do p=${d#/proc/}; st=$(cat "$d/stat" 2>/dev/null) || continue; '
        'rest=${st##*) }; set -- $rest; state=$1; pp=$2; shift 19; '
        'case "$state" in Z*|X*) continue;; esac; '
        'case "$keep" in *" $p "*) continue;; esac; '
        'case "$stopped" in *" $p "*) continue;; esac; '
        'case "$basetxt" in *" $p:$1 "*) continue;; esac; cand="$cand $p:$pp"; done; '
        # a child of this shell (a command substitution of the scan itself) is not the run's
        'for q in $cand; do k=${q%%:*}; pp=${q#*:}; [ "$pp" = "$me" ] || fresh="$fresh $k"; done; }; '
        'settle() { while :; do scan_new; [ -n "$fresh" ] || break; kill -STOP $fresh 2>/dev/null; '
        'stopped="$stopped$fresh "; done; }; '
        'stopped=" "; settle; '
        'if [ "$stopped" != " " ]; then kill -TERM $stopped 2>/dev/null; kill -CONT $stopped 2>/dev/null; i=0; '
        f"while [ $i -lt {ticks} ]; do alive=\"\"; "
        'for p in $stopped; do st=$(cat /proc/$p/stat 2>/dev/null) || continue; rest=${st##*) }; '
        'case "$rest" in Z*) ;; *) alive="$alive $p";; esac; done; '
        '[ -n "$alive" ] || break; sleep 0.1 2>/dev/null || sleep 1; i=$((i+1)); done; '
        'kill -STOP $stopped 2>/dev/null; settle; kill -KILL $stopped 2>/dev/null; fi; true'
    )
async def stop_agent(environment: Any, names: tuple[str, ...] | list[str],
                     grace_s: int = GRACE_S, baseline: str | None = None) -> None:
    """Run `stop_agent_script` in the task's container, as root (the agent's
    user is not always the one that can signal what it started). Never the
    trial's failure: the timeout it answers is the cause."""
    try:
        await environment.exec(command=stop_agent_script(names, grace_s, baseline), user="root",
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
