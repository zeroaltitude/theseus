# CLOUD_REPORT: the helpers deferral family and a static fan-out cue (theseus-z9mze.4)

Branch `cloud/20261027-p3t-fanout-cue`. Commits: aa95e3c (step 1), 754c06f (step 2), bc8e63d (steps 3 and 4 together).

## What I checked in git log origin/main before starting
`origin/main` had moved past the stated base (head 7d39d57 at fetch time: wf482 fallback, awycv, ...). I stayed on the
base 9470df70 as told. `.2` and `.3` (task_wait, task_fan_out) are in the base's tree (task_wait.rs, task_fan_out.rs,
tests_task_wait.rs, tests_task_fan_out.rs) and I did not touch them. No commit in `git log 9470df70..origin/main`
mentions z9mze/fan-out by subject (grep), so nothing of this row had landed.

## Citations at my base
- `tools_defer/family.rs`: `HELPERS` at :22 (doc at :21), mapping at :140, `FAMILIES` at :26, comment at :44, mapping
  test cases at :331-332, `HEADLESS_TOKENS` at :263. All correct. The comment at :44-48 (and the AGENTS.md paragraph on
  task.wait) says "no place loads and no cue names yet": that wording needs updating (see docs).
- `want.rs`: `By` at :103 with three variants, `as_str` at :113-119, `Admitted` at :124. Correct.
- `route_step.rs`: the gate point is `compile_first` (`turn/route_step.rs`, after `self.note_routed(...)` and before
  `if self.draft_open(...)`, now about :601). The verdict is known there, the first compile is made, the route row is
  written. `data_step.rs` is a sibling; nothing there needed.
- Not in the brief but true: `route_step.rs` already declares a child module (`mod correct;` -> `turn/route_step/correct.rs`),
  so the cue module is `turn/route_step/fan_out_cue.rs` with one `mod` line there and `turn.rs` untouched.

