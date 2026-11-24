//! The owner's own instruction file, `<state_dir>/AGENTS.md`: the daemon
//! carries the version the owner last accepted, and a different file on disk
//! is named, not read, until the owner accepts it (`theseus own-file accept`).

/// The CLI's verb, the one place its name is written: `theseus own-file`
/// shows what differs, `theseus own-file accept` shows it and asks.
pub const COMMAND: &str = "own-file";

/// The words that name the accept, as every line that points to it says
/// them.
pub fn accept_command() -> String {
    format!("theseus {COMMAND} accept")
}

use serde::{Deserialize, Serialize};

/// `own_file.show`: no params.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct OwnFileShowParams {}

/// Where the file on disk stands against the accepted version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum OwnFileState {
    /// No version is accepted yet: nothing of the file is carried.
    NotAccepted,
    /// The accepted version cannot be read from the store: nothing is carried.
    Unreadable,
    /// The disk holds the accepted version (or no file, accepted as absent).
    Same,
    /// The disk holds another text than the accepted one.
    Differs,
    /// The accepted text is carried, and the file is gone from disk.
    Gone,
    /// No file was accepted, and one is on disk now.
    New,
}

/// The accepted version, as the show names it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct OwnFileAccepted {
    pub seq: u64,
    /// Its digest; none: the file was accepted as absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub digest: Option<String>,
    pub at_ms: u64,
    /// Its day on the daemon's clock, `2026-11-20`.
    pub on: String,
}

/// What `own_file.show` returns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct OwnFileShowResult {
    pub path: String,
    pub state: OwnFileState,
    /// The accepted version; none when none is, or it cannot be read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub accepted: Option<OwnFileAccepted>,
    /// The file on disk's size; none: no file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub bytes: Option<u64>,
    /// The digest an accept of what is on disk names: the disk's, empty for no file.
    pub digest: String,
    /// The changed lines with a little context, whole, or a summary that says it is one when they do not fit.
    /// None when nothing differs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub change: Option<String>,
}

/// `own_file.accept`: accept what is on disk, which must still be what the owner was shown (`digest`, from
/// `own_file.show`; empty for no file).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct OwnFileAcceptParams {
    pub digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<crate::DiscordOrigin>,
}

/// What `own_file.accept` returns: the version that stands now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct OwnFileAcceptResult {
    pub path: String,
    pub seq: u64,
    /// Whether this accept kept a new version; false when the disk already held the accepted one.
    pub kept: bool,
}
