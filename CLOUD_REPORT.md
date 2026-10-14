# CLOUD_REPORT: checkpoint-at-stop (theseus-8ub95)

Branch `cloud/20261013-checkpoint-at-stop`.

## What I found

- **The lead is confirmed in its mechanism, not reproduced in its cause.** `checkpoint_for_close` ran a
  non-durable checkpoint (`Durability::None`) and counted on redb's close to make it durable. Anything that still
  holds the store at exit keeps redb from closing, and the stop's checkpoint is then lost; the next open reads the
  last durable one (the periodic checkpoint). A planted leak (see below) reproduces exactly the journal's symptom:
  `replayed_into_index: 12` after a clean stop, from the checkpoint before.
- **I did not find the real holder.** With the owner's services on in a test daemon (MCP server with `memory = true`,
  the index tender, the ledger chain and its pin; Discord and voice left off, no turn run, no MCP client connected) the
  store closes cleanly: `index_repaired: false`, replay 0, on main too. So the new test
  `a_clean_stop_with_every_service_on_leaves_nothing_to_replay` PASSES on main; it is a guard, not the failing
  test the brief asked for. Candidates I read and ruled out by reading: `ledger-pin` (holds the core by `Weak`,
  takes the store per page and drops it), `store-verify` (holds a `HistoryCheck`, which holds no store handle),
  `blob-sweep` (`Weak`). Not ruled out: a connected MCP client's streams, Discord/voice, a turn's tasks. The fix below
  makes the outcome independent of the holder, and the daemon now NAMES a leak (below), so the owner's journal will
  say whether one exists after this lands.
- **The 385 ms open at install #28 is probably redb's repair, not the replay or the format move** (a hypothesis:
  I can't measure the owner's disk). If a leaked handle stopped redb closing, the next open repairs the index file
  (`index_repaired: true`, which I see in the leak test), and that walks the index. A 26-record replay and a
  3-sync manifest move cannot reach 385 ms. So finding and joining the real holder, not only the durability, is what
  returns the start to ~24 ms. On this VM: index open 1.0 to 1.6 ms, replay of 9 records 0.3 ms, manifest move
  1.4 ms (debug build, a tiny store), so the VM can't show the owner's numbers.

## What I changed

Commit 7ff5ba7 (all of the above).

