//! A record under the items' prefix (`labels.`) as this build reads it (issue 44), for a reader of the whole
//! prefix: one record that does not read never fails the rest. Records are never rewritten, so a record an
//! older build wrote in a shape this one does not know (from 7b89d3944 to ab786c5da a removal kept on the item
//! named no session), or one that is no item's at all, is read here as far as it reads, and left as it is.
//!
//! - **What still reads is read.** Its node, its kind and when it entered, each when it reads, and each raised
//!   label that reads by itself. Its removals are not read, so a label it holds stands: fewer labels is never
//!   the careful side, and a session's own removals still cover it where they are read.
//! - **One warning names its key**, never its payload (a decoder's error can quote the payload).

use serde_json::Value;
use theseus_store::Record;

use super::ItemLabels;
use crate::content::{ItemKind, Label};

/// A record under the prefix, as read.
pub enum Kept {
    /// It reads as an item's labels.
    Whole(ItemLabels),
    /// It does not: what still reads by itself.
    Part(Part),
}

/// What a record that does not read whole still says. A field that is absent, or does not read, is none.
#[derive(Debug, Default)]
pub struct Part {
    pub node: Option<String>,
    pub kind: Option<ItemKind>,
    pub entered_ms: Option<u64>,
    /// Each raised label that reads by itself.
    pub raised: Vec<Label>,
}

impl Part {
    /// Whether it names an item as an item's record does (its node, its kind, when it entered): an older
    /// shape of one. Otherwise it is no item's labels.
    pub fn is_item(&self) -> bool {
        self.node.is_some() && self.kind.is_some() && self.entered_ms.is_some()
    }
}

/// `r`, a record under the prefix, as read.
pub fn of_record(r: &Record) -> Kept {
    if let Ok(item) = r.decode::<ItemLabels>() {
        return Kept::Whole(item);
    }
    let key = r.key.as_deref().unwrap_or_default();
    tracing::warn!(
        key,
        "content: a record under labels. does not read as an item's labels: read as far as it reads, and left as it is"
    );
    let v: Value = serde_json::from_slice(&r.payload).unwrap_or_default();
    let raised = (v
        .get("raised")
        .and_then(Value::as_array)
        .into_iter()
        .flatten())
    .filter_map(|l| serde_json::from_value(l.clone()).ok())
    .collect();
    Kept::Part(Part {
        node: v.get("node").and_then(Value::as_str).map(str::to_string),
        kind: (v.get("kind").cloned()).and_then(|k| serde_json::from_value(k).ok()),
        entered_ms: v.get("entered_ms").and_then(Value::as_u64),
        raised,
    })
}
