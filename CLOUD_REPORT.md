# Cloud report: wal-sync (theseus-ljgm, theseus-c67g)

Branch `cloud/20261005-wal-sync`, from `main` at faaa9df6 (store format 17). The session ran from 09:35 to about
11:05 UTC on 2026-10-05. There is no format change: the frame layout and the marks are as they were.

| Commit | What |
| --- | --- |
| 65e7d449 | store: an open that finds its last segment syncs the log's directory with its first frame (theseus-c67g) |
| 9a086d12 | follow: a frame cut behind the cursor and written again is a rewind (theseus-ljgm) |
| e1a4eddf | store: a failed fdatasync's frames are cut back off, and never come back (theseus-ljgm) |
| 6ba717aa | store: the writer's view of a failed sync, through its thread (theseus-ljgm) |

## Step 1: c67g, the found segment's name

**What I found.** The issue matches the code. `append_segment` returned no directory for a last segment it found,
so a process that appended to a segment its dead predecessor created never synced the log's directory until it
rolled.

**What I changed (65e7d449).** `append_segment` now returns the log's directory for a found last segment as well,
so the first sync after the open syncs it once. The open itself still syncs nothing. I updated theseus-store's
AGENTS.md invariant and the wal.rs module docs.

**How I proved it.**
- `wal::tests::a_new_segments_name_is_synced_before_its_first_frame_is_reported_durable`: its reopen now expects 0
  directory syncs before a frame, 1 after the first append, and still 1 after the second.
- The new test `wal::tests::an_open_that_finds_its_last_segment_syncs_its_name_with_the_first_frame`:
  - A creator writes without a sync and is dropped. The reopen lists `[wal dir]` alone in `unsynced_dirs`.
  - Write and sync: `dir_syncs` is 1. A second sync: still 1.
  - An open that creates segment 1 in a directory it finds: 1, as today.
- `store::tests::a_new_stores_own_name_is_synced_with_its_first_frame`: a store opened again now counts 1 (it
  counted 0).
- **Planted revert** (`Vec::new()` back in `append_segment`): those three tests fail, with `left: 0, right: 1` and
  `left: [], right: ["/tmp/…/wal"]`. I restored the file, ran `touch` on it, and `git status` was clean.

**Cost.** The start path pays one more directory fsync before serving: in the sync of the kernel's startup frame,
on every start (the found segment is the usual case). I measured a directory fsync on this VM's ext4 virtio disk
over 50 runs, each with a fresh entry in the directory: 0.18 ms median, 0.59 ms p90, 4.1 ms max. Under /tmp it was
0.13 ms median. The gate's turn bench reports the disk's fdatasync p50 as 0.2 ms.

**Could it wait until after serving?** Not without a frame reported durable before it. The startup frame is
answered as durable by the sync that would carry the directory sync, and it lives in the found segment. Deferring
the directory sync means either the startup frame is not reported durable before serving, or it is reported over
a name a power loss could drop.

There is a cheaper rule I did not build, for the owner to decide: skip the directory sync when the open already
knows a position in the found segment was synced. That is the index's checkpoint, or a mark, at or past the
segment's first position. Some earlier sync then returned Ok for that segment, and under this change that sync
also synced the directory. That would make a clean restart free, and only a start after a crash would pay. Its
hole: an older build that found the segment (before this change) synced its frames without the directory, so
during an upgrade a mark could vouch for a name nobody synced. ext4's ordered mode hides that too.

## Step 2: ljgm, a failed fdatasync

**Differences from the issue (the code wins):**
- The log already had a `broken` state for a write cut short that can't be cut back off. I reuse it. The
  remedy-specific words ("a restart's open cuts the torn tail") moved into that one reason, since the new reasons
  need other words.
