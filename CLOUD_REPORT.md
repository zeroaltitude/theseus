# CLOUD_REPORT: tail5-t4fix

Branch `cloud/20261021-tail5-t4fix`, from `integ/tail4` at `795afd3b`. Store format 49, unchanged. No new dependency.
Commits are signed (the VM had a key; `git cat-file` shows a gpgsig on each, but `%G?` cannot verify here for want of
an allowed-signers file).

## Steps

### theseus-i7kmb (e402bd2)
- **Found.** `Work::apply` compared position only, so a job row's later samples (same position, later `at_ms`) were
  dropped; `herdr up`'s `show_workspace` failed `up` on a focus or second-list error after the tabs were built; `arm`'s
  ended check read only the work view.
- **Changed.**
  1. `work.rs`: `apply` compares `(position, at_ms)`, as `work_changed` does.
  2. `herdr_up.rs`: an error from `workspace.focus` or the second `workspace.list` prints
     `herdr: workspace N (theseus) is built, but herdr could not show it: <why>` and `up` returns `Ok`. The first
     list's error still fails `up` (nothing is built yet).
  3. `app.rs` `arm`: when the session is not in the work view, the execution view's state (`complete`, `cancelled`,
     `failed`, `budget_exhausted`) says whether it ended.
- **Tests and plants** (each plant restored and `touch`ed, `git status` clean after):
  - `work::tests::a_job_rows_last_line_advances_at_one_position`. Plant: position only. FAIL at work.rs:487
    `assertion failed: w.apply(sample(6_000, "Round 2 done."))`.
  - `up_survives_a_refused_focus` (the fake herdr gained `refuse_focus`). Plant: return the error. FAIL at
    tests/herdr_up.rs:834 (`up` exited nonzero).
  - `tests_cancel::c_on_a_finished_task_missing_from_the_work_view_says_finished` (the regatta minus the failed
    task's work view; `regatta()` is now `pub(crate)`). Plant: fallback off. FAIL at tests_cancel.rs:66.

### theseus-kg4kd (822b924)
- **Changed.** `Warm::add` stops the track it evicts. New `AfterClip::abandoned()` stops the clip's warm track;
  `songbird_io.rs`'s `add_event` error branch calls it (the `AfterClip` is now built first and shared with the `Ended`
  handler). The `lead_in_ms` comment ends `(0.7 s)` (`WARM` is 700 ms).
- **Tests and plants.** `warm::tests::an_evicted_warm_track_is_stopped`; plant: drop the `.stop()`; FAIL warm.rs:233.
  `warm::tests::a_clip_whose_end_event_fails_stops_its_warm_track`; plant: `abandoned` does nothing; FAIL warm.rs:248.
  The songbird branch itself needs a live call: the test holds `abandoned()` with the fake track, not the real
  `add_event` failure.

### theseus-kbfl0 (4f22129)
- **Changed.** `sweep()` calls the new `remove_all(paths)`; the test is now `a_made_dir_test_sweeps_only_its_own` and
  removes only its own path (it makes two dirs, the other must survive). No test sweeps `MADE`.
- **Plant.** `sweep()` in the test: FAIL scratch.rs:143 (the other dir is gone).

### theseus-7nx13 (646b82c)
- **Changed.** `title_hit` sorts then dedups the title's words (they are a set; order does not matter to the caller).
  AGENTS.md's overlong line (the paragraph's third line, 155 characters) is re-wrapped.
- **Test.** `a_titles_repeated_word_counts_once` passes. **A plant cannot make it fail:** after stopwords go, a
  non-adjacent repeat always leaves two or more distinct words, so `dedup()` alone and sort+dedup give the same answer
  for every title (the one-word case is adjacent). The change is hygiene, not a behaviour fix; the test holds the
  intent. Say so if the owner wanted a different behaviour here.

## Runs
- `cargo nextest run -p theseus-tui -p theseus-voice -p theseus` filtered to the touched tests: 70 run, 69 passed. The
  one failure, `herdr_up::a_finished_tab_closes_a_grace_after_it_is_seen_and_a_cancelled_one_at_once`, is a timing test
  (index out of bounds at tests/herdr_up.rs:671 while the suite ran in parallel); alone it passed 3 of 3 runs.
  I did not run the load recipe.
- `-p theseus-core scratch::` 4 passed, `thread_title` 4 passed.
- `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo fmt --check` clean.
- `python3 scripts/keel-guard.py --base 795afd3b --head HEAD --quiet` printed nothing (exit 0).

## Gate
`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (built with `CARGO_PROFILE_*_DEBUG=0`): door, keel, keel
tests, fmt, shape, features, clippy, clippy-probe, cockpit, test build and reader rule all passed; **the suite failed**
(7,880 tests, 35 failed), so the phases after it did not run. I did not run them by hand.
- 34 are the known L1 failures (theseusd `sandbox` tests: the VM runs as root, theseus-pv6i).
- 1 is not on the known list: `theseus-core tests_work::the_substrate_scenario_reads_as_one_tree_and_pings_for_the_right_reasons`.
  It fails the same way on 2 reruns alone. The golden (`tests/golden/work_board.txt`) has `<work>/c.txt`; the run
  prints `/tmp/theseus-test-<id>/work/c.txt`. The test's `Tidy::line` (`crates/theseus-core/src/tests_work.rs`, near line 565)
  only folds a path under `/tmp/.tmp`, and the scratch helper (theseus-5hr6i, on the base) now names dirs
  `theseus-test-…`. By reading, it fails on the base too and none of my four commits touch it (I did not run the base
  build). The fix is one more prefix in `Tidy::line`; I left it, as it is outside my row's files.

## Notes for the integrator
- `docs/status.md` (herdr line): "`herdr up` no longer fails when herdr refuses to bring the workspace to the front: it
  says the workspace is built but could not be shown."
- This VM's disk allowance (about 39 GB) filled twice with a full debug build; I cleared `target/` and gate-built with
  `CARGO_PROFILE_*_DEBUG=0`.
- Follow-ups: kg4kd F4 (`[voice] warm_ms`) only if the live check finds gaps past 0.7 s. The live checks for kg4kd and
  7nx13 and i7kmb 4 are the maintainer's: with a real call, a real thread title, and a running `herdr up` that refuses
  focus.
