"""The async bench's Layer 2 tasks (theseus-2wxa): each family's instruction,
oracle, and files, which `sync.py` writes into `tasks/<family>/`.

Twenty chores a developer does, each with independent slow sub-steps (the
tools of `tools/layer2.py`, whose durations are drawn): eight that run
independent programs and combine their results, four that read or fetch,
two that write, four controls whose steps depend on each other in order
(where running a step before its prerequisite ends costs the task), and two
that mix both. An instruction states the task, never how to run it: the
same words for every arm. The oracles do it at the ideal schedule:
independent steps at once, dependent ones in turn.

Kept out of the task images: an agent reads its instruction, not this.
"""

from __future__ import annotations

import sys
from dataclasses import dataclass, field
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent / "tools"))

import layer2 as l2  # noqa: E402

SHAPES = ("programs", "reads", "writes", "control", "mixed")


@dataclass(frozen=True)
class Family:
    name: str
    shape: str
    description: str
    tools: list[str]
    instruction: str
    solve: str
    app: dict[str, str] = field(default_factory=dict)
    timeout_sec: float = 900.0
    # The kill family (theseus-7ajc): the plain family this is the variant of,
    # and its injection (`kill_spec`).
    base: str = ""
    kill: dict = field(default_factory=dict)


PRELUDE = """#!/bin/bash
# The oracle (theseus-2wxa): {what}
set -euo pipefail
APP="${{ASYNC_ROOT:-}}/app"
out=$(mktemp -d)
trap 'rm -rf "$out"' EXIT
"""


def _solve(what: str, body: str) -> str:
    return PRELUDE.format(what=what) + body.lstrip("\n")


def _spec_md() -> str:
    lines = ["# Configuration spec", "",
             "Each file goes under /app/config/ and is YAML: one `key: value` line per key, in any order.", ""]
    for name, keys in l2.SCAFFOLD.items():
        lines += [f"## {name}", ""] + [f"- {k}: {v}" for k, v in keys.items()] + [""]
    return "\n".join(lines)


SUBJECTS = ["add the checkout totals", "round totals per line", "cache the price lookups", "batch the tax calls"]

