#!/usr/bin/env bash
# The release smoke (theseus-yf6pj): a CI-level check of a release candidate in about an hour, for a few dollars, on
# one host, against the floor the last release wrote (bench/smoke/floor.json). Not a measurement and never published
# under docs/benchmarks/. Five parts, each a PASS or FAIL line with a number:
#
#   tb      Terminal-Bench 2.0's smoke slice (bench/smoke/tb-slice.toml): 8 stable tasks and 2 controls, k=1 at HIGH,
#           4 at once, through the Harbor driver (bench/harbor/run.py), on the candidate's static binaries
#   route   `theseus judge replay --cases` over route.v10's cases, three runs: 54 of 60 on the model, every must
#   recall  scripts/recall-tests.sh, as the gate runs it
#   resume  bench/smoke/resume.py: a scratch daemon SIGKILLed mid-tool three times, at $0
#   dm      bench/smoke/dm.py: five scripted turns through the live daemon's CLI
#
# Usage: scripts/release-smoke.sh [--parts "tb route recall resume dm"] [--write-floor]
#
# It reads, from the environment:
#   ANTHROPIC_API_KEY, TYPESAFE_API_KEY  tb only: the routed arm's keys, handed to Harbor's process and never written
#   THESEUS_BENCH_BIN_DIR   the static (musl) theseus and theseusd tb uploads (default bench/bin; bench/build.sh)
#   SMOKE_BIN_DIR           the candidate's theseus, theseusd and theseus-index (default: where theseusd is on PATH)
#   SMOKE_SHA               the candidate's commit (default: SMOKE_BIN_DIR's name when it is an install's
#                           /usr/local/lib/theseus/<sha>, else this checkout's HEAD); tb fails when the static build's
#                           Rust sources (bench/bin/build-commit) are not the candidate's
#   SMOKE_SIM               theseus-sim, for resume's stand-in model and recall's driver tests
#   SMOKE_SOCKET            the live daemon's socket, for route and dm (default: the CLI's own)
#   SMOKE_HARBOR            Harbor's `harbor` (default: on PATH, else .venv/bin/harbor)
#   SMOKE_TB_STOP_USD       stop tb once its finished trials cost this much (default 6)
#   SMOKE_QUIET_WAIT_S      how long tb waits for a quiet host before it gives up (default 900)
#
# Writes bench/smoke/runs/<date>-<sha8>/: report.md (the verdict is its first heading), parts/<part>.json, and each
# part's logs. Exit 0 on PASS, 1 on FAIL, 2 on usage. `--write-floor` writes bench/smoke/floor.json from the parts
# that passed (the release step does that at each tag). No status is read from a pipeline: each command's own is.
set -uo pipefail
cd "$(dirname "$0")/.."

parts="tb route recall resume dm"
write_floor=
while [ $# -gt 0 ]; do
  case "$1" in
    --parts) parts="${2:?--parts needs a list}"; shift 2 ;;
    --write-floor) write_floor=--write-floor; shift ;;
    -h | --help) sed -n '2,30p' "$0"; exit 0 ;;
    *) echo "release-smoke: unknown argument $1" >&2; exit 2 ;;
  esac
done

py=
for c in python3.13 python3.12 python3.11 python3; do
  if command -v "$c" > /dev/null 2>&1 && "$c" -c 'import sys; sys.exit(sys.version_info < (3, 11))' 2> /dev/null; then
    py=$c
    break
  fi
done
[ -n "$py" ] || { echo "release-smoke: needs Python >= 3.11 (tomllib)" >&2; exit 2; }

bin="${SMOKE_BIN_DIR:-$(dirname "$(command -v theseusd 2> /dev/null || echo target/release/theseusd)")}"
sha="${SMOKE_SHA:-}"
if [ -z "$sha" ]; then
  sha="$(basename "$(dirname "$(realpath "$bin/theseusd")")")"
  [[ "$sha" =~ ^[0-9a-f]{8,40}$ ]] && git cat-file -e "$sha^{commit}" 2> /dev/null || sha="$(git rev-parse HEAD)"
fi
sha="$(git rev-parse "$sha^{commit}")" || { echo "release-smoke: $sha is no commit here" >&2; exit 2; }
# A fresh directory every run: an aborted run's parts must never enter another's verdict.
run="bench/smoke/runs/$(date +%F)-${sha:0:8}"
[ ! -e "$run" ] || run="$run-$(date +%H%M%S)"
mkdir -p "$run"
sim="${SMOKE_SIM:-}"
for c in "$bin/theseus-sim" target/release/theseus-sim target/debug/theseus-sim; do
  [ -n "$sim" ] || { [ -x "$c" ] && sim=$c; }
