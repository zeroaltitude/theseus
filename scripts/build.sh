#!/usr/bin/env bash
# The release build, the one way (theseus-goa8; review 2's SC3): every install,
# and scripts/repro.sh, builds with this, so that a binary is a function of the
# commit and the pinned toolchain (rust-toolchain.toml), and not of the directory
# it was built in or the minute it was built.
#
#   scripts/build.sh [--profile release|release-thin] [cargo build arguments…]
#   scripts/build.sh --shipped   (prints the five packages below, and builds nothing)
#   scripts/build.sh --installed (prints the four binaries an install copies, and builds nothing)
#
#   release       fat LTO and one codegen unit: the install profile and a tagged release (the
#                 default; theseus-o2myi).
#   release-thin  thin LTO and 16 codegen units: the quick build, for a bench, the AWS hand,
#                 or a try-out, never an install (Cargo.toml says why it is not `install`,
#                 and scripts/AGENTS.md has the measurements).
#
# What it builds: the five binaries an install ships, theseusd, theseus, theseus-tui,
# theseus-sim, and theseus-index, and what they link (theseus-o8nk). The crates still
# waiting for their roadmap rows (judge, exam, mcp, ontology, memory, aws-guard) are
# not compiled. scripts/gate.sh's features phase reads the list (`--shipped`).
#
# Cargo unifies a dependency's features over the packages it builds, so building some
# packages can give a shared crate fewer features than the whole workspace, which the
# gate tests, gives it. For these five it gives none fewer: no waiting crate adds a
# feature to anything they link (checked 2026-10-03 with cargo's unit graph). The
# gate's features phase holds it (theseus-dr2x): it fails naming a crate that does,
# and the feature is then named in the shipped crate that links it, as
# theseus-discord names twilight-gateway's TLS roots.
#
# theseus-sim is built for the size check and the benches, which run the copy in target/, and is not installed
# (theseus-rwpbg.5): an install copies the four `--installed` prints, and a bench tool is run from its target dir.
#
# After the build: the size check (`theseus-sim bench size --check`) on what was just built, below.
#
# A `-p`, `--package`, `--workspace`, or `--all` of your own replaces the five.
#
# What it fixes, beyond the profile:
#   - `--locked`: Cargo.lock is the dependency set, and a build never changes it.
#   - `--remap-path-prefix` for the source tree, the cargo home, the rustup home,
#     and the target directory: rustc writes source paths into panic messages and
#     `file!()`, and a build in another directory would differ in those bytes.
#   - no `.eh_frame` (5.8 MB of theseusd, theseus-rwpbg.5): `-C force-unwind-tables=no` for the crates rustc compiles,
#     and scripts/discard-eh-frame.ld at the link for the rest (std's, the C code's; the flag alone moves 0.05 MB).
#     The release profiles are `panic = "abort"` and stripped, so nothing unwinds and a panic's record (crash.rs:
#     thread, location, message) has no backtrace to lose. What goes: `perf record --call-graph dwarf`, gdb's `bt`, and eBPF stack walks of a
#     live release daemon. (Frame-pointer walks, and a debug or `check` build, which this does not touch, stay.)
#   - SOURCE_DATE_EPOCH, the commit's time, for any build script or tool that
#     wants a clock.
#   - rust-embed's `deterministic-timestamps` (theseusd's manifest): the embedded
#     web files carry no modification time.
# The cockpit's build (cockpit/dist) is not committed; build it first with
# `npm ci && npm run build` in cockpit/, and the binary embeds it.
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"

# The five, one list: `build.sh --shipped` prints it, one package a line, and builds nothing (the gate's features
# phase reads it, theseus-dr2x).
shipped=(theseusd theseus theseus-tui theseus-sim theseus-index)
if [ "${1:-}" = --shipped ]; then
  printf '%s\n' "${shipped[@]}"
  exit 0
fi

# The four an install copies: the shipped five less theseus-sim.
if [ "${1:-}" = --installed ]; then
  printf '%s\n' "${shipped[@]}" | grep -vx theseus-sim
  exit 0
fi

profile=release
while [ $# -gt 0 ]; do
  case "$1" in
    --profile) profile="${2:?--profile needs release or release-thin}"; shift 2 ;;
    *) break ;;
  esac
done
case "$profile" in release | release-thin) ;; *)
  echo "build: no profile $profile (release or release-thin)" >&2
  exit 2
  ;;
esac

root="$(pwd -P)"
cargo_home="${CARGO_HOME:-$HOME/.cargo}"
rustup_home="${RUSTUP_HOME:-$HOME/.rustup}"
target_dir="${CARGO_TARGET_DIR:-$root/target}"
# Later mappings win, so the target directory, which usually sits inside the
# tree, comes after it.
flags="--remap-path-prefix=$cargo_home=/cargo --remap-path-prefix=$rustup_home=/rustup"
flags="$flags --remap-path-prefix=$root=/theseus --remap-path-prefix=$target_dir=/target -C force-unwind-tables=no"
# The flag reaches only the crates rustc compiles; the precompiled std and the C code keep theirs, so the link drops the
# section whole (scripts/discard-eh-frame.ld).
flags="$flags -C link-arg=$root/scripts/discard-eh-frame.ld"
export RUSTFLAGS="${RUSTFLAGS:+$RUSTFLAGS }$flags"
# The commit's time; a tree with no history (a `git archive`) is given it.
: "${SOURCE_DATE_EPOCH:=$(git log -1 --format=%ct 2>/dev/null || echo 0)}"
export SOURCE_DATE_EPOCH

pkgs=()
for p in "${shipped[@]}"; do pkgs+=(-p "$p"); done
for arg in "$@"; do
  case "$arg" in -p | -p?* | --package | --package=* | --workspace | --all) pkgs=() ;; esac
done
cargo build --locked --profile "$profile" "${pkgs[@]}" "$@"

# The size check (theseus-2m7jg): a release binary exists only here, so this is where its size is judged. The
# stamp (scripts/tree-stamp.sh, and theseusd's byte count) is the build's proof of its tree, which the gate's size
# phase reads. Growth of more than 1 MB over the last recorded size of this profile fails the build; §9's 60 MB is
# printed with its gap and fails nothing. THESEUS_SIZE_ACCEPT_GROWTH=1 records the growth as accepted.
out="$target_dir/$profile"
if [ -f "$out/theseusd" ]; then
  printf '%s %s\n' "$(scripts/tree-stamp.sh)" "$(stat -c %s "$out/theseusd")" >"$out/.theseus-build-stamp"
fi
if [ -x "$out/theseus-sim" ] && [ -f "$out/theseusd" ]; then
  accept=()
  [ -z "${THESEUS_SIZE_ACCEPT_GROWTH:-}" ] || accept=(--accept-growth)
  "$out/theseus-sim" bench size --check --dir "$out" \
    --record "${THESEUS_BENCH_HISTORY:-$HOME/.cache/theseus/bench-history.csv}" \
    --label "$(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo detached) $(git describe --always --dirty 2>/dev/null || echo none) size:$profile" \
    --baseline-tag "size:$profile" ${accept[@]+"${accept[@]}"} || {
    echo "build: the size check failed: a binary grew more than 1 MB over its last recorded size (THESEUS_SIZE_ACCEPT_GROWTH=1 accepts it)" >&2
    exit 1
  }
else
  echo "build: size check skipped: no theseusd and theseus-sim in $out (a build of other packages)"
fi
