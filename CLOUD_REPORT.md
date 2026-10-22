# CLOUD_REPORT: tail6-t5fix (theseus-4v88e, theseus-8zgji)

Base `33e589e2` (integ/tail5). Branch `cloud/20261021-tail6-t5fix`. Store format untouched (49). No new dependencies.
No file went over its line ceiling. Commits are signed (`-S` worked on this VM).

## theseus-8zgji: the sticky reason keeps its admitting bar

**Found.** `TurnRunner::stick` replaced the reason with `Sticky` for every `applied()` decision, so a held change from an
`applied_unsure` answer lost the bar that admitted it.

**Changed** (`14ae2330`). New `EffortReason::StickyUnsure`, ledger word `sticky_unsure`, counted by `applied()`.
`stick()` yields it when the decision it holds was `AppliedUnsure`; every other held change still says `sticky`, so
every existing `sticky` row means what it did. I picked a new value over a flag because it adds no field to
`route.decided`, the result, or the TypeScript. Core `AGENTS.md`'s effort section has one line on it.

**Proved.** `a_sticky_hold_of_an_unsure_answer_keeps_its_bar` (`tests_effort_sticky.rs`): medium at 0.95, then high at
0.42, on a no-per-message-effort model with `keep_warm_min_tokens = 1`. The second row says
`sticky_unsure / ran high / sent medium`. `cargo nextest run -p theseus-core effort`: 47 passed.
Planted revert (`Sticky` set alone again): the test fails, `left: ("sticky", "high", "medium")`,
`right: ("sticky_unsure", ...)`. File restored and touched.

## theseus-4v88e

Commits `03dc785a` and `36d78877`.

**F1, the restart test.** `a_restart_removes_the_earlier_runs_dm_board_once_the_home_stands` (`tests_task_home.rs`).
The first run (home off) draws the DM's board; its outbox is stopped; a second binding starts on the same stand-in
Discord with a fresh state dir and the home on. The home answers, the earlier pinned board leaves the DM, every
other DM message is kept, no board is drawn in the DM.
- Rig: no rig could start twice, so I added `Rig::start_over(fake, ...)` to `tests_gateway.rs` (`start_tweaked` now
  calls it) and made `tune` in `tests_default_forum.rs` `pub(crate)`.
- Difference from the brief: the second start has a fresh store, so the home is *made or found by name* at the second
  start, not found from a recorded META key. It is still a new process with an empty lane map and a pinned board left
  by the earlier one, which is what the removal has to handle. A copy of the first store was not possible while the
  first core holds it open.
- Plant (the `BoardMoved` send removed in `task_home.rs`): this test and `the_home_standing_removes_the_dms_board`
  both fail, `timed out: the earlier board is gone` / `the DM's board is gone`. Restored and touched.
- Not separately planted: the DM lane being absent when `ensure_task_home` sends. The test passes only because the
  lane exists and receives `BoardMoved` in a real second start.

**F2, the archive tick.** `archive_tick` reads `tick()` before `tokio::spawn`.
`the_archive_tick_holds_on_a_multi_thread_runtime` (`tests_forum_archive.rs`, `flavor = "multi_thread"`) sets the
override, ages a post, and calls `archive_tick()` itself on the test thread. Plant (`tick()` read inside the block
again): the test times out waiting for the archive (ran 100 s with nextest's kill), and
`a_read_post_leaves_on_the_archive_ticks_own_run` still passes. Restored and touched.
One caveat: the rig's own binding starts on a worker thread, where the override is unset by design. A rig test of the
tick therefore has to call `archive_tick()` from the test thread, as this one does.

**F3, the named assert.** The old `kept.len() + 1 >= dm_before.len()` is replaced by `only_the_board_left(before,
board, after) -> Result<(), String>`. It names the board's id, requires it gone, and requires every other earlier
message kept. The test asserts the result with `assert_eq!(..., Ok(()))`. The fake Discord has no delete call, so
"remove a non-board message in the fake" could not be planted. The helper is instead tested directly
(`the_check_names_the_board_and_keeps_every_other_message`): one other message gone fails, and the board staying
while another message goes fails (the old check passed that case). My first version moved the assertion into the
helper, and the keel guard flagged `assert-removed`; the third commit keeps the assertion in the test fn.

## Runs

- `cargo nextest run -p theseus-discord task_home archive board`: 93 passed. Whole `-p theseus-discord`: 629 passed
  (before the final helper change); `task_home` alone after it: 11 passed.
- Load (`nice -n 19` plus four busy loops at nice 0, build done first, `--stress-count 10 --no-fail-fast`, `task_home`):
  `Summary [379.184s] 10/10 stress run iterations: 10 passed`. The race noted in the brief did not show here.
  A first attempt compiled at nice 19 beside the loops and was killed by its time limit before running a test.
- `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check`: clean.
- Keel guard, `python3 scripts/keel-guard.py --base 33e589e2 --head HEAD --quiet`, run on `36d78877`: empty output, exit 0.

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` **did not complete**: phases door, keel, keel tests, fmt,
shape, features, clippy, clippy-probe and cockpit passed (last run: 0, 5, 0, 14, 6, 1, 449, 62, 52 s), then the
**test build failed three times with `No space left on device`**. This VM's writable allowance (about 39 GB) cannot hold
the workspace's test build (target was 28 GB). The suite phase and the machine checks after it did not run, so
these are not proven here and the maintainer's gate must run them: the whole `theseus-core` suite, `tests_registry`,
`tests_output`, the layouts. My changes touch only tests, one spawn argument, and a new ledger word; the focused
suites above pass. Known VM failures from the brief (L1 tests, golden under UTC, the hohs and 1n2y flakes) were
not reached.

## Live check for the maintainer

```
scripts/gate.sh                                  # whole suite, on the owner's machine
cargo nextest run -p theseus-core effort         # 47 pass
cargo nextest run -p theseus-discord task_home archive board   # 93 pass
```
A live scratch-daemon check of `sticky_unsure` needs a model without per-message effort (Opus 5.5) past
`keep_warm_min_tokens` and Jev answering one step off at 0.35 to 0.60 confidence; `theseus why` then shows
`effort: Jev said high (sticky_unsure)`.

## For the integrator

`docs/status.md`: "`route.decided`'s `effort_reason` gained `sticky_unsure`: a one-step hold of an `applied_unsure`
answer; `sticky` is unchanged."

Follow-ups: `theseus-protocol`'s `why.rs` prints `effort_reason` as is, so `sticky_unsure` shows verbatim; the
cockpit's `routefooter.ts` was not checked for a switch over the reason words.
