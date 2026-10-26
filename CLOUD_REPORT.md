# CLOUD REPORT: theseus-otkwu (a gate that never started is no verdict)

**Store format: NOT bumped (stays 51).** "Two in a row" is read from existing `self.branch.built` rows
(`gate`, `numbers.exit`, `numbers.gate_secs`); no stored record gained a field.

Branch `cloud/20261026-p3n-halt-on-infra`, base `92e3dbed`. Commits: start `a6ba3d71`, work `9258338a`.

## What I found
- At the base the gate's outcome is recorded in `pr.rs` `fn built` (`gate_secs`/`exit`, now ~:577). `a.gate` is an
  `Option`, so "no gate" was already representable. Prompt line numbers: pr.rs:570 -> 570 before my edit (577 after);
  pr.rs ~:540 verdict text -> `fn said` ~:534-544; joining.rs:208 and install.rs:251 matched exactly.
  Measured sizes at base: pr.rs 712, backlog.rs 583, read.rs 660, self_build.rs 433. Now: 719 / 604 / 660 / 433.
- **backlog/read.rs does not read `self.branch.built` at all** (its `KINDS` has no such kind), so there was no
  "observation from a branch row" to suppress. The real re-filing engine is in `backlog.rs` `backlog_file`: a build
  that ends with no green branch closes (fails) its item, and the cause is observed again next night; since only an
  *open* item of a cause absorbs new evidence, a **fresh item is filed**. The guard therefore went in
  `backlog_file`, and `read.rs` is untouched.
- **Revert path:** `rsi/revert/*` reads joined merges and post-install signals, never `self.branch.built`; an
  infrastructure failure at build time cannot trigger a revert. (`revert/doing.rs` records `gate_secs` as a string
  for a revert's own gate.) No finding to fix.
- Other "gate did not start" shape found: `attempt()` returns `verdict "failed"` when `tree::gate` errs
  ("The gate did not run"); its row has `gate_secs: null, exit: null`. Counted as infrastructure.

## What I changed (9258338a)
- New `crates/theseus-core/src/rsi/infra.rs`: pure classifier `judge` -> `Outcome {Green, Red, Infrastructure}` via
  `of_attempt` (finished attempt) and `of_row` (ledger row); `streak`, `unjudged_items`; `watch` (halt) and `tell`.
- `pr.rs`: one call to `infra::watch` before `built(..)` (+7 lines). `mod.rs`: `pub mod infra;`.
  `tree.rs`: `Notify::send` (4 lines; the sink field was private to `tree`).
- `backlog.rs`: `backlog_night` reads the history once and passes the unjudged set to `backlog_file`, which skips
  (counted in `kept_out`) a cause that has a *closed* item whose newest attempt was infrastructure.
- `"empty"` still short-circuits `built` first and is untouched; infrastructure is distinct: the attempt is recorded.

**JOIN SEAM:** the telling is one call in one function: `crates/theseus-core/src/rsi/infra.rs:189` `tell(notify, run)`
(text built there), reached from `watch` (`:140`). It sends through `Place.notify` (#88's `Notify` sink, `Board::notify`
in pr.rs); `None` means no notice. No new config key.

**Halt mechanism reused:** `Core::self_halt` (`rsi/switch.rs`, the `halt self` kill switch). It is idempotent, so a
third failure while halted changes nothing and tells nothing (the telling is gated on `changed`). The notice names
no person or host; it says the gate did not start, the two branches and exit codes, that a person is needed, and
`theseus self resume`.

## Boundary decisions
- `green` is green even at exit 0 / 0 s (the gate said so). `keel` is judged (guard ran), however fast.
- `red` + exit 127 -> infrastructure whatever the seconds (a command the gate needs is missing).
- `red` + 0 s (or no seconds) -> infrastructure. `red` + no exit but real seconds (e.g. the 2 h cut-off) -> red: it ran.
- `halted`/`empty`/unknown -> no outcome: neither counts nor resets.
- **A real red (or green, or keel) resets the run.** A gate that ran proves the plumbing works, so the earlier
  failure was not a standing fault; "two in a row" stays a statement about consecutive attempts. Owner may prefer
  "two within N hours"; that would need a time window, not a store change.

## Proof
`cargo nextest list -p theseus-core -E 'test(/rsi::infra::/) | test(/rsi::tests_pr::infra::/) | test(/tests_self_backlog::infra::/)'`
selects 13: `rsi::infra::tests::*` (6), `rsi::tests_pr::infra::*` (5: one_infrastructure_failure_does_not_halt,
two_in_a_row_halt_and_tell_the_owner_once, a_real_red_between_two_resets_the_run, an_attempt_halted_between_phases_neither_counts_nor_resets,
a_judged_attempt_never_halts), `rpc::tests_self_backlog::infra::*` (2). Run: 13 passed.
Planted reverts (each restored + `touch`ed, `git status` clean after):
- infra arm -> red: 8 failed (halt, resets, streak, classifier, unjudged, and `an_item_whose_gate_never_started_is_not_filed_again`).
- `HALT_AFTER = 1`: 3 failed (`one_infrastructure_failure_does_not_halt`, `two_in_a_row...`, `a_real_red_between_two_resets_the_run`).
- backlog guard removed: 1 failed (`an_item_whose_gate_never_started_is_not_filed_again`).
Red-first caveat: I wrote tests and code together and did not run the tests against the unmodified base; the planted
reverts are the red evidence (the base lacks `rsi::infra` entirely, so these cannot compile there).
The regression guard (`an_item_whose_gate_ran_red_is_filed_again_as_it_always_was`) passes under every revert.
I did **not** extend `crates/theseusd/tests/self_build.rs`; the halt/notice tests are core-level (`tests_pr/infra.rs`,
real kill switch, counting sink). That test file is a gap if the maintainer wants a daemon-level one.

## Live check for the maintainer
On a scratch daemon with `[self] mode="act"` and a repo whose `scripts/gate.sh` is `exit 127`: run two
`self.propose_change` attempts; expect `theseus self` to show halted, one quiet notice in the owner DM, and a
`self.halted` row; a third call is refused by the halt. Then `theseus self resume`.

## Open / uncertain
- The no-re-file guard is per-item and **durable**: the cause stays unfiled until the owner retries the item
  (`theseus self retry`), since the item is closed and can never be judged on its own. That is a deliberate
  trade-off the owner should hear; it is counted in the night's `kept_out`.
- `watch` runs before the attempt's own row is written; if the index is still being built it does nothing.

## Gate
`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` FAILED in the door/keel phase, on findings **not in my
diff** (persona_cmd.rs tests, tests_m3/ask/due.rs, tests_gateway/cards.rs, first_request.rs, keel-guard.py): the
guard's range base was a different commit (`2d1d8d6f07`; origin/main has moved to `78fcfb6d`), so main's changes
read as erosions; the old Keel acks also fail signature verification here (`%G? is E`). My own range raises none.
Later phases run by hand: `cargo fmt --all -- --check` OK; `scripts/shape.sh` OK; `cargo clippy --workspace
--all-targets -D warnings` OK; theseus-core `rsi::`, `tests_self*`, `tests_registry`: 410 passed. Not run: the full suite,
lifecycle benches, deny, cockpit, `cargo test -p theseusd`.
Keel guard output for my commit: the 8 findings above, none in files I touched.
Doc to update at review: the spec's Part III item for this row; `crates/theseus-core/AGENTS.md` self-improvement
paragraph (the halt after two infrastructure failures; the backlog's unjudged guard).
