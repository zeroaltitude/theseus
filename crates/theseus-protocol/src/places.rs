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
    /// bindings file binds with `private = true`: everything.
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

/// Health's `places` block: each place Theseus speaks in, and its class.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
