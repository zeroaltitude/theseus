//! Self-improvement's surfaces (theseus-pw1q.2, theseus-pw1q.4): the kill
//! switch (`self.halt`, `self.resume`), the ledger's view of what Theseus
//! changed about itself (`self.log`), and the weekly digest (`self.digest`).
//! Each name carries `Self`, since the web apps' types share one namespace.
//!
//! The rows a self step writes are declared here, in `SELF_ROWS`, with their
//! fields, before the rows that write them land: a kind becomes a
//! `LedgerKind` on the commit that first writes it (the reader rule), and
//! `self.log` reads every name in the table already. Every `self.*` row's
//! data carries `what`, `why`, `numbers` and `undo` beside its own fields,
//! so the log shows a new kind with no change of its own.

use serde::{Deserialize, Serialize};

/// A `self.*` row a later step writes: its kind's name, its own fields, and
/// what it records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelfRowKind {
    pub name: &'static str,
    /// Its own fields, beside `what`, `why`, `numbers` and `undo`.
    pub fields: &'static [&'static str],
    pub doc: &'static str,
}

/// The rows of every self step, by the step that writes them: the backlog
/// (self-backlog), the branch builder (self-pr), the review and the join
/// (self-join), the install, auto-revert, the owner's veto, the exams
/// (self-exams), the budget (self-budget), and the kill switch (this step,
/// which writes its two). `self.log` reads each by its name.
pub const SELF_ROWS: &[SelfRowKind] = &[
    SelfRowKind {
        name: "self.backlog.filed",
        fields: &["item", "title", "evidence", "source"],
        doc: "An item filed on the self backlog, with the evidence that filed it (a ledger row, \
              a health line, a judge report, a benchmark gap).",
    },
    SelfRowKind {
        name: "self.backlog.ranked",
        fields: &["item", "rank", "score", "was"],
        doc: "An item's place on the backlog moved: its rank, the score that placed it, and the \
              rank it had.",
    },
    SelfRowKind {
        name: "self.backlog.started",
        fields: &["item", "branch", "reserved_usd"],
        doc: "The top item started: the branch it builds on and what its budget reserved.",
    },
    SelfRowKind {
        name: "self.branch.built",
        fields: &["item", "branch", "head", "gate", "report", "cost_usd"],
        doc: "A self branch built and gated: its head commit, the gate's verdict, its frozen \
              report, and what it cost.",
    },
    SelfRowKind {
        name: "self.review.verdict",
        fields: &["branch", "head", "reviewer", "verdict", "planted_revert"],
        doc: "An independent review's verdict on a branch, from a different session, with its \
              planted revert's result.",
    },
    SelfRowKind {
        name: "self.joined",
        fields: &["branch", "head", "merge", "gate", "cost_usd"],
        doc: "A self branch joined main: the merge commit and the gate it passed.",
    },
    SelfRowKind {
        name: "self.installed",
        fields: &["merge", "binaries", "version"],
        doc: "A joined build installed: what was swapped in.",
    },
    SelfRowKind {
        name: "self.reverted",
        fields: &["merge", "revert", "signal", "window"],
        doc: "A join reverted on its own after a red: the signal that turned red, and the revert \
              commit.",
    },
    SelfRowKind {
        name: "self.vetoed",
        fields: &["merge", "revert", "by", "place"],
        doc: "The owner's veto: a join reverted by the owner's word.",
    },
    SelfRowKind {
        name: "self.exam.counted",
        fields: &["item", "family", "sealed", "reviewer"],
        doc: "An exam item counted after its independent review; a sealed one names no text.",
    },
    SelfRowKind {
        name: "self.budget.reserved",
        fields: &["branch", "reserved_usd", "day_usd"],
        doc: "A self step reserved its worst case on the self budget.",
    },
    SelfRowKind {
        name: "self.budget.settled",
        fields: &["branch", "spent_usd", "released_usd"],
        doc: "A reservation settled at what the step spent.",
    },
    SelfRowKind {
        name: "self.budget.stopped",
        fields: &["branch", "line", "line_usd", "spent_usd"],
        doc: "A self step stopped at a budget line ($10 a branch, $30 a day): which line, and \
              where it stood.",
    },
    SelfRowKind {
        name: "self.halted",
        fields: &["by", "place"],
        doc: "The kill switch thrown: every self step stops before its next phase.",
    },
    SelfRowKind {
        name: "self.resumed",
        fields: &["by", "place"],
        doc: "The owner, from a private place, released the kill switch.",
    },
];

