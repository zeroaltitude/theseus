//! Health's sections for the store's refused reads (R4, theseus-15g) and the
//! last crash (Review 2's consideration 1), and the binary's build
//! (theseus-9o5n); `HealthResult` is in lib.rs.

use serde::{Deserialize, Serialize};

/// The store's reads (R4, theseus-15g). After the history check finds a
/// frame corrupt (the `store.verify` phase), a read of one of its records is
/// refused. A list read skips it instead of failing whole, and counts it
/// here, so a corrupt record degrades what reads it rather than stopping
/// every continuation, and says so.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct StoreStatus {
    /// Records list reads skipped since the daemon started, each once.
    pub refused_records: u64,
    /// The first of their WAL positions, lowest first.
    #[serde(default)]
    pub refused_positions: Vec<u64>,
    /// What repairs it, when any are refused: `theseusd restore --repair`
    /// with a copy of the store that holds the frame whole.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub repair: Option<String>,
}

/// The crash file a start found (Review 2's consideration 1): the release
/// build aborts on a panic, and its panic hook first writes the thread, the
/// location, and the message beside the store. The next start moves the
/// file into `crashes/` and says so (`server.crashed`). The message stays in
/// the file, as in the log.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct CrashStatus {
    pub at_unix_ms: u64,
    /// The panicking daemon's pid and build.
    pub pid: u32,
    pub version: String,
    pub thread: String,
    /// `file:line:column`.
    pub location: String,
    /// The crash file, under the state dir's `crashes/`.
    pub file: String,
    /// This start found it: the run before this one ended in it.
    pub this_start: bool,
}

/// A binary's build (theseus-9o5n): constants taken when it was compiled,
/// never computed at a start. Health and each `server.started` row name it,
/// so a start whose build differs from the one before it was an install,
/// not a restart of the same binary.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Build {
    /// The workspace's version (`CARGO_PKG_VERSION`).
    pub version: String,
    /// The git commit it was built from, in full; absent from a build made
    /// outside a git checkout without `THESEUS_COMMIT` set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub commit: Option<String>,
}
