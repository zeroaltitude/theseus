#!/usr/bin/env bash
# The two binaries a benchmark's task container runs (theseus-n88g.3): `theseus`
# and `theseusd`, static, for x86_64-unknown-linux-musl, so they run in any image
# whatever its libc (two of Terminal-Bench 2.0's 89 images have a glibc older than
# the host build needs). An install stays the host's glibc build (scripts/build.sh,
# scripts/AGENTS.md "glibc or static musl"); this is for benchmark containers only.
#
#   bench/build.sh [OUT_DIR]
#
# It builds through scripts/build.sh (the pinned toolchain, --locked, the
# release-thin profile, the fixed paths), copies the two into OUT_DIR (default
# bench/bin, which git ignores), checks that neither asks for a dynamic loader, and
# prints the line that points the Harbor adapter at them:
#   export THESEUS_BENCH_BIN_DIR=…/bench/bin
# It needs the musl target (`rustup target add x86_64-unknown-linux-musl`) and
# musl-gcc (musl-tools) for ring's C; .cargo/config.toml names both. The bench
# profile turns the web UI off, so the cockpit need not be built first.
set -euo pipefail
cd "$(dirname "$0")/.."
out="${1:-bench/bin}"
target=x86_64-unknown-linux-musl
scripts/build.sh --profile release-thin --target "$target" -p theseus -p theseusd
built="${CARGO_TARGET_DIR:-target}/$target/release-thin"
mkdir -p "$out"
for b in theseus theseusd; do
  # Copy, then rename: a run may be reading the old one.
  cp "$built/$b" "$out/.$b.new"
  mv -f "$out/.$b.new" "$out/$b"
  if readelf -l "$out/$b" | grep -q 'program interpreter'; then
    echo "bench/build.sh: $out/$b asks for a dynamic loader; it is not static" >&2
    exit 1
  fi
done
# The commit the binaries were built from, which `theseus --version` does not say (it is 0.0.1 for every build):
# the adapter writes it to each trial's record (`build_commit`), so two builds tell apart. A tree with changes to
# tracked files says so, since its binaries are not that commit's.
commit="$(git rev-parse HEAD 2>/dev/null || echo unknown)"
if [ "$commit" != unknown ] && [ -n "$(git status --porcelain --untracked-files=no 2>/dev/null)" ]; then
  commit="$commit-dirty"
fi
printf '%s\n' "$commit" > "$out/build-commit"
ls -l "$out/theseus" "$out/theseusd" "$out/build-commit"
echo "export THESEUS_BENCH_BIN_DIR=$(cd "$out" && pwd)"
