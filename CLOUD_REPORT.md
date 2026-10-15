# CLOUD_REPORT: own-bench-whole (theseus-pw1q.20)

Branch `cloud/20261013-own-bench-whole`. Code commits: 71881796 (the feature), a466ed44 (clippy: long functions split).

## What I found
- Nothing records file pre-images. "Pre-images where they exist" is read as: what the session's tool calls recorded. `fs.edit` has its input's old/new text; `fs.write` has `created` or the result's whole diff. `fs.patch`, any other writing tool, and an overwrite whose diff was cut (over 16 KB) cannot be put back, so the turn is refused, naming the file.
- A program's file changes (`proc.run`) are unrecorded. They are counted in the row's `caveats`, not refused (a coding turn nearly always ran one).
- L1 mounts the workspace as a read-only overlay whose writes are discarded per call. So files a program writes are not visible to the replay's later calls; only the `fs.*` tools change the copy.
- Fixed in passing: `compare_replay` now takes the original's final reply (it used the first answer), and `call_replay` is split into `call_model` + wrapper.

## What changed
- `turn.replay {scope:"whole_turn"}` (CLI `--whole`): `own_bench/whole/` = `scratch.rs` (owner-only copy under the temp dir, skipping `target/`, `node_modules/`, `.theseus-files/` and the gate's approve/floor paths, caps `[route] whole_tree_mb`; removed on drop), `undo.rs`, `hands.rs` (file/git tools on the copy with any path outside refused; `proc.run` through `theseus_kernel::probe_run::run_in` in L1 over the copy only, egress `[sandbox] egress`, bare env; every other tool a stub: the original turn's recorded result for that tool, else a sentence), `runloop.rs` (caps `whole_loops` 24, `whole_seconds` 300, `whole_usd` 0.50; each call reserved first and held on the day ceiling), `compare.rs` (last test run pass/fail, path set, exit-code set; "test" is told by argv words and every run's argv/exit is in the row), `label.rs`.
- Labels: a judged match at `[route] whole_label_bar` (0.9) with no cap, same tests and same paths writes a `judge.label` (source `replay`, weight 0.4, question `mode`, the class whose profile list leads with the rung, written once per judgment and turn). `learning::without_machine` drops them from the report holdout and `propose::gather` from the holdout and its sizing; they are counted apart (`LabelCounts.replay`). Route pack stays in `LEFT_ALONE`; there is no writer.
- Report: `RouteCell.whole_replayed/whole_matched/tests_same`, `RouteReportResult.labels_fed`; CLI and cockpit "Your work" words. Protocol TS regenerated.
- Config `[route] whole_*` (5 keys, defaults, template, tests). No store format change (a row's fields and a label source only; `LabelCounts.replay` is `serde(default)`).
- Kernel: `probe_run::run_in`; tools: `change::unapply`.

## Proof
- `cargo nextest` `own_bench::whole` 11/11; `machine_label` tests 3/3; `theseusd` `whole_replay` passes as root (L1 refusal, program `refused`, nothing ran elsewhere) and under `setpriv --reuid=1000` (program ran in L1, exit 0, tests same). Cockpit `node --test test/yourwork.test.ts` 3/3, `tsc --noEmit` clean.
- Planted reverts (each failed, file restored and touched): R1 hands pointed at the real dir -> `a_whole_turn_replay_changes_nothing_outside_its_scratch_tree`; R2 report holdout unfiltered -> `a_machine_label_never_grades_the_reports_holdout`; R3 propose holdout from any-label -> `a_machine_label_trains_but_never_enters_the_holdout`; R4 loop cap off -> `the_cap_stops_a_long_replay`; R5 stubs run -> same changes-nothing test; R6 pre-image check skipped -> `a_turn_without_pre_images_is_refused_naming_the_file`; R7 label bar off -> `a_weak_or_contradicted_replay_writes_no_label`; R8 path check off -> changes-nothing test.
- Scoped suite (core, protocol, tools, kernel, cli): 3464 of 3478 pass; the 14 failures are job-wrapper/cancel tests (`tests_job_handles`, `tests_jobs`, `telemetry::tests_cancel`, `tests_resumed`) that pass when rerun alone (21/21): they fail under whole-suite load/low disk here. Not mine. The full workspace suite was not completed (disk allowance ran out twice).
- `cargo clippy --workspace --all-targets -D warnings` clean; `scripts/shape.sh` 0.
- Lifecycle bench (debug build, this VM): start, shutdown, SIGKILL restart, swap budgets ok; "the push's seed: first executions.watch" 1.2 ms p50 against 2 ms but p95 2.8 ms: MISSED on this VM. This change touches no start or push path; the maintainer should rerun on the owner's machine.

## Gate
`scripts/gate.sh` stopped in its door/keel phase and never reached build or tests: the keel guard reports `crates/theseus-sim/src/lifecycle.rs:155` budget-raised from older commits (1c2beb2747, 238ecead06) whose owner acks it cannot verify here (unsigned/unknown key). Not from this branch. I ran clippy, shape and the scoped suites by hand instead. **Keel findings expected from my commits: none.**

## Live check (maintainer)
- `cargo nextest run -p theseusd -E 'binary(whole_replay)'` (unprivileged shell for the L1 path).
- On a scratch daemon: `theseus turn replay SESSION TURN -P haiku --whole` on a turn with edits and a test run. Expect `whole turn:` lines (calls ran/stub/refused), `paths changed` and `tests same`, a routing label line, and the real tree untouched; `theseus route report` then shows the whole-turn columns and "Machine labels fed".

## Left / uncertain
- The nightly sampler does not run whole turns yet (owner's run only); say if it should, under `replay_day_usd`.
- Choices for the owner: the label is the class leading with the rung (coding classes first); weight 0.4; "test" by argv words; programs' writes discarded by L1 per call; `fs.patch` turns refused.
- Docs to update at review: spec Part III item, `docs/status.md`.
