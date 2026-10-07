# Cloud report: daemon-stops (theseus-yg1y, theseus-jo7f, theseus-autz, theseus-xva3)

Branch `cloud/20261006-daemon-stops`. Session started 2026-10-07 01:17 UTC. All four steps are done, each a gated
commit, pushed as it landed.

**Base.** The branch's task commit (b5b4fe65) sits on d279767f. The resume note asked for main as it is now, so the
branch's first commit is a merge of origin/main at 57f265f2 (01c8f7ca), made before any work and never repeated.
Main had moved theseus-store's store.rs (imported-skip's keyed reads), which step 4 changes, so this was the only way
to build on it without a force-push. Drop or flatten that merge as you like when you join the branch.

| Step | Commit | Issue |
| --- | --- | --- |
| 1. `--stdio` stops on `shutdown` | d1193873 | theseus-yg1y |
| 2. A restart in place ends as a stop does | db48bbc2 | theseus-jo7f |
| 3. A stop ends an import or erase between frames | f2a5b09e | theseus-autz |
| 4. An upgrade syncs the log's directory before its manifest moves | e292da30 | theseus-xva3 |

## Step 1: a `--stdio` daemon stops on `shutdown` (theseus-yg1y), d1193873

**Found.** The stop's wake is `core.shutdown.notify_waiters()` (`wake_after_answer`, and `Core::stop` for a
restart), which wakes only the waiters registered at that moment. `serve_socket` registers one before its loop. The
stdio arm's `select!` had no such branch, so the daemon answered `shutdown`, wrote its `server.stopping` row and
checkpoint, and went on serving.

**Changed.** theseusd main.rs's stdio arm registers `core.shutdown.notified()` (pinned and `enable()`d) before
`after_serving` is spawned and anything is served, and ends on it, as `serve_socket` does. About the restart: the
restart sets `restart` (the watch) and then calls `stop()`, which notifies. So `restart_requested()` is already
`Some` whichever branch wins (`restart_asked` or the stop), and `exit` picks `Exec` either way.

**Proved.**
- `versions.rs::a_stdio_daemon_stops_on_the_shutdown_method`: `shutdown` answers `{"ok": true}`, the daemon exits 0,
  the next open of `store-stdio` replays 0 and repairs nothing, and the last `server.stopping` row's data is null
  (the method's own; a signal's row names the signal).
- Plant: the branch removed. The test fails at the rig's 40 s guard (`no the stop in 40 s`, versions.rs:119,
  40.37 s). The file was restored and touched, and `git status` was clean.

**Live check (maintainer).** Use a scratch state dir and a scratch config (the template made safe, as `common::safe_note`
does):
```sh
S=$(mktemp -d)
{ printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"health"}' '{"jsonrpc":"2.0","id":2,"method":"shutdown"}'; sleep 10; } \
  | (time theseusd --config <scratch.toml> --state-dir $S/state --stdio) ; echo "exit $?"
{ printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"health"}'; sleep 2; } \
  | theseusd --config <scratch.toml> --state-dir $S/state --stdio | jq -c '.result.startup[] | select(.name=="store") | .detail'
```
The first command should print two answers, the second `{"ok":true}`, then `exit 0` within about a second, long
before the 10 s `sleep` closes stdin. Before this fix it ran on until stdin closed. The second command should show
`"replayed_into_index":0,"index_repaired":false`.

## Step 2: a restart in place ends as a stop does (theseus-jo7f), db48bbc2

**Found.** As the brief said: `Exit::Exec` shut the runtime down with a 500 ms bound and then exec'd. On `--stdio` it
never waited for the stdout thread.

**Changed.**
- `main` now ends both ways on one path. It drops the runtime, which waits for its tasks, so the store closes. Then
  `stop_phase("runtime dropped")`, then `stdio::flush(500 ms)` for `--stdio`, then either `Ok(())` or `exec_self`.
  The match on `Exit` now chooses only the last step.
- `stdio::planted_hold` now runs in the socket arm too, just before `serve_socket`, so the test covers both modes.
  Its doc says so.
