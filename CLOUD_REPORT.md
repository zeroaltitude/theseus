# CLOUD_REPORT: p3e-fanout-kernel (theseus-z9mze.1)

Branch `cloud/20261024-p3e-fanout-kernel`, base `ac298666`. Code commit: `ca78f10d`.

## 1. For the maintainer
- Store format stays 51: not bumped. The new `Execution.helper` is an optional field with serde default and
  skip-if-none, so old records read unchanged and a plain task writes no key. `SCHEMA` stays 2 (untouched).
  If you hold that a stored-record field needs the bump by the version rule, bump it at the join and add the
  old-layout sample; I judged it not required (the old bytes decode unchanged, and a new binary's non-helper
  records are byte-identical to the old ones).
- Ceilings: none raised. `kernel.rs` +1 line (a literal), `turn.rs` untouched. `rpc/task_start.rs`'s `task_start_as`
  is at clippy's 100-line limit, so there the literal uses `..Default::default()` (and drops the explicit
  `wake_parent: false`, the default).
- No `OWNER CALL`, no STOP.
- Environment: the disk allowance filled during the cold `--workspace --all-targets` build; I deleted
  `target/debug/incremental` (inside the repo) and ran the rest with `CARGO_INCREMENTAL=0`. The full
  `scripts/gate.sh` was not run (no cockpit build, no `cargo deny`); I ran its checks piecewise (section 5).

## 2. The API
`Kernel::take_reports_of(&self, guard: &TurnGuard, ids: &[ExecutionId], extra: impl FnOnce(&[ExecutionId]) -> Result<Vec<NewRecord>>) -> Result<TakenReports>`
in `crates/theseus-kernel/src/tasks.rs`, beside `take_reports`. Both call one private
`take_reports_where(guard, wanted, via, extra)`; `take_reports` passes `|_| true` and no `via`, so its frame and its
ledger row are unchanged (same keys, same order).
- Takes the named tasks that are on the parent's `reports`, oldest first (the order of `reports`, not of `ids`),
  in one frame: the parent's record, `extra`'s records, and one `task.reports_read` row
  `{execution_id, tasks, woke, turn, via: "wait"}` (`tasks` is the taken ids).
- A named task not on `reports` (running, or already taken) is not taken and not an error. If none is on
  `reports`, nothing is written and `extra` is not called.
