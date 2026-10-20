#!/usr/bin/env bash
# The live acceptance journey (theseus-ypnxt.8, theseus-h9tu1): the cross-surface journey of
# crates/theseusd/tests/acceptance_journey.rs run on a scratch daemon of this tree's build, with its
# real-time part, and its steps and measurements written to bench/results/acceptance-journey-<date>.md.
#
#   scripts/acceptance/journey.sh [--profile PROFILE] [--out FILE]
#
# The daemon is a scratch one: its own socket and state dir in a temp directory, a stand-in model
# (`theseus-sim fake-model`), a stand-in Discord (in the test's process) and the real `theseus`.
# It never touches the owner's daemon, its socket, its port or its bindings. `--profile release`
# runs it on an install build (nextest builds the tests with that profile, so the daemon and the CLI
# are the install's binaries): do that only with the owner's OK, and still never on the owner's own
# daemon, which this script cannot reach.
#
# Not driven here: the herdr steps (a tab for each task, `blocked` while one asks, marked done at
# the end). herdr needs a herdr session of its own; with one running, follow a scratch daemon in it:
#   theseus --socket <scratch sock> herdr up --follow --herdr-socket <its socket>
# and watch the tabs while this journey runs against the same socket. The report says whether herdr
# is installed and that the steps were not run.
set -euo pipefail
cd "$(dirname "$0")/../.."

profile=""
date_now="$(date +%F)"
out="bench/results/acceptance-journey-${date_now}.md"
while [ $# -gt 0 ]; do
  case "$1" in
    --profile) profile="$2"; shift 2 ;;
    --out) out="$2"; shift 2 ;;
    *) echo "usage: $0 [--profile PROFILE] [--out FILE]" >&2; exit 2 ;;
  esac
done

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
profile_args=()
build_args=()
if [ -n "$profile" ]; then
  profile_args=(--cargo-profile "$profile")
  build_args=(--profile "$profile")
fi

# The sim and the CLI are built beside the daemon the tests spawn (nextest builds only the tests').
cargo build "${build_args[@]}" -p theseus -p theseus-sim -p theseusd >&2

run() { # name filter extra...
  local name="$1" filter="$2"
  shift 2
  THESEUS_JOURNEY_REPORT="$work/$name.json" TZ="${TZ:-America/Phoenix}" \
    cargo nextest run "${profile_args[@]}" -p theseusd --test acceptance_journey \
    -E "$filter" "$@" --no-fail-fast >"$work/$name.log" 2>&1 && echo pass >"$work/$name.status" \
    || echo fail >"$work/$name.status"
}
run journey 'test(the_owner)'
run realtime 'test(a_change_reaches)' --run-ignored only

herdr="not installed"
command -v herdr >/dev/null 2>&1 && herdr="installed ($(command -v herdr)); its steps were not driven by this script"

mkdir -p "$(dirname "$out")"
python3 - "$work" "$out" "$herdr" "$(git rev-parse --short HEAD)" "$date_now" <<'PY'
import json, os, sys
work, out, herdr, rev, day = sys.argv[1:6]
def status(n):
    p = os.path.join(work, n + ".status")
    return open(p).read().strip() if os.path.exists(p) else "not run"
def report(n):
    p = os.path.join(work, n + ".json")
    return json.load(open(p)) if os.path.exists(p) else None
def tail(n, k=25):
    p = os.path.join(work, n + ".log")
    return "".join(open(p).readlines()[-k:]) if os.path.exists(p) else ""
j, rt = report("journey"), report("realtime")
L = [f"# The acceptance journey, {day}", "",
     f"Tree `{rev}`. A scratch daemon of this build, the stand-in model and Discord, the real CLI.",
     f"herdr: {herdr}.", "",
     f"- journey (the gate's run): **{status('journey')}**",
     f"- real time (10 ms p95 for a change to reach a watching client): **{status('realtime')}**", ""]
if j:
    L += ["## Steps", "", "| step | at (ms on the rig's clock) |", "|---|---:|"]
    L += [f"| {s['step']} | {s['at_ms']} |" for s in j["steps"]]
    L += ["", "## Measured (ms)", "", "| what | ms |", "|---|---:|"]
    L += [f"| {k} | {v} |" for k, v in sorted(j["ms"].items())]
    i = j["interrupts"]
    L += ["", "## Interrupts (the Discord rings the task paths made)", "",
          f"- questions {i['questions']}, finishes {i['finishes']}, failures 0",
          f"- closest two rings in one conversation: {i['closest_ms']} ms",
          f"- the chat's own pings (the owner's choice, not judged): {len(i['chat'])}"]
    L += [f"  - {c}" for c in i["chat"]]
    f = j["fast"]
    L += ["", "## Still fast", "",
          f"- a plain turn: {f['frames']} frames (budget 4)",
          f"- {f['changes']} changes read by a watching client: p95 {f['p95_ms']} ms, max {f['max_ms']} ms"]
    for k, v in sorted(j.get("captures", {}).items()):
        L += ["", f"## Capture: {k}", "", "```", v.rstrip(), "```"]
if rt:
    L += ["", f"Real-time run: p95 {rt['fast']['p95_ms']} ms over {rt['fast']['changes']} changes."]
for n in ("journey", "realtime"):
    if status(n) != "pass":
        L += ["", f"## {n}: the end of its log", "", "```", tail(n).rstrip(), "```"]
open(out, "w").write("\n".join(L) + "\n")
print(out)
PY
[ "$(cat "$work/journey.status")" = pass ]