FAMILIES: list[Family] = [
    # ------------------------------------------------ independent programs, then combine
    Family(
        "suites", "programs", "Four packages' test suites, then the failing tests named.", ["run-suite"],
        "The packages auth, billing, catalog and search each have a test suite: `run-suite PACKAGE` runs one, and "
        "each takes a while.\n\nFind every failing test and write them to /app/failing.txt, one per line, as "
        "`run-suite` names them (PACKAGE::TEST).\n",
        _solve("the four suites at once, then the failures gathered.", """
pids=()
for p in auth billing catalog search; do run-suite "$p" > "$out/$p" & pids+=($!); done
for pid in "${pids[@]}"; do wait "$pid" || true; done
cat "$out"/* | awk '$1 == "FAIL" {print $2}' > "$APP/failing.txt"
""")),
    Family(
        "ci-checks", "programs", "Lint, type-check and test one project, then a summary.",
        ["lint", "typecheck", "unit-tests"],
        "Before the release, check the project: run its linter (`lint`), its type checker (`typecheck`) and its "
        "unit tests (`unit-tests`); each takes a while.\n\nThen write /app/summary.txt with three lines: "
        "`lint N` (the linter's warnings), `types N` (the type errors) and `tests PASSED/TOTAL`.\n",
        _solve("the three checks at once, then the summary.", """
lint > "$out/lint" & a=$!
typecheck > "$out/types" & b=$!
unit-tests > "$out/tests" & c=$!
wait "$a" || true; wait "$b" || true; wait "$c" || true
{
  echo "lint $(sed -n 's/^lint: \\([0-9]*\\) warnings$/\\1/p' "$out/lint")"
  echo "types $(sed -n 's/^typecheck: \\([0-9]*\\) errors$/\\1/p' "$out/types")"
  echo "tests $(sed -n 's/^unit-tests: \\([0-9]*\\) passed, [0-9]* failed, \\([0-9]*\\) in all$/\\1\\/\\2/p' "$out/tests")"
} > "$APP/summary.txt"
""")),
    Family(
        "build-configs", "programs", "Three configurations built, then their sizes reported.", ["build-config"],
        "Build the app in each of its three configurations, debug, release and minsize, with "
        "`build-config CONFIG`; each build takes a while.\n\nWrite each binary's size to /app/sizes.txt, one line "
        "per configuration: the configuration, a space, and its size in bytes.\n",
        _solve("the three builds at once, then the sizes.", """
pids=()
for c in debug release minsize; do build-config "$c" > "$out/$c" & pids+=($!); done
for pid in "${pids[@]}"; do wait "$pid"; done
cat "$out"/* | sed -n 's/^built \\([a-z]*\\): .*, \\([0-9]*\\) bytes$/\\1 \\2/p' > "$APP/sizes.txt"
""")),
    Family(
        "bench-settings", "programs", "A benchmark at four settings, then the best written to a config.",
        ["bench-batch"],
        "The service's throughput depends on its batch size. Benchmark the sizes 16, 32, 64 and 128 with "
        "`bench-batch SIZE`; each run takes a while.\n\nThen write the size with the highest throughput to "
        "/app/bench.toml as the line `batch_size = SIZE`.\n",
        _solve("the four runs at once, then the best.", """
pids=()
for b in 16 32 64 128; do bench-batch "$b" > "$out/$b" & pids+=($!); done
for pid in "${pids[@]}"; do wait "$pid"; done
cat "$out"/* | sed -n 's/^batch size \\([0-9]*\\): \\([0-9]*\\) ops\\/s$/\\2 \\1/p' | sort -n | tail -1 \\
  | awk '{print "batch_size = " $2}' > "$APP/bench.toml"
""")),
    Family(
        "host-logs", "programs", "Six hosts' logs fetched, then the one with the error named.", ["fetch-log"],
        "One of the hosts web-1 to web-6 logged an ERROR last night. `fetch-log HOST` prints a host's log, and "
        "each takes a while to fetch.\n\nFind the host and write its name alone to /app/culprit.txt.\n",
        _solve("the six logs at once, then the host with the error.", """
pids=()
for h in web-1 web-2 web-3 web-4 web-5 web-6; do fetch-log "$h" > "$out/$h" & pids+=($!); done
for pid in "${pids[@]}"; do wait "$pid"; done
basename "$(grep -l ' ERROR ' "$out"/* | head -1)" > "$APP/culprit.txt"
""")),
    Family(
        "repos-behind", "programs", "Five repositories' fetch and status, then those behind listed.",
        ["repo-status"],
        "Five repositories are checked out: api, web, worker, infra and docs. `repo-status REPO` fetches one "
        "and prints its status, which takes a while.\n\nWrite the names of the repositories that are behind "
        "origin/main to /app/behind.txt, one per line.\n",
        _solve("the five fetches at once, then those behind.", """
pids=()
for r in api web worker infra docs; do repo-status "$r" > "$out/$r" & pids+=($!); done
for pid in "${pids[@]}"; do wait "$pid"; done
for f in $(grep -l 'is behind' "$out"/*); do basename "$f"; done > "$APP/behind.txt"
""")),
    Family(
        "csv-merge", "programs", "Five CSV exports converted, then merged.", ["convert-export"],
        "Five monthly exports, jan, feb, mar, apr and may, need converting: `convert-export NAME` writes "
        "/app/exports/NAME.csv, and each conversion takes a while.\n\nThen merge the five into /app/merged.csv: "
        "the header `id,amount` once, then every row of every export.\n",
        _solve("the five conversions at once, then the merge.", """
pids=()
for e in jan feb mar apr may; do convert-export "$e" > /dev/null & pids+=($!); done
for pid in "${pids[@]}"; do wait "$pid"; done
{ echo "id,amount"; for e in jan feb mar apr may; do tail -n +2 "$APP/exports/$e.csv"; done; } > "$APP/merged.csv"
""")),
    Family(
        "health-restart", "programs",
        "Four services' health checked, the failing one restarted, then verified.", ["health", "restart"],
        "Check the health of the services auth, billing, search and mail with `health SERVICE`; each check "
        "takes a while.\n\nRestart the one that is failing with `restart SERVICE`, check it again to verify it "
        "is healthy, and write its name alone to /app/restarted.txt. Leave the healthy services alone.\n",
        _solve("the four checks at once, then the failing one restarted and checked.", """
pids=()
for s in auth billing search mail; do health "$s" > "$out/$s" & pids+=($!); done
for pid in "${pids[@]}"; do wait "$pid" || true; done
bad=$(basename "$(grep -l '503' "$out"/* | head -1)")
restart "$bad" > /dev/null
health "$bad" > /dev/null
echo "$bad" > "$APP/restarted.txt"
""")),
    # ------------------------------------------------ independent reads and fetches
    Family(
        "doc-questions", "reads", "Three questions answered from eight long notes.", ["archive-get"],
        "The team's eight notes, notes-01 to notes-08, are on the archive server: `archive-get NAME` prints "
        "one, and the server is slow.\n\nFrom them, answer three questions:\n\n1. What is the release codename?\n"
        "2. How many days does the on-call rotation last?\n3. Which port does the primary database listen on?\n\n"
        "Write the answers to /app/answers.txt as three lines: `1 ANSWER`, `2 ANSWER` and `3 ANSWER`.\n",
        _solve("the eight notes at once, then the answers.", """
pids=()
for i in 01 02 03 04 05 06 07 08; do archive-get "notes-$i" > "$out/$i" & pids+=($!); done
for pid in "${pids[@]}"; do wait "$pid"; done
{
  echo "1 $(cat "$out"/* | sed -n 's/^The release codename is \\([A-Z]*\\)\\.$/\\1/p')"
  echo "2 $(cat "$out"/* | sed -n 's/^The on-call rotation lasts \\([0-9]*\\) days\\.$/\\1/p')"
  echo "3 $(cat "$out"/* | sed -n 's/^The primary database listens on port \\([0-9]*\\)\\.$/\\1/p')"
} > "$APP/answers.txt"
""")),
    Family(
        "api-summary", "reads", "A package's six modules read, then its API summarised.", ["module-source"],
        "Write an API summary of the toolkit package to /app/API.md. Its six modules are core, io, net, auth, "
        "cache and util; `module-source MODULE` prints one from the package mirror, which takes a while.\n\n"
        "Give each module a section headed `## MODULE` that lists its public functions (those whose names do "
        "not start with an underscore), and leave the private ones out.\n",
        _solve("the six modules at once, then the summary.", """
pids=()
for m in core io net auth cache util; do module-source "$m" > "$out/$m" & pids+=($!); done
for pid in "${pids[@]}"; do wait "$pid"; done
for m in core io net auth cache util; do
  echo "## $m"; echo; sed -n 's/^def \\([a-z][a-z_]*\\)(.*/- \\1/p' "$out/$m"; echo
done > "$APP/API.md"
""")),
    Family(
        "advisories", "reads", "Five advisories queried from a slow local registry.", ["advisory"],
        "Look up five security advisories in the local registry, ADV-1001 to ADV-1005: `advisory ID` prints "
        "one, and the registry is slow.\n\nWrite /app/advisories.txt, one line per advisory: its ID, its "
        "severity, and the version that fixes it, separated by spaces.\n",
        _solve("the five lookups at once, then the table.", """
pids=()
for a in 1001 1002 1003 1004 1005; do advisory "ADV-$a" > "$out/$a" & pids+=($!); done
for pid in "${pids[@]}"; do wait "$pid"; done
for a in 1001 1002 1003 1004 1005; do
  echo "ADV-$a $(sed -n 's/^  severity: //p' "$out/$a") $(sed -n 's/^  fixed in: //p' "$out/$a")"
done > "$APP/advisories.txt"
""")),
    Family(
        "config-diff", "reads", "Four environments' configurations compared.", ["env-config"],
        "Compare the configuration of the four environments dev, staging, prod and perf: `env-config ENV` "
        "prints one environment's settings from the config service, which is slow.\n\nWrite the names of the "
        "settings whose values are not the same in all four environments to /app/differs.txt, one per line.\n",
        _solve("the four configurations at once, then the settings that differ.", """
pids=()
for e in dev staging prod perf; do env-config "$e" > "$out/$e" & pids+=($!); done
for pid in "${pids[@]}"; do wait "$pid"; done
cat "$out"/* | sort -u | cut -d= -f1 | uniq -d > "$APP/differs.txt"
""")),
    # ------------------------------------------------ independent writes
    Family(
        "scaffold", "writes", "Eight config files scaffolded from a spec, each validated.", ["validate-config"],
        "Create the eight configuration files that /app/spec.md describes, under /app/config/. Then check each "
        "with `validate-config FILE` (each check takes a while), and fix any it rejects, until all eight pass.\n",
        _solve("the eight files written, then validated at once.", """
mkdir -p "$APP/config"
awk -v d="$APP/config" '/^## / {f = d "/" $2} /^- / && f {sub(/^- /, ""); print > f}' "$APP/spec.md"
pids=()
for f in "$APP"/config/*.yaml; do validate-config "$(basename "$f")" > /dev/null & pids+=($!); done
for pid in "${pids[@]}"; do wait "$pid"; done
"""),
        app={"spec.md": _spec_md()}),
    Family(
        "rename", "writes", "One rename across six files, then the tests.", ["run-tests"],
        f"Rename the function {l2.RENAME_OLD} to {l2.RENAME_NEW} everywhere in the six modules under /app/src, "
        "then run the tests with `run-tests` (they take a while) and make sure they pass.\n",
        _solve("the six edits, then the tests.", f"""
sed -i 's/\\b{l2.RENAME_OLD}\\b/{l2.RENAME_NEW}/g' "$APP"/src/*.py
run-tests > /dev/null
"""),
        app={f"src/{k}": v for k, v in l2.RENAME_FILES.items()}),
    # ------------------------------------------------ dependent controls
    Family(
        "pipeline", "control", "Build, then run, then check the output.", ["build", "run-app", "check-output"],
        "Build the app with `build`, then run it with `run-app`, then check its output with `check-output`; "
        "each step takes a while.\n\nWrite the verdict `check-output` prints to /app/verdict.txt, alone.\n",
        _solve("each step after the one it needs.", """
build > /dev/null
run-app > /dev/null
check-output | sed -n 's/.*verdict \\([0-9a-f]*\\)$/\\1/p' > "$APP/verdict.txt"
""")),
    Family(
        "service", "control", "Start a service, then query it.", ["start-service", "query-service"],
        "Start the inventory service with `start-service`, which takes a while to come up, then ask it for the "
        "stock count with `query-service stock`, and write the number alone to /app/stock.txt.\n",
        _solve("the service up, then the query.", """
start-service > /dev/null
query-service stock | sed -n 's/^in stock: //p' > "$APP/stock.txt"
""")),
    Family(
        "migrations", "control", "Three ordered migrations.", ["migrate-db"],
        "Apply the database migrations 001, 002 and 003 with `migrate-db MIGRATION`; each takes a while, and "
        "each builds on the one before it.\n\nThen write the schema checksum the last one prints to "
        "/app/schema.txt, alone.\n",
        _solve("the migrations in their order.", """
for m in 001 002 003; do migrate-db "$m" > "$out/$m"; done
sed -n 's/.*schema checksum //p' "$out/003" > "$APP/schema.txt"
""")),
    Family(
        "edit-test", "control", "Edit, then test, then fix.", ["codemod", "test-suite"],
        f"Rename the flag {l2.FLAG_OLD} to {l2.FLAG_NEW} in the code under /app/src with the project's codemod "
        "(`codemod`), then run the tests with `test-suite`; both take a while.\n\nFix whatever the tests report, "
        "until `test-suite` passes.\n",
        _solve("the codemod, the tests, the fix, the tests again.", f"""
codemod > /dev/null
test-suite > "$out/first" || true
sed -i 's/{l2.FLAG_OLD}/{l2.FLAG_NEW}/g' "$APP/tests/test_flags.py"
test-suite > /dev/null
"""),
        app=dict(l2.EDIT_FILES)),
    # ------------------------------------------------ mixed
    Family(
        "link", "mixed", "Two independent slow builds, then the link.", ["build-lib", "link-app"],
        "Build the two libraries with `build-lib core` and `build-lib ui`, then link the app with `link-app`; "
        "each step takes a while.\n\nWrite the artifact's name that `link-app` prints (app-…) to "
        "/app/artifact.txt, alone.\n",
        _solve("the two builds at once, then the link.", """
build-lib core > /dev/null & a=$!
build-lib ui > /dev/null & b=$!
wait "$a"; wait "$b"
link-app | sed -n 's/^linked out\\///p' > "$APP/artifact.txt"
""")),
    Family(
        "bisect", "mixed", "Four commits tested in their own worktrees, then the first bad one named.",
        ["make-worktree", "test-commit"],
        "A bug landed in one of the four commits listed in /app/commits.txt, oldest first; the oldest is good. "
        "Test the commits, each in a worktree of its own: `make-worktree COMMIT` makes one, and "
        "`test-commit COMMIT` runs the tests in it; both take a while.\n\nWrite the first commit whose tests "
        "fail to /app/first-bad.txt, alone.\n",
        _solve("each commit's worktree, then its test, the commits at once.", """
pids=()
for c in $(awk '{print $1}' "$APP/commits.txt"); do
  (make-worktree "$c" > /dev/null && test-commit "$c" > "$out/$c") & pids+=($!)
done
for pid in "${pids[@]}"; do wait "$pid" || true; done
for c in $(awk '{print $1}' "$APP/commits.txt"); do
  if grep -q FAIL "$out/$c"; then echo "$c" > "$APP/first-bad.txt"; break; fi
done
"""),
        app={"commits.txt": "".join(f"{c} {s}\n" for c, s in zip(l2.COMMITS, SUBJECTS))}),
]

