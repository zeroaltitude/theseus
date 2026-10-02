#!/usr/bin/env bash
# The release build, the one way (theseus-goa8; review 2's SC3): every install,
# and scripts/repro.sh, builds with this, so that a binary is a function of the
# commit and the pinned toolchain (rust-toolchain.toml), and not of the directory
# it was built in or the minute it was built.
#
#   scripts/build.sh [--profile release|release-thin] [--shipped] [cargo build arguments…]
#
#   release       fat LTO and one codegen unit: a tagged release (the default).
#   release-thin  thin LTO and 16 codegen units: the install profile, for a chain's
#                 install, built far faster (Cargo.toml says why it is not `install`,
#                 and scripts/AGENTS.md has the measurements).
#
# What it builds: the whole workspace, as the gate and the tests do, so that what is shipped
# is what was tested. Cargo unifies a dependency's features across the packages it builds,
# and the whole workspace gives 29 of the shipped binaries' 334 shared crates more features
# than the four binaries alone do (serde_json's `alloc`, `time`'s `serde-well-known`, and
# twilight-gateway's `rustls-native-roots`, which the voice crate asks for): a build of just
# the shipped binaries is a different build from the tested one.
#
# `--shipped` builds only the four binaries an install ships (theseusd, theseus, theseus-tui,
# theseus-sim): 335 of the workspace's 607 crates, since the rest (theseus-index's candle and
# tantivy, the voice stack, the AWS clients) are linked into no shipped binary yet. A cold
# build is shorter, and the binaries have the narrower features above: use it to look, not
# to install. Any `-p`, `--package`, or `--workspace` of your own is passed through.
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
shipped=""
while [ $# -gt 0 ]; do
  case "$1" in
    --profile) profile="${2:?--profile needs release or release-thin}"; shift 2 ;;
    --shipped) shipped=1; shift ;;
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

pkgs=()
[ -z "$shipped" ] || pkgs=(-p theseusd -p theseus -p theseus-tui -p theseus-sim)
exec cargo build --locked --profile "$profile" "${pkgs[@]}" "$@"
