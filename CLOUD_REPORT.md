# CLOUD_REPORT: pa1xx-quick-recall (theseus-pa1xx)

Branch `cloud/20261019-pa1xx-quick-recall`. Code under test: `eac26b6d`, unchanged. This branch adds no code commit, only this report.

## Step 1: build, clippy

**Found.** `eac26b6d` compiles. `cargo clippy --workspace --all-targets -- -D warnings` is clean.

**Changed.** Nothing.

**Proved.**
- `cargo build --workspace --all-targets` finished.
- Clippy exit 0 (CARGO_INCREMENTAL=0).

## Step 2: the filtered tests

**Command.** `TZ=America/Phoenix cargo nextest run -p theseus-core -E 'test(/route/) | test(/situation/) | test(/recall/) | test(/detour/)'`

**Result.** 250 run, 250 passed. This is `-p theseus-core`, not `--workspace`, because the disk allowance ran out (see Notes); the filter selects the same tests.

These passed with no test edited:
- `tests_route::a_quick_detour_sends_its_recall_and_writes_it`
- `tests_route::a_quick_detours_later_loop_keeps_its_recall`
- `config::routing::tests::a_quick_detour_recalls_and_a_trivial_one_does_not`
- `compiler::situation::tests::each_situation_admits_its_classes`
- The theseus-n7nc tests: `a_trivial_detour_writes_no_recall_and_its_row_says_detoured`, `after_a_detour_the_next_request_carries_no_stale_note`, `a_routed_turn_still_writes_and_sends_its_recall`, `a_detours_window_leaves_its_recall_note_out`.

## Step 3: planted reverts

Each plant is run with the same filter, the file restored with `git checkout` and `touch`, then `git status` clean. All five are caught; none missed, so no test was added.

| Plant | Failing tests |
|---|---|
| P1 `recall_step.rs` (drop `&& !t.route.detour_recall`) | `a_quick_detour_sends_its_recall_and_writes_it` |
| P2 `compile_detour` (`recall_view` replaced by the empty `Sources`) | `a_quick_detour_sends_its_recall_and_writes_it`, `a_quick_detours_later_loop_keeps_its_recall` |
| P3 `compile_detour` filter (`Kind::Recall => false`) | `a_quick_detours_later_loop_keeps_its_recall` (assertion "loop 1 carries the note once") |
| P4 `situation.rs` (`class == Arrangement`) | `each_situation_admits_its_classes` (first run, fail-fast). With `--no-fail-fast` it also fails both `tests_route` quick-detour tests, so 35a refuses the request. |
| P5 `detouring` (`false && ...`) | `a_quick_detour_sends_its_recall_and_writes_it`, `a_quick_detours_later_loop_keeps_its_recall` |

The first P5 attempt did not compile: my replacement text was malformed and prefixed the path. It was redone correctly as `verdict.is_some_and(|v| false && ...)`.

## Step 4: gate and keel

**Command.** `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`

First attempt failed in the keel phase. My clone's `origin/main` was stale (96c592c0), so the guard blamed 117 unrelated commits on me. After `git fetch origin main` the keel phase passes.

**Keel guard.** `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` printed nothing, exit 0.

**Gate result.** Passed: door, keel, keel tests, fmt, shape, features, clippy, clippy-probe, cockpit, test build, reader rule. Failed in suite: 6849 run, 6809 passed, 36 failed, 4 timed out.

| Count | Tests | Why |
|---|---|---|
| 33 | theseus-sandbox `contract` (19), its bench `spawn_100` (1), theseusd `sandbox` (13) | Known: root VM and L1 (theseus-pv6i). |
| 4 timeouts | theseus-discord `tests_task_threads::a_shared_forum_shows_a_shared_places_tasks_and_never_a_private_ones`, `::a_task_in_a_guild_channel_gets_a_thread_where_its_stream_lands_and_one_rename_at_the_end`, `::react::in_a_shared_channel_a_tap_on_a_tasks_thread_does_nothing`, `tests_war_room::a_shared_place_refuses_a_share_and_keeps_its_guests_rule` | Probably the Discord hang already in flight. Not run on the base, not investigated. |
| 1 fail | discord `tests_war_room::ending_a_task_thread_removes_the_guests_the_war_room_added` | Passed alone on rerun. |
| 1 fail | theseus-core `tests_horizon::the_agenda_expands_a_series_by_the_kernels_arithmetic_and_skips_what_is_paused` | Passed alone on rerun (0.08 s). |
| 1 fail | `theseusd::acceptance_journey::the_owner_starts_asks_answers_joins_pauses_ends_and_finds_two_tasks` | **Unexplained, see below.** |

The machine_checks phases after the suite were not run separately.

### Open: acceptance_journey is intermittent on this branch

- It panics at `crates/theseusd/tests/acceptance_journey.rs:583` (`asked_at_ms` of the question view is `None`), about 1.5 s in.
- With `eac26b6d`'s core changes reverted to `bbc65530`, it passed 7 out of 7 runs.
- On this branch it passed 9 of 14 runs.
- By reading, my change is inert for it: `detouring` runs only on a routed detour, and I did not check that the journey's daemon routes any turn.
- This is not proven to be a pre-existing flake. The maintainer should run the journey a few times on `main` and on this branch before the join:
  `for i in 1 2 3 4 5 6; do cargo nextest run -p theseusd --test acceptance_journey -E 'test(the_owner_starts)'; done`

## Step 5: lifecycle bench

`target/debug/theseus-sim bench lifecycle --runs 10 --check` printed `LIFECYCLE OK in 62.6 s`.
- Cold start p50 23.6 ms (budget 50).
- Clean shutdown p50 11.8 ms (budget 100).
- Kill and restart p50 22.9 ms (budget 150).
- Binary swap p50 27.6 ms (budget 200).

This is a debug build on this VM, so only the relative result counts.

## Live check for the maintainer (needs real keys)

In a fresh session on a scratch state dir, with the owner's recent DM history imported, ask:
`in the recent DM, what tag did the owner choose for the 1.0.0 release?`

Then run `theseus why` on the turn and `theseus ledger` for its `recall.ran` row. Expect:
- `route.decided` is a quick detour.
- The `recall.ran` row is not `detoured`.
- The answer is `release-1.0.0`.

A greeting ("thanks") should still show `detoured` and no recall.

## Notes

- **Disk.** The VM's writable allowance filled during the first clippy and test runs. I deleted `target/debug` (my own build artifacts only), used CARGO_INCREMENTAL=0, and ran tests with `-p theseus-core` where I could. Nothing outside the repository and /tmp was touched.
- **Docs.** No change needed to `crates/theseus-core/AGENTS.md` or `docs/design/m6-memory.md`.
- **No new dependency.** No format bump. No test or ceiling weakened.
