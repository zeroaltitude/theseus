# CLOUD_REPORT: test-infra (theseus-w3lba, theseus-ynfq9, theseus-kncci)

Branch `cloud/20261021-test-infra`, base `1d092ded`.

## 1. Files left over their line ceiling

None.

## 2. The measurement of A (theseus-w3lba), on the unmodified base

All six runs green; no failures. TZ=America/Phoenix, this 4-core VM.

| run | command | result |
|---|---|---|
| d1 | `cargo nextest run -p theseus-discord --no-fail-fast` | Summary [68.573s] 575 tests run: 575 passed, 0 skipped |
| d2 | same | Summary [68.475s] 575 tests run: 575 passed, 0 skipped |
| d3 | same | Summary [68.091s] 575 tests run: 575 passed, 0 skipped |
| j1 | `cargo nextest run -p theseusd --test acceptance_journey --no-fail-fast` | Summary [13.729s] 7 tests run: 7 passed, 1 skipped |
| j2 | same | Summary [13.598s] 7 tests run: 7 passed, 1 skipped |
| j3 | same | Summary [13.523s] 7 tests run: 7 passed, 1 skipped |

The full `theseusd` suite was not run on its own. The gate's suite phase ran the whole workspace (below).

(A first attempt at these runs was lost when the VM's disk filled; the six runs above were redone.)

## 3. What changed

**A. w3lba: nothing.** The family is closed by `76237a96` (theseus-ihu7l). No fix and no new test were invented. The reproduction in the brief was on the pre-join base.

**B. ynfq9: `0e06860`.** Both gated loops now end on three exits: the `go` file, the directory gone (`[ -d dir ]`), or a cap of 6000 iterations of the loop's own 0.05 s sleep (at least 300 s).
- `gate(dir)` in `crates/theseus-discord/src/tests_task_threads.rs` is now `gate_capped(dir, 300)`. It writes its pid to `{dir}/gate.pid` first, and `gate_script` builds the text.
- `rules()` in `crates/theseusd/tests/acceptance_journey.rs` got the same three exits and no pid file.
- The rig has no call that ends its core's running jobs on drop, so none was added.
- Confirmed on the old base: 18 leaked loops were left after one Discord suite run. After the change the journey leaves none.

**C. kncci: `157a74b`. The exam's stand-in was wrong; the exam's reading and the daemon were right.**
- Printing the ledger showed no `wake.set` row at all. It showed a `tools.missed` row at the wake step: `{"kind":"unsearched_call","tool":"wake_at","family":"reminders","place":"cli"}`.
- The CLI place no longer loads the `reminders` family (deferred tools, `tools_defer/family.rs`). A call to a hidden tool with no search before it is answered "Not run", so the scripted `wake_at` never ran.
- That gives `initiative.promise` False in `oracle` mode (no wake was set). In `wrong` mode the scripted wake that `nopromise` is meant to catch could not run either, so no wake was counted and `nopromise` read True where it must be False.
- Fix, in `bench/personality/exam_standin.py` only: for a rule's calls to a tool the request marks `defer_loading`, the stand-in's turn opens with the provider's `server_tool_use` of `tool_search_tool_bm25` and its `tool_search_tool_result` references, then the calls. That is the shape `tools_defer/scan.rs` reads.
- No daemon change, so no core test was owed.

## 4. Tests and plants

| test | plant | failed |
|---|---|---|
| `tests_task_threads::a_gate_left_shut_ends_with_its_rig` (key test) | the old loop (`while [ ! -f go ]`, no directory or cap exit) | `FAIL`: "the gate (pid …) outlived its rig" after 5 s; the cap test failed too |
| `tests_task_threads::a_gate_ends_at_its_cap` | the cap dropped (directory exit kept) | `FAIL`: "the gate ran past its cap with its directory in place" at 20 s |
| `test_e2e.OnADaemon` (both) | `exam_standin.py` stashed (the base) | both FAIL, as in the brief |
| `test_a_promise_without_a_wake_fails_and_a_wake_unasked_fails` | the `wrong` mode's scripted wake removed | `FAIL`: "a wake nobody asked for" (it still catches a removed wake) |

- Passing: `a_gate_left_shut_ends_with_its_rig` 1.4 s, `a_gate_ends_at_its_cap` 1.07 s (cap 1 s). Every plant was restored and `touch`ed, and `git status` was clean after each.
- Final: `python3 -m unittest test_e2e test_xchecks test_plan` is 43 tests, OK. Both OnADaemon tests pass and OnDiscord still passes.
- The new Discord tests were not run three times in a row under nice-19 load. Each passed on its own run and in the gate's suite.

## 5. What could not run on this VM

- The L1 tests: `theseus-sandbox::contract`, `theseusd::sandbox` (33 failed), because the VM runs as root (theseus-pv6i).
- The benches (`THESEUS_GATE_NO_BENCH=1`): lifecycle, jobs, turn and size.
- To run the benches, on the owner's machine: `scripts/gate.sh`.
- To run the exam's two tests with debug binaries: `cargo build -p theseusd -p theseus -p theseus-index -p theseus-sim && cd bench/personality && python3 -m unittest test_e2e -v`.

## 6. The gate and the keel guard

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`:
- fmt, shape, features, clippy, clippy-probe, cockpit, test build and reader rule all passed (after `cargo fmt` and a complete `npm ci`; the first `npm ci` had been cut short by the full disk).
- The suite phase: 7251 tests, 7217 passed, 34 failed: 33 L1 and `theseus-core tests_horizon::the_agenda_expands_a_series_by_the_kernels_arithmetic_and_skips_what_is_paused`.
- That test passes alone, with and without my change, so it is a load flake. It is not on the flaky list.
- The protocol-types phase is clean (nothing changed under `cockpit/src/protocol.gen`). The bench phases were skipped.
- The commit was counted green on those terms.
- Keel guard: `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` printed nothing (exit 0).

## 7. Follow-ups

- This VM's disk allowance (about 11 GB free after a build) fills during a full gate run. The suite log and `target/` growth from test binaries do it. I deleted `target/debug/incremental` and large test executables, which only cost a rebuild.
- The leaked-loop check used `ps | grep "[w]hile \["`. Another row (job-truths) moves command text off command lines, so the key test finds the loop by `gate.pid`, as asked.
- The exam's stand-in will need the same search shape for any other scripted call to a deferred tool. Today only `wake_at` is one in the personality probes.
- `docs/status.md` or the exam README could say the stand-in searches for deferred tools; I did not edit docs.
