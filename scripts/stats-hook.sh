#!/usr/bin/env bash
# STATS.md's landing count (issue 45): install or check scripts/hooks/post-merge, which counts STATS.md again
# into every merge made on `main` (its own header says how, and what it does not cover).
#
#   scripts/stats-hook.sh install   copy the hook into the repository's shared hooks directory as `post-merge`
#                                   and `post-commit`; a no-op when the installed copies are current; refuses to
#                                   replace a hook of either name it did not write, and to run from a linked worktree
#   scripts/stats-hook.sh check     say whether both are installed and current; exit 1 when not
#
# As scripts/main-tree-guard.sh does: the installed hook is a COPY, never a link into the tree, and it is installed only
# from the main working tree, since the hooks directory is shared by every worktree.
#
# Exit status: 0 installed and current; 1 not (check), or a hook of someone else's is in the way or this is a lane
# (install); 2 usage.
set -u
ROOT=$(cd "$(dirname "$0")/.." && pwd -P)
SRC=$ROOT/scripts/hooks/post-merge
MARK=theseus-stats-landing
NAMES=(post-merge post-commit)

common=$(git -C "$ROOT" rev-parse --path-format=absolute --git-common-dir) || exit 2
gd=$(git -C "$ROOT" rev-parse --path-format=absolute --git-dir) || exit 2
hooks=$common/hooks
# core.hooksPath, when set, is where git looks instead, and a copy put elsewhere would never run.
custom=$(git -C "$ROOT" config --get core.hooksPath || true)
[ -z "$custom" ] || hooks=$(cd "$ROOT" && mkdir -p "$custom" && cd "$custom" && pwd -P)

case "${1:-}" in
install)
  if [ "$gd" != "$common" ]; then
    echo "stats hook: $ROOT is a linked worktree; install from the main working tree."
    exit 1
  fi
  for n in "${NAMES[@]}"; do
    if [ -e "$hooks/$n" ] && ! grep -q "$MARK" "$hooks/$n"; then
      echo "stats hook: $hooks/$n is a hook this script did not write; nothing was installed."
      echo "stats hook: add the lines of $SRC to it by hand, or move it aside and run this again."
      exit 1
    fi
  done
  mkdir -p "$hooks" || exit 1
  for n in "${NAMES[@]}"; do
    if [ -e "$hooks/$n" ] && cmp -s "$SRC" "$hooks/$n"; then
      echo "stats hook: $n installed and current"
      continue
    fi
    cp "$SRC" "$hooks/$n.new" && chmod 0755 "$hooks/$n.new" && mv -f "$hooks/$n.new" "$hooks/$n" || exit 1
    echo "stats hook: $n installed ($hooks/$n)"
  done
  ;;
check)
  bad=0
  for n in "${NAMES[@]}"; do
    if [ -e "$hooks/$n" ] && cmp -s "$SRC" "$hooks/$n"; then
      echo "stats hook: $n installed and current"
    else
      echo "stats hook: $n not installed or older than $SRC: run scripts/stats-hook.sh install"
      bad=1
    fi
  done
  exit $bad
  ;;
*)
  echo "usage: scripts/stats-hook.sh install|check" >&2
  exit 2
  ;;
esac
