//! A record under the items' prefix (`labels.`) as a reader of the whole prefix reads it (issue 44). Records
//! are never rewritten, so a store can hold one this build does not read as an item's labels. Each record is
//! read as one of these, decided here alone ([`of_record`]):
//!
//! - **Whole**: it reads as an item's labels ([`ItemLabels`]). Read as ever.
//! - **Older**: it reads in the shape the builds from 7b89d3944 to ab786c5da wrote, whose removals kept on the
//!   item name no session (at least one): its node, kind, entry and every raised label read. Its raised labels are read in
//!   full; its removals are not read, so each label it holds stands (a session's own removals still cover it
//!   where they are read). A record of that shape with a raised label that does not read is not this.
//! - **Unread**: anything else, a damaged item's record or one that is no item's. Nothing in it is read for
//!   labels: not a raised label that reads by itself, not its node. The rebuild passes it over and counts it;
//!   a fork whose bounds hold the item its key or its `node` names is refused, and any other fork passes it
//!   over. Either way it is left as it is, and named in the log by its key alone ([`passed_over`]), never by
//!   its payload (a decoder's error can quote the payload).

use serde::Deserialize;
use theseus_store::Record;

use super::{Asked, ItemLabels};
use crate::content::{ItemKind, Label, Removed};

/// A record under the prefix, as read.
pub enum Kept {
    /// It reads as an item's labels.
    Whole(ItemLabels),
    /// It reads in the older shape: as an item's labels with no removals.
    Older(ItemLabels),
    /// It reads as neither.
    Unread,
}

/// An item's record as the builds from 7b89d3944 to ab786c5da wrote it, field for field: a removal kept on the
/// item named no session. Its removals are read to know the shape, and not kept.
#[derive(Deserialize)]
struct Older {
    node: String,
    kind: ItemKind,
    source: String,
    entered_ms: u64,
    #[serde(default)]
    asked: Option<Asked>,
    #[serde(default)]
    raised: Vec<Label>,
    removed: Vec<Removed>,
}

/// `r`, a record under the prefix, as read.
pub fn of_record(r: &Record) -> Kept {
    if let Ok(item) = r.decode::<ItemLabels>() {
        return Kept::Whole(item);
    }
    match r.decode::<Older>() {
        Ok(o) if !o.removed.is_empty() => Kept::Older(ItemLabels {
            node: o.node,
            kind: o.kind,
            source: o.source,
            entered_ms: o.entered_ms,
            asked: o.asked,
            raised: o.raised,
            removed: Vec::new(),
        }),
        _ => Kept::Unread,
    }
}

/// The one warning for a record read as neither and passed over: its key, never its payload.
pub fn passed_over(key: &str) {
    tracing::warn!(
        key,
        "content: a record under labels. could not be read as an item's labels: passed over, and left as it is"
    );
}
