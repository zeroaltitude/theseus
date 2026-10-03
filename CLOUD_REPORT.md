# Cloud report: theseus-gt12, the WAL's open cuts acknowledged frames after a bad one

Branch `cloud/20261003-wal-rot`, from `main` at `a59b7c1`. One code commit, `d19c733`, then this report.
Started 20:00 UTC, report written 20:55 UTC (2026-10-03).

## What I found

**The writer's batch has no bound.** `Inner::write_loop` (crates/theseus-store/src/store.rs) takes the first job,
then `jobs.try_iter()`: every job queued, of any size. Nothing limits frames or bytes per sync, and one frame can be
up to about 4 GB. Each appender waits for its answer, so in practice a batch holds at most one frame per appending
thread, but the code enforces no limit. So the "batch's bound" candidate fails. A whole frame any distance after the
bad one could belong to the same unsynced batch. Adding a bound now would not cover logs that older builds wrote.

**The index's checkpoint claims only synced positions.** `checkpoint_as` takes `appending` for writing. The writer
holds it shared from a batch's first `wal.write` to its `index.apply`, and `commit` indexes a batch only after
`wal.sync()` returns Ok. So when a checkpoint reads `wal.last_position()`, every frame up to it has been written and
its batch's sync has returned. A sync covers every frame written before it, and frames are written in position
order. So **any frame whose first position is at or before the checkpoint was synced.** That is a slightly stronger
test than the brief's "the checkpoint claims a position at or past the whole frame that follows". It also covers a
synced last frame that rots with nothing after it.

The checkpoint's limits, all of them also in the commit message and the store's AGENTS.md:
- With `fsync: false` (benches, `open_unsynced`), a checkpoint claims positions that were never synced. Then a
  power loss can tear a frame at or before the checkpoint. Main cut that frame and then refused anyway (below). The
  new code refuses without cutting. Both refuse, so nothing gets worse.
- A batch whose sync failed leaves its frames written, and `next_position` past them. A later checkpoint claims
  them only after a later batch's sync succeeded, and that sync covers them. (fsyncgate aside.)
- The checkpoint says nothing about frames after it. **Rot after the last checkpoint** (up to 1,000 records, or
  whatever a crash left past the last durable checkpoint) **is still cut.** The tail-only open checks exactly those
  frames.

**A second bug on main, which this commit fixes.** When the frame that holds the checkpoint's own record went bad,
`tail_after` could not find that record and returned `None`. The full walk then *cut* that frame as a torn tail
(`set_len`, `sync_all`), and only after that did `open_once` bail with "index checkpoint N is past the WAL's last
position". The open refused, but it had already destroyed the acknowledged frame. The new store test pins this
case.

**The no-index case has no evidence.** A full replay with the index gone (`cp = 0`) and `theseusd restore` (a
staging store with no index) see only the WAL's bytes. As the brief says, those bytes cannot tell rot from a torn
batch. So no rule built on the WAL alone can refuse there without sometimes refusing a real torn batch after a power
loss. See "Design for what is left" below.

## What I changed (`d19c733`)

- `WalConfig::synced_to: u64` (default 0) is a position known synced, from outside the log's bytes.
- `torn_or_rot` in wal.rs is one decision, which both walks call: the full walk in `open_from` and `tail_after`.
  - For a frame that does not check (short, bad magic, absurd length, crc mismatch) in the last segment, it first
    scans for a whole frame after it. The scan searches for MAGIC from the bad frame's start plus one. A candidate
    counts only if its first record's position is past the expected one and it passes `check_frame` (crc, records
    decode, positions in sequence).
  - If the bad frame's first position is at or before `synced_to`, the open **refuses** with
    `WalError::Corrupt { segment, offset, .. }`. The error names the position, the checkpoint, any whole frame
    found after it, "nothing was cut", and `theseusd restore --repair`. No bytes change.
  - Otherwise it cuts, as before. `Recovery::cut: Option<Cut>` reports the segment, offset, bytes, the position
    that was due, and `whole_after: Option<(offset, first position)>`. A cut with a whole frame after it also logs
    a `warn` that says it was either a torn batch or rot past every known-synced position.
- `tail_after` used to return `Ok(None)` (go back to the full walk) for any bad frame that was not the last
  segment's torn tail. It now returns the refusal error directly. The full walk would reach the same frame and the
  same decision, so this only saves a second walk.
