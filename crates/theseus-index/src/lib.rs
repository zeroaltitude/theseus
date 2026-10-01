//! The index tender (M6 §2.2, step 29b; theseus-zaz.12): a child process of
//! the daemon that follows the store's WAL read-only, extracts each node's
//! text and entities, indexes them in tantivy, and answers `index.query`
//! (BM25 and entities, fused, as of a position) on its own socket.
//!
//! ```text
//! WAL (the core appends) ── theseus-follow (read-only, inotify) ── extract ── chunk ── tantivy
//!                                                                                      │
//! core ── index.query { text, k, as_of, … } ── <index>/sock (0600) ── BM25 + entity ── fused hits
//! ```
//!
//! It decodes nodes as JSON values, so it never links `theseus-core`, and a
//! core change never rebuilds the search engine. The core's half (the
//! `Tender` child kind, the spawn after serving, health's `index`, and the
//! CLI) is the wire-in, roadmap row 51.

pub mod chunk;
pub mod client;
pub mod engine;
pub mod entity;
pub mod extract;
pub mod proto;
pub mod server;
pub mod state;
pub mod tender;

pub use tender::{Config, OpenError, Shared, Tender};

/// A tender's whole life: take the index directory, answer on its socket
/// (from the start, so a query is answered during a backfill), and follow
/// the WAL until the process ends. SIGTERM's default action is the stop:
/// ingest is idempotent, so nothing is lost but the batch in flight.
pub fn serve(cfg: Config) -> Result<(), OpenError> {
    let tender = Tender::open(cfg)?;
    server::spawn(&tender.paths().socket(), tender.shared()).map_err(|e| {
        OpenError::Other(anyhow::Error::from(e).context("binding the index's socket"))
    })?;
    tender.run()?;
    Ok(())
}

#[cfg(test)]
mod tests;