BY_NAME = {f.name: f for f in FAMILIES}


def task_toml(f: Family) -> str:
    if f.kill:
        return kill_task_toml(f)
    return f"""schema_version = "1.4"

[metadata]
category = "async"
description = "{f.description}"

[metadata.async]
family = "{f.name}"
layer = 2
shape = "{f.shape}"

[verifier]
timeout_sec = 120.0

[agent]
timeout_sec = {f.timeout_sec}

[environment]
build_timeout_sec = 600.0
cpus = 2
memory_mb = 2048
"""


def dockerfile(f: Family) -> str:
    app = "COPY app/ /app/\n" if f.app else ""
    # A kill family's tools write an `effect` record per step whose effect must
    # not repeat, on the marker (`asyncbench.effects_on`).
    marker = " && echo kill-family > /var/lib/async/kill-family" if f.kill else ""
    return f"""FROM python:3.12-slim

# The tools (/opt/async/bin) and their library; the ledger and the tools' state
# live in /var/lib/async, outside the working directory.
COPY async/ /opt/async/
RUN chmod 755 /opt/async/bin/* && mkdir -p /app /var/lib/async/state{marker}
{app}ENV PATH=/opt/async/bin:$PATH
WORKDIR /app
"""


def test_sh(f: Family) -> str:
    return f"""#!/bin/bash
# The ledger checked as a record of itself, then the family's order rules and
# outcome: reward.json and the ledger's copy in the verifier's log directory
# (layer2.py check).
python3 "$(dirname "$0")/layer2.py" check {f.name}
exit 0
"""


