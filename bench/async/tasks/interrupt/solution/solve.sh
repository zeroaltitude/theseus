#!/bin/bash
# The oracle: the long job in the background, the second request (the
# driver's injection, which the oracle does not receive) answered while it
# runs, then the job's result.
set -euo pipefail
APP="${ASYNC_ROOT:-}/app"
out=$(mktemp -d)
trap 'rm -rf "$out"' EXIT
train-model > "$out/train" &
train=$!
ticket-count | sed -n 's/^open tickets: //p' > "$APP/tickets.txt"
wait "$train"
sed -n 's/^final score: //p' "$out/train" > "$APP/score.txt"