- The tests' `--stdio` client moved from versions.rs to `tests/common/stdio.rs` and versions.rs uses it. A reader
  thread feeds a channel, so each answer is awaited with a bound: a request the old image's stdin thread reads during
  a restart is never answered. It also has `hold`/`release` for its reads and `pending()` (the pipe's unread bytes,
  `FIONREAD`).
- theseusd's AGENTS.md trap paragraph covers the restart, the stdio stop wake, the plant in both modes, and the
  shared client.

**Proved** (config_copy.rs, which drives restarts and is the smaller file):
- `a_restart_in_place_closes_the_store_before_the_exec`: the copy at a $100 limit and the vault's note at $42.5, with
  `THESEUS_TEST_HOLD_CORE_MS=1500`. On the socket and then on `--stdio`, the next image's `store` phase shows
  `replayed_into_index` 0 and `index_repaired` false. The stdio half waits for the log's second "serving protocol on
  stdio" before it asks anything, then stops with `shutdown`, which also exercises step 1. Plant (`shutdown_timeout`
  kept for `Exec`, with no flush, as on main): the socket half fails with `replayed_into_index: 12`. With the socket
  assert disabled for one run, the stdio half fails with `replayed_into_index: 13`.
- `a_stdio_restart_in_place_keeps_every_line_whole`: deterministic, as follows.
  1. The vault is held (`hold` file) and the client handshakes, then holds its reads.
  2. The client sends about 150 KB of `health` requests (29 of about 5 KB). That is past the 64 KB pipe and short of
     what the pipe, the relay's 64 KB buffer and the socket pair (212 KB `wmem_default`) hold together, so the core
     never blocks mid-line.
  3. The client waits until the relay thread `stdio-out` is blocked in `write(1, …)`, read from
     `/proc/<pid>/task/*/syscall`. The pipe's fill alone was not a usable signal: 16 slots can fill below 65,536
     bytes.
  4. The client releases the vault and waits for the debug line `stop … runtime dropped` (`THESEUS_LOG=info,
     theseus_core::startup=debug`). It then reads again **100 ms later**: well inside the flush's 500 ms, and long
     after an exec with no flush would have come.
  5. Every line must parse and end in `\n`, each backlog answer must come once and be one of the backlog ids, more
     than 64 KB of them must arrive, and a `health` sent after the second "serving protocol on stdio" must be answered
     with `config.restarted` set.

  Plant (no flush on `Exec`): it failed 5 runs of 5 with `a line cut at the restart`, a health answer cut at about
  4 KB and glued to the next image's answer. The fix passed 5 of 5. Without the 100 ms wait the plant failed only 3
  runs of 4, because the client's release raced the exec. That is why the wait is there.
- A request in flight at the restart gets no answer. The old image's stdin thread may read it and dies with the exec,
  as a socket client's request in flight at a restart does. The test sends its last request only once the new image
  serves.
- Under load (four busy loops at nice 0, the runs at nice 19): versions.rs and config_copy.rs passed 5 runs of 5, all
  14 tests each run, about 39 s a run. The final round on the head is below.

**Live check (maintainer).** This needs a vault config and its fake or real `op`.
1. Start once with `--stdio` on a scratch state dir so the copy is kept, then stop it.
2. Change the note in the vault (for example `[kernel] spend_limit_usd`).
3. Start again while a client streams requests and checks each line:
   ```sh
   (i=0; while :; do i=$((i+1)); echo "{\"jsonrpc\":\"2.0\",\"id\":$i,\"method\":\"health\"}"; sleep 0.005; done) \
     | theseusd --config op://<vault>/<item>/notesPlain --state-dir $S/state --stdio 2>$S/log \
     | while IFS= read -r l; do printf '%s' "$l" | jq -e . >/dev/null || echo "CUT: ${l:0:80}"; done
   ```
   No `CUT` line should appear. `grep -c 'serving protocol on stdio' $S/log` should be 2. Once it has restarted, a
   `health`'s store detail (as in step 1) should show `replayed_into_index` 0 and `index_repaired` false. A few ids
   around the restart will have no answer (see above).

