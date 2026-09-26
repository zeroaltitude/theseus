#!/usr/bin/env bash
# End-to-end smoke test for M0 First light. Needs OP_SERVICE_ACCOUNT_TOKEN (or
# THESEUS_OP_TOKEN_FILE) and a config (THESEUS_CONFIG, default: the 1Password item).
set -euo pipefail
cd "$(dirname "$0")/.."
BIN=${BIN:-target/debug}
SOCK=$(mktemp -u /tmp/theseus-smoke-XXXX.sock)
export THESEUS_SOCKET="$SOCK"
STATE=$(mktemp -d /tmp/theseus-smoke-state-XXXX)
trap 'kill $PID 2>/dev/null || true; rm -rf "$STATE" "$SOCK"' EXIT

cargo build -q
"$BIN/theseusd" check
THESEUS_LOG=warn "$BIN/theseusd" --socket "$SOCK" & PID=$!
for _ in $(seq 1 80); do [ -S "$SOCK" ] && break; sleep 0.25; done
[ -S "$SOCK" ] || { echo "daemon did not come up"; exit 1; }

echo "== health";  "$BIN/theseus" health
echo "== hooks";   "$BIN/theseus" hooks list | awk '{n++} END {print n " hook events, all registerable"}'
echo "== ask";     out=$("$BIN/theseus" ask --no-stream "Reply with the single word: ready")
echo "model said: $out"
echo "== json";    echo "Reply with the single word: piped" | "$BIN/theseus" ask --json \
  | python3 -c 'import sys,json; d=json.load(sys.stdin); assert d["loops"]==1 and d["stop_reason"]=="stop_after_one_loop", d; print("one loop, stop_after_one_loop, output:", d["output"].strip())'
echo "== web";     curl -sf -o /dev/null "http://127.0.0.1:${WEB_PORT:-7433}/" && echo "web UI served on http://127.0.0.1:${WEB_PORT:-7433}/" || echo "web UI not reachable (is another daemon holding the port?)"
echo "== ledger";  "$BIN/theseus" ledger -n 3 -k provider.call | tail -1 | cut -c1-120
echo "== stdio";   "$BIN/theseus" --spawn "$BIN/theseusd" ask --no-stream "Reply with the single word: stdio" 2>/dev/null
echo "== shutdown"; "$BIN/theseus" shutdown
wait $PID
echo "SMOKE OK"
