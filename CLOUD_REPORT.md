# CLOUD_REPORT: p3h-admission-headroom (theseus-o1h3f.4)

Branch `cloud/20261025-p3h-admission-headroom`, from `0fda4d57`. Task commit `e9f50b0b`.

## 1. For the maintainer
- Store format 51 and kernel SCHEMA 2: both untouched. No config key added.
- **The ceiling was not raised.** `admission_ceiling` (8), `admission_stuck_secs` (600), `AGE_STEP`, `LEND_AFTER` unchanged. The rule only lowers what task turns may hold.
- No `OWNER CALL:` and no STOP.
- The tests live in `crates/theseus-core/src/turn/tests_admission.rs` (it already existed; the brief said `admission/tests.rs`). `turn.rs` not touched.

## 2. The slot rule
- Count: `State.tasks` (of `in_use`; lent slots are not counted). Limit: `Admission::task_limit()` = `ceiling - 1`, at least 1. **Ceiling 1 gives a limit of 1**, so task turns still run (no spare slot exists to reserve).
- Incremented in `State::admit_now(class)` (try_take, join fast path) and in `hand_on` on a grant. Decremented in `give_back(class)` (Slot drop, `lend`, Grant drop) and in `hand_on`'s send-failed path. `Grant` now carries its class; `lend` takes the class from the `bound` map.
- `pop_next(now, step, limit)` skips a queue head that `may_admit` refuses (a non-retake task at the limit) and returns the other queues' best head, plus its class. The declined waiter is not popped.
- `hand_on`: when `pop_next` returns None (empty or only declined tasks) it sets `moved = now` and returns, so a free slot nobody may take counts as the queue moving.
- Fast paths (`try_take`, `join`) now test `!admissible_waiter(limit)` instead of `waiting() == 0`. Without this a person arriving behind a declined task queued with a slot free and was never granted (found by my first test run). A newcomer that `may_admit` therefore passes declined tasks.
- Retake (`front`): never refused by the limit (it already held a slot; refusing would hang the wait). Reason is in the module doc. Consequence: while a task lends and a fresh task takes its slot, the retake can bring the task count one past the limit; `in_use` still never passes the ceiling.

## 3. Aging
`Class::aged` floors `Class::Task` at `Wake` rank. Wake and Person aging unchanged. `AGE_STEP`'s doc and the module doc corrected.

## 4. Tests (all in tests_admission.rs) and plants
New: `a_task_turn_never_takes_the_last_slot` (P1), `a_free_slot_nobody_may_take_is_still_the_queue_moving` (P2), `a_retake_is_not_blocked_by_the_task_limit` (P3), `a_task_turn_ages_to_the_wakes_rank_and_no_higher` (P4), `the_rule_lowers_a_limit_and_never_raises_the_ceiling` (5).
- P1 (limit removed in `may_admit`): 3 red (never_takes_last_slot, queue_moving, retake).
- P2 (`hand_on` leaves `moved` alone when waiters remain): queue_moving red.
- P3 (limit applied to `front`): retake red (timeout assertion, no hang).
- P4 (floor off): the aging test and the rewritten old aging test red.
- Each file restored, `touch`ed, `git status` checked. I did not run the new tests against the unmodified base (the fix and the tests came together); the plants prove them.
- `cargo test -p theseus-core --lib admission`: 20 passed.
- Load (nice 19 beside four nice-0 busy loops, pids killed by number): 3 runs, 20 of 20 passed each, no negative assertion failed. The tests use tokio's paused clock.

## 5. Keel findings expected
Two existing expectations rewritten in place in `tests_admission.rs`, same assertion counts, names kept:
- `turns_of_one_class_are_admitted_in_their_arrival_order` and `a_stuck_queue_fails_its_waiter_with_words_that_say_so`: the slot holders taken with `try_take(Class::Task)` at ceilings 3 and 2 are now `Class::Wake` (old rule let tasks fill the ceiling).
- `a_task_turn_waiting_past_the_step_is_admitted_ahead_of_new_wakes`: the "two steps" block asserted an aged task passes a new typed turn (`order[2] == "older task"`); it now asserts `"new typed"` (the new aging rule), and the drop order is swapped to match.

## 6. Gate
- `cargo fmt`: clean. `cargo clippy -p theseus-core --all-targets -- -D warnings`: clean. `scripts/shape.sh`: ok.
- `TZ=America/Phoenix cargo nextest run -p theseus-core`: 4541 run, 4541 passed, 25 skipped.
- **`scripts/gate.sh` fails in its keel phase and in the door** (main's gate judges the tree and fails the same way). Cause: not my range. It reports erosions in `persona_cmd.rs`, `scripts/keel-guard.py`, `scripts/test_keel_guard.py` and refuses earlier Keel acks as unverifiable ("signature does not verify (%G? is E)": this VM has no signing keys). My files are untouched by those. So the gate's later phases did not run; I ran the ones above by hand. I did not run theseusd's task and admission tests or the other machine_checks.

## 7. Keel (my range)
`python3 scripts/keel-guard.py --base 0fda4d57 --head HEAD --quiet`: no output, clean.

## 8. DM golden
`git diff --exit-code 0fda4d57 -- crates/theseus-discord/src/golden/dm_flow.txt`: exit 0, no diff.

## 9. Join notes
- `crates/theseus-core/src/turn/admission.rs`: module doc (top), `AGE_STEP` doc, `Class::aged`, `Waiter` and `State` fields, `pop_next`, `may_admit`, `admissible_waiter`, `admit_now`, `Grant`, `Slot` drop, `task_limit`, `try_take`, `join`, `lend`, `give_back`, `hand_on`, the test helper `waits_with_a_slot_free`.
- `crates/theseus-core/src/turn/tests_admission.rs`: the edits in section 5 and new tests appended at the end.
- `crates/theseus-core/AGENTS.md`: one new paragraph before "The steer".
- No file of the other four rows was touched.

## 10. Follow-ups
- The stuck window still runs from the last event: task waiters held by the limit while task holders run past `admission_stuck_secs` (600) will fail `admission_stuck`, as they would at a full ceiling today. Helper turns shorter than ten minutes are fine; if longer fan-outs are wanted, count a reserved free slot as moving in `take()`'s timer path.
- Fan-out arithmetic: at ceiling 8, at most 7 task turns run at once, and a lending task's slot can be taken by another task, so the count may briefly be 8 tasks' worth of turns only through retakes.

## 11. Data floor
Nothing is sent to a model or routed anywhere; only which queued turn is admitted next changes.
