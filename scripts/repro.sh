#!/usr/bin/env bash
# Two builds of one commit, in two different directories, compared byte for byte
# (theseus-goa8; review 2's SC3). A binary that matches its commit can be checked:
# once Theseus builds Theseus, an installed theseusd should be the one its commit
# builds, on any machine, in any directory.
#
#   scripts/repro.sh [--rev REV] [--profile release|release-thin] [--keep]
#
# For each of two builds it extracts REV (default HEAD) with `git archive` into a
# directory of its own (their names differ in length, so that a build that embeds
# its own path shows), then builds it with scripts/build.sh into a fresh target
# directory, with no compile cache: a cached object would make two builds the same
# by copying, and prove nothing. So this is two full builds of the four shipped
# binaries, from scratch: tens of minutes. Run it niced, and detached. It prints each binary's sha256, then
# `cmp`s the two, and exits 1 when any differs, with how many bytes.
#
#   THESEUS_REPRO_DIR   where the two trees and targets go (default: a temporary
#                       directory, removed at the end unless --keep). Needs 10 GB.
#   THESEUS_REPRO_BINS  the binaries to compare (default: theseusd theseus
#                       theseus-tui theseus-sim)
#
# A nightly job is not installed: scripts/AGENTS.md, "Reproducible builds", says
# how to schedule one.
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"

rev=HEAD profile=release keep=""
while [ $# -gt 0 ]; do
  case "$1" in
    --rev) rev="${2:?--rev needs a revision}"; shift 2 ;;
    --profile) profile="${2:?--profile needs release or release-thin}"; shift 2 ;;
    --keep) keep=1; shift ;;
    *) echo "repro: unknown argument $1" >&2; exit 2 ;;
  esac
done
commit="$(git rev-parse --verify "$rev^{commit}")"
bins="${THESEUS_REPRO_BINS:-theseusd theseus theseus-tui theseus-sim}"

if [ -n "${RUSTC_WRAPPER:-}" ] || [ -n "${CARGO_BUILD_RUSTC_WRAPPER:-}" ]; then
  echo "repro: a compile cache is set (RUSTC_WRAPPER): unset for these builds, which must not copy objects"
fi
unset RUSTC_WRAPPER CARGO_BUILD_RUSTC_WRAPPER

work="${THESEUS_REPRO_DIR:-$(mktemp -d "${TMPDIR:-/tmp}/theseus-repro.XXXXXX")}"
mkdir -p "$work"
cleanup() { [ -n "$keep" ] || rm -rf "$work"; }
trap cleanup EXIT
epoch="$(git log -1 --format=%ct "$commit")"
echo "repro: $commit ($profile), built twice under $work"

build() { # build <name>: the tree in $work/<name>/…, its target in $work/<name>-target
  local name=$1 src="$work/$1"
  mkdir -p "$src"
  git archive "$commit" | tar -x -C "$src"
  echo "repro: building $name in $src ($(date +%T))"
  (cd "$src" && SOURCE_DATE_EPOCH="$epoch" CARGO_TARGET_DIR="$work/$name-target" scripts/build.sh --profile "$profile") >"$work/$name.log" 2>&1 || {
    echo "repro: the build in $src failed; its log ends:"
    tail -20 "$work/$name.log"
    exit 1
  }
  echo "repro: built $name ($(date +%T))"
}
build one
build two-with-a-longer-name

status=0
for b in $bins; do
  a="$work/one-target/$profile/$b" c="$work/two-with-a-longer-name-target/$profile/$b"
  if [ ! -f "$a" ] || [ ! -f "$c" ]; then
    echo "repro: $b is missing from a build"
    status=1
    continue
  fi
  sa="$(sha256sum "$a" | cut -c1-16)" sc="$(sha256sum "$c" | cut -c1-16)"
  if cmp -s "$a" "$c"; then
    echo "repro: $b identical ($sa, $(stat -c %s "$a") bytes)"
  else
    echo "repro: $b DIFFERS: $sa against $sc; $(cmp -l "$a" "$c" | wc -l) bytes differ, the first at offset $(cmp "$a" "$c" | awk '{print $5}' | tr -d ,)"
    status=1
  fi
done
if [ "$status" -ne 0 ]; then
  echo "repro: not reproducible. The trees and targets are kept in $work; compare with"
  echo "       cmp -l one-target/$profile/<bin> two-with-a-longer-name-target/$profile/<bin>, and strings(1)"
  keep=1
else
  echo "repro: reproducible: every binary is identical"
fi
exit "$status"
