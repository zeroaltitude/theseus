# CLOUD_REPORT: tail7-effort (theseus-t7b96, items 1, 2, 4)

Branch `cloud/20261021-tail7-effort`, base `ae420a49`. Code commit `70253aea`. Store format stays 49: a new value in an existing field, no layout change.

## What I found
- `decide` gave `HeldHigh` to every held xhigh/max answer, sure or not, so the row could not tell them apart.
- The `effort_reason` doc on the wire lacked `held_high`, `applied_unsure`, `sticky_unsure` and `floored`.
- `cockpit/src/lib/route.ts` used `.replace('_', ' ')`, which only replaces the first underscore.
- `cockpit/src/lib/routefooter.ts` does **not** switch on effort reason words: it reads only `route.source`, `mode`, `reason` (the route reason) and `from`. Nothing to change.
- The restart test only called `stop_sending()` on run 1. A run's driver and binding tasks were never held, so they kept running.
- Gateway events go only to the client that identified last, so run 1 could not take events after run 2 identified. What it could still do was keep its tasks running against the shared fake. `Rig::stop` ends those.

## What I changed (70253aea)
- `routing/effort.rs`: new `EffortReason::HeldHighUnsure`, word `held_high_unsure`. It is given when xhigh/max is held at high and the answer was not `sure`. `held_high` keeps the sure case. It counts as applied. The unit test that asserted `held_high` for an unsure xhigh (`an_unsure_xhigh_between_the_bars_runs_high`) now asserts `held_high_unsure`.
- `stick`: unchanged. It maps only `AppliedUnsure` to `StickyUnsure`. A held-high answer held again by the sticky rule says plain `sticky` (as `HeldHigh` did), because the held-high word is already lost at that point and `sticky_unsure` means "an `applied_unsure` answer". Say if the owner wants `sticky_unsure` here too.
- `theseus-protocol/src/route.rs` (doc only): lists all 14 words in `as_str` order (`applied, clamped, held_high, held_high_unsure, unsure, applied_unsure, unclear, fixed, no_effort, carried, recorded, sticky, sticky_unsure, floored`), each with a few words of meaning. `cockpit/src/protocol.gen/TurnRoute.ts` regenerated.
- A test in core (`the_wires_doc_lists_every_effort_reason_word`) reads the protocol source with `include_str!` and checks that every word is in the doc, in order. It lists the variants by hand, so a new variant must be added to that list too.
- `cockpit/src/lib/route.ts`: `replaceAll('_', ' ')`.
- `Rig::stop` in `tests_gateway.rs` (stops outbox, aborts driver and binding). The restart test calls it for run 1 and run 2. `assert!(home != ANA_DM)` is replaced by an assert that the one channel made with the config's name `theseus-tasks` has the id `home`. Every existing assert is kept.
- Core `AGENTS.md` (effort section, two lines) and the template's `held_high` comment line.

## Proof
- `cargo nextest run -p theseus-core effort`: 50/50. `-p theseus-protocol`: 200/200. `-p theseus-discord task_home`: 11/11.
- Cockpit `npm test`: 523 pass, 0 fail. `npx tsc -b`: clean.
- Planted reverts:
  - Map both to `HeldHigh`: `routing::effort::tests::an_unsure_max_held_high_says_so` and `tests_route_effort::an_unsure_max_held_at_high_says_held_high_unsure` FAIL.
  - Remove `sticky_unsure` from the doc: `the_wires_doc_lists_every_effort_reason_word` FAILS ("`sticky_unsure` is missing from the doc").
  - Back to `.replace('_', ' ')`: route.test.ts fails (expected `Jev: max, held high unsure`, got `held high_unsure`).
  - Restart test with the stop skipped: **not run, so no plant to report.** The rig does not count run 1's events, so I could not show a test failing. The change rests on review, and `Rig::stop` aborts only the tasks the rig started. A task the binding spawned itself ends with the runtime.
- Gate (`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`): door, keel, fmt, shape, features, clippy, clippy-probe, cockpit and test build pass. The suite ran 7,962 tests: 7,928 passed, 34 failed. All 34 are the known L1 sandbox group (theseus-sandbox contract tests, its `bench spawn_100`, theseusd's `sandbox` tests). No other failure.
- Not run: the gate phases after the suite. `cargo deny --offline check` failed because the advisory database was not available offline (the `cargo deny fetch` from setup did not finish). The lifecycle, jobs, turn and size phases are skipped by `THESEUS_GATE_NO_BENCH`. The generated-TS check was not run on its own; the diff is committed.
- Disk: this VM ran out of disk twice (linker bus errors); I cleared `target/debug/incremental`, test binaries and examples and used `CARGO_INCREMENTAL=0`. Not a code issue.
- Keel guard (`python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet`): empty output, exit 0.
- The VM has no usable signing key: `git commit -S` ran but the commit shows no valid signature (`%G?` = N). The maintainer's merge commit is signed.

## Live check for the maintainer
Run a scratch daemon with `[routing]` live and `high_effort_confidence` above the answer's confidence, ask a hard question where Jev says max with confidence between `effort_confidence` and the confirm bar, then `theseus ledger` the `route.decided` row. It should show `effort_reason: held_high_unsure` and `effort_ran: high`.

## docs/status.md sentence
"An xhigh or max answer held at high now says `held_high_unsure` when it was under its confirm bar; `held_high` means the sure case; the wire's `effort_reason` doc lists all 14 words, the cockpit shows multi-underscore words in full, and the Discord restart test stops run 1 for real (theseus-t7b96)."

## Follow-ups
- The Rust doc test depends on the hand-written variant list; a `strum`-style iterator would remove that but needs a dependency.
- `clippy --workspace --all-targets -D warnings` and `fmt --check` passed inside the gate.
