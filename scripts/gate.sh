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
# The lifecycle budgets of §9 (FAST, theseus-qa0): cold start, clean shutdown
# with a job running, SIGKILL then restart, 10 runs each on an empty store,
# p95 against the budget plus the measured noise margin. Debug binaries: they
# are never faster than release, so a pass here holds for release.
cargo build -q -p theseusd -p theseus-sim
target/debug/theseus-sim bench lifecycle --runs 10 --check
cargo deny --log-level error check
if [ -d web/node_modules ]; then (cd web && npm run -s lint >/dev/null && npm run -s build >/dev/null); fi
git diff --quiet -- crates/theseusd/web/dist || { echo "web dist changed by the build: commit it"; exit 1; }
echo "gate: ok"
