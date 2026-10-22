# CLOUD_REPORT: tail5-threads (theseus-ddfe7, theseus-qiyxq)

Branch `cloud/20261021-tail5-threads`, base `f1fc9b03`. Work commit `895d1f9`. Store format unchanged (49): nothing stored was added (the move mark is in memory).
No file went over its line ceiling.

## theseus-ddfe7 (the tail3-threads review)

- **F1, a mid-turn move.** `State::take_grew` (`thread_flow.rs`) now also marks the turn in a bounded ring (`ungrown`, capped at `TURNS` = 512, beside the `grew` ring). `thread_grow` (`rpc/thread_grow.rs`) returns at once for a marked turn (`State::is_ungrown`).
  - Rule picked: **skip**, not "grow the destination". The destination grows from the message's own resubmitted turn (theseus-iw50k), so growing it here too would double-count and would need the destination's session.
  - `thread_move.rs` is untouched: `thread_ungrow` already calls `take_grew`, so the mark is set there.
- **F2.** `added_by` takes the turn's `touched` set and subtracts it, so an entry both named and touched is the turn's and stays on a move. Doc updated.
- **F3.** `route.grew` is pushed only when `added` is not empty.
- **F4.** The TUI test `a_placed_threads_other_turns_task_is_not_shown_but_a_workers_is` (`tests_thread_ask.rs`). It checks that a `TaskChanged` with `turn_id: Some(other)` is not counted in the chat, and that `turn_id: None` still is.

## theseus-qiyxq (the tail2-route review)

- **F2.** `a_compile_failure_in_a_private_place_still_fails` now asserts one `privacy.warned` row with `held: false`.
- **F3.** `held` (`turn/route_step.rs`) now calls `record_route` (no decision, the verdict, its reason, default timing) and `note_routed`.
  - Not called: `engage_verdict`, because no persona answers the fixed held line.
  - The wait time and `late` are not timed here, so the row shows 0. That is a small undercount of wait stats for held turns.
  - No double rows: the held arm returns the failure, and the normal path after a successful compile is not reached.
- **Docs.** Two AGENTS.md sentences in core: the privacy warnings one (cljlz), and the `ContextDetour` `recall`/`steered` one (nfrlj), which was absent on main.

## Proof

Targeted run: 16 passed (`thread_grow`, `hold_compile` and the new tests).
Whole crates: `cargo nextest run -p theseus-core -p theseus-tui`: **4416 passed, 25 skipped, 0 failed**, run with `TZ=America/Phoenix`.
`cargo clippy --workspace --all-targets -- -D warnings`: clean. `cargo fmt --check`: clean.

Planted reverts (each restored and `touch`ed, `git status` clean afterwards):

| Plant | Test that failed |
|---|---|
| remove the `is_ungrown` check | `rpc::tests_thread_grow::a_turn_moved_before_its_growth_grows_nothing_in_the_origin` |
| drop the `touched` subtraction in `added_by` | `thread_grow::tests::a_named_and_touched_entry_stays_on_a_move` |
| drop the empty-`added` guard | `rpc::tests_thread_grow::an_empty_growth_pushes_no_grew` |
| `personal_warn` marks `held` in a private place | `tests_hold_compile::a_compile_failure_in_a_private_place_still_fails` |
| remove the new `record_route` in `held` | `tests_hold_compile::a_held_compile_failure_records_its_route` |
| drop `ours_in` from the `TaskChanged` arm | `chat::tests_thread_ask::a_placed_threads_other_turns_task_is_not_shown_but_a_workers_is` |

The first five were planted together in one run (5 failed, 11 passed); the TUI one separately (1 failed, 10 passed).

## Not done / caveats

- **The full `scripts/gate.sh` was not run here.** I ran the crate suites, clippy and fmt myself (above); the maintainer's gate covers the rest.
- **Runs under load:** the brief asks for these only where the task does, and this task does not. I ran none.
- **Commit trailer and signing:** the commit carries the task's `Co-Authored-By: Claude (cloud session, Sonnet 5.5)` line, not the base's `Tabitha/Claude` one. It is unsigned: there is no key on this VM.
- **Keel guard** (`python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet`): empty output, exit 0.
- **Live check:** none is needed for the maintainer's keys. To see F1 on a scratch daemon: submit to a thread a message naming a new ticket, `thread move` it while the turn still runs, and check the origin's working set lacks the ticket.

## For the integrator

- `docs/status.md`: "A message moved while its turn still ran no longer grows the thread it was moved from; an entry the turn touched stays on a move; a held compile failure now counts in route stats (theseus-ddfe7, theseus-qiyxq)."
- Follow-up only (a wire change, not made): carry `turn_id` on `confirm.list` and the budget question.
