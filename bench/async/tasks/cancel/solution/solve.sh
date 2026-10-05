#!/bin/bash
# The oracle: the migration started in a process group of its own, then
# cancelled (the driver's injection, which the oracle does not receive) by a
# SIGTERM to the whole group, so none of its workers outlives it.
set -uo pipefail
set -m
migrate &
pid=$!
set +m
# Its workers are started within a moment.
sleep 1
kill -TERM -- "-$pid"
wait "$pid"
echo "the migration was cancelled"
exit 0
