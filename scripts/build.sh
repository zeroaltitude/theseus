#!/usr/bin/env bash
# The release build, the one way (theseus-goa8; review 2's SC3): every install,
# and scripts/repro.sh, builds with this, so that a binary is a function of the
# commit and the pinned toolchain (rust-toolchain.toml), and not of the directory
# it was built in or the minute it was built.
#
#   scripts/build.sh [--profile release|release-thin] [cargo build arguments…]
#
#   release       fat LTO and one codegen unit: a tagged release (the default).
#   release-thin  thin LTO and 16 codegen units: the install profile, for a chain's
#                 install, built far faster (Cargo.toml says why it is not `install`,
#                 and scripts/AGENTS.md has the measurements).
#
# What it builds: the five binaries an install ships, theseusd, theseus, theseus-tui,
# theseus-sim, and theseus-index, and what they link (theseus-o8nk). The crates still
# waiting for their roadmap rows (judge, exam, mcp, ontology, memory, aws-guard) are
# not compiled. scripts/gate.sh's bench build uses the same five.
#
# Cargo unifies a dependency's features over the packages it builds, so building some
# packages can give a shared crate fewer features than the whole workspace, which the
# gate tests, gives it. For these five it gives none fewer: no waiting crate adds a
# feature to anything they link (checked 2026-10-03 with cargo's unit graph; the
# one-line recheck is in scripts/AGENTS.md). A crate that does is caught by that
# recheck, and the feature is named in the shipped crate that links it, as
# theseus-discord names twilight-gateway's TLS roots.
#
# A `-p`, `--package`, `--workspace`, or `--all` of your own replaces the five.
#
# What it fixes, beyond the profile:
#   - `--locked`: Cargo.lock is the dependency set, and a build never changes it.
#   - `--remap-path-prefix` for the source tree, the cargo home, the rustup home,
#     and the target directory: rustc writes source paths into panic messages and
#     `file!()`, and a build in another directory would differ in those bytes.
#   - SOURCE_DATE_EPOCH, the commit's time, for any build script or tool that
#     wants a clock.
#   - rust-embed's `deterministic-timestamps` (theseusd's manifest): the embedded
#     web files carry no modification time.
# The cockpit's build (cockpit/dist) is not committed; build it first with
# `npm ci && npm run build` in cockpit/, and the binary embeds it.
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"

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
flags="$flags --remap-path-prefix=$root=/theseus --remap-path-prefix=$target_dir=/target"
export RUSTFLAGS="${RUSTFLAGS:+$RUSTFLAGS }$flags"
# The commit's time; a tree with no history (a `git archive`) is given it.
: "${SOURCE_DATE_EPOCH:=$(git log -1 --format=%ct 2>/dev/null || echo 0)}"
export SOURCE_DATE_EPOCH

pkgs=(-p theseusd -p theseus -p theseus-tui -p theseus-sim -p theseus-index)
for arg in "$@"; do
  case "$arg" in -p | -p?* | --package | --package=* | --workspace | --all) pkgs=() ;; esac
done
exec cargo build --locked --profile "$profile" "${pkgs[@]}" "$@"
