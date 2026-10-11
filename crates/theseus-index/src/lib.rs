//! The index tender (M6 §2.2, steps 29b and 29c; theseus-zaz.12,
//! theseus-nz8): a child process of the daemon that follows the store's WAL
//! read-only, extracts each node's text and entities, indexes them in
//! tantivy, embeds each chunk, and answers `index.query` (BM25, entities, and
//! vectors, fused, as of a position) on its own socket.
//!
//! ```text
//! WAL (the core appends) ── theseus-follow (read-only, inotify) ── extract ── chunk ── tantivy
//!                                                                              │
//!                                     the embedding thread (nice 19) ── Nomic v1.5 on candle ── <index>/vectors
//!                                                                                      │
//! core ── index.query { text, k, as_of, … } ── <index>/sock (0600) ── BM25 + entity + vector ── fused hits
//! ```
//!
//! It decodes nodes as JSON values, so it never links `theseus-core`, and a
//! core change never rebuilds the search engine. The core's half (the
//! `Tender` child kind, the spawn after serving, health's `index`, and the
//! CLI) is the wire-in, roadmap row 51.

pub mod ahead;
pub mod chunk;
pub mod client;
pub mod embedder;
pub mod engine;
pub mod entity;
pub mod extract;
pub mod fuse;
pub mod model;
pub mod priority;
pub mod proto;
pub mod server;
pub mod state;
pub mod tender;
pub mod vectors;
pub mod weights;
pub mod wordpiece;

pub use tender::{Config, OpenError, Shared, Tender};

/// A tender's whole life: take the index directory, answer on its socket
/// (from the start, so a query is answered during a backfill), embed on its
/// own thread, and follow the WAL until the process ends. SIGTERM's default
/// action is the stop: ingest is idempotent, and a vector file's torn tail
/// is cut, so nothing is lost but the batches in flight. Its own work runs at
/// `nice` in the idle I/O class, its queries at the priority it started at
/// ([`open_served`]).
pub fn serve(cfg: Config, nice: i32) -> Result<(), OpenError> {
    open_served(cfg, nice)?.run()?;
    Ok(())
}

/// [`serve`] but the WAL's following: the tender opened and answering on its
/// socket, its embedding thread started. The socket's thread is started
/// first, at the calling thread's priority, which the queries keep; then the
/// calling thread lowers itself to `nice` in the idle I/O class, so the
/// tender's own threads (tantivy's, the embedding thread, the ingest it runs
/// on the caller) are born lowered (`priority`, theseus-y9a3).
pub fn open_served(cfg: Config, nice: i32) -> Result<Tender, OpenError> {
    let socket = state::Paths::new(&cfg.index_dir).socket();
    let (give, shared) = std::sync::mpsc::channel();
    let bound = server::spawn_early(&socket, shared).map_err(|e| {
        OpenError::Other(anyhow::Error::from(e).context("starting the socket's thread"))
    })?;
    priority::lower(nice);
    let tender = Tender::open(cfg)?;
    tender.shared().worked_at(priority::current());
    let _ = give.send(tender.shared());
    let binding = |e: std::io::Error| {
        OpenError::Other(anyhow::Error::from(e).context("binding the index's socket"))
    };
    bound
        .recv()
        .map_err(|_| binding(std::io::Error::other("the socket's thread ended")))?
        .map_err(binding)?;
    let shared = tender.shared();
    if shared.vectors.enabled() {
        std::thread::Builder::new()
            .name("index-embed".into())
            .spawn(move || shared.vectors.run(&shared.engine))
            .map_err(|e| {
                OpenError::Other(anyhow::Error::from(e).context("starting the embedding thread"))
            })?;
    }
    Ok(tender)
}

#[cfg(test)]
mod ftests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_import;
#[cfg(test)]
mod tests_priority;
#[cfg(test)]
mod tests_query;
#[cfg(test)]
mod vtests;
