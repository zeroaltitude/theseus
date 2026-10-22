# CLOUD_REPORT: tail5-discord (theseus-bs8dl)

Branch `cloud/20261021-tail5-discord`, code commit `dcf7db8` on base `f1fc9b03`. Store format unchanged (49). No new
dependencies. Commits are unsigned (no key on this VM).

## Ceilings
No file went over its ceiling. `runtime.rs` is still 3,499 lines: two `mod` lines became `pub(crate) mod` in place (`forum`, `task_home`), nothing was added.

## What changed

1. **The archive tick's rig test** (`tests_forum_archive::a_read_post_leaves_on_the_archive_ticks_own_run`).
   The tick's period is overridable per test thread (`archive::tick_for_tests`, `#[cfg(test)]`, as `v2.rs` does for the card window). The test shortens it to 200 ms,
   ends and clears a task, moves its post's end back 25 h in the binding's own in-memory list, and waits for the post to be archived and locked. Only the
   tick can do that: the start sweep is past and the daily one is a day away.
2. **The home's rig test** (`tests_task_home::the_home_standing_removes_the_dms_board`).
   The home is off at the start (`task_home = ""`), so the DM draws its board. The test then reaches the daemon's own `Shared` and looks for the home
   (`look_for_task_home`). The DM's board is gone, the board is at the home, and no other DM message left. The registry that reaches the binding is `task_home::daemon_for_tests`
   (a `cfg(test)` map from core to `Weak<Shared>`, filled in `start_task_home`).
3. **An earlier run's board** (`courier/board.rs`, `courier.rs` +1 field `seek_stale`, `courier/v2.rs` one `pub(super)`).
   `board_moved` with no board of its own and no earlier pin lookup sets `seek_stale`. `drop_stale_board` then calls `earlier_board`, which reads the DM's pins
   once (V2 first, then text) and deletes the one it finds. Test: `courier::tests_v2::an_earlier_runs_dm_board_goes_when_the_home_stands`.

**How the board is recognized, and why nothing else matches.** In the DM's pins only: a message by a bot (`author.bot`) that is either a V2 message (flag `IS_COMPONENTS_V2`) whose components carry the board's
Clear button id, or a text message beginning `📋 **Task board**`. Both finders are the ones that find the board after a restart today. The test pins another bot message
and asserts it is unchanged (serialized form identical, still pinned). It runs on each start once (the `BoardMoved` send fires when the home first stands), after serving, off the start path.

## What the owner sees change
Only a stale board pinned by an earlier run leaving the DM. No message's words, layout, order or emoji changed.

## Plants (each restored, `touch`ed, `git status` clean)
- skip the lookup (`seek_stale = false`): `an_earlier_runs_dm_board_goes_when_the_home_stands` fails at `tests_v2.rs:351` (board still there).
- drop the `BoardMoved` send: `the_home_standing_removes_the_dms_board` fails (waits out `until`, 100 s, "the DM's board is gone").
- drop `self.archive_tick();`: `a_read_post_leaves_on_the_archive_ticks_own_run` fails the same way ("the post is archived").

Runs: `cargo nextest run -p theseus-discord`: 617 passed, 0 failed. The three new tests together pass in 2.5 s. No separate run under load.

## Keel guard
`python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet`: empty output, exit 0.

## Gate
`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 CARGO_INCREMENTAL=0 scripts/gate.sh`: keel, fmt, shape, features, clippy (`-D warnings`), clippy-probe, cockpit, test build all ok.
Suite: 7,849 run, 7,815 passed, 34 failed, 53 skipped. All 34 are the known root-VM L1 cases (theseus-pv6i): theseus-sandbox `clause_01..12`, `egress_18b_*`, `exit_status_and_signals`, `scratch_is_reported_and_discarded`,
`sigterm_is_forwarded_to_the_command`, `spawn_100`, and theseusd's L1 tests (`a_cancel_of_an_l1_job...`, `a_job_that_cannot_start_says_why`, `l1_argv_routes_a_call_to_l1`, `the_jobs_bench_l1_row`, `health_reports_the_last_real_l1_launch`, etc.). No Discord test failed.
After the suite I ran the rest by hand: generated TypeScript unchanged (`git status` clean after the suite), `cargo deny --offline check`: advisories, bans, licenses and sources ok. The benches were not run (lane gate). The disk filled twice during the gate (the VM's allowance); I cleared `target/debug` and set `CARGO_INCREMENTAL=0`.

## For the integrator
- `docs/status.md`: "The DM's board pinned by an earlier run is removed when the task home stands (theseus-bs8dl); rig tests now hold the forum archive's own tick and the home's `BoardMoved`."
- Discord `AGENTS.md`: one sentence added in the task-home section (committed).

## Follow-ups
- The `BoardMoved` send happens once at each start the home stands, so the pin lookup costs one or two GETs of the DM's pins per start (after serving).
- The unmatched-license-allowance warning in `cargo deny` is pre-existing.
