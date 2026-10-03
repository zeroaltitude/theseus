//! Places (the place rule, theseus-nbsh): every place a session speaks in is
//! private or shared. The rule itself is the core's (`places.rs`).

use serde::{Deserialize, Serialize};

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