## Step 1: deferred by default
Found: the helpers family is already in no `PlaceKind::defaults()` list, so it is deferred everywhere by construction
(only the ledger's learned use or an admission can load it). So there is NO red at base for step 1: it is a guard test,
`family::tests::the_helpers_family_is_deferred_by_default_in_every_place`, which also pins every place's defaults.
`HEADLESS_TOKENS` untouched. Honest note: no behaviour change.

## Step 2: By::Cue
`By::Cue` ("cue") and `want::cued(family, catalog, loaded, fit)`. Test `want::tests::a_cue_is_a_fourth_way_in_and_the_other_three_admit_as_before`
proves Asked/Named/Jev still admit and order as before and the cue respects the same ceiling. (Red at base: the test does not compile without the variant.)

## Step 3: the cue (`turn/route_step/fan_out_cue.rs`)
- Classes (the verdict's `mode`): `deep_coding`, `review`, `sophisticated` (as in `turn/tier.rs`).
- Open-ended words (whole tokens, lower case, the first 4,000 chars of the ask): audit, survey, explore, investigate,
  inventory, across, everywhere, throughout, comprehensive, exhaustive, exhaustively, thoroughly, every, each, entire,
  whole, all. List revision `WORDS_REV = 1`.
- Not a task: `t.tc.task.is_none()`.
- Room for two helpers (`room_for_two`): `task_limit - in_use >= 2`, nothing waiting in the admission queue, and half of the
  carve `task_fan_out`'s own split would make (half of what the session has left; of the greater of that and its limit
  under a notify limit) affords two at `max([tools] helper_floor_usd, 2x one call's worst case)`. I replicated the 6 lines
  of `task_fan_out::split`'s arithmetic rather than touch that file.
- Row `fan_out.cue {class, words, fired, v9}` (new `LedgerKind::FanOutCue`, `fact/fan_out.rs`), written for every acting verdict of an
  ELIGIBLE class, fired or not. A verdict of any other class writes no row (a chat turn would otherwise add a row to every
  plain turn). **OWNER CALL:** `v9` was not defined in the brief; I wrote the word list's revision number there (1). Say if z9mze.5 means something else.
- On a fire: the family is admitted into the request's own `Deferral` (`loaded` + `added`, as a found family joins) when it fits
  the ceiling. **OWNER CALL:** the first request of the turn is already compiled (beside the verdict), so the tools change
  from the next compile (a recompile, as `defer_found` does: tools lead the cached prefix, so one cache miss on that loop).
  The tail line therefore says the tools load with `tools_find` if not listed.
- No model call, judge call or spend anywhere in the cue.

## Step 4: tail line and the labelled set
- Tail: `Self::fan_out_line` calls `attach_hint` (the coding hint's own helper: `attach_text` after the cache mark, counted in the estimate), called from
  `compile_step.rs` `tail_blocks` (beside `coding_hint`, line ~92) and on the first compile in `compile_first`. It is the
  last block of the request's last message, after `cache_control`. Proof: `tests_fan_out_cue::a_firing_cue_writes_its_row_and_one_tail_line_and_moves_no_prefix_byte`
  checks system and tools equal an ordinary turn's byte for byte and the line is the last block.
- 40-ask set, in `fan_out_cue::tests::SET`. On my own set: precision 0.900, recall 0.900 (tp 18, fp 2, fn 2). Misses: "go through the repository and
  report the dead modules", "find who calls the retry decision from anywhere in the tree"; false positives: "make the whole line say ready instead",
  "fix all the typos in the readme". I wrote the set and deliberately included four hard cases; a first draft scored 1.000/1.000 and I judged that
  uninformative. The floor in the test is 0.85 each. This is the author's own set, so treat the numbers as a sanity check, not a measure.

## Proofs
- `cargo nextest list` selecting the 9 tests (all appear): tests_fan_out_cue (3), family helpers test, want cue test, fan_out_cue unit tests (4).
- Run: `cargo nextest run -p theseus-core --lib -E 'test(/fan_out_cue|helpers_family|cue_is_a_fourth/)'` -> 9 passed.
- Planted reverts (restored from a backup, `touch`ed, `cmp` identical, `git status` clean of the plant):
  (a) removed `!look.task` from `decide`: `the_four_conditions_each_hold_the_cue_back` FAILED at `assert!(!decide(&Look { task: true, ..base }).1)`.
  Not covered at turn level: the `t.tc.task.is_some()` wiring in `fan_out_cue()` has no integration test (a task session's turn through the rig); only the pure decision is.
  (b) removed `&& look.room`: `the_four_conditions_each_hold_the_cue_back` FAILED at `assert!(!decide(&Look { room: false, ..base }).1)` and
  `tests_fan_out_cue::no_room_for_two_helpers_holds_the_cue_back` FAILED at `assert!(!carries(&r.claude.requests()[0]))` (admission ceiling 2 leaves no task slot).
- Red at base for the behaviour: none exist before the change (the module, the variant and the kind do not exist; the new tests cannot compile at base).

## Keel
`python3 scripts/keel-guard.py` with its default base (merge-base with the local main, which is far behind this checkout) lists findings that belong to the merged
content-flags work (data.rs etc.), none of mine. With the real base: `python3 scripts/keel-guard.py --base 9470df70` ->
`keel: ok (9470df70..the working tree, 11 files changed, 0 findings acked, 0.2 s)`. The gate was run with `THESEUS_KEEL_BASE=9470df70` for the same reason.
**Keel findings expected:** none.

## Gate (`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 THESEUS_KEEL_BASE=9470df70 scripts/gate.sh`, run once on the tree of the three commits)
Passed: door, keel, keel tests, fmt, shape, recall tests, features, clippy, clippy-probe, cockpit, test build, reader rule.
Suite: 8751 run, 8717 passed, 34 failed, 56 skipped; all 34 are the known L1/root failures: theseus-sandbox::contract (20 incl. clause_01..12,
egress_18b_*, a_job_that_cannot_start_says_why, exit_status_and_signals, scratch_is_reported_and_discarded, sigterm_is_forwarded_to_the_command),
theseus-sandbox::bench spawn_100, and theseusd::sandbox (13, incl. `a_host_beyond_the_list_once_approved_is_outside_text`, which took 30 s, and `the_jobs_bench_l1_row`).
That is 34 where the brief says about 33; all are sandbox tests that need L1 as non-root. No other test failed.
I then ran the later phases by hand: protocol types (cockpit/src/protocol.gen unchanged, nothing to add), `theseus-sim bench turn --check --runs 5 --burst 0`
(first-request tokens DM 9571/15000, headless 8006/8600, task 6745/8000 ok; plain turn and tool-call frames check passed), size step skipped (no release build).
Lifecycle and jobs benches not run (NO_BENCH). The dm_flow golden is byte identical (no file under theseus-discord changed; the discord suite passed).
Deny phase not reached by the gate failure's route; I did not run `cargo deny` (disk was tight); say so.

## Not done / uncertain
- Steps 3 and 4 are one commit (the line is the cue's consequence and its test shares the rig).
- Environment: the VM's disk allowance filled twice (target/ plus incremental); I deleted `target/debug/incremental` and old test executables, nothing outside the repo.
- The tail line is not on a detour's request (a detour compiles its own tail); a routed switch recompiles through `tail_blocks`, which has it.
- Docs to change (not edited): crates/theseus-core/AGENTS.md (task.wait paragraph: "no cue names yet" -> the fan-out cue); the comment at tools_defer/family.rs:44-48; the spec's Part III item and status row.
- The OWNER CALLs above: the meaning of `v9`; the one-loop cache miss when the helpers family joins after the verdict.

CLOUD REPORT COMPLETE