- The store's open (`Inner::open_once`) passes `synced_to = max(cfg.synced_to, checkpoint)`.
- `theseus_store::index::checkpoint_of(path)` reads an index's checkpoint through redb's `ReadOnlyDatabase`, which
  writes no byte. It returns `None` when the index is missing, is dirty (redb would need a repair), or has no
  checkpoint.
- The repair (`theseus_core::restore::repair_with`) reads the checkpoint of the store it repairs and opens the
  staging store with `Store::open_synced_to(staging, synced_to)`. That function is new and is the only new public
  function in core. A staged WAL that still holds a bad frame the store had synced is now refused at its open. It
  no longer opens short and relies on the `cut > 0` check after the open. I left that check in as a second guard.
- `restore` (not repair) is unchanged. It still opens with no evidence. I chose not to read the source's index for
  `restore`. A backup taken by copying a running store's directory can hold an index copied *after* its WAL. That
  index's checkpoint could then claim positions past a torn copy, and `restore` would refuse a backup that restores
  today.
- crates/theseus-store/AGENTS.md: I replaced the "theseus-gt12, open" invariant with the rule as built, including
  what it still cuts.
- No new dependencies. Cargo.lock is unchanged. The record and frame formats are unchanged, and there is no
  MANIFEST_FORMAT bump.

## How I proved it

New tests (crates/theseus-store):
- `wal::tests::a_synced_frame_gone_bad_in_the_last_segment_is_refused_not_cut` (rot): five synced frames, one
  record each, with frame 3's body flipped. With `synced_to` 3 and 5, the open refuses at frame 3's offset. The
  message names position 3, the whole frame after it, and the repair, and the segment is byte-identical afterwards.
- `wal::tests::a_batch_torn_before_its_sync_is_cut_though_a_whole_frame_follows` (torn batch): the same five
  frames with frame 3's last 16 bytes zeroed and frames 4 and 5 whole, with `synced_to = 2`. The open cuts from
  frame 3, `last_position` is 2, and `cut.whole_after` is `Some((frame 4's offset, 4))`.
- `wal::tests::torn_tail_is_truncated_and_earlier_frames_survive` (plain torn tail, extended): a short last frame
  past every synced position (`synced_to = 10`) is cut as before. `truncated_bytes` is 52 and `cut.whole_after` is
  `None`.
- `wal::tests::the_tail_only_open_refuses_rot_and_cuts_a_torn_batch_alike`: `open_from` with a checkpoint at
  position 2. It asserts `checked_from.is_some()`, so the test runs the tail-only walk. That walk refuses with
  `synced_to = 5` and cuts the torn batch with `synced_to = 2`.
- `store::tests::a_checkpointed_frame_gone_bad_is_refused_before_anything_is_cut`: a real `WalStore` checkpointed
  at 5, with record 5's position byte flipped. The open refuses, and the segment is byte-identical (main cut it
  first).
- The existing `restore::tests::a_repair_takes_the_bad_frame_from_a_copy_and_keeps_every_later_write` still passes.

Planted reverts. After each one I restored the file from a copy, ran `touch`, and checked `git status`:
1. **Refusal off** (`if false && expected <= synced` in `torn_or_rot`): the rot test FAILED (it opened) and the
   store test FAILED at `"nothing was cut"`, which is main's cut-then-refuse. The torn-batch and torn-tail tests
   passed.
2. **The naive rule** ("refuse whenever a whole frame follows": `expected <= synced || whole_after.is_some()`): the
   torn-batch test FAILED (it refused). The rot, torn-tail and store tests passed.
3. **The tail walk without the evidence** (`tail_after` passing 0): the tail-only test FAILED.
4. **The repair's patch planted out** (`repaired.push((seg, live))`): the repair test FAILED. The staged open
   refused with: `opening the repaired WAL in …/store.repairing-…: opening store …: opening WAL: corrupt frame in
   segment 1 at offset 1297: crc mismatch in the last segment, at position 5, which position 6 of the index's
   checkpoint says was synced: … a whole frame follows it at offset 1696 (position 6) … nothing was cut …
   theseusd restore --repair`. Before this change, the staged open succeeded with 3 of 5 sessions.

Under load (four `while :; do :; done` loops at nice 0, the tests at `nice -n 19`): `cargo nextest run --workspace
-E 'package(theseus-store) | test(/restore::/)'` ran five times, and each run passed 49 of 49 tests. I stopped the
loops by their pids.

**FAST.** The healthy path gains only a copied `u64` in the config. The scan and the decision run only inside the
`Bad::Torn` arm. To measure it, I ran `theseus-sim bench lifecycle --sessions 2000 --runs 20` against two daemons
built here, `main`'s store and this commit's, interleaved A/B five times each. "store" is the daemon's own timing
of its store open, the p50 over 41 starts:

