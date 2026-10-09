//! The ontology's wire shapes (spec §4.1a; M4 design §2.8; theseus-8kk.1):
//! `ontology.list`, and the operator's three writes, `ontology.category.add`,
//! `ontology.guidance.set`, and `ontology.membership.set`. The records
//! themselves, their rules, and the compile walk are `theseus-ontology`'s and
//! the core's; these are what a client sends and reads.

use serde::{Deserialize, Serialize};

use crate::DiscordOrigin;

/// `ontology.list`: the kinds table, every category with its guidance and
/// its count of sessions, and memberships: one session's (given ones from
/// its place, and interpreted ones), or, without a session, every
/// interpreted one stored.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct OntologyListParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub session_id: Option<String>,
    /// `false` leaves the memberships out (each category's `members` still
    /// counts them): a reader of the tree alone, at an import's size (tens of
    /// thousands of memberships), reads only the tree. Default `true`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub memberships: Option<bool>,
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
    /// The sessions whose stored memberships hold it (an interpreted kind's;
    /// a given one is read from a place, and counts none).
    #[serde(default)]
    #[cfg_attr(test, ts(type = "number"))]
    pub members: u64,
    /// A person's handles (theseus-wy7y): `discord:<id>`, `slack:<id>`,
    /// `email:<addr>`, `name:<display name>`; a DM's person holds its
    /// `discord:<id>` without storing it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[cfg_attr(test, ts(as = "Option<Vec<String>>", optional))]
    pub handles: Vec<String>,
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
    /// `transport` (given, read from the session's place, never stored),
    /// `operator`, or `import` (an imported session's labels, by
    /// `import.topics`).
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
    /// A person's handles (theseus-wy7y): `discord:<id>`, `slack:<id>`,
    /// `email:<addr>`, `name:<name>`. A handle another person holds exactly
    /// (not a name) merges this one into that person instead.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[cfg_attr(test, ts(as = "Option<Vec<String>>", optional))]
    pub handles: Vec<String>,
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

/// `ontology.proposals` (M5 28b): what `categorize.v1`, in shadow, proposed
/// and the operator has not answered: each answered judgment whose `topic`
/// names a topic its session is not in, or `new_topic`, newest first. Jev
/// writes no membership; the operator accepts or rejects each.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct OntologyProposalsParams {
    /// Only this session's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub session_id: Option<String>,
    /// At most this many (default 50).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct OntologyProposalsResult {
    pub proposals: Vec<OntologyProposal>,
    /// Unanswered proposals past `limit`, left out.
    #[serde(default)]
    pub more: u32,
}

/// One proposal: a `categorize.v1` judgment's `topic` answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct OntologyProposal {
    /// The judgment (`jdg_…`), which `ontology.proposal.accept` and
    /// `ontology.proposal.reject` name.
    pub judgment: String,
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub session_title: Option<String>,
    /// The topic it proposes (`topic:harbor`); absent for `new_topic`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub topic: Option<String>,
    /// The topic's name, as the ontology holds it now.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub topic_name: Option<String>,
    /// Jev's answer was `new_topic`: the messages share a subject no topic
    /// covers, and the operator names it at accept.
    pub new_topic: bool,
    /// The top choice's probability.
    pub confidence: f64,
    /// `act`, `confirm`, or `escalate`, by the pack's thresholds.
    pub band: String,
    /// When it was judged.
    pub at_ms: u64,
}

/// `ontology.proposal.accept`: the operator's yes to a proposal. The
/// session joins the topic (an `operator`-origin membership, through the
/// ontology's write path) and the judgment gets its `judge.label` row, in
/// one frame. For `new_topic`, `topic` names the topic: an existing one, or
/// a new one made in the same frame, with `description`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct OntologyProposalAcceptParams {
    pub judgment: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub topic: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<DiscordOrigin>,
}

/// `ontology.proposal.reject`: the operator's no; the judgment gets its
/// `judge.label` row, and nothing else changes.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct OntologyProposalRejectParams {
    pub judgment: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<DiscordOrigin>,
}

/// What an accept or a reject wrote.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct OntologyProposalAnswered {
    pub judgment: String,
    pub session_id: String,
    /// The label row's key (`lbl_…`).
    pub label_id: String,
    /// `accepted` or `rejected`.
    pub label: String,
    /// The topic the session joined (accept).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub topic: Option<String>,
    /// The session's interpreted memberships after it.
    pub memberships: Vec<OntologyMembership>,
}

/// `ontology.person.merge` (theseus-wy7y): two people who are one. `survivor`
/// keeps its id and takes `absorbed`'s handles, memberships and guidance;
/// `absorbed` is written as merged into it, and the `ontology.merged` row
/// holds what moved, which `ontology.person.unmerge` reads to undo it. A DM's
/// person (the transport's) always survives. The owner's act, from a private
/// place.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct OntologyPersonMergeParams {
    /// Each by id, or by its name among the people.
    pub absorbed: String,
    pub survivor: String,
    /// `ontology.person.unmerge`: undo the newest merge of `absorbed` (an id),
    /// from its row; `survivor` is then ignored.
    #[serde(default)]
    pub undo: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<DiscordOrigin>,
}

/// What a merge, or its undo, did.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct OntologyPersonMerged {
    pub survivor: OntologyCategory,
    pub absorbed: String,
    /// The sessions whose person lists moved.
    #[cfg_attr(test, ts(type = "number"))]
    pub sessions: u64,
    pub guidance_moved: bool,
    /// True for an undo.
    #[serde(default)]
    pub undone: bool,
}

/// `ontology.proposal.accept_all` (theseus-wy7y): the operator's yes to every
/// unanswered proposal of a kind at or above a confidence, each as
/// `ontology.proposal.accept` would take it (a proposal naming a new topic
/// or person with no name is left for one at a time).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct OntologyProposalAcceptAllParams {
    /// `topic` or `person`; absent: both.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub kind: Option<String>,
    /// At least this top-choice probability (default 0).
    #[serde(default)]
    pub min_confidence: f64,
    /// Exactly these judgments (the cockpit's selection), else every match.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[cfg_attr(test, ts(as = "Option<Vec<String>>", optional))]
    pub judgments: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<DiscordOrigin>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct OntologyProposalAcceptAllResult {
    pub accepted: Vec<String>,
    /// Matching proposals left unanswered, each with why.
    pub left: Vec<String>,
}
