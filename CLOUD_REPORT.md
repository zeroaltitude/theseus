# CLOUD_REPORT: p3a-mem-spike (theseus-xxac7)

Branch `cloud/20261024-p3a-mem-spike`, base `ac298666`. A spike: it measures and recommends. **Not done** (time, one
4-core VM, a disk allowance that ran out once): a massif heap profile of `theseusd`, the full gate (see 8), and the
model-dependent numbers (see 7).

## 1. For the maintainer

- Store format **51** and the vector file format are untouched. No default changed, no dependency added
  (`Cargo.lock` unchanged), no ceiling raised (`vectors.rs` 2,561 → 2,567 of 2,650).
- **Spike-only code** (drop it all with the files below):
  - `crates/theseus-index/src/spike_view.rs` and `tests_view.rs`, plus the lines that call them: `lib.rs` (the `mod`
    and a `cfg(test)` global allocator), `main.rs` (the global allocator), `weights.rs` (a 3-line early return when
    `THESEUS_INDEX_VIEW_WEIGHTS` is set), `vectors.rs` (`spike_view::unpin` in `VectorConfig::new`, `release_pages` at
    the idle unload, `after_batch` after a backfill batch, and `mod spike`).
  - `crates/theseus-index/src/vectors/spike.rs`: test-only tools (recall by layout, a vector filler).
  - `crates/theseus-sim/src/memsynth.rs` and the `SynthMemory` subcommand in `main.rs`.
  - `bench/memory/` (`rss_bench.py`, `rss_daemon.py`).
- The shipped `theseus-index` now carries `ViewAlloc`, a pass-through global allocator over `System` with one
  range check on `dealloc`. Inert unless the env var is set. **OWNER CALL** if it ships (see 6).
- `THESEUS_INDEX_SPIKE_UNPINNED=1` makes the index pin the files it finds instead of the published SHA-256. It exists
  only so a seeded full-size model loads in the bench. Do not ship it.

## 2. The synthetic stores

`theseus-sim synth-memory --dir D/store --nodes N [--seed 1]` writes parked sessions of 24 nodes each, invented text
under `example.test`, from a seeded invented vocabulary. (The existing `synth-store` writes one identical text per
session, so it cannot make distinct chunks; the exam's generator is item-based and does not scale.)

| store | nodes | chunks (`documents`) | sessions | records | WAL |
|---|---|---|---|---|---|
| large | 229,000 | 343,044 | 9,542 | 267,168 | 711 MB |
| small (a tenth) | 23,000 | 34,511 | 959 | 26,836 | 71 MB |

The owner's is about 229k nodes and 360k chunks. A model for the bench (the real model is unreachable here, see 7):
`THESEUS_SPIKE_MODEL_DIR=<w> cargo test --release -p theseus-index --lib -- --ignored spike_write_model` writes the
published architecture at full size with seeded f32 weights (547 MB, same compute) and a tokenizer whose vocabulary
holds the invented words. Vectors: `spike_fill_vectors` (env `THESEUS_SPIKE_INDEX`, `THESEUS_SPIKE_WEIGHTS`,
`THESEUS_SPIKE_BACKLOG`, with `THESEUS_INDEX_SPIKE_UNPINNED=1`) gives every chunk a random vector of the serving
stamp, leaving a backlog for the embedding thread. The bench's `--unpinned` flag sets the env for the tender.

## 3. Where the memory goes (index tender, large store, release build, `/proc/<pid>/status`, MB)

Base (copying load, seeded full-size model, 4 threads), after a model load and 30 vector searches:

| | VmRSS | RssAnon | RssFile |
|---|---|---|---|
| BM25 only (no vectors), caught up | 109 | 96 | 13 (peak 522 while building the index) |
| busy (model loaded, searching, backlog embedding) | 1,274 to 1,358 | 850 to 932 | 425 |
| idle, after the unload (nothing pending) | 760 | 333 | 427 |

This reproduces the owner's busy numbers (935 / 904 on a real store). Holders of the busy anon:
- model weights, 530 MB (the copies in `weights.rs`);
- about 190 MB vector side (int8 cuts 88 MB, rows, entries; matches the agqn figure of about 535 bytes a chunk);
- the rest (about 100 MB) is the tender's baseline, compute scratch and thread pools.

The 768-d f16 vectors are **not** held: `Cache::full` reads them with `pread` for the 100 re-score candidates only.
Lever (b) is already in the base (theseus-agqn). RssFile 425 MB is tantivy's mmap, reclaimable.

**Not reproduced:** the owner's idle RssAnon of 22 MB. Here an idle tender keeps about 333 MB anon after the model
unloads: the vector table (the cuts, rows) and allocator retention stay. The unload drops only the model. The idle
number on the owner's machine is probably after a tender restart; the local half should check (7).

`theseusd` (a **debug** build, so absolute numbers run high; large store; 50 sessions with a turn each on the
stand-in model): settled 163 to 184 MB RSS (anon 72 to 100, peak 119, peak RSS 210); after the load and 20 s of quiet
132 MB, anon 38.6. `health`'s resident block: heap in use 101 MB, then 21 MB after the second trim. Caches it names:
node cache 0.1 MB of a 64 MB cap, index cache bound 16 MB, import catalog 0. **No unbounded cache showed.** The busy
anon is a startup transient (store open and index follow, about +80 MB of heap in use) that the daemon's trim gives
back after about 10 s of quiet. That matches the owner's 221 busy / 30 idle anon. I did not run massif (valgrind is
installed) for lack of time, so the transient's allocation sites are unnamed.