done
sock=()
[ -z "${SMOKE_SOCKET:-}" ] || sock=(--socket "$SMOKE_SOCKET")
musl="${THESEUS_BENCH_BIN_DIR:-$PWD/bench/bin}"
# The candidate's own static build for tb: its Rust sources must be the candidate's, whatever commit built them.
build_line="$(cat "$musl/build-commit" 2> /dev/null)" || build_line=unknown
build="${build_line%-dirty}"
smoke() { PYTHONDONTWRITEBYTECODE=1 "$py" bench/smoke/smoke.py "$@"; }
now() { date +%s; }
echo "release-smoke: $run (candidate $bin, tb's static binaries $musl, sim ${sim:-none})"

part_tb() {
  local jobs="$run/tb-jobs" harbor="${SMOKE_HARBOR:-}" t0 rc=0 waited=0 spent
  [ -n "$harbor" ] || harbor="$(command -v harbor 2> /dev/null || echo .venv/bin/harbor)"
  if [ -z "${ANTHROPIC_API_KEY:-}" ] || [ -z "${TYPESAFE_API_KEY:-}" ]; then
    smoke fail "$run" tb "ANTHROPIC_API_KEY and TYPESAFE_API_KEY are needed (the routed arm)"
    return
  fi
  [ -x "$harbor" ] || { smoke fail "$run" tb "no harbor at $harbor (SMOKE_HARBOR)"; return; }
  [ -x "$musl/theseusd" ] || { smoke fail "$run" tb "no static theseusd in $musl (bench/build.sh)"; return; }
  if ! git cat-file -e "$build^{commit}" 2> /dev/null \
    || ! git diff --quiet "$build" "$sha" -- crates Cargo.toml Cargo.lock; then
    smoke fail "$run" tb "the static build ($build) is not the candidate's sources (${sha:0:12}): run bench/build.sh"
    return
  fi
  # Each trial is a compose project with its own network; leftover networks exhaust Docker's address pools and every
  # trial then fails before its agent starts (2026-10-30). Probe one first.
  if ! docker network create "smoke-probe-$$" > /dev/null 2>&1; then
    smoke fail "$run" tb "Docker cannot create a network (address pools full? \`docker network prune\`)"
    return
  fi
  docker network rm "smoke-probe-$$" > /dev/null 2>&1
  local line=("$py" bench/harbor/run.py --plan bench/plans/smoke-tb10.json -- harbor run -d terminal-bench@2.0
    --agent-setup-timeout-multiplier 3 -a theseus_agent:TheseusRouted -m anthropic/claude-sonnet-5-5 -n 4
    --job-name smoke-tb10 -o "$jobs")
  local env=(env PATH="$(dirname "$harbor"):$PATH" PYTHONPATH="$PWD/bench/harbor" HARBOR_TELEMETRY=0 BENCH_NO_CAP=1
    THESEUS_BENCH_BIN_DIR="$musl")
  # run.py refuses (exit 3) while the gate holds its lock or the CPU is busy: wait for a quiet host, bounded.
  until "${env[@]}" "${line[@]:0:2}" --dry-run "${line[@]:2}" > "$run/tb-quiet.log" 2>&1; do
    if [ "$waited" -ge "${SMOKE_QUIET_WAIT_S:-900}" ]; then
      smoke fail "$run" tb "the host was not quiet for ${waited} s: $(tail -1 "$run/tb-quiet.log" | cut -c1-160)"
      return
    fi
    sleep 30
    waited=$((waited + 30))
  done
  t0=$(now)
  # Its own process group, so a stop reaches Harbor itself (which cancels its trials), not only run.py.
  setsid "${env[@]}" "${line[@]}" > "$run/tb.log" 2>&1 &
  local pid=$!
  while kill -0 "$pid" 2> /dev/null; do
    # A spend that cannot be read stops the run: the stop must never be silently off.
    spent="$(smoke spent "$jobs" 2> /dev/null)" || spent=unread
    if [ "$spent" = unread ] || "$py" -c "import sys; sys.exit(float('$spent') < float('${SMOKE_TB_STOP_USD:-6}'))"; then
      echo "release-smoke: tb stopped at $spent dollars of finished trials" >> "$run/tb.log"
      # SIGTERM, not SIGINT: a background job of a script starts with SIGINT ignored, and run.py and Harbor
      # inherit that. Harbor takes SIGTERM as an interrupt and cancels its trials.
      kill -TERM -- "-$pid" 2> /dev/null
    fi
    sleep 30
  done
  wait "$pid" || rc=$?
  # The leader can go before its group: wait for the rest (Harbor's trials) so no trial outlives the part.
  while pgrep -g "$pid" > /dev/null 2>&1; do sleep 5; done
  echo "release-smoke: tb's run exited $rc" >> "$run/tb.log"
  smoke tb "$run" "$jobs" "$(($(now) - t0))"
}