- **Wake decision:** the taken tasks leave `report_wakes` in the same frame (the row's `woke` names them). Reason:
  the parent is mid-turn reading them, and a wake kept for a taken report would queue it for a turn with nothing to
  read. A wake of a report not named stays, and with it the parent's next turn. Tested.
- Helper mark: **one field**, `helper: Option<String>` (the fan-out call's correlation id) on `Opening` and on
  `Execution`, since a helper always belongs to a fan-out. `carve_task` copies it. Accessor
  `Execution::helper_of() -> Option<&str>`, documented as: a helper reads and reports, and at its carve it ends with
  stop reason `budget` instead of asking the budget question (row C's change in `turn.rs`, not made here).

## 3. For rows B and C
- Call `take_reports_of(guard, &ids, |taken| ...)` with the helper ids; `TakenReports.ids` is what was taken, so the
  running ones are the named ids missing from it. `woke` says which of the taken had asked for a turn.
- Build each report as `read_reports` does: `Node::relayed(session, Some(turn), Origin::Harness, &format!("task:{}", r.short), &r.node_text())`
  from `task::load_report`. It is a `UserMessage` body, not a tool result, so the clearing pass never stubs it.
  (`read_reports`' builder is inline in `turn.rs`, so the test repeats it; I did not move it.)
- `read_reports`' edge (`DerivedFrom`/`VIA_REPORT`) and the external-text hold (`external::taken`) are in that
  closure too; row B must repeat them or the held-session rule is skipped for a waited report.
- Accessor: `Execution::helper_of()`. `Opening { helper: Some(call_id), .. }` marks a helper at the open.

## 4. Tests (all pass; each red with its planted revert)
In `crates/theseus-kernel/src/tests_tasks.rs`:
1. `take_reports_of_takes_the_named_reports_in_one_frame`: a, b, c ended; takes [c, a] in one frame (frame count +1,
   row `via: wait`), b stays, `take_reports` then returns b alone and its row has no `via`. **P1** (take every report):
   all 3 `take_reports_of` tests FAIL.
2. `take_reports_of_leaves_a_running_task_and_writes_nothing_when_none_ended`. **P2** (drop the ended-only check):
   this test FAILS.
3. `take_reports_of_drops_the_wakes_of_what_it_takes_and_keeps_the_rest` (the wake decision).
4. `a_helper_mark_is_stored_and_a_plain_task_writes_none`: stored, read back after a crash/reopen, no `helper` key
   on a plain task, an old record without the key reads as not a helper. **P3** (carve does not copy it): FAILS.

In new `crates/theseus-core/src/tests_task_reports_of.rs` (one `mod` line in `lib.rs`):
5. `a_report_taken_by_name_is_a_relayed_harness_node_and_is_written_once`: two real tasks end; one report is taken
   by name; the session holds it as a harness-origin `task:<short>` node whose kind is not `tool_result`; the next
   parent turn delivers the other; each report is in the session exactly once. **P4** (the taken report stays on
   `reports`): FAILS.
Each plant was restored, `touch`ed, and checked identical with `cmp`. The tests do not compile at the base
(the API is absent), so they are red there.

## 5. Gate (piecewise)
- `cargo fmt --check`: ok.
- `cargo clippy --workspace --all-targets -- -D warnings`: clean (after the `task_start_as` line fix above).
- `TZ=America/Phoenix cargo nextest run -p theseus-kernel -p theseus-core`: 4753 run, 4752 pass, 1 fail:
  `rpc::tests_peek::a_slow_client_hears_events_lost_and_recovers_by_a_peek`, a timing test under load; it passes
  alone (rerun). Kernel alone: 259/259.
- `cargo nextest run -p theseus-sim -p theseus-exam`: 174 pass, 1 skipped.
- `scripts/shape.sh`: ok.
- Not run: the full `scripts/gate.sh`, the cockpit build, `cargo deny`, the benches. Nothing touched protocol types, so
  no TypeScript regenerated.
- Live check: none needed (no behaviour a user sees changes).

## 6. Keel
`python3 scripts/keel-guard.py --base ac298666 --head HEAD --quiet` printed nothing and exited 0.

## 7. DM golden
`git diff --exit-code ac298666 -- crates/theseus-discord/src/golden/dm_flow.txt`: no output, exit 0.

## 8. Join notes
Touched (additions of `helper: None,` or the pass-through unless noted):
- crates/theseus-kernel/src/tasks.rs (Opening field; destructure and pass-through at three sites; carve literal;
  `take_reports`/`take_reports_of`/`take_reports_where`)
- crates/theseus-kernel/src/types.rs (`Execution.helper`, `helper_of`)
- crates/theseus-kernel/src/kernel.rs (literal, 1 line)
- crates/theseus-kernel/src/retry.rs (literal)
- crates/theseus-kernel/src/routines.rs (Opening literal)
- crates/theseus-kernel/src/tests_deferred.rs (Opening literal)
- crates/theseus-kernel/src/tests_tasks.rs (new tests, appended)
- crates/theseus-core/src/push.rs (Execution literal)
- crates/theseus-core/src/task.rs, draft.rs, draft/first_pass.rs, turn/draft_step.rs (Opening literals)
- crates/theseus-core/src/rpc/task_start.rs (Opening literal: `..Default::default()`)
- crates/theseus-core/src/lib.rs (`mod tests_task_reports_of;`), crates/theseus-core/src/tests_task_reports_of.rs (new)
- crates/theseus-sim/src/synth.rs, crates/theseus-exam/src/fixture.rs (Execution literals)
None of `progress_step.rs`, `tests_turn_progress.rs`, `fact/progress.rs`, `compiler/clearing.rs`,
`compiler/tests_ring.rs`, `theseus-tools/src/fs.rs`, `turn.rs` or `theseus-core/AGENTS.md` is touched.

## 9. Follow-ups
- `crates/theseus-kernel/AGENTS.md`'s `tasks.rs` line should name `take_reports_of` and the `helper` mark; I left the
  docs to you.
- Row B must also decide what a wait does with a report whose task holds external text (see section 3).

## 10. Data floor
Nothing is sent to a model or routed anywhere by this row.
