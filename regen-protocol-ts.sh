#!/usr/bin/env bash
# theseus-97b8c: regenerate the cockpit's protocol types in this lane and print
# every file the generator added or changed, so the main machine can write them
# into the branch and sign the commit there. Nothing is committed here.
set -u
cd "$(dirname "$0")" || exit 1
cargo nextest run --workspace -E 'package(theseus-protocol)' 2>&1 | tail -3
echo "=== CHANGED ==="
git status --porcelain -- cockpit/src/protocol.gen
for f in $(git status --porcelain -- cockpit/src/protocol.gen | awk '{print $2}'); do
  echo "=== FILE $f ==="
  cat "$f"
done
echo "=== END ==="
