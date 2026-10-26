# CLOUD REPORT: theseus-5tl9v (a person's turn is never failed by the admission stuck window)

Branch `cloud/20261026-p3n-admission-notify`, base `92e3dbed`. Commits: `d5c4f0c7` (start), `4246ac5e` (rewrite of one existing test), `b270deac` (the change and its five tests).

## What changed
- `turn/admission.rs`: in `take`'s stuck branch, a `Class::Person` waiter no longer returns `Err(Stuck)`. It stays in its queue and waits another window. Wake and Task take the old path unchanged (`rx.close()`, sweep, `Err(Stuck{..})`).
- **`rx.close()` and the sweep on the person's path:** neither runs. The receiver stays open and the waiter stays queued, so a later grant reaches it. Test 4 proves it. The local `since` is moved to `now` on the continue path so the next deadline is `now + stuck` rather than already past (otherwise `sleep_until` would spin).
- **Held-since:** `bound`'s value is now a small struct `Bound { lent, class, since }` (was a tuple); `since` is set in `Slot::bind`. The notice lists only slots not lent.
- **Sink:** `pub struct Notice(Arc<dyn Fn(String)+Send+Sync>)`, held as `OnceLock<Notice>` on `Admission`, set once with `Admission::set_notice`. None means no notice (no error, no panic). `notice_to_operator(&Arc<Core>)` builds the core's sink the way `rsi/pr.rs`'s `Board::notify` does (`stage_to_operator`, `store.append`, `outbox.posted`; `quiet: true`; warns on failure). It is wired in `rpc/mod.rs` beside the other `attach` calls (+1 line, +2 after fmt).
- **Latch:** `State.told: Option<Instant>` holds the `moved` value of the episode already told. A notice is staged only when `told != Some(moved)`. Any movement of the queue (grant, free slot taken, `hand_on`) changes `moved` and so ends the episode; a later one is told afresh. The sink is called after the state lock is dropped. Only a Person's keep-waiting stages a notice; Wake/Task failures are told by their error as before.
- Notice text: execution ids and durations only, e.g. "A typed message is waiting for a turn slot, and no turn has been admitted for 600 s (1 of 1 slots held, 1 waiting). It keeps waiting. Holding: exe_held for 601 s."
- **toolrun.rs: 0 lines added** (`wc -l` 2499 before and after). `Admission::new`'s signature is unchanged.
- **config.rs: not touched.** `admission_stuck_secs` keeps 600.
- Store format and kernel SCHEMA untouched; no new dependency (`Cargo.lock` unchanged).

## Existing test rewritten (own commit `4246ac5e`)
`a_stuck_queue_fails_its_waiter_with_words_that_say_so` pinned that a *person* fails. It now spawns a `Class::Wake` waiter and expects "wake turn"; every assertion is kept. The keel guard did not flag it. Keel findings expected: none.

## Tests (turn/tests_admission.rs; 5 new, 18 in the module)
`cargo nextest list -p theseus-core -E 'test(/turn::admission::tests/)'` (a bare `test(tests_admission)` selects 0 tests):
    theseus-core turn::admission::tests::a_free_slot_nobody_may_take_is_still_the_queue_moving
    theseus-core turn::admission::tests::a_lent_slot_goes_to_the_next_turn_and_is_taken_back_at_the_head
    theseus-core turn::admission::tests::a_person_waits_past_the_window_and_one_notice_names_the_holder
    theseus-core turn::admission::tests::a_person_who_kept_waiting_is_admitted_when_the_holder_ends
    theseus-core turn::admission::tests::a_person_with_no_notice_sink_simply_keeps_waiting
    theseus-core turn::admission::tests::a_retake_is_not_blocked_by_the_task_limit
    theseus-core turn::admission::tests::a_stuck_episode_is_told_once_however_long_it_lasts
    theseus-core turn::admission::tests::a_stuck_queue_fails_its_waiter_with_words_that_say_so
    theseus-core turn::admission::tests::a_task_turn_ages_to_the_wakes_rank_and_no_higher
    theseus-core turn::admission::tests::a_task_turn_never_takes_the_last_slot
    theseus-core turn::admission::tests::a_task_turn_waiting_past_the_step_is_admitted_ahead_of_new_wakes
    theseus-core turn::admission::tests::a_task_waiter_still_fails_on_the_window
    theseus-core turn::admission::tests::a_typed_turn_is_admitted_before_queued_task_turns
    theseus-core turn::admission::tests::a_wait_past_the_window_while_the_queue_moves_does_not_fail
    theseus-core turn::admission::tests::a_waiter_that_went_away_loses_no_slot
    theseus-core turn::admission::tests::no_wake_up_is_lost_when_releases_race_arrivals
    theseus-core turn::admission::tests::the_rule_lowers_a_limit_and_never_raises_the_ceiling
    theseus-core turn::admission::tests::turns_of_one_class_are_admitted_in_their_arrival_order