- I took the recommended design: cut the segment back, sync the cut, and roll the writer back.
- I found two things the brief did not name:
  1. Concurrent syncs on dup'd descriptors. `sync` clones the handle, and a writeback error is reported to one
     fsync per open file description, so two concurrent syncs could see one Err and one Ok over the same lost
     pages. The Ok one would then advance the frontier past them. Syncs now run one at a time: they hold
     `Wal::durable` from capture to answer, and a roll holds it too. Lock order is `durable` before `w`. The
     store's single writer thread never contends it.
  2. `Wal::append` called from several threads at once (restore tooling and tests). Caller B's frame can be cut
     by caller A's failed sync between B's write and B's sync, and B's own sync then returns Ok. Each successful
     cut now records its first position (`Writer::cuts`), and `append` fails a frame a cut took since its write.
     `write_timed` (the store's writer) is unchanged: it writes and syncs alone.

**The rule (e1a4eddf):**
- When a sync fails, it cuts the last segment back to the end of the last frame a sync that returned Ok covered
  (`Durable`: segment, length, total, next position), then runs `set_len` and `sync_data` on the cut. The writer's
  length, total, and next position roll back with it. `synced` is untouched.
- **After a successful cut, the log takes frames again.** The cut was synced, so the segment on disk is exactly what
  earlier Ok syncs made durable, and the next frame starts fresh at the first cut position (gapless). PostgreSQL
  panics after a failed fsync because it can't tell what it lost. Here we can: everything past the frontier, and
  it is gone. A disk whose errors persist then fails each batch in turn, each answered failed and each cut, and
  never writes past a gap. If the owner prefers broken-until-restart, it is a one-line change in `cut_back`.
- The log goes broken in three cases. Each message names the sync's error and the remedy.
  - The cut or its sync fails.
  - The cut is not in the segment written last. That can't happen, but it is checked.
  - The open found frames past the last position known synced, and no sync of this log has covered them yet. Their
    writer may have answered them Ok, so they can't be cut. If they were dirty pages a failed fdatasync dropped, no
    later sync may claim them either. This log's own frames are still cut first. The first sync that returns Ok
    clears it.
- **A roll's sync** (`sync_all` of the segment it leaves) fails the same way and is cut the same way, and no segment
  is created. I checked it: unsynced frames are only ever in the last segment. A roll syncs the old segment before
  it creates the next, and the frontier moves to the new segment's start only after that sync returns Ok.
- **Nothing new on a sync that succeeds**, apart from one uncontended mutex (`durable`) held across it, and a copy of
  four numbers into it.

**The test hooks:** `fail_next_sync` and `fail_next_cut`. They are cfg(test), beside `cut_next_write`, and grouped
as `Writer::planted`.

**The tests, wal/tests/sync.rs.** The first five failed on main's code with only the hook added: 5 of 5 failed,
with the store test as the sixth. On main, `fail_next_cut` was a no-op.
- `a_failed_syncs_frames_never_come_back`: on main the segment kept the batch (`left: 176, right: 56`). Fixed: the
  frames are cut, the next frame takes position 2, and a reopen reads `kept, after, more` at 1, 2, 3 with nothing
  cut.
- `a_later_good_sync_never_claims_a_failed_batch`: on main the frames at or before `synced` were `kept, failed, good`.
  Fixed: `kept, good`.
- `a_failed_sync_at_a_roll_cuts_the_segment_it_leaves`: on main the roll's failure was simply returned, and the
  frames stayed. Fixed: segment 1 is cut back to frame 1, no segment 2 is created, the next frame is position 2 in
  segment 1, and a reopen reads frames 1 and 5.
- `a_failed_cut_leaves_the_log_broken`: on main the next append succeeded. Fixed: it is refused with "takes no more
  frames … could not be cut back off … a sync that failed".
- `a_failed_sync_over_frames_the_open_found_unsynced_breaks_the_log`: on main nothing was cut (`left: 3, right: 2`).
  Fixed: its own frame is cut, the log is broken with "found at its open", and after a restart and one good sync, a
  failed sync cuts and the log goes on.
- `an_append_whose_frame_another_failed_sync_cut_fails` (new with the fix): the cut log's arithmetic.

**Planted reverts.** For each, I restored the file, ran `touch` on it, and `git status` was clean.
- **The cut skipped** (no `cut_back` in `sync`): five tests fail, `a_failed_syncs_frames_never_come_back`,
  `a_later_good_sync_never_claims_a_failed_batch`, `a_failed_cut_leaves_the_log_broken`,
  `a_failed_sync_over_frames_the_open_found_unsynced_breaks_the_log`, and the store test. The frames come back at
  the open.
- **`synced` advanced past a failed batch** (`fetch_max(last)` before the cut): the same five fail. In the store
  test the reopen itself refuses: a later frame's mark claims a cut position, which the open reads as rot.
- **A failed cut not marking the log broken**: `a_failed_cut_leaves_the_log_broken` fails.
- **The directory left out after an open**: see step 1.

## The followers (9a086d12)

**The miss, first.** theseus-follow could miss a cut. A running follower checked nothing behind its cursor. When a
cut's replacement frames were the cut ones' size, its offset landed on a frame boundary holding exactly the next
position it expected. It read on, and silently missed the records that now hold the positions it had already read.
`tests::a_follower_that_read_frames_since_cut_meets_a_rewind` shows it on main's code: the read returned Ok where a
rewind was due. A cut that left the file shorter than the cursor was already a rewind.

**The fix.** After each read, `still_read` runs one 12-byte pread of the header of the frame the read began after,
and checks its length and crc against the cursor's mark. A cut before or during the read is `Rewound` (the index
tender rebuilds; the durability tender ships again), whatever the read made of the bytes, a would-be `Corrupt`
included. A cut after the read is met at the next read, by the frame the read ended on. The cursor stays where the
read began. A frame rewritten with identical bytes passes, which is harmless.

