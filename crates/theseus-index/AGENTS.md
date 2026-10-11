# theseus-index

The index tender (M6): a child of the daemon that follows the WAL read-only into BM25, exact entities, and vectors, and answers on `<state>/index/sock`. An installed binary of its own, beside `theseusd`.

Key modules: `tender.rs`, `engine.rs`, `vectors.rs`, `server.rs`, `extract.rs`. Read by: `theseusd`, which runs it after serving (row 51; the core's `tender.rs`).

- **The embedding thread** (`vectors.rs`, `index-embed`) runs at nice 19, and between two pieces of work waits
  while the machine is busy (`theseus_store::pressure`, `VectorConfig::yield_bound`; tests give zero). It is not in
  `SCHED_IDLE` (theseus-tood): it also loads the model a waiting query needs, and candle's rayon pool takes the
  policy of the thread that first runs it, so query embeddings could inherit it.
- **Queries go ahead of the backfill** (`ahead.rs`, theseus-w9qv). candle's softmax and layer norms are rayon's
  `par_chunks`, and the global pool (one thread, `[index] threads`) is usually born of the embedding thread, at nice
  19: a query that embedded there waited behind the backfill's jobs at its priority (171 to 747 ms under load). A
  query's embedding runs in a rayon pool of its own (`ahead::install`), and while one does (`Ahead::hold`) the
  embedding thread starts no batch; the query's end wakes it. `index.embed` (the memory pass's) stays on the global
  pool. Tests: `ahead::tests`, `vtests::queries_are_served_ahead_of_the_backfill` (the global pool held busy),
  `vtests::a_query_holds_the_backfill_while_it_embeds` (a real query takes the hold).
- **A recall's query fits its deadline** (theseus-zo1y). The vector source embeds `vector_text` (the turn's new text)
  when a query carries one, cut at `vector_tokens` word pieces by the tokenizer (`Embedder::tokenize_query`, one
  window), while BM25 and entities read `text` whole. The queries' pool has `ahead::query_threads` threads (half the
  cores, 1 to 4; `serve --query-threads`), `RAYON_NUM_THREADS` is the larger of that and `[index] threads` (candle
  splits each matmul by it, in the pool it runs in), and the global pool, the backfill's, is built with `[index]
  threads` from the embedding thread at nice 19 (`ahead::build_backfill_pool`). A query whose caller closed its
  connection stops at the embedder's next layer (`server::closed`, `POLLRDHUP`, asked between layers through
  `NomicBert::forward_while`), so its slot frees within a layer. `VectorConfig::query_step` is a test's slow model.
  Tests: `tests_query.rs`.
- **A node written again with nothing to index leaves the index** (`Tender::ingest`'s skip; an import's tombstone,
  theseus-0lrr.6): that holds inside one batch too, where the node's first record is indexed but not yet committed
  (`fresh`), as a rebuild meets a node and its tombstone together; `engine.holds` reads only what is committed.
  Test: `tests_import.rs`.
- **The weights are mapped and hashed once** (`mapped.rs`, `weights.rs`; theseus-agqn). `model.safetensors` is
  mapped and each tensor copied out a window at a time, each window's pages released after (`Map::release`), so a
  load holds the f32 tensors (compute stays f32: the owner's D-3) and never the file beside them. Its SHA-256 is
  taken while the first load copies and kept by the file's (device, inode, size, mtime): a load after the idle
  unload copies and hashes nothing. Tests: `tests_weights.rs` (the peak resident set through `clear_refs`, and
  `mapped::hashes_of`, the counting seam).
- **The heap per chunk** (`vectors/entries.rs`, `vectors/heap.rs`; theseus-agqn): a vector file's entries by hash
  are an open-addressed table of entry numbers keyed by the hashes the file already holds (`Entries`), a text's rows
  one inline (`Holders`), and a file's memory is shrunk after its read. About 535 bytes a chunk at 30,000 (665
  before), of which the int8 cut is 256; the 768-d f16 vectors stay on disk. `heap::tests` holds the bound and the
  fixture's top-k; `bench_the_heap_at_300k_chunks` prints the measure.
- **The status never waits on the embedding work** (`vectors/status.rs`, `server.rs`; theseus-uazd). Its table
  counts are read with `try_read` and kept: while a writer holds the table (a compaction's rewrite, a batch's
  append) or waits for it, the status answers the last counts and their time (`counted_ms`). Past
  `MAX_CONNECTIONS`, up to `STATUS_LANE` connections more are served for `index.status` alone, and refuse anything
  else with why. Tests: `tests_status.rs`.
