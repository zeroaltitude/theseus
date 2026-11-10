#!/usr/bin/env bash
# The gate's prompt phase (theseus-4vsrz, B7): `theseus-sim bench prompt` on the debug binaries, for the record
# and never for the verdict.
#
#   scripts/prompt-step.sh
#
# What it is for. `[routing] cold_switch_tokens` bounds the session's own context, and until 2026-11-08 one
# assertion in crates/theseus-core/src/tests_route_cliff.rs watched an ordinary turn against it. It went on the
# owner's word -- rightly: it was a size assertion on a quantity that grows with every tool we add, it passed by
# 46 tokens of 30,000 on main, and one ordinary tool definition (+89) crossed it. Nothing then watched the figure
# at all, though an ordinary one-message turn was half of the bound. `bench prompt` already measures a fresh
# session's first request per place against a committed baseline and warns past +5 % without failing, so it is
# where the figure belongs: it now records the session's own context and its share of that bound beside the
# total. This script is what makes the gate run it.
#
# **It never fails the gate.** Every trouble -- a missing binary, a daemon that will not start, a census that
# errors -- is printed and returns 0. That is deliberate and is the owner's call recorded in the row: a second
# hard gate on a threshold nobody has calibrated would redden departures for figures legally inside their limits,
# and scripts/shape.sh already owns the hard ceilings. The alarm here is a line a reader sees in the gate's log
# and a baseline whose diff shows the move.
#
# Environment (for the tests): THESEUS_PROMPT_TARGET (the cargo target directory, default target) and
# THESEUS_SIM (the bench binary, default <target>/debug/theseus-sim).
set -uo pipefail
cd "$(dirname "$0")/.."
target="${THESEUS_PROMPT_TARGET:-target}"
sim="${THESEUS_SIM:-$target/debug/theseus-sim}"
if ! [ -x "$sim" ]; then
  echo "prompt: skipped: no $sim (the test build builds it)"
  exit 0
fi
if ! [ -x "$target/debug/theseusd" ]; then
  echo "prompt: skipped: no $target/debug/theseusd, which the census runs on (the test build builds it)"
  exit 0
fi
# The daemon it counts with is the one beside the bench binary, which is the tree under test's.
# `status` is taken on the line after the call and nowhere else: `$?` read after an `if ... fi` is the `if`'s
# own status, which is 0 even when its command failed, and this phase would then report "exit 0".
"$sim" bench prompt --theseusd "$target/debug/theseusd"
status=$?
if [ "$status" -eq 0 ]; then
  exit 0
fi
echo "prompt: the census did not finish (exit $status); the figures above are incomplete. This phase never fails"
echo "prompt: the gate (theseus-4vsrz): it is a record and a warning, not a ceiling. scripts/shape.sh owns those."
exit 0
