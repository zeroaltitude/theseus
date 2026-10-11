# CLOUD_REPORT: index-memory (theseus-agqn, theseus-uazd)

Branch `cloud/20261010-index-memory`, from 2fd1f654 (store format 26, unchanged: no stored record changed).
Started 01:50 UTC, report 03:15 UTC.

| Commit | What |
|---|---|
| f867aa4 | index: the status never waits on the embedding work, answers past a full socket, and health calls an old answer stale (theseus-uazd) |
| 8a50870 | index: the weights mapped and hashed once a file, the vector maps cut, health's index line shows the backlog and the tender's RSS (theseus-agqn) |

## Step 1: the weights mapped, hashed once a signature (theseus-agqn)

**Found.** The brief says `read_pinned` reads the whole `model.safetensors` into the heap. It doesn't: `read_pinned`
reads only `tokenizer.json` (0.7 MB). The weights already went through `read_safetensors`, which streamed the file
through a 1 MiB buffer, hashing as it went. So a load already held the tensors once, not twice. What it did cost was a
SHA-256 over all 547 MB on **every** load, including each reload after the 10-minute idle unload.

**Changed** (`mapped.rs`, `weights.rs`):
- The file is mapped read-only. Each tensor is copied out of the map 1 MiB at a time, and each window's pages are
  released after the copy (`MADV_DONTNEED`; the page cache keeps them). A load holds the f32 tensors plus at most one
  window of the file. Compute stays f32 (D-3).
- The SHA-256 is taken during the first load's copy and kept by the file's (device, inode, size, mtime), read with
  `fstat` on the descriptor that is mapped. A later load with the same signature copies and hashes nothing. A
  touched or replaced file is hashed again. A refused signature is refused again without a pass. A file that doesn't
  parse is still named by its hash.

