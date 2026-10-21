# CLOUD_REPORT: recall-abstain (theseus-53ujg.8, theseus-53ujg.7)

Branch `cloud/20261021-recall-abstain`. Note: the branch's base is `88ddae5` (the clone's main), not `1d092ded` as the task text says; nothing I touched depends on the difference. Store format: unchanged (no stored record changed). No new dependency. No line ceiling raised or exceeded.

## Part A: a job found alive after a restart gets its handle (theseus-53ujg.8), commit `ad74f7f`

**Found.** As the task said: `check_dispatched`'s alive branch answered with a placeholder that had no short id and no meta, so `handles::read` never saw the job.

**Changed.**
- `toolrun/resume.rs`: the alive branch now calls `Handles::assign` (argv from the call's proposal via `jobs_tools::argv_of`, now `pub(super)`), writes meta `{"pid", "job"}` (the keys `handles::read` reads), and uses new `adopted_text`: `Still running as job jN (<label>) in <dir>: the harness restarted meanwhile and found it running. job_read, job_wait or job_stop it; its result also arrives by itself when it ends.` Facts only. The command and directory are left out when unknown. The directory is the action's `resource`, the same field `job_stop`'s summary uses.
- I did not share a function with `placeholder`/`started`: their text differs enough that sharing would have touched them. `wait` is untouched.
- `crates/theseus-core/AGENTS.md`: one sentence added to "A job's handle".
- The unknown branch, `wrapper_alive`, and what a restart adopts are unchanged.
- Rollback: revert `ad74f7f`. No switch.

**Tests.**
- `crates/theseusd/tests/restart_handle.rs` (new file; `job_handles.rs` is job-truths' and was not touched):
  - `a_job_found_alive_after_a_restart_has_its_handle` (the key test): a blocking `proc_run` (`proc_sync_secs` 600), SIGKILL while it blocks, new daemon. Asserts the placeholder's meta (`node.list`) has `job` = `j1` and a `pid`; the model's next request has `job j1` and `the harness restarted meanwhile`; a scripted `job_read` with no job lists `j1`; a scripted `job_wait j1` returns `the job is done` once the test creates the release file. The job is found through the node's meta and the handle, never through `/proc`.
  - `an_adopted_jobs_handle_survives_a_second_restart`: after a second SIGKILL and restart, `job_read` with no job still lists `j1` (the map is read back from the meta).
- Core unit test `toolrun::resume::tests::a_job_found_running_is_named_with_its_handle_and_tools`.

**Proof.**
- `cargo nextest run -p theseusd --test continuations --test restart_handle`: 5 passed, including `a_restarts_continuation_runs_on_the_profile_of_the_turn_it_cut`, unchanged.
- Core test: 1 passed.
- Planted revert: `resume.rs` restored to its original (no assign, no meta). Result: **both** daemon tests FAILED (`meta["job"]` Null, expected `"j1"`). Restored, `touch`ed, `git status` clean. I did not plant a separate "meta without `job`" revert for the second-restart test. The same revert fails it, but at its first assertion (the meta), so it is not independently proven for a meta that has `pid` but no `job`.
- Both tests ran L0 here as root. They need no L1.

## Part B: the recall scorer's abstention shapes (theseus-53ujg.7), commit `51b8254`

**Changed.** `bench/recall/score.py`:
- New `abstained_as(reply, probe, facts)`, plus the `Scored.abstained_as` field (None except for abstention probes) and `ABSTENTION_SHAPES`.
- `summarize` adds `abstention_by_shape` and `abstention_not_misleading`.
- `report.md`'s abstention section gains a table of the three counts and the not-misleading rate, with a one-line definition.
- `bench/recall/README.md` "Each score" describes the shapes and says the strict accuracy is unchanged.
- `correct`, `confident_wrong` and `abstention_accuracy` are untouched. Every arm goes through the same code, and nothing reads the arm.

**Rule, and one choice the owner should hear about.** The task's literal rule ("names a word of another fact's subject and no word of the probe's own subject") cannot pass its own example: "the alder relay" and "the cedar relay" share `relay`. So I used distinctive words:
- Own words are the probe subject's words that no same-kind fact's subject has.
- Others' words are the other subjects' words not in the probe's subject.
- Both drop articles, possessives and the kind's noun.
- A clause qualifies as a neighbour's when it has one of the others' words and none of the own words. Whole words, case-folded, clauses from `_clauses`.
- Where a probe has no distinctive own word, shared words count as own, which is the conservative side.

Misleading means no admission, or a value with no subject, or a value in a clause that names the asked subject.

**Tests** (`bench/recall/test_score.py`; `python3 -m unittest test_score`: 19 tests, OK):
- `test_abstention_shapes` (the task's table, with 5-digit ports because the port regex wants them).
- `test_strict_abstention_accuracy_unchanged`.
- `test_summary_counts_the_shapes`.

**Plants.**
- Classify every admitted reply with a value as `neighbours` (the subject check disabled): `test_abstention_shapes` FAILED. Restored.
- Let `neighbours` count as correct: `test_strict_abstention_accuracy_unchanged` FAILED. Restored and `touch`ed.

**Other failure found.** `python3 -m unittest discover -s bench/recall`: 96 tests, 1 failure, `test_generate.Budget.test_the_prices_are_the_catalogs` (a catalog price, 0.1 vs 0.2). It is unrelated to this change (it concerns a model price in theseus-core's catalog versus the bench's table). I did not fix it.

**Maintainer: rescore the kept v1 run** (I don't have it):
`python3 bench/recall/score.py <theseus run dir> <claude-code run dir> <pi run dir> --out <dir>`
Expected: the strict abstention numbers match the v1 report (Theseus 38 misses of 60, Claude Code 36, Pi 21). The new table splits those misses into neighbours and misleading. The counts-only pass predicts Theseus's 38 are all admitted-then-neighbour values, so most should land in `neighbours`. Any `misleading` that remains is the next row. Mind that a reply naming a neighbour's value in a clause without its subject words (for example a bare list "4432, 51234") will be classed `misleading` by design.

## Gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (last full run, on the tree as committed in `ad74f7f`): door, keel, keel tests, fmt, shape, features, clippy, clippy-probe, cockpit, test build and reader rule passed. The suite phase: 7252 run, 7218 passed, 34 failed:
- 33 are the known L1/root failures: `theseus-sandbox::contract` (20), `theseus-sandbox::bench spawn_100`, and `theseusd::sandbox` (12).
- 1 is `theseus-core tests_m3::a_jobs_raw_output_stays_while_its_wrapper_lingers_and_goes_when_it_ends`, which is not on the known list. It passed when run alone (`cargo nextest run -p theseus-core a_jobs_raw_output_stays`: 1 passed). I judge it load flakiness, but I did not reproduce the failure, so I can't say more. The maintainer should rerun it.
- The phases after the suite (protocol types, lifecycle, jobs, turn and size benches) were not run: the benches are off by `THESEUS_GATE_NO_BENCH`, and I changed no protocol type (so `cockpit/src/protocol.gen` is unchanged). A first gate run failed clippy (`redundant_clone` in my new test); I fixed it, and the second run died on a full disk (see below). The third run is the one reported.
- `cargo deny` was not run separately (no dependency change).

**Keel guard:** `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` printed nothing, exit 0.

## Environment notes

The VM's disk filled twice during the gate (the debug workspace is about 28 GB). I deleted `target/debug/incremental` and the large test executables (rebuildable) and set `CARGO_INCREMENTAL=0`. Neither counts as a test result.

## Docs to change at the maintainer's review

- `docs/spec` Part III: part A's step. A job a restart found alive has its short id and the job tools' words, so `job_read` with no job lists it.
- `docs/status.md`: the same, and part B's shapes in the recall bench's entry.
- `docs/benchmarks` (recall): the abstention table after the rescoring.

## Follow-ups (not changed)

- The `service` chore's other failure (the model polled before `start-service` ended) is the model's choice, as the task said.
- An adopted job whose `resource` is empty, or whose proposal is gone, gets words without a command or directory. That is harmless, but job-truths' work on `wrapper_alive` may want the same facts.
