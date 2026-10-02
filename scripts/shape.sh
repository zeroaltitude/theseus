#!/usr/bin/env bash
# The shape budget's file ceiling (theseus-goa8; review 2's C1): no Rust file
# under crates/ passes 2,500 lines unless scripts/long-files.txt lists it, and a
# listed file stays under its own ceiling. The list can only shrink or be
# raised on purpose:
#   - a file over 2,500 lines with no entry fails: split it, or list it with a
#     ceiling and the reason;
#   - a listed file past its ceiling fails: split it, or raise the ceiling in the
#     same commit and say why;
#   - a listed file at or under 2,500 lines fails: its entry is stale, delete it.
# Function length and complexity are clippy's (`too_many_lines`,
# `cognitive_complexity`, in the workspace lints), held by the gate's clippy.
set -euo pipefail
cd "$(dirname "$0")/.."
limit=2500
list=scripts/long-files.txt

declare -A ceiling
while read -r path c _; do
  case "$path" in '' | '#'*) continue ;; esac
  ceiling[$path]=$c
done <"$list"

status=0
declare -A seen
while IFS= read -r f; do
  [ -f "$f" ] || continue
  n=$(wc -l <"$f")
  c="${ceiling[$f]:-}"
  if [ -z "$c" ]; then
    if [ "$n" -gt "$limit" ]; then
      echo "shape: $f has $n lines, over $limit, and no entry in $list: split it, or list it with a ceiling and the reason"
      status=1
    fi
    continue
  fi
  seen[$f]=1
  if [ "$n" -le "$limit" ]; then
    echo "shape: $f has $n lines, at or under $limit: delete its entry in $list"
    status=1
  elif [ "$n" -gt "$c" ]; then
    echo "shape: $f has $n lines, over its ceiling of $c in $list: split it, or raise the ceiling in this commit and say why"
    status=1
  fi
done < <(git ls-files --cached --others --exclude-standard -- 'crates/*.rs')

# An entry for a file that is gone is as stale as one for a file that shrank.
for f in "${!ceiling[@]}"; do
  if [ -z "${seen[$f]:-}" ]; then
    echo "shape: $list lists $f, which is not a Rust file under crates/ any more: delete its entry"
    status=1
  fi
done

[ "$status" -eq 0 ] && echo "shape: ok (no file over $limit lines but the ${#ceiling[@]} listed)"
exit "$status"
