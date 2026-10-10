//! The import (theseus-0lrr.6): the operator's past history, from another
//! assistant platform, as imported sessions in the store.
//!
//! An outside pipeline writes episode files (`episode.rs`: JSON Lines, one
//! episode a line, format 1). Each episode becomes one **imported
//! session**: a `SessionRecord` with `imported` set, which is closed and
//! read-only. It has no execution, so the kernel never drives it, and
//! `turn.submit` refuses it (`refusal`), so it never takes a turn, is never
//! resumed, and is never compiled into a turn's context: it reaches a model
//! only as recall's testimony.
//!
//! - **Its id is its episode's** (`session_id_of`: `ses_ep` and the
//!   episode's 64 hex digits), and its nodes' ids are the session's
//!   with their index, so a retry of a batch writes the same records, and
//!   an episode already imported is found by one key's read.
//! - **Its place is private** under the place rule: it is the owner's own
//!   history, whatever place the episode names (a channel, a DM), so a
//!   shared place never recalls it (`TurnRunner::place_of` reads the id).
//!   The episode's place is kept as a label, for later disclosure
//!   decisions, as its other labels are: recorded facts, not enforcement.
//! - **Each message is a node** of origin `import` (`Body::Imported`: its
//!   source, unit and sha256, and its integrity), whose time is the
//!   message's own. Outside text (`integrity: outside`) is marked external
//!   by the index, so recall keeps it out unless the config admits
//!   external text, and frames it as such when it does.
//! - **The summary** is a node (`Body::ImportedSummary`) citing its
//!   messages' node ids.
//! - **The erase** (`write::erase`) tombstones a tag's every node and
//!   session (§5.6's erasure marker: `Body::Erased`, `ImportedFrom.erased`),
//!   and the index forgets them; its second half (`topics::unassign`) takes
//!   the erased sessions' memberships and the topics nothing else uses.
//! - **The topics** (`topics::assign`, theseus-anh3): the sessions' topic
//!   labels as the ontology's topics, a tree, and their memberships, by the
//!   owner's `import.topics`.
//! - **The people** (`people::assign_unless`, theseus-wy7y): the sessions'
//!   person authors and DM parties as the ontology's people, with their
//!   handles, and their memberships, by the owner's `import.people`.

pub mod catalog;
pub mod episode;
pub mod people;
pub mod topics;
pub mod write;

#[cfg(test)]
pub(crate) mod tests;
#[cfg(test)]
mod tests_catalog;
#[cfg(test)]
mod tests_people;
#[cfg(test)]
mod tests_topics;

use serde::{Deserialize, Serialize};

/// An imported session's id starts so. `new_id`'s ids are `ses_` and hex,
/// and `p` is no hex digit, so no other session's id can.
pub const SESSION_PREFIX: &str = "ses_ep";

/// The META key of a tag's counts (`TagCounts`).
pub fn tag_key(tag: &str) -> String {
    format!("import.tag.{tag}")
}

/// The scope an imported session's record is written under: a tag's
/// sessions are one range scan.
pub fn tag_scope(tag: &str) -> String {
    format!("import:{tag}")
}

/// Whether `session_id` is an imported session's: by its id alone, no read.
pub fn is_imported(session_id: &str) -> bool {
    session_id.starts_with(SESSION_PREFIX)
}

/// An episode's session id: `ses_ep` and its id's 64 hex digits
/// (`ep_<64 hex>`, checked by `episode::parse`), whole, so two episodes
/// never share one.
pub fn session_id_of(episode_id: &str) -> String {
    let hex = episode_id.strip_prefix("ep_").unwrap_or(episode_id);
    format!("{SESSION_PREFIX}{hex}")
}

/// A message's node id: its session's tail and its index.
pub fn node_id_of(session_id: &str, idx: u32) -> String {
    let tail = session_id
        .strip_prefix(SESSION_PREFIX)
        .unwrap_or(session_id);
    format!("imp_{tail}_{idx}")
}

/// An episode's summary's node id.
pub fn summary_id_of(session_id: &str) -> String {
    let tail = session_id
        .strip_prefix(SESSION_PREFIX)
        .unwrap_or(session_id);
    format!("imp_{tail}_summary")
}

/// Why `turn.submit` refuses `session_id`, when it is an imported session.
pub fn refusal(session_id: &str) -> Option<String> {
    is_imported(session_id).then(|| {
        format!(
            "{session_id} is an imported session: closed and read-only, it takes no turn. \
             Recall reaches it from a private place."
        )
    })
}

