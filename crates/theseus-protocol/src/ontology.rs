//! The ontology's wire shapes (spec §4.1a; M4 design §2.8; theseus-8kk.1):
//! `ontology.list`, and the operator's three writes, `ontology.category.add`,
//! `ontology.guidance.set`, and `ontology.membership.set`. The records
//! themselves, their rules, and the compile walk are `theseus-ontology`'s and
//! the core's; these are what a client sends and reads.

use serde::{Deserialize, Serialize};

use crate::DiscordOrigin;

/// `ontology.list`: the kinds table, every category with its guidance, and
/// memberships: one session's (given ones from its place, and interpreted
/// ones), or, without a session, every interpreted one stored.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct OntologyListParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct OntologyListResult {
    /// By precedence, lowest first: the order the compile walk admits them.
    pub kinds: Vec<OntologyKind>,
    /// The category tree, depth first: the roots by name, each followed by
    /// its children.
    pub categories: Vec<OntologyCategory>,
    pub memberships: Vec<OntologyMembership>,
}

/// A row of the kinds table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct OntologyKind {
    pub name: String,
    /// `given` (the transport's facts) or `interpreted`.
    pub basis: String,
    /// The origins that may assign a membership: `transport`, `operator`.
    pub assigned_by: Vec<String>,
    /// How many a session may hold: a number, or `many`.
    pub per_session: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub parent: Option<String>,
    pub precedence: u32,
    /// `chain` or `intent_line`.
    pub rule: String,
    #[serde(default)]
    pub description: String,
    pub version: u32,
    pub added_by: String,
}

/// A category, with its guidance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct OntologyCategory {
    /// `<kind>:<local>`: `topic:theseus`, `channel:<id>`.
    pub id: String,
    pub kind: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub parent: Option<String>,
    /// How deep it nests: a root is 1.
    pub depth: u32,
    #[serde(default)]
    pub description: String,
    pub added_by: String,
    /// Absent when it has none, or its guidance was taken away (empty text).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub guidance: Option<OntologyGuidance>,
}

/// A category's guidance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct OntologyGuidance {
    pub category: String,
    /// Empty: no guidance.
    pub text: String,
    /// 1 when first written, and one more at each change.
    pub version: u32,
    /// The first 16 hex digits of the SHA-256 of `text`, as a compile's
    /// manifest records it.
    pub digest: String,
    pub added_by: String,
}

/// A session's membership in a category.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct OntologyMembership {
    pub session_id: String,
    pub kind: String,
    pub category: String,
    /// `transport` (given, read from the session's place, never stored) or
    /// `operator`.
    pub origin: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub confidence: Option<f32>,
    pub as_of_ms: u64,
}

/// `ontology.category.add`: the operator declares a category of an
/// interpreted kind (a topic). Its id is made from its name. Given kinds'
/// categories come from the transport, and are refused here.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct OntologyCategoryAddParams {
    /// Default `topic`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub kind: Option<String>,
    pub name: String,
    /// The parent: its id, or its name among its kind's categories.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub parent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub description: Option<String>,
    /// Who made it, as a label. Default: the connection. It names and
    /// proves nothing; the connection's surface decides.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<DiscordOrigin>,
}

/// `ontology.guidance.set`: a category's guidance, replaced whole. Empty
/// text takes it away. A session whose compilation carries it recompiles
/// once (`system_changed`) at its next turn.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct OntologyGuidanceSetParams {
    /// Its id, or its name among the topics.
    pub category: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<DiscordOrigin>,
}

/// `ontology.membership.set`: add a session to categories of interpreted
/// kinds, or take it out. Given kinds' memberships are the session's place,
/// and are refused here. The change waits for the session's next recompile.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct OntologyMembershipSetParams {
    pub session_id: String,
    /// Categories, each by id or by its name among the topics.
    #[serde(default)]
    pub add: Vec<String>,
    #[serde(default)]
    pub remove: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<DiscordOrigin>,
}

/// What `ontology.membership.set` left: the session's interpreted
/// memberships, every kind's.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct OntologyMembershipResult {
    pub session_id: String,
    pub memberships: Vec<OntologyMembership>,
    /// The lists it wrote: none when nothing changed.
    pub changed: Vec<String>,
}
