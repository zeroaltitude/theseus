//! What a replay needs of a view turn's request (own-benchmark, `own_bench::rebuild`): the session holds
//! none of the view's front, window or beside block (the module above), so when a chain's first request goes
//! out each is kept as a blob named by its digest, as the system blocks are (`compiler::cache`): identical
//! text is one file, so a front that did not change since the last turn adds nothing. The view record carries
//! the digests ([`Kept`]), and the ids a replay asks the gate about again before it sends anything.
//!
//! The memory page needs nothing here: it is the end of the session block, which the compilation's own
//! block layout keeps.

use serde::{Deserialize, Serialize};

use super::Frozen;
use crate::blobs::Blobs;
use crate::node::{Body, Node};

/// The blobs a view turn's chain was laid from, by digest, and what it laid from beyond the session.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Kept {
    /// The fixed blocks and the ladder's nodes, as one JSON array of their texts.
    pub front: String,
    /// The window's nodes, as one JSON array.
    pub window: String,
    /// The block beside the message, as its text; none when there was none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub beside: Option<String>,
    /// The sessions and the exchanges the view laid or folded, and the nodes its recall laid: a replay asks
    /// the gate of now about each before it sends anything.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sessions: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exchanges: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recalled: Vec<String>,
}

/// What `f` lays beyond the session, as [`Kept`] and the texts to keep as blobs; none when a part does not
/// serialize.
pub fn kept(f: &Frozen) -> Option<(Kept, Vec<String>)> {
    let front: Vec<String> = f.front.iter().map(text_of).collect();
    let window: Vec<&Node> = f.window.iter().map(|(_, n)| &**n).collect();
    let mut texts = vec![
        serde_json::to_string(&front).ok()?,
        serde_json::to_string(&window).ok()?,
    ];
    texts.extend(f.beside.as_ref().map(text_of));
    let digest = |t: &String| crate::blobs::digest(t.as_bytes());
    let k = Kept {
        front: digest(&texts[0]),
        window: digest(&texts[1]),
        beside: texts.get(2).map(digest),
        sessions: f.sessions.clone(),
        exchanges: f.exchanges.clone(),
        recalled: f.recalled.clone(),
    };
    Some((k, texts))
}

/// The frozen view `k` names, read back from `blobs` for `turn` of `session`; the blob that is missing or
/// does not read is the error.
pub fn frozen(blobs: &Blobs, k: &Kept, (session, turn): (&str, &str)) -> Result<Frozen, String> {
    let front: Vec<String> = read(blobs, &k.front, "front")?;
    let window: Vec<Node> = read(blobs, &k.window, "window")?;
    let beside = match &k.beside {
        Some(d) => Some(text(blobs, d, "beside block")?),
        None => None,
    };
    let block = |t: &str| Frozen::block(session, turn, "", t);
    Ok(Frozen {
        turn_id: turn.into(),
        front: front.iter().map(|t| block(t)).collect(),
        window: (window.into_iter().enumerate())
            .map(|(i, n)| (i as u64 + 1, n.into()))
            .collect(),
        beside: beside.as_deref().map(block),
        recalled: k.recalled.clone(),
        ..Frozen::default()
    })
}

/// A harness block's text.
fn text_of(n: &Node) -> String {
    match &n.body {
        Body::UserMessage { text, .. } => text.clone(),
        _ => String::new(),
    }
}

/// The text of blob `digest`, which must hash to its name.
fn text(blobs: &Blobs, digest: &str, part: &str) -> Result<String, String> {
    let bytes = (blobs.read(digest))
        .filter(|b| crate::blobs::digest(b) == digest)
        .ok_or_else(|| {
            format!("the view's {part} blob {digest} is missing or does not hash to its name")
        })?;
    String::from_utf8(bytes).map_err(|_| format!("the view's {part} blob {digest} is not text"))
}

/// Blob `digest`, decoded.
fn read<T: serde::de::DeserializeOwned>(
    blobs: &Blobs,
    digest: &str,
    part: &str,
) -> Result<T, String> {
    serde_json::from_str(&text(blobs, digest, part)?)
        .map_err(|e| format!("the view's {part} blob {digest} does not read: {e}"))
}
