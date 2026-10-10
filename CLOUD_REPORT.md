# CLOUD_REPORT: bench-fixes (theseus-2x5y, theseus-ufe5)

Branch `cloud/20261010-bench-fixes`. Commits: f3ad819 (ufe5), d781acd (2x5y).

## Step 1: the turn bench's frame check (theseus-2x5y), d781acd
- **Found:** `Driver::measured` counted a frame of only `ledger:memory.*` rows (a memory pass between turns) with the turn's.
- **Changed:** `split_memory_passes` in crates/theseus-sim/src/perf.rs; `measured` returns the passes apart (fourth tuple item; perf/long.rs ignores them), the trace-count check and budgets see the turn's frames only, and the labels end with `+N memory pass frame(s)`. Budgets untouched.
- **Proof:** `cargo test -p theseus-sim --bin theseus-sim perf`: 17 passed, 1 ignored (a measure). New tests: `a_memory_pass_between_a_turns_frames_is_not_the_turns`, `a_turn_frame_that_also_holds_a_memory_row_still_counts`. Planted revert (filter replaced by `true`): the first failed; restored, touched, `git status` clean. Clippy `--workspace --all-targets -D warnings` clean.
- **Live check (owner's machine):** `nice -n 19 theseus-sim bench turn` beside four busy loops, a few runs: the tool-call kind should no longer fail on "trace counts N frames, and the WAL holds N+1"; a printed frame list may end with `+1 memory pass frame`.
- **Not done:** I did not reproduce the 4-of-4 loaded failure live (needs the owner's setup); the test covers the split only.

## Step 2: bench/harbor sampler bounds (theseus-ufe5), f3ad819
- **Changed (test_sampler.py only; no test function removed or renamed):**
  - (a) A counting seam `reads_of` over `sm._read`, `Tracker.classify` and `_own_class`. `test_a_sample_reads_what_it_must_and_no_more` asserts one sample's exact reads on the fixture: stat N, cmdline 1 (harness only), status 2 (harness and its child), classify 1, own_class N. **Deviation from the brief:** the sampler does read `status`, for tree processes (harness, wrapper, work: their memory), never the container's own; the count says so.
  - New `test_the_count_catches_each_plant_the_review_used` (status of every process, cmdline of every process, classification twice, stat twice, F5 as the sampler).
  - (b) The F5 check is the median over rounds of paired sampler/F5 ratios at 0.85 (`median_ratio`); (c) the teeth test keeps F4 only (median ratio > 0.85, plus a read count, so its assertion count is kept). `bare_pass` and `procfs_stat_us` removed (dead).
  - (d) `InANamespace` runs an F5-variant sampler beside the real one in the same namespace over the same 8 s and bounds the real share by 0.85 of the variant's. Quiet VM: ratios 0.65, 0.68, 0.77. `NAMESPACE_RATIO` gone, no host constant.
  - sampler.py's head already says the whole time is counted twice (160 ticks for a true 80); no edit needed.
- **Proof:** the five plants applied to sampler.py itself, each failing `test_a_sample_reads_what_it_must_and_no_more` with its count (status 3, cmdline 50, classify 2/own_class 100, stat 100, cmdline 50); restored and touched. Quiet: test_sampler 23 OK under python3 (3.11) and Harbor's 3.12 venv, 3 runs each; full bench/harbor: 147 OK (40 skipped) under 3.11, 147 OK under the venv.
- **Not proved (honest gap):** the ten loaded runs of the suite. The first attempt (suite at nice 19 beside four busy loops, plus my cargo build) took 750 s a run and failed in pre-existing timing tests (test_measure StopAgent, test_sampler.Scripts' "start waits for its first sample"), so it measured starvation on this 4-core VM, not my change. A second attempt ran test_sampler alone loaded: one run took 601 s and failed the same pre-existing `Scripts` test. I did not get pass counts for my new cost/namespace tests under load; the owner's 16-core host should run `nice -n 19 python3 -m unittest test_sampler` ten times beside four busy loops.
- **Live check:** on the 16-core host with builds running, run test_sampler (above); SamplerCost and InANamespace should pass, and a planted extra read should fail the count.

## Keel findings expected
None from my commits (test_sampler's assertion count is kept). `keel-guard.py` over the base range reports findings from main's own range, not mine: crates/theseus-tui/src/tests.rs assert-removed, allow-added in theseus-index vectors.rs and theseus-protocol tests_work_join.rs, ceiling-raised in scripts/long-files.txt.

## Gate
`THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` stopped at the keel phase on those findings (2 s). I then ran by hand: `cargo fmt --check` ok, `scripts/shape.sh` ok, workspace clippy `-D warnings` clean, `cargo deny check` ok, `cargo nextest run -p theseus-sim`: 64 passed, 1 skipped, 1 failed (`the_restore_rows_store_is_the_same_with_the_cancel_row_selected`: "target/debug/theseusd is not built"; I built only theseus-sim). **Not run:** the full workspace suite, the cockpit/protocol-types phases and the lifecycle/jobs/turn phases (no full workspace build here); the first commit (f3ad819) went in before the gate, at the stop hook's request, covered by bench's suite as the brief allows for Python-only commits.