The cost is one open and one pread per read call. I touched no line of the durability-fixes lane's bounded read:
the check wraps `read`. Expect a textual merge near `read()` if theirs changes it.

**Positions are reused** after a cut, so the index tender, which follows unbounded, depends on this check.

## Step 3: the writer's view (6ba717aa)

`store::tests::a_batch_whose_sync_fails_is_answered_failed_and_never_comes_back` runs through the writer thread.
Three appends queue behind a held `appending` lock and go in one batch whose one sync fails:
- Each append is answered with "a sync that failed". The syncs counter is unchanged, none of the three is indexed,
  and `last_position` is 1.
- The next append is answered `[2]`.
- A reopen holds exactly `(1, before), (2, after)`.

On main it failed at the open's read-back (`left: [..one..]` past the expected two). The store's writer code is
untouched: it answers as before.

## Not done: a failed index write after a good sync

It still fails every frame of its batch, though they are durable, and they come back at the next open. Two ways to
fix it:
- (a) Answer them Ok once indexed: on an index error, keep the batch's entries and retry `index.apply` (or replay
  the WAL from the index's last position) before the next batch, answering only when indexed. That is the honest
  route, since the frames are durable.
- (b) Cut them. That means a cut of synced frames (another sync), a rewind for followers, and a `synced` that must
  move back, which the marks forbid. Already-written later marks would claim the cut positions.

I'd recommend (a), with a bounded retry, then a broken state that refuses frames until a restart replays the tail.
It belongs to store.rs's writer, which batch 6 renumbers, so I left it.

## Proof offline (all on this VM)

- **Gate** (`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`), before each commit: fmt, shape, features,
  clippy, cockpit, test build, and reader rule were green. The suite failed only on cases outside this change, and I
  then ran the phases after it myself (protocol types, turn bench with frames 5 and 9 within budget, deny offline):
  green each time. Suite counts:
  - c67g: 2,574 run, 34 failed.
  - follow: 2,575 run, 33 failed.
  - ljgm: 2,581 run, 33 failed.
  - writer test: 2,582 run, 34 failed.
