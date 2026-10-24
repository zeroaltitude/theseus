//! A compilation's cache layout: its breakpoints, and the system blocks kept
//! by digest (own-benchmark, theseus-pw1q.12).
//!
//! A compilation keeps the SHA-256 of each system block's text (`stamp`, when
//! the compilation is first persisted), and the text is stored once in the
//! blobs under that name (`keep`), so a past request's system can be rebuilt
//! byte for byte (`turn.replay`). Sessions of a profile share the header, so
//! a handful of blobs a day.

use std::sync::Arc;

use super::{Compilation, ProviderRequest};
use crate::blobs::Blobs;

/// The texts of a request's system blocks, in order.
fn texts(request: &ProviderRequest) -> Vec<&str> {
    request
        .system
        .iter()
        .map(|b| b.get("text").and_then(|t| t.as_str()).unwrap_or_default())
        .collect()
}

/// Write each system block's digest into the compilation's cache layout, and
/// return the texts to keep. Only when the layout names the request's blocks
/// one for one: otherwise nothing is stamped, and a replay says the blocks
/// were not kept.
pub fn stamp<'a>(c: &mut Compilation, request: &'a ProviderRequest) -> Vec<&'a str> {
    let texts = texts(request);
    let Some(layout) = c.manifest.cache.as_mut() else {
        return Vec::new();
    };
    if layout.blocks.len() != texts.len() {
        return Vec::new();
    }
    for (b, t) in layout.blocks.iter_mut().zip(&texts) {
        b.digest = Some(crate::blobs::digest(t.as_bytes()));
    }
    texts
}

/// The texts `stamp` returned that no blob holds yet, to `keep` (each once:
/// `put_many` writes a repeated one once).
pub fn fresh(blobs: &Blobs, texts: &[&str]) -> Vec<Vec<u8>> {
    texts
        .iter()
        .filter(|t| !blobs.path(&crate::blobs::digest(t.as_bytes())).exists())
        .map(|t| t.as_bytes().to_vec())
        .collect()
}

/// Store `fresh`'s texts as one batch on the blocking pool, so the request
/// never waits on its syncs (theseus-nsp9l.4: two files and the directory,
/// ~7 ms each on the owner's disk, were ~21 ms of the send path). The turn
/// starts it once its call's plan frame is durable, so the syncs do not slow
/// the frames before the request (beside them, each of the two took ~12 ms
/// instead of ~7 under strace), and waits for it once its body is done
/// (`TurnRunner::blocks_kept`). A crash before the batch lands,
/// or a failure (said), costs the compilation its replay (`rebuild_first`
/// names the missing blob), never the turn. `None` when there is nothing to
/// store; outside a runtime the batch is stored here.
pub fn keep(blobs: &Arc<Blobs>, fresh: Vec<Vec<u8>>) -> Option<tokio::task::JoinHandle<()>> {
    if fresh.is_empty() {
        return None;
    }
    let blobs = blobs.clone();
    let store = move || {
        let all: Vec<&[u8]> = fresh.iter().map(Vec::as_slice).collect();
        for r in blobs.put_many(&all) {
            if let Err(e) = r {
                tracing::warn!(error = %e, "a system block was not kept; its turns cannot be replayed");
            }
        }
    };
    match tokio::runtime::Handle::try_current() {
        Ok(rt) => Some(rt.spawn_blocking(store)),
        Err(_) => {
            store();
            None
        }
    }
}