## 4. Levers

**(a) Weights straight from the map** (`spike_view.rs`). candle's CPU storage is a `Vec<f32>`, so a tensor cannot
borrow a map. The spike builds each tensor's `Vec` over the mapped bytes (`Vec::from_raw_parts`) and installs a global
allocator that never frees pointers inside registered mapped ranges. The map lives for the process and is read-only.
Compute is the same f32 on the same bytes. Alignment: a tensor not 4-aligned (or any tensor on a big-endian target)
is copied with `from_le_bytes`; a real checkpoint's header is 8-padded, so all are viewed.

| large store, busy | base | view | view + release after each batch (small store only) |
|---|---|---|---|
| RssAnon | 850 to 932 | 322 to 404 | 79 to 115 on the small store (base 580 to 640 there) |
| RssFile | 425 | 859 to 947 | 472 to 572 |
| VmRSS | 1,274 to 1,358 | 1,180 to 1,352 | about 550 to 670 on the small store |
| search p50 / p95 (ms) | 385 to 466 / 426 to 617 | 368 to 503 / 443 to 665 | 270 / 312 (view), same band |
| backfill texts per 100 s (small) | 197 | 196 | 199 |
| first search after the unload | 849 ms (re-copy) | 446 to 508 ms | 218 to 256 ms (small) |
| idle after unload, VmRSS | 760 | 722 (with release at unload; 1,156 without it) | |

Honest reading: **RssAnon falls by about 480 MB, VmRSS does not** (the pages become file-backed, reclaimable, and
still count in VmRSS until the kernel needs them or `release_pages` runs). The first run left the pages resident after
the unload (VmRSS 1,156, worse than base), so the spike releases them at unload and, with
`THESEUS_INDEX_VIEW_WEIGHTS=release`, after each backfill batch; that costs no throughput here (199 against 197
texts per 100 s). A reload after the unload is twice as fast, since nothing is copied. The first load is 11.4 to
12.5 s in both, dominated by hashing the 547 MB file once.
Tests (`tests_view.rs`, in `theseus-index`): bit-identical embeddings to the copying load on the tiny model, also
after `release_pages`; **planted revert** (the viewed tensor scaled by 1.0000001): `viewed_weights_embed_bit_identically…`
and `a_viewed_load_adds_no_anonymous_memory…` FAIL, restored and `touch`ed; the misaligned tensor is copied and reads
right; a view of another file is refused.

**(b) Vectors not held whole**: already so at base (above). The remaining resident vector memory is the int8 cuts
(88 MB at 343k) plus rows, about 190 MB in all. A smaller rows layout is a possible follow-up (not measured).

**(c) Lower-precision layouts**: recall@k against an f32 brute-force top-k (`spike_recall_by_layout`; synthetic
clustered 768-d vectors with decaying energy, 100,000 vectors, 100 queries near data points). **Synthetic vectors are
not the model's, and these queries are easy; read the ranking of layouts, not the levels.**

| layout | @1 | @6 | @10 | @20 |
|---|---|---|---|---|
| base: int8-256 scan, top 100, f16-768 re-score | 1.000 | 1.000 | 1.000 | 1.000 |
| int8-256 scan alone, no re-score | 1.000 | 0.860 | 0.878 | 0.914 |
| int8-256 scan, top 30, re-score | 1.000 | 1.000 | 1.000 | 0.999 |
| int8-256 scan, top 300, re-score | 1.000 | 1.000 | 1.000 | 1.000 |
| int8-128 scan, top 100, re-score | 1.000 | 1.000 | 1.000 | 1.000 |
| int8-768 scan alone | 1.000 | 0.990 | 0.984 | 0.989 |
| int8-768 scan, top 100, re-score | 1.000 | 1.000 | 1.000 | 1.000 |
| f16-768 brute force | 1.000 | 1.000 | 1.000 | 1.000 |

Dropping the re-score costs recall (0.86 at 6), so the f16 file is worth its disk. A 128-d cut would halve the
resident cuts (44 MB) at no loss here; any new layout needs a new file format, so it is a **PROPOSAL** only, to be
checked on real embeddings.