1. `WalStore::checkpoint_for_close` is durable by itself (`checkpoint_as(true)`). The drop-time
   `checkpoint_at_close` (for frames a late writer appended after the stop's checkpoint) stays non-durable: the
   close follows it for certain, so theseus-02k's saving holds there. Cost: one durable commit at the stop.
   `stop_record` (the shutdown's first call) and `finish_stop` (the last) both call it; the second is free when
   nothing came between them (`durable_to == last`).
2. `theseus_store::leak` (new module): `watch` remembers the store by `Weak`, `holders` counts handles alive. The
   stop calls `watch`; `theseusd`'s `main` logs a warning after `drop(rt)`: "handles on the store outlived the
   runtime, so it is not closed (the stop's checkpoint was durable by itself)", with the count. That is the leak's
   name for the owner's journal.
3. A debug-build plant, `THESEUS_TEST_LEAK_STORE` (outbox.rs, beside `planted_late_write`): forgets one store handle.
4. Debug spans for the open (4): `store: open phases` (index open ms, replay ms, replayed), and the manifest move
   now logs its `ms`. 
5. Tests: `crates/theseusd/tests/stop_checkpoint.rs` (two), and the store's
   `a_checkpoint_for_close_is_durable_without_the_close` (replaces `..._is_made_durable_by_the_close`; same 7
   assertions in spirit, see keel). AGENTS.md of theseus-store and two comments updated.

## How I proved it

- Daemon test with a leaked handle, planted revert (`checkpoint_as(false)` in `checkpoint_for_close`):
  `a_handle_that_outlives_the_runtime_costs_the_next_start_no_replay` FAILS with `replayed_into_index: 12,
  index_repaired: true`; restored and touched, it passes.
- Store test, same revert: `a_checkpoint_for_close_is_durable_without_the_close` fails
  (`durable_to` 5, not 6). The test copies the store's files while it is still open, as a kill leaves them.
- The "handle fix" half of the planted-revert request cannot be shown: I found no real leak to fix. The guard is
  the no-leak assertion in the first daemon test (the log must not contain "outlived the runtime"), which fails if
  anything starts to hold the store at exit.
- `cargo nextest run -p theseus-store`: 114 passed. `-p theseusd --test stop_checkpoint`: 2 passed.
- Lifecycle bench, `target/debug/theseus-sim bench lifecycle --runs 10`, this 4-core VM, indicative only
  (before -> after, p50): cold start 22.4 -> 23.6 ms; from config copy 24.3 -> 22.8; clean shutdown with jobs
  11.1 -> 11.2; clean shutdown with Discord post in flight 71.2 -> 73.4; SIGKILL restart 28.4 -> 23.7; binary swap
  27.7 -> 25.4. The `health ... MISSED` (5 ms budget, 4.4 p50 before) line is the debug build, same before and after.
  The extra durable commit is cheap on this VM's disk; on the owner's (~14 ms) the shutdown stays inside 100 ms
  (p50 was 35.5 ms) — the maintainer should read the real number from the first install.

## Install #28's start (item 4)

- Timed on this VM (debug build, a 40-record store): format-29 store's first open: index open 1.6 ms, replay 0,
  manifest move 1.4 ms, serving 15 ms. Format-32 after clean stop: index open 1.0 ms, serving 11 ms. After a kill
  -9 (9 records to replay): index open 1.4 ms, replay 0.3 ms, serving 12 ms. The format move is a rewrite plus two
  syncs and the log directory's sync (3 syncs; ~40 ms at 14 ms each on the owner's disk), the replay of 26 records
  is sub-millisecond of CPU plus one non-durable commit. Neither makes 385 ms; redb's repair of an unclosed file
  is the candidate (needs the owner's store to confirm: start the same copy under `THESEUS_LOG=theseus_store=debug`
  and read `index_open_ms`).
- A start after this fix serves near #27's 24 ms only if redb closed at the previous stop, i.e. only if no handle
  leaked; the fix removes the replay either way, not the repair.
- `ledger.pin` (ledger_pin.rs): runs once per store. `pin_with` returns `Already` when META `ledger.history_pinned`
  exists, and writes that key with the pin's row in one frame. It runs on its own `ledger-pin` thread, begins
  `START_AFTER` = 2 s after serving, pages with `quiet_blocking` pauses, holds the core by `Weak` and a store clone
  only for one page at a time. Nothing serving needs waits on it. The 2.11 s is the one-time read of the old
  history on the first start after the chain landed; it does not repeat. Not changed.
- theseus-4ra3: already done by theseus-ptx1 ("No checkpoint here": an open that replayed takes no checkpoint, the
  writer's periodic one covers it after serving). Nothing to build; the replayed tail counts toward the periodic one.

## Live check for the maintainer

1. Install the build; `THESEUS_LOG=info,theseus_store=debug`.
2. `theseus shutdown`, then start. Journal: `store open`, no `store: index rebuilt from WAL`, and health's store
   phase `replayed_into_index: 0`, `index_repaired: false`. Repeat three times with the MCP client connected and a
   Discord turn in flight.
3. If the stop's log contains `handles on the store outlived the runtime` with a count, that is the real leak:
   the count and `ps -L` of the exiting daemon name the holder. File that as the follow-up (join it).
4. `theseus-sim bench lifecycle --runs 10` on the owner's machine for the budgets.

## Keel findings expected

None acked. `scripts/keel-guard.py` reports one "the same test ... renamed and rewritten" note for
`a_checkpoint_for_close_is_made_durable_by_the_close` -> `..._is_durable_without_the_close` (information only; its
assertions are kept: 7 -> 7, the `!index_repaired` one now on a reopen after the close). Findings: 0.

## Not done / uncertain

- The real holder was not found (a connected MCP client, Discord, voice and a running turn were not in my test).
- theseus-zsu9 untouched. Voice is not enabled in the test (no stand-in Deepgram wired in this session).

## Gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 CARGO_INCREMENTAL=0 scripts/gate.sh` (the disk filled once from
incremental caches; I deleted `target/debug/incremental`, rebuildable). Door, keel (0 findings), fmt, shape, features,
clippy, cockpit, test build, reader rule: pass. Suite: 4383 run, 4349 passed, 34 failed:
- 33 are the known L1 failures (theseus-sandbox contract tests and `spawn_100`, theseusd's `sandbox` tests): this VM
  runs as root (theseus-pv6i).
- 1 is not on the known list: `theseus-discord tests_process::an_answer_the_post_writes_after_the_thinking_is_still_the_answer`
  (times out at tests_gateway.rs:161, "the thinking's create, held, and the reply's post"). It fails the same,
  alone, with my changes stashed (main as cloned), so it is not mine. It touches no store code.
The phases after the suite: protocol types (no protocol change; `cockpit/src/protocol.gen` clean); lifecycle (run by
hand, above; only the debug build's `health` 5 ms budget is MISSED, 4.4 ms p50, the same before my change); jobs and
turn benches skipped (`THESEUS_GATE_NO_BENCH=1`).
