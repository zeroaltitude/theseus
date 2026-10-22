# CLOUD_REPORT: tail4-tests (theseus-93lkm, theseus-5hr6i)

Branch `cloud/20261021-tail4-tests`, base `8260dbab`. Store format: unchanged (49). No daemon or product behaviour
changed; the only non-test line outside `bench/` is a `#[cfg(test)]` branch in `rsi/mint/run.rs` (below). No file
passes its line ceiling; no new dependency (`tempfile` and `libc` were already core dependencies).

## Step 1: theseus-93lkm, the recall bench's two red smoke tests

**Found.** Both reds reproduced here on x86 (debug binaries, python 3.11): the smoke compacted at turn 1, seed 12 at
turn 4, instead of the mark (10).

- The first compaction row: `outcomes: ['compaction']`, range `first 19, last 62`, 4 messages, at turn 1. Turn 0's
  context was 19,059 tokens against a daemon overhead of 7,345 (the run's own `first_turn` reading was 7,383) and a
  request budget of 19,154 (window 31,000, output cap 7,750, 4,096 margin).
- The gap is the tools. The request carries 40 tools, 23 of them `defer_loading: true` (a deferred tool costs the
  provider nothing until it is found). The daemon counts those as one constant (`tools_defer::tools_bytes`:
  `DEFERRED_BYTES` 223 for all, `SEARCH_BYTES` 634 for the search tool), so its overhead is 7.3k. The bench's
  stand-in, `standin.py`, counts a request with `tokens.py`'s `census_of_request`, which counted every tool's whole
  schema (44,866 bytes, about 11,000 tokens too many). Each stand-in call therefore reported about 19k input tokens, and
  the daemon's ring and compaction followed the reported usage, not its own estimate. **The bench is wrong, the daemon
  is right** (the daemon's rule is documented at `tools_defer.rs:156` and measured against the provider). The mirror
  predates the deferral and was never updated. The base's history is one squashed commit, so I could not name the join
  that moved it; the 11k gap is measured directly instead of bisected.
- **The `f001` finding (x86).** Not delivered because the turn that said it was compacted away at turn 1-5 along with
  the rest of the early turns; it is the same cause. With the fix `delivered.json` is whole on x86 (the test asserts
  it).

**Second finding, after the first fix.** The smoke then failed `moved == 0`: a second, unplanned compaction at turn
23-24 moved one `near` probe (`p007`) to `compaction`. From about turn 20 on, the index tender is up, and recall adds
a pack to each request (`recalled: 6`, about +1.2k tokens, up to `[memory] recall_budget_tokens` 1,500). The plan's
"turns before a mark fit" bound ignored recall, so the unmarked second session overflowed. It depended on timing (it
showed with a short tmp path and was absent once on a long one); in six runs on a short path it happened six times.
`generate.fit_bound` now reserves `RECALL_TOKENS = 1500` for it. This changes the generator's plan (windows and
bulks may differ from before); `test_generate` has no pinned window and passes.

**Changed.** `f895dab` (`bench/recall/`): `tokens.py` (`tools_bytes`, mirroring `tools_defer::tools_bytes`;
`census_of_request` uses it), `generate.py` (`RECALL_TOKENS`, `fit_bound`), `test_generate.py` (a test that reads the
two constants and the search tool's name out of `tools_defer.rs` and checks `tools_bytes` and the census by hand).
The tests' assertions in `test_drive.py` are untouched.

**Proved.**
- `python3 -m unittest bench.recall.test_drive`: before, `Ran 24 tests ... FAILED (failures=2)` (the two named
  tests, `4 not found in (10, 11)` and `1 not found in (10, 11)`); after, `Ran 24 tests in 44.1s: OK`, run twice.
- `python3 -m unittest bench.recall.test_generate`: 31 tests, one failure, the known
  `Budget.test_the_prices_are_the_catalogs` (`0.1 != 0.2`, not this row).
- Plant 1, `tokens.py` put back as it was (stashed): `TheseusDriver` fails the same two tests (`4 not found in (10,
  11)`, `1 not found in (10, 11)`). Plant 2, `RECALL_TOKENS = 0`:
  `test_the_smoke_runs_end_to_end_on_a_scratch_daemon_and_leaves_nothing` fails with `AssertionError: 1 != 0`
  (`s["moved"]`). Plant 3, `tokens.py` reverted: the new `test_generate` test errors (`tools_bytes` missing). Each
  restored and `touch`ed; `git status` showed only the intended files.

**Live check for the maintainer.** `cargo build --workspace && python3 -m unittest bench.recall.test_drive -v` should
show `Ran 24 tests ... OK`, on ARM too. Then one real smoke on a real model (`bench/recall/README.md`'s command) should
compact at the mark or the turn after it, and `moved` should be 0 in its summary.

**Left / uncertain.** The plan now carries a recall reserve it did not before, so a real full-size progression's window
may grow a little; the conductor should regenerate before the next real run (the progression digest changes). Recall
only appears once the index tender has caught up, so a real run still sees it from some turn on; the reserve is for the
worst case. The stand-in's census and the daemon's estimate now agree on deferral; nothing else was found to differ.

## Step 2: theseus-5hr6i, tests that leave their temp dirs behind

**Found (the leak sources).** One `cargo nextest run -p theseus-core` with `TMPDIR` set to an empty directory
(4,031 tests) left **268 directories** on the unchanged tree: 187 `theseus-test-*` (`rpc/tests.rs:27, 123, 853,
1065, 1202, 1466, 1613, 1887, 1925`, `rpc/tests_voice_pick.rs:130`), 31 `.tmp*`, 16 `theseus-mine-*`
(`rpc/tests_self_patterns.rs:57`), 12 `tool_*` (`rsi/mint/run.rs:476`, the fixtures' `Tmp`), and the rest
`theseus-scope-`, `theseus-owned-`, `theseus-flow-` and `*.flow.json` files (`rsi/mine/read/tests.rs`,
`rsi/mint/tests_owned.rs`, `rpc/tests_self_deploy.rs`). (A watcher counted and removed dirs older than 150 s as the
run went, so the disk would hold; the number is the tally plus what remained.) The `.tmp*` ones came from a `TempDir`
held in a leaked `Arc<Core>` or `mem::forget`: `rsi/tests_tree.rs:68` (the self-repair repository, forgotten by
`rpc/tests_self_backlog.rs:42` and `tests_self_graduate.rs:42`), and `tests_route.rs:71, 83`, `tests_security.rs:102`,
`tests_stop_nudge.rs:47`, `tests_tasks.rs:121`. I found the last five by naming every `tempfile::tempdir()` in the core
by file and line in a throwaway run (reverted, not committed) and reading the names that were left.

**Shape of the cleanup.** `crates/theseus-core/src/scratch.rs` (test-only, one `mod` line in `lib.rs`): `dir(tag)`,
`file(tag, ext)` and `tempdir()` name the path `theseus-test-<tag>_<id>`, note it, and remove everything noted when the
process exits (`libc::atexit`; nextest gives each test its own process and a core is handed to tasks that never end,
so a `Drop` cannot be relied on). The first path a process notes also prunes `theseus-test-*` entries older than six
hours under the temp dir (killed tests), never `.tmp*` or anything else. The ten PREFIX makers, the miner's and
minter's fixtures and the five `tempdir()` rigs above use it. `scratch::tempdir()` returns a real `TempDir`, so it still
drops normally. The one change outside a test file is `rsi/mint/run.rs`'s `Tmp::new`, which under `#[cfg(test)]` makes
its directory through `scratch::dir`; outside tests it is byte for byte as before.

**Changed.** `a52dd68` (scratch.rs, the makers, `rpc/tests_scratch.rs`), then the five rigs in a following commit.

**Dirs left, before and after** (one `cargo nextest run -p theseus-core --no-fail-fast`, `TMPDIR` an empty directory):
before **268** (263 tallied and removed during the run, 5 `.tmp*` left); after the first commit 7 (all `.tmp*` with a
`store`, from the five rigs); after the second commit **0**. `theseus`, `theseus-tui`, `theseus-kernel`,
`theseus-store` and `theseus-judge` (1,664 tests): 0 left. **`theseusd` was not measured**: the disk (a fixed allowance,
about 11 GB here) ran out linking its roughly sixty integration binaries (`ld` bus error). Its tests hold a `TempDir` in
a harness struct that drops normally (read, not run), so a leak there would come only from a test that panics or is
killed; the six-hour prune does not reach `.tmp*` by design. Follow-up: run the `theseusd` suite once with a scratch
`TMPDIR` on a machine with room and give any leaker `scratch`-style handling.

**Tests and plants.**
- `rpc::tests_scratch::a_test_core_leaves_no_dir_behind`: re-runs the test binary with `TMPDIR` a fresh directory; the
  child makes a `test_core` and `mem::forget`s it; the parent asserts the child's directory was under that `TMPDIR`,
  named `theseus-test-*`, and that nothing is left. Plant: `sweep` does nothing: fails with `the child left
  [DirEntry("/tmp/.tmpeR5UR3/theseus-test-t_01a164afc6b0755aa24e4aa5ceadd199")]` (`tests_scratch.rs:57`).
- `scratch::tests::a_made_dir_is_swept`: the same plant fails it (`scratch.rs:136`).
- `scratch::tests::the_prune_removes_only_old_theseus_test_dirs`: a fresh `theseus-test-*`, an old foreign directory and
  an old `.tmpOLD` stay, an old `theseus-test-*` goes. Plant: prune by prefix alone: fails
  `assertion failed: fresh.exists() && other.exists() && tmp.exists()` (`scratch.rs:125`).
- Each plant restored, `touch`ed, `git status` checked.
- `cargo nextest run -p theseus-core --no-fail-fast` after: **4035 tests: 4033 passed, 2 failed**, both on the
  unchanged tree too: `tests_first_request::every_kind_of_place_is_within_its_budget` and
  `tests_work::the_substrate_scenario_reads_as_one_tree_and_pings_for_the_right_reasons`. (The unchanged tree also
  failed `tests_term::a_tasks_terminal_closes_at_its_sessions_end` once; it passed on the later runs, so it is a flake
  here.) Run alone, `own_bench::whole::tests::a_whole_turn_replay_changes_nothing_outside_its_scratch_tree` and
  `aws::tests_l1::a_call_that_names_its_aws_endpoint_waits_and_mints_nothing_before_the_answer` fail on the unchanged tree
  too and pass inside the whole run; not from this change.

**Live check for the maintainer.** `mkdir /tmp/empty && TMPDIR=/tmp/empty cargo nextest run -p theseus-core` and then
`ls -A /tmp/empty | wc -l` should print 0 (it was 268). Re-run it once on the ARM host before removing its sweep.

**Design choices for the owner.** The cleanup runs at process exit (and a six-hour prune), not on `Drop`. A test that
is killed leaves its directory until a later run prunes it. A test that needs a directory to outlive its own process
(none does today) would have to leave `scratch`.

## Docs that should change at the merge
- `bench/recall/README.md` (the `tokens.py` row): say it mirrors the deferred-tool constant too.
- `crates/theseus-core/AGENTS.md`, Tests: a line that a test's temp directory comes from `scratch.rs` (`dir`, `file`,
  `tempdir`) and is removed at exit. (Added in this branch's AGENTS.md commit if it is listed below.)

## Keel guard
`python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` printed nothing, rc 0.

## The gate
See the last section, written when it finished.