/// `[self] mode`, as the daemon read it at its start.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum SelfMode {
    /// Nothing self-directed runs (the default).
    #[default]
    Off,
    /// Self steps run while the kill switch is released.
    Act,
}

/// The kill switch and the mode, as every surface shows them.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SelfState {
    pub mode: SelfMode,
    /// Whether the switch holds every self step. A store that has never seen
    /// the owner's resume is halted.
    pub halted: bool,
    /// The owner has never resumed: halted from the first day.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub never_resumed: bool,
    /// When the switch last moved, who moved it and from where, and why (a
    /// halt's words).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub via: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub why: Option<String>,
    /// What a self step would hear now: `off`, `halted` or `allowed`.
    pub gate: String,
}

/// `self.halt`'s params. Anyone who may speak to the daemon may halt.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SelfHaltParams {
    /// Why, in the halter's words.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub why: Option<String>,
    /// Who, as a label; set by the Discord binding.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    /// Set by the Discord binding: the channel and user it came from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<crate::DiscordOrigin>,
}

/// `self.resume`'s params: only the owner, from a private place, counts.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SelfResumeParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<crate::DiscordOrigin>,
    /// The `THESEUS_SESSION` of the shell that sent it, which the CLI sends:
    /// a job's shell never resumes, and its refusal is ledgered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub from_job: Option<String>,
}

/// `self.halt`'s and `self.resume`'s answer: the state after it, and
/// whether this call moved the switch (a halt of a halted switch does not).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SelfSwitchResult {
    pub state: SelfState,
    pub changed: bool,
}

/// `self.log`'s params.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SelfLogParams {
    /// Only rows at this time or after (unix ms).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub since_ms: Option<u64>,
    /// At most this many rows (default 50, at most 500).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub limit: Option<usize>,
}

/// One change Theseus made to itself, or one move of its switch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SelfLogRow {
    pub position: u64,
    pub at_unix_ms: u64,
    pub kind: String,
    /// What changed, in a sentence.
    pub what: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub why: Option<String>,
    /// The numbers it carries (a cost, a replay's fixed and broken, a share).
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    #[cfg_attr(test, ts(type = "unknown"))]
    pub numbers: serde_json::Value,
    /// The command that reverses it, where there is one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub undo: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub session_id: Option<String>,
}

/// `self.log`'s answer: the rows, newest first, and the switch now.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SelfLogResult {
    pub rows: Vec<SelfLogRow>,
    pub state: SelfState,
    /// More rows matched past `limit`.
    pub more: bool,
    /// The ledger's index is still being built after a start: nothing could
    /// be read yet, so the list is empty for now, not empty for good.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub building: bool,
}

/// `self.digest`'s params: the week that ends at `until_ms` (default now).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SelfDigestParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub until_ms: Option<u64>,
}

/// `self.digest`'s answer: the week's text, and its window.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SelfDigestResult {
    pub since_ms: u64,
    pub until_ms: u64,
    pub text: String,
    pub rows: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every declared row has a kind's form under `self.`, and one name.
    #[test]
    fn every_self_row_is_a_self_kind_once() {
        let mut names: Vec<&str> = SELF_ROWS.iter().map(|k| k.name).collect();
        assert!(names.iter().all(|n| n.starts_with("self.")));
        let n = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), n, "a self row declared twice");
        for k in SELF_ROWS {
            for f in ["what", "why", "numbers", "undo"] {
                assert!(!k.fields.contains(&f), "{}: {f} is every row's", k.name);
            }
        }
    }
}