/// An imported session's place in a recalled item's header: `the imported
/// dm wren (openclaw-2026-10, openclaw-store)`, or its id when its record
/// does not read.
pub fn place_name(store: &crate::store::Store, session_id: &str) -> String {
    match store.get_session::<crate::session::SessionRecord>(session_id) {
        Ok(Some(r)) => match r.imported {
            Some(i) => match &i.place.name {
                Some(name) => format!(
                    "the imported {} {name} ({}, {})",
                    i.place.kind, i.tag, i.source
                ),
                None => format!("the imported {} ({}, {})", i.place.kind, i.tag, i.source),
            },
            None => session_id.to_string(),
        },
        _ => format!("the imported session {session_id}"),
    }
}

/// Nodes as a listing shows them (`session.history`, `node.list`): an
/// imported node by its newest record, so an erased one is its tombstone,
/// never the payload its earlier record still holds, and once. Any other
/// node as it was read: a listing with no imported node costs one pass.
pub fn shown(
    store: &crate::store::Store,
    nodes: Vec<(u64, crate::node::Node)>,
) -> anyhow::Result<Vec<(u64, crate::node::Node)>> {
    if !nodes.iter().any(|(_, n)| is_imported(&n.session_id)) {
        return Ok(nodes);
    }
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::with_capacity(nodes.len());
    for (p, n) in nodes {
        if !is_imported(&n.session_id) {
            out.push((p, n));
            continue;
        }
        let newest = match &n.body {
            crate::node::Body::Erased { .. } => (p, n),
            _ => store.get_node(&n.id)?.unwrap_or((p, n)),
        };
        if seen.insert(newest.1.id.clone()) {
            out.push(newest);
        }
    }
    Ok(out)
}

/// Whose words an imported message was, onto the node provenance the core
/// has: the operator's, an agent's, or outside text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Integrity {
    Operator,
    Agent,
    Outside,
}

impl Integrity {
    pub fn as_str(self) -> &'static str {
        match self {
            Integrity::Operator => "operator",
            Integrity::Agent => "agent",
            Integrity::Outside => "outside",
        }
    }
}

/// Where an episode happened, as the pipeline names it: kept as a label.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EpisodePlace {
    /// `dm`, `slack-channel`, `discord-channel`, `cli`, `cron`, `heartbeat`,
    /// or `file`.
    pub kind: String,
    /// As the source records it, or none (a CLI, a heartbeat, many
    /// channels).
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub id: Option<String>,
}

/// The pipeline's labels: recorded facts for later disclosure decisions,
/// never enforcement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EpisodeLabels {
    /// `personal`, `company-confidential`, `partner-confidential`, `public`.
    pub sensitivity: String,
    /// `partner-candidate:<codename>`.
    #[serde(default)]
    pub partner: Option<String>,
    #[serde(default)]
    pub topic: Vec<String>,
    /// One of the books (`diary`, `casebook`, ...).
    #[serde(default)]
    pub book_hint: Option<String>,
    /// The pipeline removed a credential (`⟦credential redacted⟧`).
    #[serde(default)]
    pub credential_redacted: bool,
}

/// The pipeline's triage of an episode (absent for curated sources).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EpisodeTriage {
    pub category: String,
    pub keep: f64,
    pub model: String,
}

/// An episode's span of time, in unix ms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AsOf {
    pub start_ms: u64,
    pub end_ms: u64,
}

/// A tombstone's receipt (§5.6): who ordered it, when, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Erased {
    pub at_ms: u64,
    pub by: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}

/// An imported session's provenance, on its `SessionRecord` (store format
/// 23): the episode it came from, its labels, and when it came in.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImportedFrom {
    pub tag: String,
    pub episode_id: String,
    /// The episode's own hash: the same id with another is refused.
    pub hash: String,
    pub source: String,
    #[serde(default)]
    pub agent: Option<String>,
    pub place: EpisodePlace,
    pub labels: EpisodeLabels,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub triage: Option<EpisodeTriage>,
    pub as_of: AsOf,
    /// Its messages, and whether a summary node was written.
    pub messages: u32,
    pub summary: bool,
    /// The file it was read from, as the client named it, and its line.
    pub file: String,
    pub line: u64,
    pub imported_at_ms: u64,
    /// Tombstoned by `import.erase`: its nodes' payloads are gone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub erased: Option<Erased>,
}

/// A tag's counts, a META record (`tag_key`), written in every frame of a
/// batch or an erase, as they stand after it: what `import.list` and health
/// show. `erased` is the tag's sessions erased, counted whole by each erase
/// (theseus-mce3).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TagCounts {
    pub tag: String,
    pub sessions: u64,
    pub nodes: u64,
    #[serde(default)]
    pub erased: u64,
    #[serde(default)]
    pub sources: std::collections::BTreeMap<String, u64>,
    #[serde(default)]
    pub first_ms: Option<u64>,
    #[serde(default)]
    pub last_ms: Option<u64>,
    pub updated_ms: u64,
}
