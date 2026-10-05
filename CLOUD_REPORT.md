# Cloud report: wal-mark-skip (theseus-3q29)

Branch `cloud/20261005-wal-mark-skip`, from main at 60b43fb6 (store format 20). Started 20:22 UTC, report at 21:25 UTC.

## Step 1: the skip (e4818dc6)

**Found.** The code matched the brief. `open_from` already computes `synced = max(synced_to, walk.mark)`, capped at
the last position found. Since a frame's mark is below its own position, `synced >= first position of the found
segment` is the whole rule. The walk didn't keep that first position, so `Walk` now has a `first` field, set where
`Walk::segment` starts a segment from offset 0. After either walk it holds the first position of the segment walked
last, which is the found segment.

What differs from the brief:
- `open_from` went over clippy's 100-line limit (110). I moved the walk over every segment (the `None =>` arm)
  into its own function, `walk_every`, beside `tail_after`, without changing it. Only `cfg.synced_to` was renamed
  to the parameter `synced`. Its tests didn't change, and the crash test still passes.
- In the tail-only open, the checkpoint's record in the found segment vouches by itself, whatever `synced_to` says,
  as the brief asked. The store always passes the checkpoint as `synced_to` anyway, so this branch only matters for
  a caller that passes `at` without `synced_to`. A test opens with `synced_to = 0` to hold the branch.
- Both existing tests keep their 1, checked: c67g's test (its creator never synced), and xprd's reopen (its found
  segment 4 holds one frame, whose mark is segment 3's last position).

**Changed.**
- `wal.rs`:
  - `Recovery::vouched: Option<Vouch>`, where `Vouch` is `Checkpoint | Mark`.
  - `fn vouch` makes the decision. An empty found segment is never vouched for: its first position is past the last
    position found.
  - `append_segment` takes `vouched` and leaves the log's directory out of `unsynced_dirs` when it is set.
  - When vouched, the open logs it at debug level: `wal: the found segment's name is vouched for`, with `segment`
    and `by`.
  - Module docs (the c67g paragraph) and the `unsynced_dirs` doc are updated.
- `crates/theseus-store/AGENTS.md`: the invariant "A segment's name is as durable as its frames" now describes the
  skip.
- New `wal/tests/vouch.rs`, five tests. Each counts the directory syncs at the first frame, then at the second frame:
  - two synced batches in the found segment, no checkpoint: `Mark`, `unsynced_dirs` empty, (0, 0);
  - one batch in the found segment, with the checkpoint at its position: `Checkpoint`, (0, 0);
  - the tail-only open, with the checkpoint in the found segment (`synced_to = 0`): `Checkpoint`, (0, 0). With the
    checkpoint in segment 1 and a mark in the found segment: `Mark`, (0, 0);
  - a mark and a checkpoint that vouch only for the segment before: `None`, (1, 0), in both walks;
  - an empty found segment 3, with every position checkpointed: `None`, (1, 0), in both walks.

**Proved.**
- `cargo nextest run -p theseus-store`: 79/79.
- Planted reverts, each restored with `touch` and checked with `git status`:
  - Any mark vouches (`synced >= first` changed to `synced > 0`). Fails xprd's
    `a_new_segments_name_is_synced_before_its_first_frame_is_reported_durable` (its reopen) and
    `vouch::a_mark_for_the_segment_before_vouches_for_nothing`.
  - The found segment is never synced (`if vouched || true`). Fails c67g's
    `an_open_that_finds_its_last_segment_syncs_its_name_with_the_first_frame`, xprd's test, and the vouch tests for
    the empty segment and the segment before.
  - Vouching ignored, as on main (`append_segment(.., false)`). Fails the three vouch tests whose count is 0 (mark,
    checkpoint, tail-only).

## Step 2: the close (d87ffc32)

**Found.** `open_once` reads `behind` before the WAL, as the brief says. When `behind` is set, the log's directory now
joins `name_dirs` in `sync_with_first_frame`. That call dedups, so a log with no segment, whose open created
segment 1, still syncs its directory once. `a_new_stores_own_name_is_synced_with_its_first_frame` did not move.
Its reopened store's segment holds one frame, whose mark is 0, and `with_checkpoint_every(0)` leaves no checkpoint,
so nothing vouches and the count stays 1.

**Changed.**
- `store.rs` `open_once`: six lines. `MANIFEST_FORMAT` is untouched, and nothing new is stored.
- `crates/theseus-store/AGENTS.md`: one sentence.
- New test in `store/tests.rs`: `a_store_an_older_format_wrote_syncs_its_logs_name_with_the_first_frame`.
  - A store written with two batches, then reopened: `Mark`, 0 directory syncs.
  - The manifest rewritten to `MANIFEST_FORMAT - 1`: 1 directory sync at the first frame, 1 after the second, and
    the manifest moves to the current format.
  - Reopened once more: 0.

