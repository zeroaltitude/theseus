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
/// is cut, so nothing is lost but the batches in flight.
pub fn serve(cfg: Config) -> Result<(), OpenError> {
    let tender = Tender::open(cfg)?;
    server::spawn(&tender.paths().socket(), tender.shared()).map_err(|e| {
        OpenError::Other(anyhow::Error::from(e).context("binding the index's socket"))
    })?;
    let shared = tender.shared();
    if shared.vectors.enabled() {
        std::thread::Builder::new()
            .name("index-embed".into())
            .spawn(move || shared.vectors.run(&shared.engine))
            .map_err(|e| {
                OpenError::Other(anyhow::Error::from(e).context("starting the embedding thread"))
            })?;
    }
    tender.run()?;
    Ok(())
}

#[cfg(test)]
mod ftests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_import;
#[cfg(test)]
mod tests_query;
#[cfg(test)]
mod vtests;
