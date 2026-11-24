#!/usr/bin/env bash
# route.v10's cases, measured again where a change moves their state (1.0.7, issue 5): one command.
#
# A cases file's state is derived (the daemon's own rule says it) but what Jev answers to it is measured. When a
# change moves the state of some cases (Sonnet 5.5's cache read at $0.10 moves the switch-cost word of 53 of 62), this
# replays the file as it was (`--base`) and as this tree has it, interleaved, so a drift in Jev between the two runs
# is in both arms, then says which changed cases Jev answers differently. Three runs of each arm, each asking with its
# own copy of the packs (the release smoke's `bust`: a run must not be served the last run's answer), and the route
# pack and the effort pack both, as the tag gate grades them. Both arms ask with THIS tree's packs: the cases file is
# the only thing that differs between them, so a change that also edits a pack needs its own base for the packs.
#
# Usage: scripts/remeasure-route-cases.sh [--base REV] [--runs N] [--out DIR]
#   --base REV  the commit holding the cases file as it was (default origin/main)
#   --runs N    runs of each arm (default 3)
#   --out DIR   where the results go (default bench/smoke/runs/remeasure-<date>-<time>)
#
# It reads, from the environment, what release-smoke.sh does:
#   SMOKE_BIN_DIR   the candidate's `theseus` (default: where `theseusd` is on PATH)
#   SMOKE_SOCKET    the live daemon's socket (default: the CLI's own). The daemon holds the judge's key; this script
#                   never reads or prints one.
#
# Cost: each 62-case replay of a pack cost about $0.005 on 2026-11-02, so 12 replays (3 runs x 2 arms x the route and
# effort packs) are about $0.06, in a few minutes. It writes DIR/{route,effort}-{base,new}-N.json (the replays, which
# are the evidence), base.cases.toml and new.cases.toml, the packs' copies, report.md and summary.json. It changes
# nothing in the tree. Exit 0 with a report; 1 when a replay failed or a run did not answer every case; 2 on usage.
set -uo pipefail
cd "$(dirname "$0")/.."

base=origin/main
runs=3
out=
while [ $# -gt 0 ]; do
  case "$1" in
    --base) base="${2:?--base needs a revision}"; shift 2 ;;
    --runs) runs="${2:?--runs needs a number}"; shift 2 ;;
    --out) out="${2:?--out needs a directory}"; shift 2 ;;
    -h | --help) sed -n '2,25p' "$0"; exit 0 ;;
    *) echo "remeasure: unknown argument $1" >&2; exit 2 ;;
  esac
done
[[ "$runs" =~ ^[1-9][0-9]*$ ]] || { echo "remeasure: --runs is a positive number, not $runs" >&2; exit 2; }

py=
for c in python3.13 python3.12 python3.11 python3; do
  if command -v "$c" > /dev/null 2>&1 && "$c" -c 'import sys; sys.exit(sys.version_info < (3, 11))' 2> /dev/null; then
    py=$c
    break
  fi
done
[ -n "$py" ] || { echo "remeasure: needs Python >= 3.11 (tomllib)" >&2; exit 2; }

bin="${SMOKE_BIN_DIR:-$(dirname "$(command -v theseusd 2> /dev/null || echo target/release/theseusd)")}"
[ -x "$bin/theseus" ] || { echo "remeasure: no theseus in $bin (SMOKE_BIN_DIR)" >&2; exit 2; }
sock=()
[ -z "${SMOKE_SOCKET:-}" ] || sock=(--socket "$SMOKE_SOCKET")
help="$("$bin/theseus" judge replay --help 2>&1)"
[[ "$help" == *--cases* ]] || { echo "remeasure: this binary has no \`judge replay --cases\`" >&2; exit 2; }

# the cases are written for route.v10 and read for route.v12, the pack that acts (route_cases::read_as)
cases=crates/theseus-judge/packs/route.v10.cases.toml
route_pack=crates/theseus-judge/packs/route.v12.toml
effort_pack=crates/theseus-judge/packs/effort.v1.toml
[ -n "$out" ] || out="bench/smoke/runs/remeasure-$(date +%F-%H%M%S)"
[ ! -e "$out" ] || { echo "remeasure: $out exists; results of two runs must not mix" >&2; exit 2; }
mkdir -p "$out" || exit 2
git show "$base:$cases" > "$out/base.cases.toml" || { echo "remeasure: $base has no $cases" >&2; exit 2; }
cp "$cases" "$out/new.cases.toml"
smoke() { PYTHONDONTWRITEBYTECODE=1 "$py" bench/smoke/smoke.py "$@"; }
ids="$(PYTHONDONTWRITEBYTECODE=1 "$py" bench/smoke/remeasure.py cases "$out/base.cases.toml" "$out/new.cases.toml")" || exit 2
[ -n "$ids" ] || { echo "remeasure: $cases is the same at $base and in this tree: nothing to measure" >&2; exit 2; }
echo "remeasure: $(tr ',' '\n' <<< "$ids" | wc -l | tr -d ' ') cases changed ($base -> this tree); $runs runs of each arm, into $out"

n=0
for i in $(seq 1 "$runs"); do
  for arm in base new; do
    n=$((n + 1))
    # Its own copy of the packs and of the arm's cases file, versions bumped, so Jev answers this run.
    copies=()
    mapfile -t copies < <(smoke bust "$out" "$n" "$out/$arm.cases.toml" "$route_pack" "$effort_pack")
    if [ "${#copies[@]}" -ne 3 ]; then
      echo "remeasure: $arm run $i got ${#copies[@]} pack copies, not 3" >&2
      exit 1
    fi
    for q in route effort; do
      pack="${copies[0]}"
      [ "$q" = effort ] && pack="${copies[1]}"
      rc=0
      "$bin/theseus" ${sock[@]+"${sock[@]}"} --json judge replay --pack-file "$pack" --cases "${copies[2]}" \
        > "$out/$q-$arm-$i.json" 2> "$out/$q-$arm-$i.err" || rc=$?
      if [ "$rc" -ne 0 ]; then
        echo "remeasure: the $q replay of $arm run $i exited $rc: $(head -c 200 "$out/$q-$arm-$i.err" | tr '\n' ' ')" >&2
        exit 1
      fi
    done
    echo "remeasure: $arm run $i replayed"
  done
done
PYTHONDONTWRITEBYTECODE=1 "$py" bench/smoke/remeasure.py report "$out"
