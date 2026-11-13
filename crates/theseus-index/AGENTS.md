# theseus-index

The index tender (M6): a child of the daemon that follows the WAL read-only into BM25, exact entities, and vectors, and answers on `<state>/index/sock`. An installed binary of its own, beside `theseusd`.

Key modules: `tender.rs`, `engine.rs`, `vectors.rs`, `server.rs`, `extract.rs`. Read by: `theseusd`, which runs it after serving (row 51; the core's `tender.rs`).

- **The embedding thread** (`vectors.rs`, `index-embed`) runs at nice 19, and between two pieces of work waits
  while the machine is busy (`theseus_store::pressure`, `VectorConfig::yield_bound`; tests give zero). The wait has no bound
  (`pressure::PATIENT`, theseus-195j): a busy machine gets no backfill batch until it is quiet or the tender stops. It is not in
  `SCHED_IDLE` (theseus-tood): it also loads the model a waiting query needs, and candle's rayon pool takes the
  policy of the thread that first runs it, so query embeddings could inherit it.
- **Queries at the daemon's priority** (`priority.rs`, theseus-y9a3). The tender's own work (the WAL's ingest,
  tantivy's merges, the backfill) runs at `serve --nice` (10; the embedding thread lowers itself to 19) in the idle
  I/O class; its socket's threads, and so each query and the queries' embedding pool, keep the priority the tender
  started at (the daemon's). A thread cannot take back a priority (`RLIMIT_NICE` is 0 under the daemon's unit), so
  `open_served` starts the socket's thread first, lowers the caller, opens, and only then hands the socket its
  `Shared` to bind: a tender refused its index never binds. Lowered whole, beside a busy machine a recall's 3 ms word
  search answered at ~250 ms, ~450 ms cold, past its deadline. `index.status` says both (`queries_at`, `work_at`).
  Tests: `tests_priority.rs`.
- **Queries go ahead of the backfill** (`ahead.rs`, theseus-w9qv). candle's softmax and layer norms are rayon's
  `par_chunks`, and the global pool (one thread, `[index] threads`) is usually born of the embedding thread, at nice
  19: a query that embedded there waited behind the backfill's jobs at its priority (171 to 747 ms under load). A
  query's embedding runs in a rayon pool of its own (`ahead::install`), and while one does (`Ahead::hold`) the
  embedding thread starts no batch; the query's end wakes it. `index.embed` (the memory pass's) stays on the global
  pool. Tests: `ahead::tests`, `vtests::queries_are_served_ahead_of_the_backfill` (the global pool held busy),
  `vtests::a_query_holds_the_backfill_while_it_embeds` (a real query takes the hold).
- **A recall's query fits its deadline** (theseus-zo1y). The vector source embeds `vector_text` (the turn's new text)
  when a query carries one, cut at `vector_tokens` word pieces by the tokenizer (`Embedder::tokenize_query`, one
  window), while BM25 and entities read `text` whole. The queries' pool has `ahead::query_threads` threads (four, never more than the
  cores, since theseus-d7fy; `serve --query-threads`, which the daemon passes from `[index] query_threads` when it is not 0; the load gate below runs a query on one while the machine is busy), `RAYON_NUM_THREADS` is the larger of that and `[index] threads` (candle
  splits each matmul by it, in the pool it runs in), and the global pool, the backfill's, is built with `[index]
  threads` from the embedding thread at nice 19 (`ahead::build_backfill_pool`). A query whose caller closed its
  connection stops at the embedder's next layer (`server::closed`, `POLLRDHUP`, asked between layers through
  `NomicBert::forward_while`), so its slot frees within a layer. `VectorConfig::query_step` is a test's slow model.
  Tests: `tests_query.rs`.
- **A load gate in front of the queries' pool** (`gate.rs`, `ahead::install_in`; theseus-d7fy). Four threads win on an
  idle machine (75 ms against 90) and lose beside a busy one (theseus-y9a3: each matmul waits for its slowest job).
  Before a query embeds, `Gate::admit` reads pressure (`theseus_store::pressure::busy`, CPU 20 % or IO 10 %, kept
  for one second: never a file a query; no PSI reads as calm). Calm: the full pool, any number at once. Busy: a
  pool of **one** thread (`Lane::One`) and one query at a time, the rest in line in order on their own connections'
  threads (the daemon's priority), each leaving at its caller's close (`gone`, the recall's deadline) or at
  `MAX_WAIT` (1 s, for a caller with no deadline) with its words alone (`skipped["vector"]`, never an error); a
  line whose machine goes calm is let in at once. `RAYON_NUM_THREADS` stays the larger of the two pools' sizes
  (process-wide, read at each matmul): gemm runs its jobs with `into_par_iter` in the pool its caller is in, so in
  the one-thread pool a matmul's four jobs run one after another on that thread, and no other thread computes.
  `index.status`'s `vectors.gate` and health's `query gate` say the pool, whether it is shut now, and the queries
  that waited and expired. Tests: `tests_gate.rs` (fake PSI in a mount namespace; the threads a lane used, counted
  by CPU time), `ahead::tests`.
- **A kept file's read text is indexed beside its node** (`files.rs`, theseus-lv2u). A file the owner attaches is
  kept whole in the store's blobs and the node names, by digest, the blob holding what was read of it (a PDF's
  pages, a document's sections). The extractor carries the reference (`Extracted.files`), and the tender reads that
  blob from `<store>/blobs/` at index time: the one thing it reads outside the WAL, and the reason the text is not
  in the node (a node is a frame every recovery and every rebuild reads whole). **Once per digest**: a file's chunks
  carry its digest in the `file` field, and a digest the index already holds under *another* node is left to it, so
  the same bytes attached twice are one copy; under this node it is taken again, since `Writer::replace` deletes a
  node whole. **The cap** is `files::MAX_BYTES` (256 KiB, `[tools] max_read_bytes`'s default): sections in order to
  the cap, the last cut on a character boundary, the rest dropped and said in a chunk of its own naming the file and
  `file_read`. **The chunking** is per section, so no chunk spans two pages, and every chunk is headed
  `[<file> · <section>]`, which is what a hit cites. A text blob that is gone or in no known shape leaves its node
  indexed without it. `index.status` counts all four (`files_indexed`, `files_repeated`, `files_cut`,
  `files_unread`; a blob whose bytes do not hash to its name, or that is past `MAX_BLOB`, counts there: on a store with on-disk damage a non-zero `files_unread` is the guard working, not a new fault), and `theseus index status` prints them. On disk one capped file of varied prose measured 132
  chunks, 411 KB of BM25 and 234 KB of vectors (1,816 bytes a chunk), about 2.5x the text taken. Known limit: a
  digest's one copy belongs to the node that carried it first, so erasing that node takes the text out even where a
  later node still carries the file; a rebuild does not put it back. The same reach crosses sessions: a hit cites the first node's session (see `files.rs`'s header). Tests: `tests_files.rs`, `files::tests`.
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
