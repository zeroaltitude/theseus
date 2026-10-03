//! Credential requests under L1 (M4 18d; design `m4-boundaries` §2.4,
//! decision 15): what a job's helper (`theseus-cred get <secret>`) and its
//! socket say to each other, health's counts, and the one wording of a
//! request that every surface shows.
//!
//! The socket's two shapes are not on the protocol's wire and have no
//! TypeScript: one line of JSON each way, the helper's `CredAsk` and the
//! daemon's `CredAnswer`. An answer's value is never logged: its `Debug`
//! names its length alone.

use serde::{Deserialize, Serialize};

/// What a request asks for. `aws` is the AWS seam (§2.4): its session
/// credentials will come through the same socket, at the same posture rule,
/// and until then it answers "not yet".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CredKind {
    Secret,
    Aws,
}

impl CredKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Secret => "secret",
            Self::Aws => "aws",
        }
    }
}

/// The helper's request: one line on the job's socket.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CredAsk {
    pub kind: CredKind,
    /// The `[secrets]` name.
    pub name: String,
}

/// The daemon's answer: the value, or why not.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CredAnswer {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl std::fmt::Debug for CredAnswer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CredAnswer")
            .field(
                "value",
                &self.value.as_ref().map(|v| format!("<{} bytes>", v.len())),
            )
            .field("error", &self.error)
            .finish()
    }
}

impl CredAnswer {
    pub fn refused(why: impl Into<String>) -> Self {
        Self {
            value: None,
            error: Some(why.into()),
        }
    }
}

/// Health's credential requests since the daemon started (M4 18d): how many
/// were asked, granted, and declined, and how many wait for the operator now.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct CredRequests {
    pub since_start: u64,
    pub granted: u64,
    pub declined: u64,
    pub waiting: u64,
}

/// How a request reads on every surface: "job a1b2c3 (`cargo publish`, L1)
/// asked for `crates_io_token`". `short` is the job's short id.
pub fn asked(short: &str, command: &str, secret: &str) -> String {
    format!("job {short} (`{command}`, L1) asked for `{secret}`")
}
