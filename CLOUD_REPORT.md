# CLOUD_REPORT: tail6-threads (theseus-hat0c, theseus-m1a51, theseus-fkvb0)

Branch `cloud/20261021-tail6-threads`, from `integ/tail5` `33e589e2`. Store format untouched (49). No new dependency. `crates/theseus-protocol/src/lib.rs` not edited (2,563 lines). Commits are unsigned (no key on this VM).

## theseus-hat0c: the growth race
- Found: `thread_grow` checked `is_ungrown` once. A move landing after the check marked the turn, yet the growth committed, and `route.grew` then pushed an entry after the mark.
- Changed: commit `06da7acf`. `State::grew` (thread_flow.rs) now returns false when the turn is marked, and checks and notes under one lock. It is called inside the store's update closure, so a move either lands first (the closure commits nothing) or lands after (it finds the entry and `thread_ungrow` takes the growth back). The early check stays.
- Race seam: `Core::thread_grow_between(g, turn, between)` runs `between` after the early check and the calls' read, before the update. `thread_grow` passes a no-op.
- Test `a_move_during_growth_commits_nothing_to_the_origin`. Plant: `if r.ungrown...` replaced by `if false`; failed at `tests_thread_grow.rs:298` (`set_of(..).refs.is_empty()`).
- Uncertain: the entry is noted before the frame is written. If the write then fails, a stale entry remains in the ring. A later move would call `without` on entries the set never gained, which is harmless.

## theseus-m1a51: the held row
- Found: `held` recorded `route.decided` with `Timed::default()` and no steering.
- Changed: commits `869056c1` and the follow-up that keeps `compile_first` under clippy's line bound. `held` takes `(Timed, Option<&Steering>)`. The row now carries `wait_ms` (the real wait), `late`, `answered_ms`, and the correction source and `follows` when the turn was steered. `est_tokens` stays 0: there is no compile. `Timed::of` builds the times once for both the failed-compile row and the routed row. The doc now says it runs on every compile failure and holds only where the verdict holds.
- Test `a_held_compile_failure_records_its_wait_and_steer` (Jev held, `max_wait_ms` 300: `wait_ms` at least 300, `late` true, `answered_ms` null). Plant: `Default::default(), None` back in the `record_route` call; failed at `tests_hold_compile.rs:197`.
- Not covered: the steering half has no test of its own. A steered turn whose compile fails needs an owner's correction and a compile that overflows in the same turn, which this rig cannot stage. The steering is passed as the routed path passes it.

## theseus-fkvb0: confirms name their turn
- Filled: `ConfirmRequest.turn_id` from the call's node in `confirm_request` (so `confirm.list`, the push and `session.history`'s `pending_confirms`), and for a budget question from `args.call.turn_id`, which `Turn::budget_call` now writes. `PendingConfirm.turn_id` (same sources, `pending_of` takes it) and `BudgetQuestionInfo.turn_id` (`budget.list`) are new serde-default, skip-if-none fields. It stays `None` for extension acks and pack promotions (no turn), for a call whose node has no turn, and for a budget question asked before this build (no `turn_id` in its `call`).
- Test `confirm_list_carries_the_asking_turn` (approval, budget question, `budget.list`). Plant: `turn_id: None` in `confirm_request`; failed at `tests_m3/answer.rs:111`.
- Protocol: `an_older_question_without_a_turn_parses` (old payloads parse, the field is omitted when none). `protocol.gen/PendingConfirm.ts` and `BudgetQuestionInfo.ts` regenerated.
- TUI: finding. The chat's `ConfirmRequested` filter (`ours_in`) already existed; it could not work after a reconnect because the daemon sent `turn_id: None`. The reconnect path is `session.watch`, which replays the push, so the daemon fix is what closes the gap. The chat reads `confirm.list` nowhere (the sidebar app does, and it has no placed-thread filter), and `session.history`'s `pending_confirms` is for the chat's own session, which `ours` already admits. So I made no TUI code change. Added `a_question_naming_no_turn_asks_in_this_chat`. Plant: `ours_in`'s turn test replaced by `true`; failed `a_placed_threads_other_turn_does_not_ask_in_this_chat`, `another_turn_of_the_thread_is_not_the_chats`, `a_placed_threads_other_turns_task_is_not_shown_but_a_workers_is`.
- AGENTS nit: "carry" to "carries" in core `AGENTS.md`.
- Discord ignores the new fields.

## Checks
- `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check`: clean.
- `cargo nextest run -p theseus-core -p theseus-protocol -p theseus-tui` (before the last refactor): 4681 passed, 26 skipped. After it: `test(route) | test(hold_compile)` 175 passed.
- Keel guard (`python3 scripts/keel-guard.py --base 33e589e2 --head HEAD --quiet`): empty, rc 0. The gate's own keel phase compares against another base and also names a tail5 test rename (`xhigh_and_max_need_confidence...`) that is not mine; see the gate section.

## Live check for the maintainer
On a scratch daemon with a thread whose placed turn asks for approval: `theseus confirm` (JSON) should show `turn_id` on the question, and `theseus budget list --json` on a budget question should show `question.turn_id`.

## docs/status.md sentence
tail6-threads: a move landing during a thread's growth no longer commits it to the origin (re-checked inside the update); a failed compile's `route.decided` row carries the turn's real wait and steering; `confirm.list`, the push and `budget.list` name the asking turn, so a chat that follows a placed thread keeps its other turns' questions out.

## Follow-ups
- The sidebar app has no placed-thread filter for questions; `confirm.list`'s result now has what it needs.
- A test for the steered held row.

## Gate
`TZ=America/Phoenix CARGO_INCREMENTAL=0 THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` at the head before this report: door, keel, keel tests, fmt, shape, features, clippy, clippy-probe, cockpit, test build and reader rule pass. The suite phase ran 7943 tests; the only failures are the known root-VM L1 cases (theseus-pv6i): theseus-sandbox `contract` (20) and `bench::spawn_100`, and theseusd `sandbox` (the 18 listed in the log, including `a_host_beyond_the_list_once_approved_is_outside_text`). No other test failed, and `the_cores_output_matches_its_golden` passed under TZ=America/Phoenix. The phases after the suite are the protocol-types check (`cargo nextest run -p theseus-protocol`: 199 passed, 1 new) and the benches, which the gate skipped (`THESEUS_GATE_NO_BENCH=1`). `cargo deny` was not run. An earlier gate run hit ENOSPC from the 19 GB of incremental build output; I deleted `target/debug/incremental` and set `CARGO_INCREMENTAL=0`.
