//! The content graph's vocabulary: the kinds of edge between nodes (§4.1,
//! §6.1). A node's label is not here: it is a field of the node (`Node.label`,
//! `theseus_protocol::label::Label`, with `Integrity` and `Readers`; M4 19a).
//!
//! Row 12 (12a) of the roadmap re-cut added the first edge, `derived_from`
//! on the report route. A variant lands
//! with its reader, on the same commit, or with a reserved marker (the reader
//! rule, P0's rule 3, theseus-wjy): the registry test, `tests_registry`,
//! enumerates `VARIANTS` and fails a variant that nothing reads. A reader
//! names the variant by its type (`EdgeKind::DerivedFrom`) in a `match` arm,
//! a pattern, or an `==`. `node.reach` (`reach.rs`) reads `derived_from`.

use serde::{Deserialize, Serialize};
use theseus_store::{kinds, NewRecord};

/// An enum from one table: each variant with its docs and its name in the
/// store and on the wire, and `VARIANTS`, built from the same table, so none
/// is left out of the registry test. `as_str` and `named` come from the table
/// too, so they name no variant by its type: the reader rule's readers are
/// the code that acts on one.
macro_rules! vocabulary {
    (
        $(#[$meta:meta])*
        pub enum $name:ident { $($(#[$doc:meta])* $variant:ident = $wire:literal,)* }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum $name {
            $($(#[$doc])* $variant,)*
        }

        impl $name {
            /// Every variant's name, with its name in the store and on the wire.
            pub const VARIANTS: &[(&str, &str)] = &[$((stringify!($variant), $wire),)*];
            const ALL: &[Self] = &[$(Self::$variant,)*];

            /// Its name in the store and on the wire.
            pub fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $wire,)*
                }
            }

            /// The variant a stored name names, or None for one this build
            /// does not know (a newer build's).
            pub fn named(name: &str) -> Option<Self> {
                Self::ALL.iter().copied().find(|v| v.as_str() == name)
            }
        }
    };
}

vocabulary! {
    /// What an EDGE record says of its two nodes (`kinds::EDGE`): see
    /// [`Edge`].
    pub enum EdgeKind {
        /// `from` copies `to` (row 12, 12a): a task's report, relayed into
        /// its parent from the task's last message, and a task's brief, from
        /// the parent's reply that started it, each into another session; and
        /// a node the owner published into a place (the place rule); and a
        /// `Recall` node to each source it renders (M6 30b, `recall`); and a
        /// task's arrangement, from each message it quotes (M5 27); and a
        /// glide's borrowed node, from each message it read (38b, `glide`).
        /// (M4 19c's graduations wrote one in the source's own session, via
        /// `graduate`: they still read.)
        DerivedFrom = "derived_from",
        /// `from` says what `to`, written before it, says (M6 31a, the memory
        /// pass's gate: cosine at or above the science's merge threshold).
        /// The duplicate stays; `baseline` keeps only the newest of a group.
        SameEntity = "same_entity",
        /// `from` corrects `to`, written before it (M6 31a: an operator's
        /// correction close enough to its top neighbour). `baseline`
        /// prefers the newer side.
        Supersedes = "supersedes",
    }
}

/// An EDGE record (§6.1), keyed `<kind>|<from>|<to>` and scoped `in:<to>`,
/// so a scan of the scope `in:X` lists every edge into X, in WAL order. That
/// scope is the reverse column, on the scope index the store already keeps:
/// no new table, and no change of layout. EDGE stays at schema 1: the edges
/// written before theseus-hco (a compilation's) carry the same fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edge {
    /// The kind's name (`EdgeKind::as_str`).
    #[serde(rename = "type")]
    pub kind: String,
    pub from: String,
    pub to: String,
    /// The route that wrote it: `report`, `brief`, `publish`, `recall`,
    /// `arrangement`, or `glide` (`graduate` in a store from 19c to the place
    /// rule).
    #[serde(default)]
    pub via: String,
    pub at_ms: u64,
}

/// The routes that write `derived_from` (12a): a task's report, its brief,
/// and an item the owner published into a place (the place rule).
pub const VIA_REPORT: &str = "report";
pub const VIA_BRIEF: &str = "brief";
pub const VIA_PUBLISH: &str = "publish";
/// A `Recall` node to each source it renders (M6 30b): the copy `node.reach`
/// counts as it counts a report's.
pub const VIA_RECALL: &str = "recall";
/// A task's arrangement, from each message it quotes (M5 27).
pub const VIA_ARRANGEMENT: &str = "arrangement";
/// A check task's arrangement, from the checked task's report, its claim
/// (M5 28a).
pub const VIA_CLAIM: &str = "claim";
/// The memory pass's gate (M6 31a): `same_entity` and `supersedes`.
pub const VIA_MEMORY: &str = "memory";
/// A glide's read (38b): the borrowed node, from each message it took.
pub const VIA_GLIDE: &str = "glide";

impl Edge {
    pub fn new(kind: EdgeKind, from: &str, to: &str, via: &str) -> Self {
        Self {
            kind: kind.as_str().into(),
            from: from.into(),
            to: to.into(),
            via: via.into(),
            at_ms: theseus_protocol::now_unix_ms(),
        }
    }

    /// The scope of every edge into `to`.
    pub fn scope_into(to: &str) -> String {
        format!("in:{to}")
    }

    /// Its record, for the frame that writes `from`.
    pub fn record(&self) -> anyhow::Result<NewRecord> {
        let key = format!("{}|{}|{}", self.kind, self.from, self.to);
        Ok(NewRecord::json(kinds::EDGE, Some(&key), self)?.scoped(&Self::scope_into(&self.to)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_edge_is_keyed_by_its_kind_and_scoped_into_its_target() {
        let e = Edge::new(EdgeKind::DerivedFrom, "msg_b", "msg_a", VIA_REPORT);
        let r = e.record().unwrap();
        assert_eq!(r.kind, kinds::EDGE);
        assert_eq!(r.key.as_deref(), Some("derived_from|msg_b|msg_a"));
        assert_eq!(r.scope.as_deref(), Some("in:msg_a"));
        let v: serde_json::Value = serde_json::from_slice(&r.payload).unwrap();
        assert_eq!(v["type"], "derived_from");
        assert_eq!(v["via"], "report");
        // An edge from before theseus-hco (a compilation's) still reads.
        let old: Edge = serde_json::from_str(
            r#"{"type":"derived_from","from":"cmp_2","to":"cmp_1","at_ms":1}"#,
        )
        .unwrap();
        assert_eq!(EdgeKind::named(&old.kind), Some(EdgeKind::DerivedFrom));
        assert_eq!(old.via, "");
        assert_eq!(EdgeKind::named("mentions"), None);
    }
}
