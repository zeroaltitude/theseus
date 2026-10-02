#!/usr/bin/env bash
# End-to-end smoke test (M0 First light onward). Needs OP_SERVICE_ACCOUNT_TOKEN (or
# THESEUS_OP_TOKEN_FILE) and a config (THESEUS_CONFIG: a file, or a 1Password note's op://
# reference; without it, theseusd's default, ~/.theseus/theseus.toml).
set -euo pipefail
cd "$(dirname "$0")/.."
BIN=${BIN:-target/debug}
SOCK=$(mktemp -u /tmp/theseus-smoke-XXXX.sock)
export THESEUS_SOCKET="$SOCK"
STATE=$(mktemp -d /tmp/theseus-smoke-state-XXXX)
PID=
trap 'if [ -n "$PID" ]; then kill "$PID" 2>/dev/null || true; fi; rm -rf "$STATE" "$SOCK"' EXIT

cargo build -q
"$BIN/theseusd" check
THESEUS_LOG=warn "$BIN/theseusd" --socket "$SOCK" --state-dir "$STATE" & PID=$!
for _ in $(seq 1 80); do [ -S "$SOCK" ] && break; sleep 0.25; done
[ -S "$SOCK" ] || { echo "daemon did not come up"; exit 1; }

echo "== health";  "$BIN/theseus" health
echo "== ask";     out=$("$BIN/theseus" ask --no-stream "Reply with the single word: ready")
echo "model said: $out"
echo "== json";    echo "Reply with the single word: piped" | "$BIN/theseus" ask --json \
  | python3 -c 'import sys,json; d=json.load(sys.stdin); assert d["loops"]==1 and d["stop_reason"]=="no_tool_calls", d; print("one loop, no_tool_calls, output:", d["output"].strip())'
echo "== web";     curl -sf -o /dev/null "http://127.0.0.1:${WEB_PORT:-7433}/" && echo "web UI served on http://127.0.0.1:${WEB_PORT:-7433}/" || echo "web UI not reachable from this instance (another daemon may hold the port; not a failure)"
echo "== telemetry"; "$BIN/theseus" health | grep -E "^telemetry"
echo "== ledger";  "$BIN/theseus" ledger -n 3 -k provider.call | tail -1 | cut -c1-120
echo "== kernel";  "$BIN/theseus" health | grep -E "^kernel"
echo "== executions"; "$BIN/theseus" executions | head -3
echo "== actions"; "$BIN/theseus" ledger -n 2 -k action.succeeded | tail -1 | cut -c1-120
echo "== tools";   "$BIN/theseus" tools 2>&1 | tail -1
echo "== catalog"; "$BIN/theseus" catalog 2>/dev/null | awk 'NR>1 {n++} END {print n " models in the catalog"}'
echo "== history"; "$BIN/theseus" history > "$STATE/history.txt" && head -3 "$STATE/history.txt"
echo "== wrapper"; "$BIN/theseusd" job-wrapper --spool "$STATE/spool" --correlation-id act_smoke_stray --deadline-ms 5000 --notify "$STATE/spool/notify.sock" -- /bin/echo hello-from-wrapper; sleep 0.5
"$BIN/theseus" health | grep -E "^kernel" | grep -q "quarantined completions 1" && echo "stray wrapper completion quarantined (never inferred)"
echo "== stdio";   THESEUS_STATE_DIR="$STATE" "$BIN/theseus" --spawn "$BIN/theseusd" ask --no-stream "Reply with the single word: stdio" 2>/dev/null
echo "== shutdown"; "$BIN/theseus" shutdown
wait $PID
echo "SMOKE OK"