**(d) `theseusd` heap**: see 3. No cap proposed or implemented: every cache `health` lists is bounded. The lever is the
startup transient (trim sooner after the start path, or bound the follower's batch), pending an allocation profile.

## 5. The exam probe

`theseus-exam probe` (model-free arms) on the exam-v2 store, `--half all`, `--arm bm25,entity --arm bm25`, a tender over
the written store with `--no-vectors`: gold nodes in the top 6, all families together: **34 of 75 for both arms**
(items with all their gold in the top 6: 27 of 68); ten base families 31 of 42, hard families 3 of 33. The vector
and fused arms need the model, which is not here, so **gold@6 for the vector and fused arms is not measured** at
the base or under any lever. For (a), rank equality follows from the bit-identical embeddings (the test above), so
the vector arms cannot differ. (b) is the base; (c) is a proposal and was measured on synthetic vectors only.

## 6. Recommendation and targets

In order of RSS saved per risk:
1. **(a) viewed weights, release at unload** (about 480 MB anon on the busy tender, no format change, no recall change,
   no throughput change, a faster reload). Risk: `unsafe` and a global allocator in the shipped binary, a read-only
   mapping that is never unmapped; a truncated weights file would SIGBUS (the same exposure as `mapped.rs` has now).
   **OWNER CALL**, since VmRSS only falls if the kernel reclaims or the release mode is on (use `release` for the
   backfill: no cost measured).
2. **Idle**: also drop the vector table at the idle unload (about 190 MB at 343k chunks, reloaded with the model);
   today it stays. Needs its own step.
3. **Daemon**: trim sooner after startup work; profile the transient (massif) before capping anything.
4. (c) layouts: no change recommended without real-embedding recall.

Targets on the owner shape (tender, release): busy anon about 350 MB, busy VmRSS about 600 MB with release mode;
idle anon below 150 MB, idle VmRSS below 100 MB once the table also unloads. Daemon: busy RSS 200 MB (release build),
idle anon 40 MB.

## 7. The local half (on a COPY of the owner's store; the numbers, not the data, go in reports)

Missing here: the real model (HuggingFace answers 403 from this VM), so no real embedding latency, no real vectors,
and no gold@6 for vector arms. On the owner's machine, with the real model in `~/.cache/theseus/models`:
```
cp -r <state>/store /scratch/copy/store          # a COPY; never the live store
cargo build --release -p theseus-index -p theseusd -p theseus-sim
python3 bench/memory/rss_bench.py tender --index-bin target/release/theseus-index --store /scratch/copy/store \
  --in-place --work /scratch/w --weights-dir ~/.cache/theseus/models --label base --backlog-secs 600
python3 bench/memory/rss_bench.py tender ... --label view --view-weights          # and: --view-weights release
python3 bench/memory/rss_bench.py daemon --theseusd target/release/theseusd --sim-bin target/release/theseus-sim \
  --store /scratch/copy/store --in-place --work /scratch/d --label daemon
cargo test --release -p theseus-index --lib -- --ignored --nocapture spike_recall   # then swap in real vectors
```
(Use a short `--work` path: a Unix socket path over 107 bytes fails.) Record per phase VmRSS/RssAnon/RssFile, the
first search after the unload, search p50/p95, backfill texts per second, and the idle anon after the unload (compare
with the 22 MB seen). Run `theseus-exam probe` with the model for the vector and fused arms at base and with
`THESEUS_INDEX_VIEW_WEIGHTS=1`: the ranks must match exactly.

## 8. Tests, gate, keel, DM golden

- `cargo nextest run -p theseus-index -p theseus-sim -E 'package(theseus-index) or test(memsynth)'` (TZ=America/Phoenix):
  **110 of 110 passed** (every `theseus-index` test, `tests_view` and `vectors::spike` among them, and the two
  `memsynth` tests), after the last code change. `cargo fmt --check` clean. `cargo clippy -p theseus-index -p
  theseus-sim --all-targets -- -D warnings`: clean (it caught one `Ok(..?)` in `spike_view.rs`, fixed).
- **Gate not run in full**: `scripts/gate.sh` did not run (nor `shape.sh`, `cargo deny`, the `theseus-exam` and
  `theseusd` suites), for lack of time and disk (the VM's allowance reached 98 %). Nothing outside `theseus-index`,
  `theseus-sim` and `bench/memory/` changed, so the other crates' tests are not affected by this branch's code.
- Keel: `python3 scripts/keel-guard.py --base ac298666 --head HEAD --quiet` printed nothing (exit 0).
- DM golden: `git diff --exit-code ac298666 -- crates/theseus-discord/src/golden/dm_flow.txt` printed nothing (exit 0).

## 9. Join notes (files touched)

`crates/theseus-index/src/{lib.rs,main.rs,weights.rs,vectors.rs,spike_view.rs,tests_view.rs,vectors/spike.rs}`,
`crates/theseus-sim/src/{main.rs,memsynth.rs}`, `bench/memory/{rss_bench.py,rss_daemon.py}`. `main.rs` of the sim
gets a `mod memsynth`, a `SynthMemory` variant and its arm. Nothing in the other lanes' areas.

## 10. Follow-ups

- Unload the vector table with the model; shrink the rows layout.
- A massif profile of the daemon's startup transient, then a decision on trimming sooner.
- Real-embedding recall for (c) and the probe's vector arms.
- If (a) ships: replace the allocator trick with a first-class path (a candle storage over a map, or `gemm` directly)
  and take the env var out; add a config key.
- The existing `bench idle --active` could print `health`'s resident block; the new `rss_daemon.py` does.
