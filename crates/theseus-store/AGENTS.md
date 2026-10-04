# theseus-store

The keel (spec §6, Part II M1): an append-only WAL of checksummed, length-prefixed atomic frames, which is the
truth, and a redb index rebuilt from it. Read by theseus-kernel, theseus-core, theseusd, and theseus-sim, and by
the reserved theseus-follow, theseus-index, and theseus-exam.

Key modules: `wal.rs`, `index.rs`, `record.rs`, `store.rs` (`MANIFEST_FORMAT`). Read by: kernel, core, theseusd, sim.

## What's here

- `wal.rs`: segment files of atomic frames, with torn-tail truncation.
- `index.rs`: the rebuildable redb projection, and `move_aside` for an index that is not a database.
- `record.rs`: records and their kinds, and the header's frozen schema field (`FROZEN_SCHEMA`).
- `store.rs`: the `Store` contract the kernel writes through, and `WalStore`, which composes the WAL and the index,
  with its writer thread. `MANIFEST.json` names the store's one format number (`MANIFEST_FORMAT`).
  `blocking` runs a wait for the disk without holding a runtime worker.

## Invariants

- **One writer** (theseus-vni9). Every append goes through `WalStore`'s `store-writer` thread: the caller hands it
  the frame and waits (in `blocking`, so no runtime worker waits), and the writer writes every frame queued, syncs
  once for all of them, indexes them in one transaction, and answers each. The answer comes after the index, so a
  caller's lock spans its read to its frame indexed (K1). Never append from the writer itself, and never hold
  `appending` while you append: the writer needs it shared.
- **A segment's name is as durable as its frames** (theseus-xprd). A roll syncs the segment it leaves; the `sync`
  that makes a new segment's first frame durable then syncs the log's directory before it returns, and a new
  log's first sync syncs the directory holding the log too. A file made durable needs its directory synced too. The
  store's open adds the holder of every directory it created, the store's own included (theseus-gf00), to those the
  first frame's sync makes durable.
- **The version rule: one format number** (P5b; Part III F4a; theseus-ptx1, Tier 7's 7.9 as Eddie amended it). Any
  step that adds a field to a stored record (nested ones included), or changes the frame or record encoding, bumps
  `MANIFEST_FORMAT`, so an older binary refuses the newer store. It lands with the reader for the layout it replaces
  (serde defaults, or a reader such as `Execution::from_stored`) and adds a sample of that layout, as literal bytes
  its build wrote, to theseus-core's `tests_layouts` when the layout is on disk somewhere. A new record kind needs no
  table. The number itself is assigned when the step lands on `main`, never in a lane: a lane that adds a field says
  so in its report.
- **A record's header schema is frozen**: `NewRecord` has none, the WAL writes `FROZEN_SCHEMA` (0), and nothing reads
  the field. Records from before one store format keep their kind's old number there.
- **Old layouts are read in place.** An open writes no manifest. The writer's first frame into a store an older
  build wrote moves its manifest to this build's format first, durably, once: two syncs, and on a daemon that
  frame is its kernel's startup frame, so the first start after an upgrade that bumps the format pays them before
  serving. A store only read keeps its format. A build older than the store refuses it, before anything is written
  ("install the newer theseusd"). There is no rolling back: keep the newer binary, or restore a copy taken before
  the upgrade.
- **The open reads only the WAL's tail**, from the frame after the index's checkpoint. The rest is checked after
  serving by core's `store-verify` thread, and a corrupt frame there is refused and loud.
- **The open makes nothing durable** (theseus-ptx1): the tables' creation is a non-durable commit, and a replay
  takes no checkpoint. The WAL is the truth, so a crash before the next checkpoint only replays the tail again; the
  replayed tail counts toward the next periodic checkpoint, which the writer takes after serving. A start then pays
  only redb's own sync at its open, and a start after a crash no repair when the run made no durable commit.
- **A list read skips a refused record** (R4, theseus-15g): `read_many` and `scan` leave out a record whose read is
  refused (a corrupt frame), log it once, and count it in `StoreStats::refused_records`, which health shows. A read
  of that record alone (`get`, `latest_by_key`) is still refused. `repair.rs` takes a frame that does not check
  whole from a copy of the store, and `theseus_core::restore::repair` swaps the repaired WAL in.
- **An open cuts a bad frame in the last segment as a torn tail only past every position known synced**
  (theseus-gt12). The index's checkpoint claims only synced positions (it takes `appending` alone, and the writer
  indexes a batch only after its sync), so the store's open passes it as `WalConfig::synced_to`, and a repair
  passes the checkpoint of the store it repairs (`index::checkpoint_of`, read-only). A bad frame at or before it is
  refused, naming `theseusd restore --repair`, and nothing is cut. Past it the bytes cannot tell rot from a batch
  torn before its sync (a later frame of the batch can reach the disk whole), so the frame is cut, with all after
  it, and `Recovery::cut` says whether a whole frame followed. Both walks (every segment, and the tail after the
  checkpoint) decide alike. What this still cuts: rot after the last checkpoint, and any bad last frame of a log
  opened with no index (a full replay with no index, `theseusd restore`). Don't open a store you suspect is
  corrupt with its index moved aside; repair it from a copy first.