1. `a_person_waits_past_the_window_and_one_notice_names_the_holder`: ceiling 1, holder `exe_held` never ends, still waiting past 2 windows, exactly one notice containing "exe_held for 601 s" and "1 of 1 slots held".
2. `a_task_waiter_still_fails_on_the_window`: `Err(Stuck{window_s:600,class:"task",in_use:1,ceiling:1,waiting:0})`, queue left, no notice.
3. `a_stuck_episode_is_told_once_however_long_it_lasts`: 4 windows, one notice.
4. `a_person_who_kept_waiting_is_admitted_when_the_holder_ends`: holder dropped, person admitted (`load` (1,0)); then a second stuck episode is told again (2 notices, the second naming the new holder).
5. `a_person_with_no_notice_sink_simply_keeps_waiting`: no sink, no panic, still grantable.

**Red-first, honestly:** I wrote the change before the tests, so no test was run against the unmodified base; the new tests need `Notice`/`set_notice`, which the base lacks, so they do not compile there. Test 2 is a regression guard and passes with the change as well as it would before. Each was instead proved by planted reverts (file restored, `touch`ed, `git status` clean after each; module run 18 tests):
- A person exemption off (`if false && class == Person`): FAIL tests 1, 3, 4, 5.
- B latch off (`true.then`): FAIL tests 1, 3 (two notices).
- C exemption for every class (`if true`): tests 2 and the Wake rewrite HANG and hit nextest's timeout (2 timed out), so a task never fails; red as a hang, not an assertion text.
- D `rx.close()` on the continue path: FAIL tests 1, 3, 4, 5.
Note: advancing a paused clock past several timers lets a woken task observe the final time, so test 1 advances one window at a time (hence "601 s", not "600 s").

## Keel guard
`python3 scripts/keel-guard.py --base 92e3dbed --head HEAD --quiet` printed nothing, exit 0, for each commit.

## Gate
Command: `CARGO_INCREMENTAL=0 THESEUS_KEEL_BASE=92e3dbed TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`. Without `THESEUS_KEEL_BASE`, the door's keel phase fails on this clone on history I did not make (acks in old commits whose signatures do not verify here, `%G? is E`); with the base set, keel, keel tests, fmt, shape, features, clippy, clippy-probe, cockpit, test build and reader rule all passed. A first full run died on a full disk (`No space left on device`): I removed `target/debug/incremental` and reran. The suite (8630 tests) failed only on:
- the known L1 set: `theseus-sandbox::contract` (clauses, egress, exit status, sigterm, scratch, a_job_that_cannot_start_says_why), `theseus-sandbox::bench spawn_100`, and 14 `theseusd::sandbox` tests (two of them took 30 s);
- `theseus-core tests_route::a_switched_turns_compilation_names_its_recall_drops`, which is not on the preamble's list: it failed under the suite and passed alone (`1 test run: 1 passed`). Unrelated to admission; named as a possible load flake.
All 18 admission tests passed inside the gate run. `cargo deny` was not installed (I skipped `cargo install cargo-deny`), so the deny phase and the later machine checks (protocol types, benches) were not run by me; this change touches no protocol type and no dependency.

## Live check for the maintainer
No model is involved. On a scratch daemon with `[kernel] admission_ceiling = 1` and a small `admission_stuck_secs`, hold the slot with a long `proc.run` job's turn from one session, then type a message in another: it should wait past the window instead of failing "admission_stuck", and the owner's DM should get one quiet notice naming the holder's execution id; ending the job should admit the message.

## Left / uncertain / design notes
- `TurnRunner::admit` has a second stuck failure, the `tokio::time::timeout(window, ended)` wait for the *same execution's* turn to end (about line 740 of admission.rs, `wait` true, i.e. a person's input). It still fails a person's turn after one window. It is outside this row's three lines; flagging it as the next candidate.
- A slot held by a turn not yet bound to an execution is counted in "N of M slots held" but not named.
- Prompt line numbers matched the base (admission.rs `take` ~464 and the `Err(Stuck)` ~501-507 moved to ~590 after my additions; toolrun.rs 593/2395 unchanged and untouched; config.rs not opened).
- Docs to change at review: `turn/admission.rs` module doc is updated; `crates/theseus-core/AGENTS.md` ("a wait fails only when no turn was admitted for admission_stuck_secs") should say a person's wait is never failed and the stuck episode stages one inform notice; the spec's Part III item should record it.
