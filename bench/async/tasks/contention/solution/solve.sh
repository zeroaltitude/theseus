#!/bin/bash
# The oracle: two lanes, never more (the service's limit), with each account's
# deposits in one lane, so no two deposits to one account overlap; then the
# balances.
set -euo pipefail
APP="${ASYNC_ROOT:-}/app"
lane() { while read -r account amount; do deposit "$account" "$amount" > /dev/null; done; }
cat "$APP/payments-a.txt" "$APP/payments-b.txt" | grep -E '^acct-(north|east) ' | lane &
a=$!
cat "$APP/payments-a.txt" "$APP/payments-b.txt" | grep -E '^acct-(south|west) ' | lane &
b=$!
wait "$a"
wait "$b"
for a in acct-north acct-south acct-east acct-west; do
  echo "$a $(balance "$a")"
done > "$APP/balances.txt"