**Proved.**
- theseus-store: 80/80.
- Planted revert, `behind` ignored (`if behind && false`): the new test fails with "the log's directory, with the
  first frame", left 0, right 1. Restored and touched.

**Uncertain. The window, plainly:** format 20 began one join before c67g (d8ac9b54, then d50e6f2f). A store last
written by a build in that window has a manifest that reads "not behind", but its marks were written without any
directory sync for a found segment. On the first start after this branch, such a store skips the sync it would need.
That only matters if, in that window, a segment's creator died before its first sync and the later process then
appended to it, followed by a power loss with a filesystem that doesn't order the entry. On ext4's ordered journal
the file's fdatasync commits the entry anyway. The next format bump (route's gaps take 21) closes the window for good.
The owner decides whether that is enough.

A side note: on a behind store, `Recovery::vouched` still reports `Mark`, because it describes what the WAL found,
while the store syncs anyway. The store's own log line ("manifest moved to this build's format at its first write")
marks that start.

## Offline proof, both steps

- **Scratch daemons with strace.** Before my first change I copied main's debug `theseusd` and `theseus` aside. Each
  run used a fresh state dir and a minimal config (`[server]` state dir and socket, Discord, web and index off, one
  `env:` secret), and its second start ran under
  `strace -f -tt -y -e trace=fsync,fdatasync`. Counting `fsync(...store/wal>)` on the second start:

  | build  | after a clean stop | after a SIGKILL |
  |--------|--------------------|-----------------|
  | main   | 1                  | 1               |
  | branch | 0                  | 0               |

  - After the SIGKILL, the branch still shows 0 because the first run wrote the startup frame and then later
    batches, whose marks are at least 1, which is the segment's first position. A kill leaves no checkpoint, so a
    mark vouches.
  - Main pays c67g's sync on every start.
  - I also set the branch's clean-stop store's manifest back to `"format": 19` and started it again under strace:
    1 directory fsync, and the manifest went back to 20.
- **Crash test.**
  - `theseus-sim crash-test --restarts 8 --writers 4`: OK, 20 iterations × 8 restarts, seed 1, tear on, 0 committed
    records lost.
  - `--iterations 50 --restarts 8 --writers 4 --tear true --seed 7`: OK, 0 lost.
- **Neighbours:** `cargo nextest run -p theseus-follow -p theseus-index`: 81 passed, 4 skipped.
- **Under load:** theseus-store's tests 5 times at `nice -n 19`, beside four busy loops at nice 0 (killed by their
  pids): 80/80 each time, about 90 s per run.

## The live check, the maintainer's

1. **strace at a restart.** For each build (main's install build, then the branch's), with a fresh state dir and a
   scratch config:
   `theseusd --config C &`, wait for `theseus --socket S health`, then `theseus --socket S shutdown`. Then
   `strace -f -tt -y -e trace=fsync,fdatasync -o T theseusd --config C &`, wait for health, shut down, and run
   `grep -c 'fsync(.*store/wal>)' T`.
   Expect main 1 and the branch 0. Repeat with `kill -9 <pid>` in place of the first shutdown: expect the branch 0.
2. **Lifecycle bench.** Frozen debug builds of main (A) and the branch (B). Run
   `theseus-sim bench lifecycle --runs 10` in the order A B B A in one hold, noting IO PSI. Expect the branch's
   kernel phase at or below main's, with its budgets met.
3. **Crash test.** `theseus-sim crash-test --iterations 50 --restarts 8 --writers 4 --tear true`: every iteration
   recovers.

## Docs the maintainer may want to touch

- `docs/spec/16-part3-item-86.md:588` lists theseus-c67g as a known gap. Part III's item for this step should say
  that the found segment's sync is now skipped when vouched for, and state the format-20 window above.
- `docs/technical-overview.md`, if it describes a start's syncs: a clean restart no longer fsyncs `store/wal`.

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` ran before each commit. These phases passed: fmt,
shape, features, clippy, cockpit, test build and reader rule.

The suite phase failed both times on the 33 known L1 tests only (theseus-pv6i, root without a job cgroup):
- theseus-sandbox's contract tests and its bench's `spawn_100`;
- theseusd's sandbox tests.

The suite ran 2,761 passing for step 1 and 2,762 for step 2. No other test failed, and no flaky retry was needed.

I then ran the phases after the suite myself:
- protocol types: unchanged;
- `theseus-sim bench turn --check --runs 5 --burst 0`: plain 5 frames against a budget of 5, tool-call 9 against 9;
- `deny_check` (offline; the `cargo deny fetch` at setup succeeded): advisories, bans, licenses and sources ok.

No dependency was added, and `Cargo.lock` is unchanged.
