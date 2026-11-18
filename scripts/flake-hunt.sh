#!/usr/bin/env bash
# The flake hunt (theseus-20): the whole suite, run again and again, to make a test that fails by chance fail.
#
#   scripts/flake-hunt.sh N [--load] [--threads T] [--spinners S] [--nice K] [--filter EXPR] [--view] [--out DIR] [--no-build]
#
# N whole runs of exactly the gate's suite command (`cargo nextest run --workspace --no-fail-fast`, the default
# profile, so no retry hides a failure), one after another, built once first. What makes it a hunt:
#   --load        S CPU burners (default twice the cores) run beside every suite run, started by load-rig.py so that
#                 none can outlive this script. --nice K runs the suite at nice K under them (default 0: equal share).
#   --threads T   nextest's test threads, raised from the profile's (NEXTEST_TEST_THREADS=T); with no burners it is the
#                 way to crowd a machine with the suite itself. Give both flags for both pressures at once.
#   --filter E    only the tests of nextest filterset E (`-E E`): to hammer one test, or a handful, in a hundred runs of seconds
#                 each. It narrows the hunt; a whole-suite hunt is the one with no filter.
#   --view        the same hunt run inside a self build's view (the sandbox the join runs the gate in: a pty, devices,
#                 nesting and an unmasked /proc granted, the floor hidden, no network, `nice -n 19`), by the one test of
#                 crates/theseusd/tests/self_build.rs that runs a command of a real checkout there. It builds the view's own
#                 target directory first, so the first run is a cold build. The four tests no grant reaches (the view's
#                 withheld set, THESEUS_VIEW_WITHHELD) are left out exactly as the gate leaves them out, and said.
# Run it on a box, never on the owner's machine: a run is minutes of every core. Before a release: ten of each,
#   scripts/flake-hunt.sh 10 --load ; scripts/flake-hunt.sh 10 --threads 96
# Output: every run's own log under DIR (default target/flake-hunt), one `HUNT` line per run, the failed tests'
# own output after it, and at the end a table of each test that failed: runs failed of runs made. Exit 0 when no
# test failed in any run, 1 when one did, 2 on a usage error or a run that built nothing. Everything it prints is
# also in DIR/hunt.log.
set -uo pipefail
cd "$(dirname "$0")/.."

usage() { echo "usage: scripts/flake-hunt.sh N [--load] [--threads T] [--spinners S] [--nice K] [--filter EXPR] [--view] [--out DIR] [--no-build]" >&2; exit 2; }
want_whole() { [[ "$2" =~ ^[0-9]+$ ]] && [ "$2" -ge 1 ] || { echo "flake-hunt: $1 takes a whole number, 1 or more (got '$2')" >&2; exit 2; }; }

[ $# -ge 1 ] || usage
runs=$1; shift
want_whole N "$runs"
flags=("$@")
load=no threads="" spinners="" nice=0 out=target/flake-hunt build=yes filter="" view=no
while [ $# -gt 0 ]; do
  case "$1" in
    --load) load=yes ;;
    --threads) [ $# -ge 2 ] || usage; want_whole --threads "$2"; threads=$2; shift ;;
    --spinners) [ $# -ge 2 ] || usage; want_whole --spinners "$2"; spinners=$2; shift ;;
    --nice) [ $# -ge 2 ] || usage; want_whole --nice "$2"; nice=$2; shift ;;
    --filter) [ $# -ge 2 ] || usage; filter=$2; shift ;;
    --out) [ $# -ge 2 ] || usage; out=$2; shift ;;
    --view) view=yes ;;
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
[ -z "${THESEUS_VIEW_GRANTS:-}" ] || cond="$cond+self-build-view"
mkdir -p "$out"

if [ "$view" = yes ]; then
  # Outside: hand this same hunt, minus --view, to the self build's view and read its verdict back from DIR.
  inner="nice -n 19 bash scripts/flake-hunt.sh $runs"
  for f in "${flags[@]}"; do [ "$f" = --view ] || inner="$inner $(printf '%q' "$f")"; done
  case " ${flags[*]} " in *" --out "*) ;; *) inner="$inner --out $(printf '%q' "$out")" ;; esac
  rm -f "$out/hunt.log" "$out/failed-tests.tsv"
  THESEUS_SELF_BUILD_TREE=$PWD THESEUS_SELF_BUILD_CMD=$inner \
    cargo nextest run -p theseusd --test self_build --run-ignored only --no-capture \
    -E 'test(=a_real_checkouts_command_runs_in_the_view)'
  [ -s "$out/hunt.log" ] || { echo "HUNT: the view ran no hunt (no $out/hunt.log); the view's own run is above" >&2; exit 2; }
  cat "$out/hunt.log"
  grep -q '^HUNT table' "$out/hunt.log" || { echo "HUNT: the hunt in the view ended with no table" >&2; exit 2; }
  [ ! -s "$out/failed-tests.tsv" ]
  exit $?
fi

exec > >(tee "$out/hunt.log") 2>&1
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

# The view's withheld set, when this runs in one: the tests no grant of the view reaches are false by construction
# there, and the gate leaves them out by name (scripts/gate.sh, view_withheld_filter); so does the hunt, and says so.
withheld=""
while IFS= read -r line; do
  bid="${line%% *}"; tname="${line#* }"
  if [ -z "$bid" ] || [ "$tname" = "$line" ]; then
    echo "flake-hunt: THESEUS_VIEW_WITHHELD entry is not '<binary id> <test name>': $line" >&2
    exit 2
  fi
  withheld="${withheld:+$withheld | }(binary_id(=$bid) & test(=$tname))"
done < <(printf '%s\n' "${THESEUS_VIEW_WITHHELD:-}" | sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//' -e '/^$/d')
if [ -n "$withheld" ]; then
  filter="${filter:+($filter) & }not ($withheld)"
  echo "HUNT: the view's withheld set is EXCLUDED from every run, by name: $(printf '%s\n' "$THESEUS_VIEW_WITHHELD" | sed '/^[[:space:]]*$/d' | paste -sd';')"
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
  reds=$(grep -E '^ +(FAIL|TIMEOUT|SIG[A-Z]+) \[' "$log" | sed -E 's/^ +[A-Z]+ +\[[^]]*\] +(\( *[0-9]+\/[0-9]+\) +)?//' | sort -u)
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
