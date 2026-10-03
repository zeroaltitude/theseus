# theseus-index

The index tender (M6): a child of the daemon that follows the WAL read-only into BM25, exact entities, and vectors, and answers on `<state>/index/sock`. An installed binary of its own, beside `theseusd`.

Key modules: `tender.rs`, `engine.rs`, `vectors.rs`, `server.rs`, `extract.rs`. Read by: `theseusd`, which runs it after serving (row 51; the core's `tender.rs`).