- **A checkpoint takes the store's `appending` lock alone**, so the position it claims is synced and indexed: the
  writer holds it shared from a batch's first write to its index. Don't take a checkpoint while holding that lock.
  The periodic one (every 1,000 records) runs on the writer, after it has answered the batch that crossed the
  mark, so no append's call pays it (theseus-avvb); an append that queues meanwhile waits for it, as it would
  wherever it ran. A stop's checkpoint (`checkpoint_for_close`) syncs nothing of
  its own: redb's close, a durable commit, makes it durable (theseus-02k). Only a durable checkpoint advances
  `durable_to`, so a durable one after it is never skipped as free.
- **The terms and sums are a projection, whole only when marked** (theseus-lv2). An open with a `Projection`
  keeps each keyed record's terms (the kernel's: an execution's state, …) and numbers (the core's: a session's
  turns, tokens, and cost, added up per kind) with every append and replay. A checkpoint marks them whole under
  the projection's name; a writer with no projection (an older build, a tool) moves the checkpoint alone, and the
  next projected open leaves them to `build_terms`, after serving, never at open. Until they are whole, the store
  answers `latest_by_terms`, `count_by_terms`, and `totals` with `None`, and the reader reads every record.
- **The history check starts at the last one's mark** (theseus-0dq): `verified.*` in the index's meta, written
  with the next checkpoint. Its frame is checked again first; a frame that no longer checks or holds other
  positions sends the check back to the log's start, which finds what is wrong.
- **Never delete what can't be rebuilt.** An index that is not a database is moved aside as
  `index.redb.bad-<unix ms>`, under the file's lock, and rebuilt from the WAL (Item 17). A real database that fails
  another way is refused: its recovery is `theseusd restore` from the WAL directory.
- **A second opener waits** for the store's lock up to `LOCK_WAIT` (3 s), then fails.

## Tests

- The crate's own tests, and `theseus-sim crash-test` (a real SIGKILL against a worker process, torn tails, a full
  disk). Use `--restarts 8`, so the stores cross checkpoints and most restarts take the tail-only path, and
  `--writers 4`, so the kills land among frames the writer commits together.
- A test holds the writer by taking `appending` for writing, as a checkpoint does: what is appended meanwhile
  queues, and goes in one batch (`the_writer_commits_every_queued_frame_with_one_sync`).
- `crates/theseus-core/tests/fixtures/store-460a35b` is a store an older binary wrote (its README says how it was
  made). Tests copy it. Never open it in place.
- `crates/theseusd/tests/versions.rs` holds the real daemon to the rules, including
  `a_clean_stop_closes_the_index_and_the_next_start_repairs_nothing`.

## Traps

- **`index_repaired: true`, or a replay, after a clean stop is a bug**: something outlived the runtime while
  holding the store. Since the open makes nothing durable, a run with no durable commit leaves no repair behind,
  so the replay is the surer sign. Keep statics and detached threads on a `Weak`.
- After `theseus shutdown` the daemon holds the store for 10 to 17 ms more (redb's close). Wait for the process to
  exit before you read the store's files.
- A kill keeps the page cache, so a crash test can't show a missing sync. Test the syncs themselves, as `restore`'s
  `Durable` does (Item 19), and as `wal`'s `a_new_segments_name_is_synced_before_its_first_frame_is_reported_durable`
  counts the directory syncs.
