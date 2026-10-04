//! Places (the place rule, theseus-nbsh): every place a session speaks in is
//! private or shared. The rule itself is the core's (`places.rs`).

use serde::{Deserialize, Serialize};

use crate::DiscordOrigin;

/// A place's class.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaceClass {
    /// The CLI, the web UI, a DM with an owner, and a guild channel the
    /// bindings file binds with `private = true`, or in a guild it trusts
    /// whole: everything.
    Private,
    /// Every other place: its own conversation, the public tools, files only
    /// under `public_paths`, and only the context files marked public.
    Shared,
}

impl PlaceClass {
    pub fn as_str(self) -> &'static str {
        match self {
            PlaceClass::Private => "private",
            PlaceClass::Shared => "shared",
        }
    }
}

/// A place's ceiling (step 38a, theseus-ext.3): what the operator's bindings
/// file lets the place have, beneath what the place rule allows. It narrows
/// and never widens: a shared place's ceiling never offers it a private tool.
/// Each field absent is no narrowing.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PlaceCeiling {
    /// No call here runs looser than this: `open`, `notify`, or `approve`.
    /// A call's posture is the strictest of the config's, a tightening, this,
    /// and an external-text hold; never a refusal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub posture_floor: Option<String>,
    /// The tool families offered here (`fs`, `git`, `web`, `proc`, …), and
    /// MCP servers as `mcp:<server>`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub tools: Option<Vec<String>>,
    /// The place's session's spend limit is the lower of this and `[kernel]
    /// spend_limit_usd`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub spend_limit_usd: Option<f64>,
    /// The place's model, as a profile name, unless a turn names one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub profile: Option<String>,
}

impl PlaceCeiling {
    /// Nothing narrowed.
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }
}

/// A guild the bindings file binds places in, and the operator's word on it
/// (step 38a): a trusted guild's channels are private unless one says not.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuildInfo {
    pub id: String,
    /// Its label in the bindings file, if it gives one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub trusted: bool,
}

/// Health's `places` block: each place Theseus speaks in, and its class.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PlacesHealth {
    /// The local surfaces first (the CLI and the web UI), then each place
    /// the bindings file binds, in its order.
    pub places: Vec<PlaceInfo>,
    /// `public_paths`, as configured: the trees a shared place's file tools
    /// may reach. Empty: a shared place gets no file tools.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub public_paths: Vec<String>,
}

/// One place and its class.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlaceInfo {
    /// `cli`, `web`, `discord:dm:<user id>`, or `discord:channel:<id>`.
    pub place: String,
    /// How it is named: `CLI`, `web`, `DM @eddie`, `#openclaw`.
    pub name: String,
    pub class: PlaceClass,
    /// A guild channel bound `private = true`, as the binding read it at its
    /// start: the people besides the owner who can view it, by name. Absent
    /// until it is read, or when it cannot be (`unchecked`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub others: Option<Vec<String>>,
    /// Why who can view it could not be read: the Server Members intent is
    /// off, or Discord refused the read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub unchecked: Option<String>,
    /// A private guild channel in a guild the operator trusts whole (the
    /// bindings file's `private = true` beside `guild_id`, theseus-rdqg):
    /// the operator's word covers the guild, so who can view it is not read.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub trusted_guild: bool,
    /// A guild channel's guild id (step 38a).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub guild: Option<String>,
    /// What the bindings file narrows here, when it narrows anything.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub ceiling: Option<PlaceCeiling>,
}

/// `place.publish` (the place rule's publish, theseus-nbsh): the owner, from
/// a private place, puts one item into a place's conversation: a node by id,
/// a file by path, or a message. Exactly one of the three.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct PlacePublishParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub node_id: Option<String>,
    /// A file the owner can read, absolute or `~/`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub text: Option<String>,
    /// The place: `#openclaw`, `discord:channel:<id>`, or its id.
    pub to: String,
    /// The owner's words above it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub note: Option<String>,
    /// Who published it, as a label. Default: the connection. It names and
    /// proves nothing; the connection's surface decides.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<DiscordOrigin>,
}

/// What `place.publish` wrote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct PublishResult {
    /// `discord:channel:<id>`.
    pub place: String,
    /// `#openclaw`.
    pub name: String,
    pub class: PlaceClass,
    /// The place's session, and the node written there.
    pub session_id: String,
    pub node_id: String,
    /// What it was, in words: `the file /w/notes.md`, `fs.read's result trs_…`.
    pub what: String,
    pub bytes: u64,
    /// The first 16 hex digits of the SHA-256 of what was published.
    pub digest: String,
}