| | store open p50, five runs (ms) | median | cold start p50 median | SIGKILL→restart p50 median |
|---|---|---|---|---|
| before (main) | 4.75 5.07 4.76 4.03 4.83 | 4.76 | 19.3 ms | 21.9 ms (a second set, after first) |
| after | 3.99 5.03 5.15 5.41 5.08 | 5.08 | 19.1 ms | 22.1 ms |

The two sets overlap within the VM's noise. In the first set the kill phase's median looked about 2 ms worse after
the change (20.7 against 22.6). I re-ran it with the order swapped, and the medians came out at 21.9 before and 22.1
after, so I read that gap as drift.

## The gate

`THESEUS_GATE_LOCK=inner THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`: fmt, shape, clippy and the reader rule passed.
The suite ran 1,691 tests: 1,689 passed and 2 failed, both on the known list:
- `theseus-core tests_output::the_cores_output_matches_its_golden` failed all 3 tries. The diff is at line 1041,
  the `context.compiled` ledger line: the request size is one byte off on this VM (theseus-6a7o).
- `theseus-sandbox::contract clause_09_limits`: root is exempt from `RLIMIT_NPROC` (theseus-pv6i).

I then ran the phases after the suite by hand, and all passed: protocol types, `theseus-sim bench turn --check
--runs 5 --burst 0` (5 frames, budget 5), `cargo deny --offline check` (a fresh `cargo deny fetch`), web lint and
build, cockpit lint, test and build, and the web dist check. The lifecycle and jobs benches were skipped
(`NO_BENCH`); every lifecycle run above reported each of its phases within budget. wal.rs is 1768 lines, under the
2,500-line shape limit.

## What is left, and the design for it

Still cut without refusing: rot after the last checkpoint, and any bad last frame when there is no index (a full
replay with the index gone, `theseusd restore`). The cut is reported: `Recovery::cut` and its `whole_after`, plus a
`warn` log line. Neither `StoreStats`, health, nor `RestoreReport` surfaces `whole_after` yet. Restore's report and
its `store.restored` ledger row carry only `truncated_bytes`.

Closing those cases needs a stored fact, which I did not build, since a format change is the maintainer's call:

- **Proposed: a synced mark in each frame's header.** The writer stamps each frame with the last position whose
  sync had returned Ok when the frame was written, which is the end of the previous batch. A whole frame Y found
  after a bad frame X, with `Y.synced_to >= X's first position`, proves X was synced. That makes it rot, and the
  open refuses. This works with no index, in `restore`, and in the tail after the checkpoint. It also covers one
  more case without help: rot in the frame before the one at the log's end. Only rot in the very last batch stays
  undecidable, and the checkpoint covers that when it reaches it. Cost per commit: 8 bytes per frame, no extra
  syscall, no extra sync. It changes the frame encoding, so `MANIFEST_FORMAT` bumps, and the reader keeps reading
  the old frame layout (where it knows nothing, as today).
- Rejected: **bounding the batch** (frames or bytes per sync) and treating a whole frame more than one bound
  away as proof. The bound would hold only for logs written after it lands. A single frame has no size limit. And
  the five-small-frame rot case would still be undecidable.
- Rejected: **a sidecar "synced to" file**. It costs a write and an fsync per commit.
- Unverified idea: the index's non-durable entries past the checkpoint. The writer indexes a batch only after its
  sync, so any position the index holds was synced. If redb keeps non-durable commits across a process crash (not a
  power loss), `max(indexed position)` would extend the evidence into the tail after a SIGKILL. I did not check
  redb's behaviour, so I did not use it.

Uncertain: `checkpoint_of` returns `None` for a dirty index (redb's `RepairAborted`). A repair of a store whose
daemon crashed then falls back to no evidence. That is safe, because the repair's own `cut > 0` check still refuses
afterwards.

## Docs the maintainer may want to change

- The spec's Part III item for theseus-gt12: the rule (the checkpoint as evidence), the cut-then-refuse bug, what
  is left, and the synced-mark design.
- `docs/status.md`: the store's recovery line, if it says an open "cuts a torn tail".
- `crates/theseus-store/src/lib.rs`'s crate docs say recovery "truncates a torn tail". That is still true, but it
  could say "past the checkpoint". I left it alone.
- The `restore` module's doc ("recovery … cuts a torn tail") could add that a restore has no index, so it cannot
  refuse rot (see above).
