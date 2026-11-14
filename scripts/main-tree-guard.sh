#!/usr/bin/env bash
# The main working tree's guard (issue 15): install or check scripts/hooks/post-checkout, which keeps the main
# working tree on `main` (its own header says how, and what it does not cover).
#
#   scripts/main-tree-guard.sh install   copy the hook into the repository's shared hooks directory; a no-op when the
#                                        installed copy is current; refuses to replace a post-checkout hook it did
#                                        not write
#   scripts/main-tree-guard.sh check     say whether it is installed and current; exit 1 when it is not
#
# The installed hook is a COPY, never a link into the tree: a link would run whatever the checked-out commit holds,
# so a branch could edit it, and a checkout of a commit without it would leave nothing to run.
#
# Exit status: 0 installed and current; 1 not (check), or a hook of someone else's is in the way (install); 2 usage.
set -u
ROOT=$(cd "$(dirname "$0")/.." && pwd -P)
SRC=$ROOT/scripts/hooks/post-checkout
MARK=theseus-main-tree-guard

hooks=$(git -C "$ROOT" rev-parse --path-format=absolute --git-common-dir)/hooks || exit 2
# core.hooksPath, when set, is where git looks instead, and a copy put elsewhere would never run.
custom=$(git -C "$ROOT" config --get core.hooksPath || true)
[ -z "$custom" ] || hooks=$(cd "$ROOT" && mkdir -p "$custom" && cd "$custom" && pwd -P)
DST=$hooks/post-checkout

case "${1:-}" in
install)
  if [ -e "$DST" ] && ! grep -q "$MARK" "$DST"; then
    echo "main-tree guard: $DST is a post-checkout hook this script did not write; it is left alone."
    echo "main-tree guard: add the lines of $SRC to it by hand, or move it aside and run this again."
    exit 1
  fi
  if [ -e "$DST" ] && cmp -s "$SRC" "$DST"; then
    echo "main-tree guard: installed and current ($DST)"
    exit 0
  fi
  mkdir -p "$hooks" && cp "$SRC" "$DST.new" && chmod 0755 "$DST.new" && mv -f "$DST.new" "$DST" || exit 1
  echo "main-tree guard: installed ($DST)"
  ;;
check)
  if [ -e "$DST" ] && cmp -s "$SRC" "$DST"; then
    echo "main-tree guard: installed and current ($DST)"
  elif [ -e "$DST" ] && grep -q "$MARK" "$DST"; then
    echo "main-tree guard: installed but older than $SRC: run scripts/main-tree-guard.sh install"
    exit 1
  else
    echo "main-tree guard: not installed: run scripts/main-tree-guard.sh install"
    exit 1
  fi
  ;;
*)
  echo "usage: scripts/main-tree-guard.sh install|check"
  exit 2
  ;;
esac
