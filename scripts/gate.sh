#!/usr/bin/env bash
# The commit gate: every check must pass, and a failure stops the chain.
# Use as `scripts/gate.sh && git commit …` (a `;` anywhere in a hand-written
# chain once let an unformatted commit through).
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -q -- -D warnings
cargo nextest run --workspace --no-fail-fast
# The lifecycle budgets of §9 (FAST, theseus-qa0): cold start, clean shutdown
# with a job running, SIGKILL then restart, a binary swap with the job's
# wrapper adopted, 10 runs each on an empty store, p95 against the budget plus
# the measured noise margin; and restore, measured, whose restored store must
# serve. Debug binaries: they are never faster than release, so a pass here
# holds for release.
cargo build -q -p theseusd -p theseus-sim
# Other processes' dirty pages are flushed first, so a start's fsync never
# pays for their writeback (1.3 GB of it once put a clean shutdown's p95 at
# 148 ms). And one stalled fsync does not fail the gate: a miss runs the
# bench once more, and only a second miss fails it, as a real regression
# does (theseus-hee).
sync
target/debug/theseus-sim bench lifecycle --runs 10 --check || {
  echo "lifecycle: a budget was missed; running the bench once more"
  sync
  target/debug/theseus-sim bench lifecycle --runs 10 --check
}
cargo deny --log-level error check
if [ -d web/node_modules ]; then (cd web && npm run -s lint >/dev/null && npm run -s build >/dev/null); fi
git diff --quiet -- crates/theseusd/web/dist || { echo "web dist changed by the build: commit it"; exit 1; }
echo "gate: ok"