# ---------------------------------------------------------------- the kill family (theseus-7ajc)

# Each plain family's kill variant: the injection's mode, its seed (in the
# task's metadata, so the same trial is killed at the same moment), the tool
# whose first start anchors a mid-step kill (and the oracle's), and the delay.
# Six chores whose steps have effects that must not repeat: a service
# restarted, a migration applied, an export converted, a worktree made, an
# app built and run, a service started. Half the kills land in a running
# sub-step (mid-step), half at a seeded moment of the ideal wall, so some
# fall between calls.
KILL_SPECS = {
    "health-restart": ("mid-step", 7101, "restart", 5.0),
    "migrations": ("random", 7102, "migrate-db", 5.0),
    "csv-merge": ("mid-step", 7103, "convert-export", 6.0),
    "bisect": ("random", 7104, "make-worktree", 6.0),
    "pipeline": ("mid-step", 7105, "run-app", 3.0),
    "service": ("random", 7106, "start-service", 4.0),
}
# The seeded window, as fractions of the family's nominal ideal wall.
KILL_WINDOW = (0.25, 0.75)


def _mid(tool: str) -> float:
    lo, hi = l2.DURATIONS[tool]
    return (lo + hi) / 2


def nominal_ideal(base: str) -> float:
    """The family's ideal wall at the midpoints of its tools' draws: its
    stages' longest steps in turn (the plain families' own `ideal`, with no
    ledger to read yet)."""
    if base in l2.STAGES:
        return sum(max(_mid(t) for t in stage) for stage in l2.STAGES[base])
    return {"health-restart": _mid("health") * 2 + _mid("restart"),
            "migrations": 3 * _mid("migrate-db"),
            "bisect": _mid("make-worktree") + _mid("test-commit")}[base]


