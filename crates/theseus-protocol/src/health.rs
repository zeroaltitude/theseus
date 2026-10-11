//! Health's sections for the store's refused reads (R4, theseus-15g), the
//! last crash (Review 2's consideration 1), the binary's build
//! (theseus-9o5n), whether jobs can write the binary (review 2's
//! consideration 3), and the secrets (theseus-qa0; their sources,
//! theseus-n88g.1); `HealthResult` is in lib.rs.

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
    /// The heat cache of decoded nodes (M6 step 33): absent from a daemon
    /// before it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub node_cache: Option<NodeCacheHealth>,
}

/// The store's heat cache of decoded nodes (M6 step 33, theseus-6fn.13):
/// its bound (`[memory] node_cache_mb`; 0 is off), what it holds, and its
/// counts since the daemon started. A hit is a node served without a decode;
/// a miss, one decoded; `failed`, a stub whose node could not be read back.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct NodeCacheHealth {
    pub cap_bytes: u64,
    pub bytes: u64,
    pub entries: u64,
    pub hits: u64,
    pub misses: u64,
    pub decodes: u64,
    pub evictions: u64,
    pub failed: u64,
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

/// Where the vault's secrets stand (theseus-qa0, spec §2 FAST): the daemon
/// answers its socket before they resolve, and each consumer waits for its own.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SecretsStatus {
    /// `resolving` until every secret has settled, then `ready`, or `failed`
    /// with `failed` naming each one that did not resolve.
    pub state: String,
    #[serde(default)]
    pub ready: Vec<String>,
    #[serde(default)]
    pub resolving: Vec<String>,
    #[serde(default)]
    pub failed: Vec<SecretFailed>,
    /// How the vault was read: `inject` (one `op inject` for every reference),
    /// or `inject, then read` after a failed injection.
    #[serde(default)]
    pub method: Option<String>,
    /// Rounds run: the first, then one per retry of what failed.
    #[serde(default)]
    pub rounds: u32,
    /// When resolution began, and when its first round settled, in ms after
    /// the process started.
    #[serde(default)]
    pub started_ms: Option<u64>,
    #[serde(default)]
    pub settled_ms: Option<u64>,
    /// Until the next fetch of what failed.
    #[serde(default)]
    pub retry_in_ms: Option<u64>,
    /// The secrets whose values come from outside the vault, the recommended
    /// source (theseus-n88g.1): an `env:` or `file:` entry, by name and
    /// source. Empty when every secret is the vault's.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outside_vault: Vec<SecretSource>,
}

impl SecretsStatus {
    /// `resolving`, `ready`, or `failed a, b`: what health says in a word.
    pub fn summary(&self) -> String {
        match self.state.as_str() {
            "failed" => format!(
                "failed {}",
                self.failed
                    .iter()
                    .map(|f| f.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            "" => "unknown".into(),
            s => s.into(),
        }
    }

    /// `outside the vault: a (env), b (file)`, naming each secret whose value
    /// does not come from the vault, with its source; `None` when every one
    /// does (theseus-n88g.1).
    pub fn outside_vault_words(&self) -> Option<String> {
        (!self.outside_vault.is_empty()).then(|| {
            let each: Vec<String> = self
                .outside_vault
                .iter()
                .map(|s| format!("{} ({})", s.name, s.kind))
                .collect();
            format!("outside the vault: {}", each.join(", "))
        })
    }
}

/// A secret that did not resolve, and why (never a value).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SecretFailed {
    pub name: String,
    pub error: String,
}

/// A secret that comes from outside the vault, and from where (theseus-n88g.1):
/// `kind` is `env` (the daemon's environment) or `file` (a private file).
/// Never the value, the variable, or the path.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SecretSource {
    pub name: String,
    pub kind: String,
}

/// Health's `tasks` block (M5 28b; the parked-task invariant, theseus-vug).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TasksHealth {
    /// Each task in progress that cannot progress by itself: no running
    /// turn, queue place, job, wake of its own, or pending question younger
    /// than 24 hours. Read from the open executions alone.
    pub parked: Vec<ParkedTask>,
}

/// A task that waits on something that will not come by itself.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ParkedTask {
    /// The task's session.
    pub task_id: String,
    pub short: String,
    pub execution_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub title: Option<String>,
    /// Its execution's state: `waiting` or `blocked`.
    pub state: String,
    /// What holds it: `input` (a task gets none), `stopped`, `approval`,
    /// `budget` (a question older than 24 hours), `blocked`, or `nothing`
    /// (it waits with no wake).
    pub blocker: String,
    /// The blocker, said.
    pub detail: String,
    /// Since when (ms since the epoch): the question's, or the execution's
    /// last change.
    pub since_ms: u64,
}

/// The binary this daemon runs, and whether its jobs can write it (review
/// 2's consideration 3). At L0 a job runs as the daemon's user, so a binary
/// that user can write, or one in a directory it can write, is one a job can
/// replace, and the next start runs what it finds there.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct BinaryStatus {
    /// The path the next start runs ("" when it could not be read).
    pub path: String,
    /// `jobs_can_write` (the file, or its directory, is writable by the
    /// daemon's user), `ok`, or `unknown` (`detail` says why).
    pub state: String,
    /// What is writable, or why it is not known, in words.
    #[serde(default)]
    pub detail: String,
}
