#!/usr/bin/env bash
# The flake hunt (theseus-20): the whole suite, run again and again, to make a test that fails by chance fail.
#
#   scripts/flake-hunt.sh N [--load] [--threads T] [--spinners S] [--nice K] [--filter EXPR] [--out DIR] [--no-build]
#
# N whole runs of exactly the gate's suite command (`cargo nextest run --workspace --no-fail-fast`, the default
# profile, so no retry hides a failure), one after another, built once first. What makes it a hunt:
#   --load        S CPU burners (default twice the cores) run beside every suite run, started by load-rig.py so that
#                 none can outlive this script. --nice K runs the suite at nice K under them (default 0: equal share).
#   --threads T   nextest's test threads, raised from the profile's (NEXTEST_TEST_THREADS=T); with no burners it is the
#                 way to crowd a machine with the suite itself. Give both flags for both pressures at once.
#   --filter E    only the tests of nextest filterset E (`-E E`): to hammer one test, or a handful, in a hundred runs of seconds
#                 each. It narrows the hunt; a whole-suite hunt is the one with no filter.
# Run it on a box, never on the owner's machine: a run is minutes of every core. Before a release: ten of each,
#   scripts/flake-hunt.sh 10 --load ; scripts/flake-hunt.sh 10 --threads 96
# Output: every run's own log under DIR (default target/flake-hunt), one `HUNT` line per run, the failed tests'
# own output after it, and at the end a table of each test that failed: runs failed of runs made. Exit 0 when no
# test failed in any run, 1 when one did, 2 on a usage error or a run that built nothing.
set -uo pipefail
cd "$(dirname "$0")/.."

usage() { echo "usage: scripts/flake-hunt.sh N [--load] [--threads T] [--spinners S] [--nice K] [--filter EXPR] [--out DIR] [--no-build]" >&2; exit 2; }
want_whole() { [[ "$2" =~ ^[0-9]+$ ]] && [ "$2" -ge 1 ] || { echo "flake-hunt: $1 takes a whole number, 1 or more (got '$2')" >&2; exit 2; }; }

[ $# -ge 1 ] || usage
runs=$1; shift
want_whole N "$runs"
load=no threads="" spinners="" nice=0 out=target/flake-hunt build=yes filter=""
while [ $# -gt 0 ]; do
  case "$1" in
    --load) load=yes ;;
    --threads) [ $# -ge 2 ] || usage; want_whole --threads "$2"; threads=$2; shift ;;
    --spinners) [ $# -ge 2 ] || usage; want_whole --spinners "$2"; spinners=$2; shift ;;
    --nice) [ $# -ge 2 ] || usage; want_whole --nice "$2"; nice=$2; shift ;;
    --filter) [ $# -ge 2 ] || usage; filter=$2; shift ;;
    --out) [ $# -ge 2 ] || usage; out=$2; shift ;;
    --no-build) build=no ;;
    *) usage ;;
  esac
  shift
done
[ -z "$spinners" ] || [ "$load" = yes ] || { echo "flake-hunt: --spinners means nothing without --load" >&2; exit 2; }
[ "$load" = yes ] && [ -z "$spinners" ] && spinners=$(( $(nproc) * 2 ))

cond="no-load"
[ "$load" = yes ] && cond="load-${spinners}-burners-nice-${nice}"
[ -z "$threads" ] || cond="$cond+threads-$threads"
[ -z "$filter" ] || cond="$cond+filtered"
mkdir -p "$out"
fails="$out/failed-tests.tsv"
: > "$fails"
echo "HUNT start: $runs whole run(s), condition $cond, $(nproc) cores, head $(git rev-parse --short=8 HEAD 2>/dev/null || echo unknown), $(date '+%F %T')"

if [ "$build" = yes ]; then
  # The test build, apart from the runs and without the thread override (nextest warns that --no-run ignores it).
  if ! env -u NEXTEST_TEST_THREADS cargo nextest run --workspace --no-run > "$out/build.log" 2>&1; then
    tail -30 "$out/build.log"
    echo "HUNT: the tests did not build, so nothing was hunted" >&2
    exit 2
  fi
fi

# The gate's own suite command, quiet about what passed: only failures reach the run's log, with their output.
suite=(cargo nextest run --workspace --no-fail-fast --status-level fail --final-status-level fail
  --failure-output final --success-output never)
[ -z "$filter" ] || suite+=(-E "$filter")
[ "$nice" = 0 ] || suite=(nice -n "$nice" "${suite[@]}")
[ -z "$threads" ] || export NEXTEST_TEST_THREADS=$threads
# A burner stops itself after this long whatever happens: a run's own time, with room.
max_secs=${FLAKE_HUNT_MAX_SECS:-7200}
# The rig waits this long for the load to show before the suite starts (its own default is 11 s).
settle=${FLAKE_HUNT_SETTLE:-11}

made=0 red_runs=0
for k in $(seq 1 "$runs"); do
  log="$out/run-$k.log"
  start=$(date +%s)
  if [ "$load" = yes ]; then
    python3 scripts/load-rig.py --spinners "$spinners" --max-secs "$max_secs" --settle "$settle" -- "${suite[@]}" > "$log" 2>&1
  else
    "${suite[@]}" > "$log" 2>&1
  fi
  rc=$?
  made=$((made + 1))
  summary=$(grep -E '^ +Summary' "$log" | tail -1 | sed -E 's/^ +//; s/ +/ /g' | cut -c1-140)
  # The failed tests, as nextest names them in its final recap: `FAIL [ 1.2s] <binary> <test>` (also TIMEOUT, SIG...).
  reds=$(grep -E '^ +(FAIL|TIMEOUT|SIG[A-Z]+) \[' "$log" | sed -E 's/^ +[A-Z]+ +\[[^]]*\] +(\([0-9]+\/[0-9]+\) +)?//' | sort -u)
  echo "HUNT run $k/$runs [$cond] exit=$rc $(( $(date +%s) - start ))s ${summary:-no summary line from nextest}"
  if [ -n "$reds" ]; then
    red_runs=$((red_runs + 1))
    while IFS= read -r t; do printf '%s\t%s\t%s\n' "$k" "$cond" "$t" >> "$fails"; done <<< "$reds"
    echo "HUNT run $k failed: $(echo "$reds" | paste -sd' ')"
    echo "HUNT run $k output of the failures (run log $log):"
    head -600 "$log"
  elif [ "$rc" != 0 ]; then
    # A run that exits non-zero and names no failed test (a build or a runner error): never read as a pass.
    red_runs=$((red_runs + 1))
    printf '%s\t%s\t%s\n' "$k" "$cond" "(no test named: exit $rc)" >> "$fails"
    echo "HUNT run $k exit=$rc with no failed test named; last lines of $log:"
    tail -20 "$log"
  fi
done

echo "HUNT table [$cond]: $red_runs of $made whole run(s) had a failure"
if [ -s "$fails" ]; then
  cut -f3 "$fails" | sort | uniq -c | sort -rn | awk -v n="$made" '{c=$1; $1=""; sub(/^ /, ""); printf "HUNT   %d of %d runs failed: %s\n", c, n, $0}'
  exit 1
fi
echo "HUNT   no test failed in $made run(s)"