def kill_spec(base: str) -> dict:
    mode, seed, tool, delay = KILL_SPECS[base]
    ideal = nominal_ideal(base)
    return {"mode": mode, "seed": seed, "after_tool": tool, "after_kind": "start", "delay_s": delay,
            "window_lo_s": round(ideal * KILL_WINDOW[0], 1), "window_hi_s": round(ideal * KILL_WINDOW[1], 1),
            "at_s": round(ideal * 1.5), "settle_s": 2}


# The plain oracle's steps, as a plan that keeps a journal: `step NAME CMD...`
# runs once ever (what the journal holds is skipped on the restart).
KILL_PLANS = {
    "health-restart": """
  pids=()
  for s in auth billing search mail; do step "h-$s" health "$s" & pids+=($!); done
  for pid in "${pids[@]}"; do wait "$pid" || true; done
  bad=$(basename "$(grep -l '503' "$out"/h-* | head -1)"); bad=${bad#h-}
  step restart restart "$bad"
  step recheck health "$bad"
  echo "$bad" > "$APP/restarted.txt"
""",
    "migrations": """
  for m in 001 002 003; do step "m$m" migrate-db "$m"; done
  sed -n 's/.*schema checksum //p' "$out/m003" > "$APP/schema.txt"
""",
    "csv-merge": """
  pids=()
  for e in jan feb mar apr may; do step "e-$e" convert-export "$e" & pids+=($!); done
  for pid in "${pids[@]}"; do wait "$pid"; done
  { echo "id,amount"; for e in jan feb mar apr may; do tail -n +2 "$APP/exports/$e.csv"; done; } > "$APP/merged.csv"
""",
    "bisect": """
  pids=()
  for c in $(awk '{print $1}' "$APP/commits.txt"); do
    { step "w-$c" make-worktree "$c"; step "t-$c" test-commit "$c"; } & pids+=($!)
  done
  for pid in "${pids[@]}"; do wait "$pid" || true; done
  for c in $(awk '{print $1}' "$APP/commits.txt"); do
    if grep -q FAIL "$out/t-$c"; then echo "$c" > "$APP/first-bad.txt"; break; fi
  done
""",
    "pipeline": """
  step build build
  step run run-app
  step check check-output
  sed -n 's/.*verdict \\([0-9a-f]*\\)$/\\1/p' "$out/check" > "$APP/verdict.txt"
""",
    "service": """
  step start start-service
  step stock query-service stock
  sed -n 's/^in stock: //p' "$out/stock" > "$APP/stock.txt"
""",
}

