//! A compilation's cache layout: its breakpoints, and the system blocks kept
//! by digest (own-benchmark, theseus-pw1q.12).
//!
//! A compilation keeps the SHA-256 of each system block's text (`stamp`, when
//! the compilation is first persisted), and the text is stored once in the
//! blobs under that name (`keep`), so a past request's system can be rebuilt
//! byte for byte (`turn.replay`). Sessions of a profile share the header, so
//! a handful of blobs a day.

use super::{Compilation, ProviderRequest};
use crate::blobs::Blobs;

impl super::CacheLayout {
    /// The breakpoints set, by name, the conversation's last.
    pub fn breakpoints(&self) -> Vec<&str> {
        let mut out: Vec<&str> = self
            .blocks
            .iter()
            .filter(|b| b.marked)
            .map(|b| b.block.as_str())
            .collect();
        if self.caches {
            out.push("conversation");
        }
        out
    }
}

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

/// Store the texts `stamp` returned, each once (a blob already stored is left
/// as it is). One batch, before the frame that names them. A failure is said
/// and costs the compilation its replay, not the turn.
pub fn keep(blobs: &Blobs, texts: &[&str]) {
    if texts.is_empty() {
        return;
    }
    let all: Vec<&[u8]> = texts.iter().map(|t| t.as_bytes()).collect();
    for r in blobs.put_many(&all) {
        if let Err(e) = r {
            tracing::warn!(error = %e, "a system block was not kept; its turns cannot be replayed");
        }
    }
}
