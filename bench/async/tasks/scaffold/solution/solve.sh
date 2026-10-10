#!/bin/bash
# The oracle (theseus-2wxa): the eight files written, then validated at once.
set -euo pipefail
APP="${ASYNC_ROOT:-}/app"
out=$(mktemp -d)
trap 'rm -rf "$out"' EXIT
mkdir -p "$APP/config"
awk -v d="$APP/config" '/^## / {f = d "/" $2} /^- / && f {sub(/^- /, ""); print > f}' "$APP/spec.md"
pids=()
for f in "$APP"/config/*.yaml; do validate-config "$(basename "$f")" > /dev/null & pids+=($!); done
for pid in "${pids[@]}"; do wait "$pid"; done
