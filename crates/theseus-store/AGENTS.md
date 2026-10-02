# theseus-store

The keel (spec §6, Part II M1): an append-only WAL of checksummed, length-prefixed atomic frames, which is the
truth, and a redb index rebuilt from it. Read by theseus-kernel, theseus-core, theseusd, and theseus-sim, and by
the reserved theseus-follow, theseus-index, and theseus-exam.

## What's here

- `wal.rs`: segment files of atomic frames, with torn-tail truncation.
- `index.rs`: the rebuildable redb projection, and `move_aside` for an index that is not a database.
- `record.rs`: records and their kinds. `kinds::SCHEMAS` is the schema this build writes for each kind, and the
  newest it reads.
- `store.rs`: the `Store` contract the kernel writes through, and `WalStore`, which composes the WAL and the index.
  `MANIFEST.json` names every kind written, with its newest schema (`MANIFEST_FORMAT`).

## Invariants

- **The version rule** (P5b; Part III F4a). A new record layout bumps its kind in `kinds::SCHEMAS`, lands with the
  reader for the layout it replaces, and brings a test that reads the old layout. A new record kind goes into the
  table too. A change to the frame or record encoding bumps `MANIFEST_FORMAT`, with its reader. In review, a new
  field on a record's struct, nested ones included, without a bump is a finding. The number itself is assigned
  when the step lands on `main`, never in a lane.
- **Writers never set a schema**: `NewRecord::json` and `NewRecord::bytes` take the table's number.
- **A migration adds nothing before serving** (§9). Old layouts are read in place, and a start writes no manifest:
  a store an older binary wrote is marked only when this build first appends a newer record. A build older than the store refuses it, before anything is written ("install the
  newer theseusd"). There is no rolling back: keep the newer binary, or restore a copy taken before the upgrade.
- **The open reads only the WAL's tail**, from the frame after the index's checkpoint. The rest is checked after
  serving by core's `store-verify` thread, and a corrupt frame there is refused and loud.
- **A checkpoint takes the store's `appending` lock alone**, so the position it claims is synced and indexed.
  Don't take a checkpoint while holding that lock.
- **Never delete what can't be rebuilt.** An index that is not a database is moved aside as
  `index.redb.bad-<unix ms>`, under the file's lock, and rebuilt from the WAL (Item 17). A real database that fails
  another way is refused: its recovery is `theseusd restore` from the WAL directory.
- **A second opener waits** for the store's lock up to `LOCK_WAIT` (3 s), then fails.

## Tests

- The crate's own tests, and `theseus-sim crash-test` (a real SIGKILL against a worker process, torn tails, a full
  disk). Use `--restarts 8`, so the stores cross checkpoints and most restarts take the tail-only path.
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
  `Durable` does (Item 19).