## Step 3: a stop ends an import or an erase between frames (theseus-autz), f2a5b09e

**Found.** As the brief said. Both ran whole in one `spawn_blocking`, and the frame boundaries waited on pressure with
the plain `quiet_blocking(BOUND)`.

**Changed** (core import/write.rs and rpc/import.rs only):
- `import_batch_unless` and `erase_unless` take `stopping`. The RPC passes `core.outbox.stopping()`. At each frame
  boundary they call `quiet_blocking_unless(BOUND, &stopping)` and return at the first boundary that finds the stop.
- The erase's read of the tag's sessions (now `sessions_of_tag`) looks every 256 records. A stop there returns having
  written nothing.
- A stopped erase breaks to its usual tail, which writes one closing frame. That frame holds the tag's `TagCounts`
  (with `erased` advanced by what was erased) and its `import.erased` row for that much. The erase returns those
  nodes, and the RPC asks the index to forget them. A batch already writes the tags' counts with every frame, so a
  stopped batch adds up as it is.
- `import_batch` and `erase` keep their signatures (`|| false`), so tests_imported.rs, tests_learning.rs and the
  other callers are untouched. The `_in` forms take the frame's record cap (`Frame::cap`): see the tests.
- **The answer is an error, not a result field.** I chose this for three reasons. At a stop the runtime's end
  usually cancels the connection's writer first, so few clients hear the answer at all. A client that does hear it
  must not take a cut batch for done. And it needs no protocol type or regenerated TypeScript, which kept me out of
  the shared protocol files. The code is `INTERNAL`. The message names what was written ("… N of its lines were read
  and M episodes imported (K frames written); send the batch again to finish it (what was written is skipped)", and
  for the erase "… S sessions (N nodes) were erased and counted (K frames written); run the erase again to finish the
  tag (what was erased is skipped)"). Its `data` is the result for what was written. The CLI prints the message
  as-is.
- To keep clippy's 100-line limit: `Frame::new(cap)`, `count()` (an episode into its tag's counts), and
  `sessions_of_tag()` were extracted.

