# CLOUD_REPORT: p2f-turn (theseus-rmjqr, theseus-5mkz1, theseus-5pg8u)

Branch `cloud/20261024-p2f-turn`, base `fb721356`.

## 1. For the maintainer first

- **Store format:** 49, unchanged. No stored field was added.
- **Protocol:** unchanged (`TurnSubmitParams` untouched; no `protocol.gen/` change).
- **`turn.rs`:** 3,473 -> 3,471 lines (ceiling 3,525). Net -2.
- **No STOP.** `python3 scripts/keel-guard.py --base fb721356 --head HEAD --quiet` printed nothing (exit 0). The 5pg8u items 1-3 are all in.
- **OWNER CALLs (listed, not touched):**
  1. rmjqr (1), the counting rule: a loop with no command still adds to `since_pass`. Not counting no-command loops would make it equal `failing`, a stricter "gate the steer on `failing > 0`". `decide` and the counting are unchanged.
  2. 5mkz1 OUT items: a config key for the progress consts; whether spend should ever stop a turn; exit 8 or 0 when a stopped turn reports; measuring the next bench run (spend on unsolved tasks, timeouts, `nr_throttled`, `turn.progress` rows per trial); pointing `Cpus::read` at the job's parent cgroup; a task's own deadline.
  3. 5pg8u design choice to hear about: a live stream now runs to 3 x `total_secs` (1,800 s by default), where before the whole call stopped at 600 s. `total_secs` still bounds the call before the stream begins. No new config key.

## 2. Per finding

Commits: `e0d0b03` (5mkz1), `d8c6ec5` (rmjqr and 5pg8u together; they share `turn.rs` and the core `AGENTS.md`, so I did not split them).

**rmjqr (1) note's words.** Done: `progress_step.rs` `note()` says "no check has run in its last N loops" when `failing == 0`. The `(f failed)` form is unchanged. Test: `progress_step::tests::the_notes_say_what_was_used_and_what_has_not_passed` (the `failing == 0` case now expects "has run"; the `(3 failed)` case was already asserted). Plant (old "has passed" arm): FAILS at `progress_step.rs:369`.

**(2) failed read marks the check failed.** Done: `Checkpoints::answered` (`checkpoint_step.rs`) sets `failed` as before for any tool, and records commands' facts in `progress_step::Commands`. `progress` takes them with `mem::take` every loop. Test: `tests_turn_progress::a_passing_command_beside_a_failed_read_is_a_pass`. Plant (command check taken from the any-tool `failed`): FAILS at `tests_turn_progress.rs:330` (a steer at loop 3).

**(5) background compares counts.** Done: a command's own `Background` outcome makes the loop no check; `State::background` is gone. Test: `progress_step::tests::a_loops_check_comes_from_its_commands_alone` (pure: Background beats a pass, Done+error is a failed check, no command is no check, a failed read alone is no check). It is a new-API test, so it does not exist at the base; I did not run a separate plant of "compare counts again", because that code no longer exists in this shape.

**(3) `deadline_secs` end to end.** Done: `rpc/tests_submit_deadline.rs` `a_submit_with_deadline_secs_tells_the_model_its_time` (Some(600) puts "minutes left of this turn's 10" in the first request; None has no line), plus one `mod` line in `rpc/mod.rs`. Green at the base by design. Plant (`None` instead of `p.deadline_secs` in `methods.rs`): FAILS at `tests_submit_deadline.rs:55`.