KILL_PRELUDE = """#!/bin/bash
# The oracle (theseus-7ajc): the plain oracle's work as a harness that keeps a journal. It is killed once, at
# the injection's trigger, with SIGKILL to its process group (the sub-steps in flight die with it, as a harness's
# own processes do), the kill recorded in the ledger first; after the settle it restarts, skips what the journal
# holds, and runs the rest once: reward 1, no effect twice, nothing left running.
set -euo pipefail
APP="${{ASYNC_ROOT:-}}/app"
out=$(mktemp -d)
trap 'rm -rf "$out"' EXIT
J="$out/journal"; mkdir -p "$J"
LIB="$(dirname "$(command -v {tool})")/../lib/asyncbench.py"
step() {{
  local n=$1; shift
  [ -e "$J/$n.done" ] && return 0
  "$@" > "$out/$n" || true
  touch "$J/$n.done"
}}
plan() {{{plan}}}
set -m
plan & harness=$!
python3 "$LIB" await --tool {tool} --kind start --timeout 600 > /dev/null || true
sleep "$(awk -v s="${{ASYNC_TIME_SCALE:-1}}" 'BEGIN {{print {delay} * s}}')"
python3 "$LIB" kill-record --trigger '{{"by": "oracle", "tool": "{tool}", "kind": "start", "delay_s": {delay}}}' \\
  --pids "[{{\\"pid\\": $harness}}]" --settle 2 > /dev/null
kill -KILL -- "-$harness" 2>/dev/null || true
wait "$harness" 2>/dev/null || true
sleep "$(awk -v s="${{ASYNC_TIME_SCALE:-1}}" 'BEGIN {{print 2 * s}}')"
python3 "$LIB" restart-record --resumable true --note "the oracle's journal" > /dev/null
set +m
plan
"""