part_route() {
  local cases=crates/theseus-judge/packs/route.v10.cases.toml i t0 rc help args=()
  help="$("$bin/theseus" judge replay --help 2>&1)"
  if [[ "$help" != *--cases* ]]; then
    smoke fail "$run" route "this binary has no \`judge replay --cases\` (it came with 1.0.4's wave 3')"
    return
  fi
  for i in 1 2 3; do
    t0=$(now)
    rc=0
    "$bin/theseus" ${sock[@]+"${sock[@]}"} --json judge replay --cases "$cases" > "$run/route-$i.json" 2> "$run/route-$i.err" || rc=$?
    if [ "$rc" -ne 0 ]; then
      # A pack the build dispatches cannot be replayed as a candidate: once route.v10 is live, say so plainly.
      smoke fail "$run" route "replay $i exited $rc: $(head -c 200 "$run/route-$i.err" | tr '\n' ' ')"
      return
    fi
    args+=("$run/route-$i.json" "$(($(now) - t0))")
  done
  smoke route "$run" "${args[@]}"
}

part_recall() {
  local t0 rc=0 sim_dir="$run/recall-bin" b
  # recall's driver tests run the binaries in THESEUS_RECALL_BIN_DIR, and skip when one is missing.
  mkdir -p "$sim_dir"
  for b in theseus theseusd theseus-index; do [ ! -x "$bin/$b" ] || ln -sf "$(realpath "$bin/$b")" "$sim_dir/$b"; done
  [ -z "$sim" ] || ln -sf "$(realpath "$sim")" "$sim_dir/theseus-sim"
  # A missing binary skips every driver test silently, and the run would still pass: refuse instead.
  for b in theseus theseusd theseus-index theseus-sim; do
    [ -x "$sim_dir/$b" ] || { smoke fail "$run" recall "no $b for the driver tests (SMOKE_BIN_DIR, SMOKE_SIM)"; return; }
  done
  t0=$(now)
  THESEUS_RECALL_BIN_DIR="$PWD/$sim_dir" scripts/recall-tests.sh > "$run/recall.log" 2>&1 || rc=$?
  smoke recall "$run" "$rc" "$(($(now) - t0))" "$run/recall.log"
}

part_resume() {
  [ -n "$sim" ] || { smoke fail "$run" resume "no theseus-sim (SMOKE_SIM)"; return; }
  local rc=0
  "$py" bench/smoke/resume.py --bin-dir "$bin" --sim "$sim" --work "$run/resume" --out "$run/resume.json" \
    > "$run/resume.log" 2>&1 || rc=$?
  [ -s "$run/resume.json" ] || { smoke fail "$run" resume "resume.py exited $rc and wrote nothing"; return; }
  smoke resume "$run" "$run/resume.json"
}

part_dm() {
  local rc=0
  "$py" bench/smoke/dm.py --theseus "$bin/theseus" ${SMOKE_SOCKET:+--socket "$SMOKE_SOCKET"} --out "$run/dm.json" \
    > "$run/dm.log" 2>&1 || rc=$?
  [ -s "$run/dm.json" ] || { smoke fail "$run" dm "dm.py exited $rc and wrote nothing"; return; }
  smoke dm "$run" "$run/dm.json"
}

for p in $parts; do
  case "$p" in
    tb | route | recall | resume | dm) "part_$p" ;;
    *) echo "release-smoke: no part $p" >&2; exit 2 ;;
  esac
done
smoke report "$run" "$sha" "candidate ${sha:0:12}, $("$bin/theseusd" --version 2>&1 | head -1); tb's static build ${build:0:12}$([ "$build" = "$build_line" ] || echo ' (dirty tree)')" \
  $write_floor