**(4) inherited deadline.** Done: `clock_step.rs` `deadline_of(session, arrived)` filters `from == arrived`; `take_deadline` drops the entry when the turn takes it (at its first loop, from `progress_step::with`, using `Turn.arrived`). A steer that joins a running turn gives it no deadline; the module doc and core `AGENTS.md` say so. `a_deadline_is_kept_by_session_until_it_passes` keeps its name and every assertion (calls adapted); `a_deadline_belongs_to_the_turn_that_arrived_with_it` adds the other-arrival case. `tests_turn_progress.rs` `Rig::turn` now passes `arrived: Some(start)`. New: `a_later_turn_of_the_session_does_not_inherit_its_deadline`. The second turn's request still holds the first turn's line in the session history, so the test asserts exactly one such line (the first turn's), not zero. Plant (lookup by session only): FAILS at `clock_step.rs:206`.

**(6) `STATES`.** Went the first way: the state is `Option<progress_step::State>` on `checkpoint_step::Checkpoints` (no change in `turn.rs` for it). `STATES`, `forget` and `held` are gone, and `clock()` and `shown()` take `&mut Turn`. A side effect: a turn with no deadline now begins its state at its first `clock()` call (one `kernel.execution` lookup), which `progress` did at the first loop's end anyway. No behaviour test (as the brief said).

**5mkz1 bench driver.** Done: `theseus_bench.py` gains `DEADLINE_MARGIN_S = 60`, `DEADLINE_ENV`, `task_timeout(task_dir)` (moved from the async agent; one helper for both), `deadline_secs(timeout)`, and `run_script(..., deadline=None)` putting `--deadline <n>` in the ask line. `Theseus.run` passes it for both arms. The docstrings, the agent's settings paragraph and `bench/README.md` are updated; the docstring says Harbor's timeout multiplier is not read, so the deadline falls early, never late. Tests in `bench/harbor/test_bench.py`: `Deadline` (task.toml 3600 and 900 absent; `deadline_secs(3600) == 3540`, `(90)` is None, `THESEUS_BENCH_DEADLINE=0` gives none) and `test_the_ask_carries_the_deadline_it_is_given` (`--deadline 3540` in the stand-in's `ask-args`, none without). These are new-API tests, so they are red at the base by not existing. `python3 -m unittest discover -s bench/harbor`: 192 tests OK (44 skipped). `... bench/async`: 131 OK (19 skipped).

**5pg8u item 1 (live stream).** Done: `provider.rs` has `LIVE_STREAM_TOTALS = 3`, `Timeouts::whole_call()`, and the stream loop's hard deadline is `started + whole_call()` set once the headers arrive; before the stream, `total_secs` bounds the call as before. The example toml's `total_secs` comments are reworded. Tests (`provider/tests_timeouts.rs`, loopback server with pings every 200 ms): a live stream of about 2.5 s with `total_secs = 1` answers Ok; one that pings forever is cut as `Timeout{Total}` between 3 and 4 s; a silent one is cut as `StreamIdle`. Plant (hard deadline back to `total_secs`): all three FAIL.

**Item 2 (kernel action deadline).** Done at `turn.rs` (in place) and `turn/compaction.rs`: `Some(whole_call().as_millis())`. `rpc/driver.rs` has no `total_secs` reader. The `telemetry/metrics.rs` comment is fixed (not `BOUNDS`). Test: `tests_timeouts::the_whole_call_is_the_live_stream_totals_times_total_secs` (3 x `total_secs`). **Gap:** no test guards the two call sites. A plant of the `turn.rs` site back to `total_secs` left all four `tests_timeouts` tests passing. The brief asked for that plant to fail, and it did not.

**Item 3 (retry once).** Done: `retry_step.rs` has a `Retried { count, whole_call }` struct (the `turn.rs` edit is in place, one line), and a `Timeout{Total}` is retried once a turn whatever `transient` allows. Test: `tests_failures::a_whole_call_timeout_is_retried_once_in_a_turn` (transient 4: [Total, answer] answers with 2 requests; [Total, Total, answer] fails with 2; [StreamIdle, StreamIdle, answer] answers with 3). Plant (Total rule dropped): FAILS at `tests_failures.rs:400`.

**Item 4 (docs).** Core `AGENTS.md` (the `[model.retries]` line, provider timeouts, and the turn-time paragraph) and the `retry_step.rs` module doc.

## 3. Gate commands and results

- Targeted `cargo nextest run -p theseus-core -E 'test(/progress_step|clock_step|tests_turn_progress|tests_submit_deadline|tests_failures|tests_timeouts|retry_step|tests_stopping/)'` with `TZ=America/Phoenix`: 27 run, 27 passed.
- `cargo fmt` applied; `cargo clippy --workspace --all-targets -- -D warnings`: clean.
- `TZ=America/Phoenix cargo test -p theseus-core`: 4,225 passed, 6 failed (`card::tests_job` x4, `import::tests_catalog::a_quiet_stretch_after_work_gives_the_free_pages_back`, `rpc::tests_self_mint::a_validation_runs_its_tiers_writes_one_row_and_loads_nothing`). Rerun alone with nextest, all 26 of those pass, so I read them as load or timing failures on this 4-core VM. I did not run them at the base to confirm.
- `cargo test -p theseus`: `cmd::tests::external_text_lines_and_the_trust_target` failed in the full `cargo test` run and passes alone (a load flake; it also passed in the gate's nextest suite).
- Full gate (`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`): `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (run with `CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0` after the first two runs filled this VM's disk: "No space left on device" in the test build, a linker bus error, not a code failure). door, keel, keel tests, fmt, shape, features, clippy, clippy-probe, cockpit, test build and reader rule all passed. The suite phase failed: 8,078 run, 8,043 passed, 35 failed. 34 are the known L1 reds (`theseus-sandbox::contract` x19, `theseus-sandbox::bench` x1, `theseusd::sandbox` x14). The 35th is `theseus-core tests_route::a_switched_turns_compilation_names_its_recall_drops`, which is not on the known list: it failed once in the full run (the log does not keep its panic message) and passed on 3 of 3 reruns alone. It is in the route area and none of my files touch it, so I read it as a load flake, but I did not run it at the base. Because the suite failed, I ran the phases after it by hand: protocol types (`the_cockpits_types_are_generated_from_the_rust_ones` passes and `protocol.gen` is unchanged) and `theseus-sim bench turn --check --runs 5 --burst 0` (rc 0, all budgets ok). Not run: lifecycle and jobs benches (`THESEUS_GATE_NO_BENCH`) and the size step (no release build, so it skips itself).

## 4. Keel and DM golden

- `python3 scripts/keel-guard.py --base fb721356 --head HEAD --quiet`: empty output, exit 0 (run after the second commit).
- `git diff --exit-code fb721356 -- crates/theseus-discord/src/golden/dm_flow.txt`: exit 0.

## 5. Join notes for `integ/p2-follow`

- `provider.rs`: `Timeouts::total_secs` doc, new `LIVE_STREAM_TOTALS` and `impl Timeouts { whole_call }` right after the struct, one line `let deadline = started + self.timeouts.whole_call();` after the response headers in `stream_message_impl`, and `mod tests_timeouts;` beside `tests_warm`.
- `turn.rs`: net -2 lines. In `run_tools`, `t.checkpoint.answered(&uses[r.index].name, &r.outcome);` before the `match`, and `CallOutcome::Done { .. } => answered += 1`; `Self::progress(t, i, run_model)` (no `&uses`); `let mut retried = retry_step::Retried::default();`; the model call's action deadline uses `whole_call()`.
- `turn/compaction.rs`: the same `whole_call()` deadline.
- core `AGENTS.md`: three small edits (the `[model.retries]` sentence, a provider-timeouts sentence, and the "A turn's time, its progress..." paragraph).
- `bench/`: `harbor/theseus_bench.py`, `harbor/theseus_agent.py`, `harbor/test_bench.py`, `async/async_agents.py` (`_task_timeout` removed, `tomllib` import dropped), `README.md`.
- `rpc/mod.rs`: one `mod tests_submit_deadline;` line.

## 6. `docs/status.md` sentence and follow-ups

Sentence: "A turn's progress check now reads a loop's commands alone and its state lives on the turn; a deadline belongs to the turn its `turn.submit` started; the bench driver passes `ask --deadline` (the task's timeout less a minute); and a live model stream runs to three times `total_secs` before being cut, with a whole-call timeout retried once a turn at most."

Follow-ups: the OUT items under the OWNER CALLs; add a test that guards the two `whole_call()` call sites; and the bench driver could keep `model.retried` rows beside `theseus-judge.json` so a trial shows its retries.