def kill_oracle(base: str) -> str:
    spec = kill_spec(base)
    return KILL_PRELUDE.format(plan=KILL_PLANS[base], tool=spec["after_tool"], delay=spec["delay_s"])


def kill_task_toml(f: Family) -> str:
    k = f.kill
    return f"""schema_version = "1.4"

[metadata]
category = "async"
description = "{f.description}"

[metadata.async]
family = "{f.name}"
layer = 2
shape = "{f.shape}"
base = "{f.base}"

# The driver's kill (bench/async/driver.py, theseus-7ajc): SIGKILL to the harness's own
# process(es) in this container, `kill` in the ledger first, a settle, then the harness
# restarted as its documentation says to continue a session. `random`: a time drawn from
# `seed` in the window (seconds after the agent started); `mid-step`: `delay_s` after the
# ledger's first `after_kind` of `after_tool`, else `at_s`.
[metadata.async.injection]
kind = "kill"
mode = "{k["mode"]}"
seed = {k["seed"]}
window_lo_s = {k["window_lo_s"]}
window_hi_s = {k["window_hi_s"]}
after_tool = "{k["after_tool"]}"
after_kind = "{k["after_kind"]}"
delay_s = {k["delay_s"]}
at_s = {k["at_s"]}
settle_s = {k["settle_s"]}

[verifier]
timeout_sec = 120.0

[agent]
timeout_sec = {f.timeout_sec}

[environment]
build_timeout_sec = 600.0
cpus = 2
memory_mb = 2048
"""


def _kill_family(b: Family) -> Family:
    return Family(f"{l2.KILL_PREFIX}{b.name}", b.shape, b.description + " The harness is killed once, mid-task.",
                  b.tools, b.instruction, kill_oracle(b.name), b.app, b.timeout_sec,
                  base=b.name, kill=kill_spec(b.name))


KILL_FAMILIES: list[Family] = [_kill_family(BY_NAME[b]) for b in l2.KILL_BASES]
KILL_BY_NAME = {f.name: f for f in KILL_FAMILIES}
ALL_BY_NAME = {**BY_NAME, **KILL_BY_NAME}