- **The suite's failures:**
  - The 33 known L1 tests (theseus-sandbox contract and `spawn_100`, theseusd `sandbox`), every run (theseus-pv6i).
  - `theseusd::mcp_server an_acting_call_waits_and_the_cli_approves_it`, in the c67g run only. It saw `waiting` on
    `input` with `last_reply: null`, and passed 5 of 5 alone. A daemon status race under load (a worktree build ran
    beside that gate). It is not on the known list or the flaky list; I did not chase it.
  - `theseus-core term::tests::sh_runs_a_command_and_ctrl_c_interrupts_one`, in the last run. It is a PTY read
    timing out at 15 s and failed 1 of 3 alone. It is not on either list and does not touch the store: a sibling of
    theseus-1n2y.
  - `the_deadline_stops_the_whole_tree_too` (theseus-g11i) did not fail.
- **theseus-store, theseus-follow, theseus-index, and the durable tender**: all passed in the last gate. That is 74
  store, 13 follow, 67 index, and 15 `durable` tests in core.
- **Under load** (four `yes >/dev/null` at nice 0, the tests at nice 19; I used `yes` because this environment
  refused `sh -c` loops): theseus-store, which holds the wal and store tests, passed 74/74 five times out of five, in
  75 to 88 s.
- **Crash test**, `target/debug/theseus-sim crash-test --restarts 8 --writers 4`:
  - 20 iterations × 8 restarts, seed 1, tear true, 28.2 s: OK, zero committed records lost.
  - `--iterations 50 --seed 7`: OK in 70.0 s, zero lost.
- **Python under bench/**: not touched.

## Live checks for the maintainer

1. **Crash test.** On the install build, from a scratch directory:
   `cd "$(mktemp -d)" && theseus-sim crash-test --iterations 50 --restarts 8 --writers 4 --tear true`
   It should end `CRASH TEST OK: 50 iterations × 8 restarts, 4 writer(s) … zero committed records lost`.
2. **The directory sync at a restart.** Use a scratch daemon on a fresh state dir, with its own `--config`,
   `--socket`, and `--state-dir` (`S=$(mktemp -d)`):
   1. Start it once: `theseusd --config $S/theseus.toml --socket $S/sock --state-dir $S/state &`. Wait until
      `theseus --socket $S/sock health` answers, then `kill -KILL <that pid>`.
   2. Start it again under strace:
      `strace -f -tt -y -e trace=fsync,fdatasync -o $S/trace theseusd --config $S/theseus.toml --socket $S/sock --state-dir $S/state &`.
      Wait for health, then run `theseus --socket $S/sock shutdown`.
   3. Read it: `grep -n -m3 -E 'fdatasync\(.*\.seg>|fsync\([0-9]+</.*/wal>' $S/trace`.
   - The first `fdatasync(N</…/store/wal/000000001.seg>)` (the startup frame) should be followed, before any other
     segment fdatasync, by exactly one `fsync(M</…/store/wal>)` of the log's directory.
   - Main's build shows no such fsync on the second start.
   - Then run `theseus-sim bench lifecycle --runs 10 --check`, which should stay within budgets. The extra cost is
     one directory fsync, about 0.2 ms here.

## Left open, and choices for the owner

- **After a successful cut, the log takes frames again.** I chose this over broken-until-restart, for the reasons in
  step 2. It is one line to change.
- **The `found_unsynced` rule** breaks the log on a failed sync before the first good one after a crash's open. It is
  strict, and it means a restart.
- **The optional skip of c67g's directory sync** when a checkpoint or mark already vouches for the segment: see step
  1.
- **The failed index write**: see above.
- **Docs to change at review:**
  - the spec's Part III item for theseus-ljgm and theseus-c67g, and §6's WAL text, if it lists what a failed sync
    does;
  - `docs/technical-overview.md`'s WAL section: a failed sync cuts its frames, and followers meet it as a rewind;
  - `docs/status.md`.

  theseus-store's AGENTS.md is updated in the commits.
