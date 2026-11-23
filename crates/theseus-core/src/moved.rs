//! Moved ranges (theseus-q9b85): the records a split (retired with threads on 2026-11-22) left in a store.
//!
//! A split quoted turns of a session into a thread and wrote one `Body::Moved` node in the source naming
//! them, and a META `moved.of.<node>` = [`MovedTo`] for each. Until 2026-11-22 the compiler left those nodes
//! out of the source's compile; nothing is withheld by a move now, so a moved node is read where it was said,
//! as any other. What stays is history: the `Moved` node decodes and renders its one line ([`line`]) at its
//! own place, and recall and `node.info` read `moved.of.<node>` to say where a node once went. A split's own
//! record (`thread.split.<id>`, `SplitRecord`) still decodes as it was stored.

/// The author of a `Moved` node.
pub const AUTHOR: &str = "harness:moved";

/// The line a range's place holds in the source's compile.
pub fn line(thread: &str, gist: &str) -> String {
    format!("moved to thread «{thread}»: {gist}")
}

/// The META key of the thread a node moved to: recall and `history` read it for one node without
/// reading a transcript. Its value is a [`MovedTo`], or null once the split is undone.
pub fn moved_of_key(node: &str) -> String {
    format!("moved.of.{node}")
}

/// Where a moved node went.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MovedTo {
    pub thread: String,
    pub split: String,
}

/// The id prefix of a claim (theseus-98bz.6, `rpc/home_claim.rs`): a [`MovedTo`] whose `split` is
/// `claim_…` keys a home turn's node to the thread it served. Recall reads it as a split's; nothing
/// left the home's compile, and a split may take the node over.
pub const CLAIM: &str = "claim";

impl MovedTo {
    /// Whether this is a claim's value, not a split's.
    pub fn is_claim(&self) -> bool {
        self.split.starts_with("claim_")
    }
}

/// The META key of a split's record (`thread.split.<id>`), as an older build wrote it.
// Read by the test-only split writer and by the old-store test: no production path reads a split's record now,
// and its typed decoder is kept so an old store's record still reads as it was stored.
#[cfg(test)]
pub(crate) fn split_key(id: &str) -> String {
    format!("thread.split.{id}")
}

/// What a split left, for its undo: the stored `thread.split.<id>` record of an older build.
#[cfg(test)]
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct SplitRecord {
    pub id: String,
    /// How the source was named, and the session it is.
    pub from: String,
    pub source_session: String,
    /// The `Moved` node in the source.
    pub moved_node: String,
    /// The thread, its session, and whether the split opened it.
    pub thread: String,
    pub target_session: String,
    pub opened: bool,
    /// The brief and the arrangement written into the target's session.
    pub target_nodes: Vec<String>,
    pub moved: u32,
    pub first: u64,
    pub last: u64,
    pub gist: String,
    pub at_ms: u64,
    #[serde(default)]
    pub undone: bool,
}