**Proved:**
- `tests_weights::a_load_holds_the_tensors_and_not_the_file_beside_them`: a 64 MiB, 16-tensor file. The test resets
  VmHWM with `/proc/self/clear_refs` and reads the load's peak: `tensors 64 MiB · peak +66.6 MiB · anonymous +64.1
  MiB · file-backed resident 9.5 MiB`. Bound: under 1.25× the tensors.
- `tests_weights::a_file_is_hashed_once_per_signature` uses the counting seam `mapped::hashes_of`: 1 pass after two
  loads, 2 after a touch, 3 after a replace, still 3 after the refused file is loaded again.
- `tests_weights::a_file_that_is_no_checkpoint_is_named_by_its_hash`.
- Planted reverts:
  - The whole file read into the heap during the load: `a_load_holds…` failed at its peak assertion
    (tests_weights.rs:94).
  - `known` forced to `None` (hash on every load): `a_file_is_hashed_once…` failed with "the same file was hashed
    again, left: 2, right: 1".
  - After each revert the file was restored and touched, and `git status` checked.
- Live, real nomic weights, release tender, 100,000 chunks (numbers below):
  - Reload after the idle unload: **3,546 ms at the base, 556 ms at the head** (no hash).
  - First load: 6,416 ms (base, colder cache) and 3,636 ms (head).
  - RSS with the model loaded: 680 MB (base) and 677 MB (head).

## Step 2: the heap per chunk (theseus-agqn)

**Found.** The heap held, per chunk:
- an 80-byte `Row`: `Option<(u8, u32)>` has no niche, so it takes 12 bytes, and the u128 hash's alignment rounds the
  row up;
- the rows' `HashMap<u128, Vec<u32>>`: a 48-byte bucket plus a 32-byte heap block for each text's one-element `Vec`;
- the file's `HashMap<u128, u32>`: 32-byte buckets, and a second copy of a hash the file's memory already holds;
- the int8 cut (256 bytes), scale, hash and dead flag, in `Vec`s that doubling left up to 2× oversized after the
  open.

The f16 768-d vectors were already only on disk. `full()` widens 100 of them per query for the re-score, and that
buffer is transient.

**Changed** (`vectors/entries.rs`, `vectors/heap.rs`, a few call lines in `vectors.rs`):
- `Entries`: an open-addressed table of u32 entry numbers keyed by `hashes[e]` (Fibonacci-mixed, at most ¾ full).
- `Holders`: one row inline, many in a `Vec`.
- `Cache::shrink` after a file's open.

The scan, the re-score and compaction are untouched (index-at-10x's).

**Proved.** The measure is a sim-style test on a synthetic table opened as a start opens it (the file read, then
the rows), using `mallinfo2` uordblks+hblkhd:

| | 30,000 chunks | 300,000 chunks |
|---|---|---|
| before | 665 B/chunk | 857 B/chunk |
| after | **535 B/chunk** | **565 B/chunk** |

```
cargo nextest run -p theseus-index --run-ignored all -E 'test(heap)' --no-capture
300000 chunks: 565 bytes of heap a chunk
30000 chunks: 535 bytes of heap a chunk (a row 80 bytes, a cut 256)
```

- `heap::tests::the_heap_per_chunk_stays_near_its_int8_cut` holds it under 600. With the cut planted back
  (reverse-applied), it measured 665 and 857.
- `heap::tests::a_query_answers_the_same_top_k_as_before`: the top ten for three queries on a 1,200-chunk fixture
  were recorded from the code before the cut and pinned as a literal. They are identical after it.
- `entries::tests`: `Entries` against a `HashMap` over 20,000 inserts with clustered low bits and repeats; `Holders`
  is ≤ 24 bytes.

**Recall's deadline** (release tender, 100,000 chunks, five queries each, tender alone):

| | embed | vector scan + re-score | total |
|---|---|---|---|
| base | 98 to 123 ms | 35 to 38 ms | 134 to 162 ms |
| head | 87 to 117 ms | 36 to 58 ms | 124 to 176 ms |

These are the same within noise, inside 250 ms. Through the daemon (`theseus index search`): embed 98 to 106 ms,
vector 40 to 45 ms, 145 to 150 ms total.

**Tender's start** (same copied index, release):
- Socket answering: base 36 ms, head 16 ms.
- Every row pointing at its vector: base 1,140 ms, head 1,062 ms.
- RSS before the model: base 142.8 MB, head 137.6 MB (RssAnon 127.4 → 122.1 MB).

The RSS gap at 100k (~52 B/chunk) is smaller than the heap test's: hashbrown's capacity at that count happened to
sit low.

**Left:**
- `Row` is still 80 bytes. Packing `vec` into a u32 plus a u8 with a sentinel, and the hash as `[u64; 2]`, would take
  it to 64. That touches every `r.vec` site, so it is left for index-at-10x, which persists and maps the rows anyway.
- The int8 cut (256 B) is now about half the heap. Mapping it needs the cuts stored contiguously, not interleaved
  with the f16 vectors in `.vec` records: also index-at-10x's.

## Step 3: health's index line (theseus-agqn)

`index_line` now adds `· N texts waiting for a vector` (the status's `pending`, distinct texts, so it says "texts"
rather than "chunks") and `· tender rss X`. Goldens `health_index.txt` and `index_status.txt` were rewritten.

Live, from the scratch daemon:
```
index: ready · hybrid · 100000 nodes in 100000 chunks · through position 350010 · 0 B behind · 0 texts waiting for a vector · tender rss 134.4 MB
… after three vector queries (model loaded): … · tender rss 667.4 MB
```

health-o1 can carry the same fields in its cached struct: nothing new was added to `IndexStatus` for this line.
`counted_ms` (step 4) is on `IndexVectorStatus`.

## Step 4: the silent status socket (theseus-uazd)

**Found** in the status path, two ways for `index.status` to stop answering:
1. **The table lock.** `Vectors::status` read its counts under `table.read()`. std's RwLock prefers writers, so a
   status queued behind any writer: `compact_now` and `forget` hold the write lock across a whole vector file's
   rewrite and fsync, and a batch's append and a reconcile hold it too. A health call gives up after 100 ms, but its
   connection's thread stays parked on the lock.
2. **The slots.** After 16 such threads, `MAX_CONNECTIONS` is full, and a connection over the limit was closed
   unread. From then on every health call failed ("the tender closed the connection") until the writer let go.
   Queries waiting on the model and an `index.embed` its caller left (no `gone` check, on the nice-19 global pool)
   hold slots too.

I could not reproduce 2 h 44 min here. A rewrite under the lock at the owner's file size, plus slots held by slow
`index.embed` calls during a backfill, is the likeliest chain; the fix covers both links whatever held the lock.

**Changed:**
- `vectors/status.rs` (the status moved out of vectors.rs): counts are read with `try_read` and kept. While the table
  is held, or a writer waits, the status answers the last counts, and `IndexVectorStatus.counted_ms` (a new wire
  field, `#[serde(default)]`, TypeScript regenerated) says when they were read.
- `server.rs`: past `MAX_CONNECTIONS`, up to `STATUS_LANE` (4) more connections are each served on a thread for one
  `index.status`. Anything else there is refused with "the index tender is serving 16 connections; past them it
  answers index.status alone".
- The core (`tender.rs`): a last answer older than `STALE_AFTER` (2 min) is worded "stale: its socket has not
  answered for 2 h 44 min (…); these numbers are its status as of 2 h 44 min ago".

**Proved:**
- `tests_status::the_status_answers_while_the_table_is_held`: a thread holds the table's write lock. The status
  answers in about 1 ms with the earlier counts and the same `counted_ms`, then reads them fresh once the lock is
  released.
