#!/usr/bin/env bash
# The commit gate: every check must pass, and a failure stops the chain.
# Use as `scripts/gate.sh && git commit …` (a `;` anywhere in a hand-written
# chain once let an unformatted commit through).
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -q -- -D warnings
cargo clippy -p theseus-core --features otel --all-targets -q -- -D warnings  # OTLP export is off by default; keep it compiling
cargo nextest run --workspace --no-fail-fast
cargo deny --log-level error check
if [ -d web/node_modules ]; then (cd web && npm run -s lint >/dev/null && npm run -s build >/dev/null); fi
git diff --quiet -- crates/theseusd/web/dist || { echo "web dist changed by the build: commit it"; exit 1; }
echo "gate: ok"
