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
#   SMOKE_SIM               theseus-sim, for resume's stand-in model and recall's driver tests
#   SMOKE_SOCKET            the live daemon's socket, for route and dm (default: the CLI's own)
#   SMOKE_HARBOR            Harbor's `harbor` (default: on PATH, else .venv/bin/harbor)
#   SMOKE_TB_STOP_USD       stop tb once its finished trials cost this much (default 6)
#   SMOKE_QUIET_WAIT_S      how long tb waits for a quiet host before it gives up (default 900)
#
# Writes bench/smoke/runs/<date>-<sha8>/: report.md (the verdict is its first heading), parts/<part>.json, and each
# part's logs. Exit 0 on PASS, 1 on FAIL, 2 on usage. `--write-floor` writes bench/smoke/floor.json from a PASS only:
# the release step does that at each tag. Every pipeline's status is read from PIPESTATUS, never a bare pipe's exit.
set -uo pipefail
cd "$(dirname "$0")/.."

parts="tb route recall resume dm"
write_floor=
while [ $# -gt 0 ]; do
  case "$1" in
    --parts) parts="${2:?--parts needs a list}"; shift 2 ;;
    --write-floor) write_floor=--write-floor; shift ;;
    -h | --help) sed -n '2,29p' "$0"; exit 0 ;;
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

sha="$(git rev-parse HEAD)"
run="bench/smoke/runs/$(date +%F)-${sha:0:8}"
[ ! -e "$run/report.md" ] || run="$run-$(date +%H%M)"
mkdir -p "$run"
bin="${SMOKE_BIN_DIR:-$(dirname "$(command -v theseusd 2> /dev/null || echo target/release/theseusd)")}"
sim="${SMOKE_SIM:-}"
for c in "$bin/theseus-sim" target/release/theseus-sim target/debug/theseus-sim; do
  [ -n "$sim" ] || { [ -x "$c" ] && sim=$c; }
done
sock=()
[ -z "${SMOKE_SOCKET:-}" ] || sock=(--socket "$SMOKE_SOCKET")
musl="${THESEUS_BENCH_BIN_DIR:-$PWD/bench/bin}"
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
  "${env[@]}" "${line[@]}" > "$run/tb.log" 2>&1 &
  local pid=$!
  while kill -0 "$pid" 2> /dev/null; do
    spent="$(smoke spent "$jobs")"
    if "$py" -c "import sys; sys.exit(float('$spent') < float('${SMOKE_TB_STOP_USD:-6}'))"; then
      echo "release-smoke: tb stopped at \$$spent of finished trials" | tee -a "$run/tb.log"
      kill -INT "$pid" 2> /dev/null
    fi
    sleep 30
  done
  wait "$pid" || rc=$?
  echo "release-smoke: tb's run exited $rc" >> "$run/tb.log"
  smoke tb "$run" "$jobs" "$(($(now) - t0))"
}

part_route() {
  local cases=crates/theseus-judge/packs/route.v10.cases.toml i t0 rc args=()
  if ! "$bin/theseus" judge replay --help 2>&1 | grep -q -- '--cases'; then
    smoke fail "$run" route "this binary has no \`judge replay --cases\` (it came with 1.0.4's wave 3')"
    return
  fi
  for i in 1 2 3; do
    t0=$(now)
    rc=0
    "$bin/theseus" "${sock[@]}" --json judge replay --cases "$cases" > "$run/route-$i.json" 2> "$run/route-$i.err" || rc=$?
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
  for b in theseus theseusd theseus-index; do ln -sf "$(realpath "$bin/$b")" "$sim_dir/$b"; done
  [ -z "$sim" ] || ln -sf "$(realpath "$sim")" "$sim_dir/theseus-sim"
  t0=$(now)
  THESEUS_RECALL_BIN_DIR="$PWD/$sim_dir" scripts/recall-tests.sh > "$run/recall.log" 2>&1 || rc=$?
  smoke recall "$run" "$rc" "$(($(now) - t0))" "$run/recall.log"
}

part_resume() {
  [ -n "$sim" ] || { smoke fail "$run" resume "no theseus-sim (SMOKE_SIM)"; return; }
  "$py" bench/smoke/resume.py --bin-dir "$bin" --sim "$sim" --work "$run/resume" --out "$run/resume.json" \
    > "$run/resume.log" 2>&1
  smoke resume "$run" "$run/resume.json"
}

part_dm() {
  "$py" bench/smoke/dm.py --theseus "$bin/theseus" ${SMOKE_SOCKET:+--socket "$SMOKE_SOCKET"} --out "$run/dm.json" \
    > "$run/dm.log" 2>&1
  [ -s "$run/dm.json" ] || { smoke fail "$run" dm "dm.py wrote nothing: $(tail -1 "$run/dm.log" | cut -c1-160)"; return; }
  smoke dm "$run" "$run/dm.json"
}

for p in $parts; do
  case "$p" in
    tb | route | recall | resume | dm) "part_$p" ;;
    *) echo "release-smoke: no part $p" >&2; exit 2 ;;
  esac
done
build="$(cat "$musl/build-commit" 2> /dev/null || echo unknown)"
smoke report "$run" "$sha" "$bin; tb's static build ${build:0:12}" $write_floor