- `tests_status::the_status_answers_past_a_full_socket`: 16 idle connections hold every slot. The status still
  answers; `index.warm` is refused with why.
- `tests_tender::an_old_answer_is_called_stale_in_words`.
- Planted reverts:
  - `try_read` → `read()`: `…while_the_table_is_held` failed (no answer within 3 s).
  - The lane removed: `…past_a_full_socket` failed.
  - Files restored and touched, `git status` clean.

**Left:**
- `index.embed` still runs to its end after its caller leaves. A `gone` check between its batches, as queries have,
  would free its slot sooner.
- Compaction under the write lock stays index-at-10x's (its short-lock compaction).

## The live check for the maintainer, on a copy of the owner's store

Build the release tender, then run a scratch daemon on a copy of the store and index (never the operator's own;
scratch socket and state dir; `[index] weights_dir` pointing at the pinned nomic files):

```
cp -r ~/.theseus/store /tmp/agqn/state/store && cp -r ~/.theseus/index /tmp/agqn/state/index && rm -f /tmp/agqn/state/index/{sock,LOCK}
theseusd --config /tmp/agqn/theseus.toml &            # [server] state_dir/socket under /tmp/agqn, [index] weights_dir
theseus --socket /tmp/agqn/state/theseus.sock health | grep '^index'
  → "… · N texts waiting for a vector · tender rss X"
theseus --socket /tmp/agqn/state/theseus.sock index search -k 3 "<a phrase from the store>"   # three times
  → first answers bm25 alone while the model loads; then embed ≈ 100 ms, vector < 100 ms, total < 250 ms
grep -E 'VmRSS|VmHWM|RssAnon' /proc/<tender pid>/status       # before the model, loaded, after an idle unload
theseus --socket /tmp/agqn/state/theseus.sock index status   # after the idle unload and a new query
  → load_ms of the second load well under the first (no hash: about 0.5 s against 3.5 s here)
```

To see the stale words: `kill -STOP <tender pid>` for over 2 minutes, then health. It should say
`stale: its socket has not answered for 2 min (…)`. Then `kill -CONT` the tender.

On this VM the check used a synthetic store (`theseus-sim synth-store --sessions 50000`: 100,000 chunks) indexed by
the tender with `--no-vectors`. Its vectors were then filled at random by the ignored tool
`THESEUS_FILL_INDEX=<index> cargo nextest run -p theseus-index --run-ignored only -E 'test(fill_an_index)'`, so no
backfill ran. The base/head tender measures come from `/tmp/live/measure.sh`-style runs of both release binaries on
the same copied index.

## Docs to change (maintainer's)

- docs/status.md: the tender's memory (weights mapped, hash once, ~535 B/chunk) and the silent-socket fix.
- The spec's Part III item for theseus-agqn/uazd.
- docs/technical-overview.md, where it describes the index status: `counted_ms`, the status lane, and "stale" in
  health.

The crate's AGENTS.md has three new bullets, in the commits.

## Keel findings expected

None. `THESEUS_KEEL_BASE=2fd1f65 python3 scripts/keel-guard.py` → `keel: ok (2fd1f65..the working tree; 19 files
changed, 0 findings acked)`.

Run without a base, the guard picks merge-base(HEAD, local `main`). The clone's local `main` is a9ad950, older than
this branch's base, so it reports three findings from commits before 2fd1f65 (the tui tests, `tests_work_join`,
long-files). None of those files are touched here. That is why the gate ran with `THESEUS_KEEL_BASE=2fd1f65`.

## The gate

`CARGO_INCREMENTAL=0 TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 THESEUS_KEEL_BASE=2fd1f65 scripts/gate.sh` on the
final tree:
- keel, fmt, shape, features, clippy, cockpit, test build and the reader rule all passed.
- Suite: 3692 run, 3659 passed, **33 failed**. They are exactly the known L1-as-root failures: theseus-sandbox's 20
  contract tests and its bench's `spawn_100`, and theseusd's 12 `sandbox` tests.
- I then ran the later phases myself: protocol types (TS staged and committed), and the turn bench
  (`theseus-sim bench turn --check --runs 5 --burst 0`: plain 5 frames, tool 9, ok). Lifecycle and jobs benches are
  skipped under `THESEUS_GATE_NO_BENCH`.
- No timing test failed. An earlier gate run hit ENOSPC (the VM's disk filled with incremental caches): I cleared
  `target/debug/incremental` and reran with `CARGO_INCREMENTAL=0`.

Each commit, built alone: commit 1's tree passed clippy and the theseus-index, theseus and tender tests (372). The full
gate ran on commit 2's tree, the final one.
