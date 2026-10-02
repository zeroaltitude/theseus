# theseus-store

The keel (spec §6, Part II M1): an append-only WAL of checksummed, length-prefixed atomic frames, which is the
truth, and a redb index rebuilt from it. Read by theseus-kernel, theseus-core, theseusd, and theseus-sim, and by
the reserved theseus-follow, theseus-index, and theseus-exam.

## What's here

- `wal.rs`: segment files of atomic frames, with torn-tail truncation.
- `index.rs`: the rebuildable redb projection, and `move_aside` for an index that is not a database.
- `record.rs`: records and their kinds. `kinds::SCHEMAS` is the schema this build writes for each kind, and the
  newest it reads.
- `store.rs`: the `Store` contract the kernel writes through, and `WalStore`, which composes the WAL and the index,
  with its writer thread. `MANIFEST.json` names every kind written, with its newest schema (`MANIFEST_FORMAT`).
  `blocking` runs a wait for the disk without holding a runtime worker.

## Invariants

- **One writer** (theseus-vni9). Every append goes through `WalStore`'s `store-writer` thread: the caller hands it
  the frame and waits (in `blocking`, so no runtime worker waits), and the writer writes every frame queued, syncs
  once for all of them, indexes them in one transaction, and answers each. The answer comes after the index, so a
  caller's lock spans its read to its frame indexed (K1). Never append from the writer itself, and never hold
  `appending` while you append: the writer needs it shared.
- **A segment's name is as durable as its frames** (theseus-xprd). A roll syncs the segment it leaves; the `sync`
  that makes a new segment's first frame durable then syncs the log's directory before it returns, and a new
  log's first sync syncs the directory holding the log too. A file made durable needs its directory synced too.
- **The version rule** (P5b; Part III F4a). A new record layout bumps its kind in `kinds::SCHEMAS`, lands with the
  reader for the layout it replaces, and brings a test that reads the old layout. A new record kind goes into the
  table too. A change to the frame or record encoding bumps `MANIFEST_FORMAT`, with its reader. A new field on a
  record's struct, nested ones included, without a bump fails theseus-core's `tests_schemas` (Review 2's R8),
  which records each kind's shape under its number in `tests/golden/record_schemas.txt`; `THESEUS_GOLDEN=write`
  adds a new number's shape and never changes a recorded one. The number itself is assigned when the step lands on
  `main`, never in a lane. A manifest's rewrite (`mark`) takes `appending` alone, so no frame is written while it
  moves.
- **Writers never set a schema**: `NewRecord::json` and `NewRecord::bytes` take the table's number.
- **A migration adds nothing before serving** (§9). Old layouts are read in place, and a start writes no manifest:
  a store an older binary wrote is marked only when this build first appends a newer record. A build older than the store refuses it, before anything is written ("install the
  newer theseusd"). There is no rolling back: keep the newer binary, or restore a copy taken before the upgrade.
- **The open reads only the WAL's tail**, from the frame after the index's checkpoint. The rest is checked after
  serving by core's `store-verify` thread, and a corrupt frame there is refused and loud.
- **A list read skips a refused record** (R4, theseus-15g): `read_many` and `scan` leave out a record whose read is
  refused (a corrupt frame), log it once, and count it in `StoreStats::refused_records`, which health shows. A read
  of that record alone (`get`, `latest_by_key`) is still refused. `repair.rs` takes a frame that does not check
  whole from a copy of the store, and `theseus_core::restore::repair` swaps the repaired WAL in.
- **An open cuts a bad frame in the last segment, and all after it, as a torn tail** (theseus-gt12, open): a bad
  frame there with good frames after it loses them. Until that is fixed, don't open a store you suspect is corrupt
  just to look; repair it from a copy first.
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

- **`index_repaired: true` after a clean stop is a bug**: something outlived the runtime while holding the store,
  and every start then pays redb's repair. Keep statics and detached threads on a `Weak`.
- After `theseus shutdown` the daemon holds the store for 10 to 17 ms more (redb's close). Wait for the process to
  exit before you read the store's files.
- A kill keeps the page cache, so a crash test can't show a missing sync. Test the syncs themselves, as `restore`'s
  `Durable` does (Item 19), and as `wal`'s `a_new_segments_name_is_synced_before_its_first_frame_is_reported_durable`
  counts the directory syncs.