**Proved** (import/tests.rs):
- `a_stop_ends_a_batch_between_frames_and_a_rerun_finishes_it`: three frames' worth, stopped once a frame has been
  written (the store's `frames_appended`). It returns after 1 frame with the tag's counts equal to what it wrote. The
  rerun skips exactly what was written, and the tag's counts equal one whole batch's (sessions, nodes computed from
  the episodes, no erased rows).
- `a_stop_ends_an_erase_between_frames_and_a_rerun_finishes_it`: stopped after its first frame, the erase returns
  after 2 frames (the closing one). Its counts and rows equal what it erased, and it returns that many node ids. The
  rerun finishes, and counts and rows sum to one whole erase's. Every session carries its receipt.
- `a_stopped_batch_or_erase_answers_what_it_wrote_and_that_a_rerun_finishes_it`: over the protocol, with the stop
  begun (`outbox.stop_sending()`), a batch of 1,100 episodes (past 4,000 records, the real cap) answers the error after
  one frame. The erase then stops in its read and answers "0 sessions (0 nodes) … (0 frames written)".
- **Frame cap in the tests:** the first two tests use a cap of 400 records and 300 episodes, not 4,000 records and
  2,500 episodes. At full size they took 1.5 s and 2.1 s idle, but over 120 s under the load recipe in a debug build:
  nextest killed them in 5 runs of 5. Every unstopped boundary also waits up to 10 s on CPU pressure by design, so the
  tests import their setup in batches under one frame. The protocol test keeps the real `FRAME_RECORDS`.
- Plant (plain `quiet_blocking`, every stop check off): all three fail. The batch returned frames 3, not 1. The erase
  returned frames 3, not 2. The protocol call returned `Ok`.
- Under load: 5 of 5 passed for `import::` and `rpc::tests_imported` (19 tests). The protocol test is the slowest,
  above 60 s under load before I trimmed it (`SLOW` but passing). `rpc::tests_imported::the_session_lists_read_no_
  imported_session_and_answer_as_before`, imported-skip's test and not mine, is also over 60 s under load. Final
  round below.

**Live check (maintainer).**
```sh
theseus --socket $S/sock import openclaw <episodes.jsonl>      # several thousand episodes, one tag
theseus --socket $S/sock import erase --tag <tag> --why test & sleep 1; kill -TERM <scratch daemon pid>; wait
```
The daemon should exit within about a frame's time (well under a second for 4,000 records), not after the whole
erase. Restart it, run `theseus import erase --tag <tag>` again (it finishes), then `theseus import list`: the tag's
`erased` equals its `sessions`, and the tag's `import.erased` rows (`theseus ledger tail` filtered by that kind) sum
to the same.

## Step 4: an upgrade syncs the log's directory before its manifest moves (theseus-xva3), e292da30

**Changed.**
- `upgrade_manifest` calls `Wal::sync_own_dir()` (when `fsync` is on) before `write_manifest`. `sync_own_dir` syncs
  the log's directory, counts it in `dir_syncs`, and takes the directory off the first frame's list.
- A failed sync fails the upgrade: the manifest does not move, and `commit` answers each job with the error, through
  the existing `upgrade_manifest` error path.
- `open_once` no longer pushes the log's directory for `behind`, so **the first frame no longer carries it.**
- theseus-store's AGENTS.md is updated in two places.

**FAST.** No start pays more. It is the same one directory sync, now before the manifest's two syncs instead of after
the frame's fdatasync, and only at an upgrade's first write (on a daemon, its kernel's startup frame, as before).

**Proved.**
- `store::tests::an_upgrade_syncs_the_logs_directory_before_its_manifest_moves`: two synced batches, the manifest at
  `MANIFEST_FORMAT - 1`, opened (0 directory syncs), `fail_next_sync()`, one append (fails). The manifest is current,
  `dir_syncs` is 1, and the failed frame was cut (last position 2). The next open vouches with `Vouch::Mark`, and an
  append there syncs no directory.
- The older-format test's count stays 1, with its message now "before the manifest moved, and not again with the
  first frame".
- Plant (the sync left to the first frame, as on main): the new test fails ("the log's directory was synced before
  the manifest moved", 0 ≠ 1).
- theseus-store's suite passed, 84 of 84, along with theseusd's versions.rs.
- Not tested: a failing directory sync itself. There is no plant for the directory sync, only for the fdatasync, and
  the failure path is `upgrade_manifest`'s existing `?`.

**Live check (maintainer).** Nothing beyond the suite. Optionally, on a copy of a store, set its manifest's `format` to
one less, start a scratch daemon on it, and `strace -f -e trace=fsync,fdatasync -p <pid>` across its first write. The
log directory's `fsync` should come before the manifest's rename.

## The gate

Before each commit, with every other step's changes stashed: `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1
scripts/gate.sh`. fmt, shape, features, clippy, the cockpit's lint/test/build, the test build and the reader rule
passed each time.

The suite failed only on the 33 known L1 tests every time (3,027 → 3,033 tests run, 2,994 → 3,000 passed, 24
skipped): 20 in `theseus-sandbox` (`bench spawn_100` and the 19 contract tests) and 13 in `theseusd::sandbox`. These
are the root-daemon refusal, theseus-pv6i. No other test failed or timed out in any gate run.

I then ran the phases after the suite myself each time, and all passed:
- protocol types: unchanged (no protocol type changed);
- `theseus-sim bench turn --check --runs 5 --burst 0`: plain 5 frames, tool 9, both within budget;
- `cargo deny --offline check`: advisories, bans, licences and sources all ok. `cargo deny fetch` worked at setup.

The lifecycle and jobs benches are skipped under `THESEUS_GATE_NO_BENCH`.

**Disk.** Halfway through, the session's disk allowance filled (a theseus-core rebuild failed with ENOSPC). I deleted
`target/debug/incremental` twice, which cargo rebuilds. Nothing outside the repository was touched.

## Left, uncertain, and for the owner

- **Docs to write at review** (I edited none):
  - Part III's item for these four issues.
  - `docs/status.md`.
  - theseusd's invariant "Every clean stop is one path" now holds for `--stdio` and for a restart's store close and
    flush. Its text needs no change; the trap paragraph is updated.
  - If the spec describes `import.erase` as one pass, it should say a stop ends it at a frame boundary with an error
    naming what was written.
- **A stop during an import or erase writes after the stop's checkpoint.** The frame in flight, and the erase's
  closing frame, come after `stop_record`'s checkpoint, so the next start replays those records (at most two frames).
  That is correct, since redb's close still makes them durable, but it is a replay the "replays nothing" rule
  otherwise avoids. Checking the stop before a full frame's append would not avoid it either, because the counts must
  land. I think this is the right trade.
- **The stdio whole-lines test reads `/proc/<pid>/task/*/syscall`.** It uses `libc::SYS_write`, so it is portable
  across Linux architectures, but it needs `/proc` access to the daemon (the same uid). It also relies on the debug
  stop-phase line's text "runtime dropped".
- **The import test cap** (`_in` with a 400-record cap) is test plumbing in the writer's signature, `pub(super)`. The
  alternative was the full 4,000-record size, which does not run under load in two minutes.

## Final round on the head (e292da30), under load

The load recipe was four `while :; do :; done` loops at nice 0 and the runs at nice 19, killed by their pids. I ran
versions.rs, config_copy.rs, `import::` and `rpc::tests_imported` together (33 tests a run) in 13 runs, in rounds of
5, 4 and 4:
- 11 runs passed all 33 tests.
- **2 runs (the first round's 1st and 4th) failed one test each,
  `config_copy::the_copy_serves_the_next_start_and_a_changed_note_restarts_the_daemon_in_place`**, at
  `Rig::stop`'s 10 s bound (config_copy.rs:155, "theseusd did not stop"), after about 32 s.
- I don't have the daemon's log for those two failures: that round kept only the summary lines.
- The same test alone under load passed 5 of 5 (about 17 s a run). The next 8 combined runs passed, with full
  output kept, so I could not reproduce it.

It is a pre-existing test, not on the brief's list. It is a timing bound (a stop within 10 s), not a negative
assertion. Its stops are plain `shutdown`s on the socket (`Exit::Done`, whose path this branch leaves as it was:
`drop(rt)` already). This branch changes the in-place restarts it makes earlier (now `drop(rt)` before the exec), but
each of those is waited out by `until` before the stop that failed. My best reading is CPU starvation of a 10 s bound,
with three daemons and the import tests' heavy work running at nice 19 beside four busy loops. I cannot rule out a
slow stop after a restart without the log.

To reproduce, run `scratchpad`-style rounds of the same filter (`cargo nextest run -p theseus-core -p theseusd -E
'test(/^import::/) | test(/^rpc::tests_imported/) | binary(versions) | binary(config_copy)'`) under the recipe, and
keep the output. On the owner's 16-core machine it should not starve. The other tests in that set passed every run.
`rpc::tests_imported::the_session_lists_read_no_imported_session_and_answer_as_before` (imported-skip's test) is
reported `SLOW` (over 60 s) in most loaded runs but always passed.

Earlier rounds, per step: versions.rs with config_copy.rs passed 5 of 5 under load (step 2). The import tests and
tests_imported passed 5 of 5 under load (step 3, before the protocol test was trimmed; the trimmed version is in the
13 combined runs above).

## Gate result

Each of the four commits: the gate passed every phase but the suite, and the suite failed only the 33 known L1 tests
(theseus-pv6i: `theseus-sandbox`'s `bench spawn_100` and 19 contract tests, and 13 `theseusd::sandbox` tests). The
phases after the suite (protocol types, the turn bench's frames, and `cargo deny --offline check`) passed when I ran
them by hand. No timing test failed in any gate run. The last gate run was on e292da30 (3,033 tests: 3,000 passed,
33 failed, 24 skipped).
