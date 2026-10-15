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
        fields: &["item", "branch", "reserved_usd", "session"],
        doc: "The top item started: the branch it builds on and what its budget reserved.",
    },
    SelfRowKind {
        name: "self.backlog.dropped",
        fields: &["item", "cause", "by", "reason"],
        doc: "An item left the backlog: the owner's drop (its cause is never filed again), or \
              the lowest-ranked one past the backlog's size.",
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
        name: "self.join.stopped",
        fields: &["branch", "head", "phase"],
        doc: "A self-join stopped before a phase (theseus-pw1q.9): a halt, a keel class waiting \
              on the owner's card, a red gate on the merge, no signing key, or a lost race.",
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
        name: "self.exam.rejected",
        fields: &["item", "family", "sealed", "reviewer", "reason"],
        doc: "A candidate exam item the independent review did not call fair, dropped, with the \
              reviewer's reason; a sealed one names no text.",
    },
    SelfRowKind {
        name: "self.exam.pass",
        fields: &[
            "day",
            "candidates",
            "counted",
            "rejected",
            "sealed_dropped",
            "unreviewed",
            "capped",
            "error",
        ],
        doc:
            "One night's exam pass: what the day's sources offered, what counted, what the review \
              rejected, the sealed items held unwritten and dropped for want of a sealed store, \
              the candidates no reviewer answered for, and those the day's cap left out.",
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
        name: "self.graduate.proposed",
        fields: &[
            "item",
            "extension",
            "digest",
            "calls",
            "failures",
            "window_days",
            "keel",
        ],
        doc: "A loaded extension used often and never failing became a self item to port it into \
              core: its counts over the window, the manifest digest it was counted for, and the \
              keel class that holds the port for the owner, if any.",
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
    SelfRowKind {
        name: "self.narrow.proposed",
        fields: &[
            "role",
            "actions_before",
            "actions_after",
            "evidence_days",
            "analyzer_findings",
            "proposal",
        ],
        doc: "A narrower policy for a hand role, built from what CloudTrail showed it calling and \
              checked by Access Analyzer and the simulator, sent to the owner's card. Nothing is \
              applied without the owner's yes.",
    },
    SelfRowKind {
        name: "self.narrow.applied",
        fields: &["role", "proposal", "stack", "change_set", "by"],
        doc: "The owner applied a narrowed policy through the hands stack's change set; a week's \
              watch for AccessDenied begins.",
    },
    SelfRowKind {
        name: "self.narrow.declined",
        fields: &["role", "proposal", "by"],
        doc: "The owner said not now to a narrowing proposal; it is asked again no sooner than a \
              week later.",
    },
    SelfRowKind {
        name: "self.narrow.red",
        fields: &["role", "proposal", "denied", "count"],
        doc: "A role that was narrowed was denied a call in the week after the apply: the denied \
              call, and the owner's one-step revert.",
    },
    SelfRowKind {
        name: "self.narrow.reverted",
        fields: &["role", "proposal", "stack", "change_set", "by"],
        doc: "The owner reverted a narrowing to the policy before it, through a change set.",
    },
    SelfRowKind {
        name: "self.pipeline.started",
        fields: &["item", "builder", "session"],
        doc: "The pipeline started a build of the top ready item (theseus-pw1q.16): its builder, \
              its session, and the slots and room it started in.",
    },
    SelfRowKind {
        name: "self.pipeline.held",
        fields: &["hold"],
        doc: "The pipeline held its next start: the slots, the disk, memory or CPU pressure, or \
              the self budget's day line. Written when the reason changes.",
    },
    SelfRowKind {
        name: "self.pipeline.returned",
        fields: &["item", "branch", "head", "findings"],
        doc: "A rejected review sent an item back to ready, once, with the reviewer's findings.",
    },
    SelfRowKind {
        name: "self.pipeline.parked",
        fields: &["item", "rejections"],
        doc: "A second rejection: the item left the pipeline for the owner's view.",
    },
    SelfRowKind {
        name: "self.pipeline.queued",
        fields: &["item", "branch", "head"],
        doc: "A passed review: the branch waits for the join lock.",
    },
    SelfRowKind {
        name: "self.pipeline.cleared",
        fields: &["item", "trees", "reason"],
        doc: "A build's trees deleted, once its branch joined or it was dropped or ended.",
    },
    SelfRowKind {
        name: "self.pipeline.ended",
        fields: &["item", "session", "reason"],
        doc: "A build ended with no green branch: its turn ended, or a halt or a drop stopped it.",
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
    /// The self budget's day, as health's `self` line shows it
    /// (theseus-pw1q.3): what the day has spent (settled and reserved) and
    /// its line. Absent while the mode is off, and from a daemon before it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub day_spent_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub day_line_usd: Option<f64>,
    /// The sealed holdout sets (theseus-pw1q.5): present when `[self]
    /// sealed_dir` is set, as counts only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub sealed: Option<SealedState>,
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
    /// The self budget: the day's spend and each branch's (theseus-pw1q.3).
    /// Absent while the mode is off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub budget: Option<SelfBudgetView>,
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

// self-budget (theseus-pw1q.3): the spend lines as `self.log` shows them.

/// The self budget now: the local day's spend against its line, and each
/// branch's against its own.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SelfBudgetView {
    /// The local day the spend is for, `2026-10-14`.
    pub day: String,
    /// What the day has spent, settled and reserved together, and its line.
    pub day_spent_usd: f64,
    pub day_line_usd: f64,
    /// The per-branch line.
    pub branch_line_usd: f64,
    /// The branches with a spend, the most recent first.
    pub branches: Vec<SelfBranchSpend>,
}

/// One branch's spend on the self budget.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SelfBranchSpend {
    pub branch: String,
    /// Settled and reserved together.
    pub spent_usd: f64,
    /// The part of it still reserved (a call in flight).
    pub open_usd: f64,
    /// It stopped at its line.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub stopped: bool,
}

// sealed-holdouts (theseus-pw1q.5): health's view of the sealed sets.

/// `[self] sealed_dir` is set: how many items each sealed set holds. Counts
/// only: an item's text, its id and its family never ride the wire.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SealedState {
    /// Items per set, by the set's name.
    pub sets: std::collections::BTreeMap<String, u64>,
}

// ---- the backlog (self-backlog, theseus-pw1q.7) ----

/// `self.backlog`'s params: a read.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SelfBacklogParams {
    /// Also the items that left the backlog (dropped, or started and ended).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub all: bool,
}

/// One item of the self backlog: a task Theseus filed for itself, with the
/// evidence that filed it and its place.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SelfBacklogItem {
    /// `tsk_…`.
    pub id: String,
    /// Its place, from 1; 0 for an item that has left the backlog.
    pub rank: u32,
    /// The deterministic score that placed it (milli-points).
    pub score: u64,
    pub title: String,
    pub objective: String,
    /// The task's state: `proposed` (waiting its turn), `waiting_human` (a
    /// keel item), `in_progress`, or a closed one.
    pub state: String,
    /// The cause's stable key; a dropped cause is never filed again.
    pub cause: String,
    /// The keel class that holds it for the owner's yes, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub keel: Option<String>,
    /// How many times the cause was seen, over every night.
    pub count: u64,
    /// The rows and lines that filed it, oldest first.
    pub evidence: Vec<String>,
    /// The owner moved it to this rank.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pinned: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub session: Option<String>,
    pub filed_ms: u64,
}

/// `self.backlog`'s answer: the ranked list, and the causes the owner dropped.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SelfBacklogResult {
    pub items: Vec<SelfBacklogItem>,
    /// Causes the owner dropped: never filed again.
    pub dropped: Vec<String>,
    pub state: SelfState,
    /// The open items allowed (`[self] backlog_max_open`).
    pub max_open: u32,
    /// The last night's read, unix ms; none before the first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub read_ms: Option<u64>,
}

/// `self.drop`'s params: the owner's, by the item's id or the end of it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SelfDropParams {
    pub item: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<crate::DiscordOrigin>,
    /// The `THESEUS_SESSION` of the shell that sent it: a job's never counts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub from_job: Option<String>,
}

/// `self.drop`'s answer.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SelfDropResult {
    pub item: String,
    pub title: String,
    /// The cause that is now never filed again.
    pub cause: String,
    /// False when the item had left the backlog already.
    pub changed: bool,
}

/// `self.move`'s params: the owner's. Rank 1 is the top.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SelfMoveParams {
    pub item: String,
    pub rank: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<crate::DiscordOrigin>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub from_job: Option<String>,
}

/// `self.move`'s answer: the item's rank before and after.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SelfMoveResult {
    pub item: String,
    pub title: String,
    pub was: u32,
    pub rank: u32,
}

// ---- self-exams (theseus-pw1q.11) ----

/// `self.exams`' params: none yet.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SelfExamsParams {}

/// One family's counted items, by half. A sealed item is only ever a number.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SelfExamFamily {
    pub family: String,
    pub open: u64,
    pub sealed: u64,
}

/// One of today's candidates and its review. The text of an open item is
/// shown; a sealed one is its id and verdict only.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SelfExamToday {
    pub item: String,
    pub family: String,
    pub sealed: bool,
    /// `counted`, `rejected` or `dropped`.
    pub verdict: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub reviewer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub reason: Option<String>,
    /// An open item's task, as the exam shows it; never a sealed item's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub task: Option<String>,
}

/// `self.exams`' answer: the counted items by family and half, and today's
/// candidates with their reviews.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SelfExamsResult {
    pub families: Vec<SelfExamFamily>,
    pub today: Vec<SelfExamToday>,
    /// Sealed items held unwritten and dropped today for want of a sealed store.
    pub sealed_dropped: u64,
    pub state: SelfState,
}

// ---- auto-revert and the owner's veto (theseus-pw1q.10) ----

/// `self.revert`'s params: the owner's veto of a self-join, by its merge commit
/// (a whole sha or a unique start of at least 7 characters).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SelfRevertParams {
    pub merge: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<crate::DiscordOrigin>,
    /// The `THESEUS_SESSION` of the shell that sent it: a job's never counts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub from_job: Option<String>,
}

/// `self.revert`'s answer: the revert commit now on main.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SelfRevertResult {
    /// The self-join's merge commit that was undone.
    pub merge: String,
    /// The signed revert commit pushed to main.
    pub revert: String,
    /// The backlog item that was dropped and stays dropped, if it was still on the backlog.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub item: Option<String>,
    /// False when the merge had been reverted already.
    pub changed: bool,
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

// ---------------------------------------------------------------- self-pr (theseus-pw1q.8)
// The branch builder: `self.branches`, the trees and the `self/*` branches
// with their verdicts and frozen reports. `theseus self branches` reads it.

/// `self.branches`'s params: none yet.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SelfBranchesParams {}

/// One self branch: its tree while one is open, its head, the gate's last
/// verdict, and its frozen report.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SelfBranch {
    /// `self/<date>-<slug>`.
    pub branch: String,
    /// The backlog item it builds, as its report names it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub item: Option<String>,
    /// Its head commit in the repo; none once the branch is gone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub head: Option<String>,
    /// The gate's verdict as its report records it: `green`, `red`, `keel`,
    /// or where a halt stopped it; none before a gate ran.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub gate: Option<String>,
    /// Its frozen report's path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub report: Option<String>,
    /// Its open worktree's path, while one is open.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub tree: Option<String>,
}

/// `self.branches`'s answer: the repo and remote it builds from and to, and
/// every branch, newest name first.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SelfBranchesResult {
    /// `[self] repo`; none: no self branches.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub repo: Option<String>,
    /// `[self] remote`; none: branches stay local.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub remote: Option<String>,
    pub branches: Vec<SelfBranch>,
    pub state: SelfState,
}

// self-view (theseus-pw1q.19): the switch's move, as a notification.

/// `self.changed`'s params: the kill switch moved, so a surface showing the
/// self log reads it again. The row's kind and when; the rows are `self.log`'s.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SelfChanged {
    /// The row written: `self.halted` or `self.resumed`.
    pub kind: String,
    pub at_ms: u64,
}

// ---------------------------------------------------------------- self-narrow (theseus-pw1q.15)
// Narrower policies for the hand roles, from what CloudTrail showed them
// calling. `theseus self narrow` reads and answers them; an IAM change is
// the owner's alone.

/// `self.narrow`'s params. `action` is `list` (the default), `run` (look at
/// a role now: `profile`, `owner` or `read`), or the owner's `apply`,
/// `decline` and `revert`, each naming a `proposal` by its id or the end of it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SelfNarrowParams {
    #[serde(default)]
    pub action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub proposal: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub profile: Option<String>,
    /// With `run`: only look. The reads are made and the proposal is printed;
    /// nothing is kept, posted or changed, and the self gate does not hold it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub dry: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<crate::DiscordOrigin>,
    /// The `THESEUS_SESSION` of the shell that sent it: a job's never counts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub from_job: Option<String>,
}

/// One narrowing proposal, in a list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SelfNarrowItem {
    pub id: String,
    pub role: String,
    /// `open`, `applied`, `declined` or `reverted`.
    pub state: String,
    pub at_ms: u64,
    pub actions_before: u64,
    pub actions_after: u64,
    pub evidence_days: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub applied_ms: Option<u64>,
}

/// `self.narrow`'s answer: the words to print, and the proposals.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SelfNarrowResult {
    pub text: String,
    pub proposals: Vec<SelfNarrowItem>,
    pub state: SelfState,
}

// ---------------------------------------------------------------- self-pipeline (theseus-pw1q.16)
// The pipeline between the backlog, the branch builder and the self-join:
// `self.pipeline`, its three queues, what holds its next start, and the
// week's measures. `theseus self pipeline [--week]` reads it.

/// `self.pipeline`'s params: `week` adds the week's measures.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SelfPipelineParams {
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub week: bool,
}

/// One item in one of the pipeline's queues.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SelfPipelineItem {
    pub item: String,
    pub title: String,
    /// `ready`, `building`, `reviewing`, `joining` or `held`.
    pub queue: String,
    /// Its place in its queue, from 1.
    pub place: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub session: Option<String>,
    /// When it entered its queue (unix ms), where known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub since_ms: Option<u64>,
    /// Reviews that rejected it so far.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub rejections: u32,
    /// Why it waits, for one held or returned.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub why: Option<String>,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

/// What the capacity guard read, and why the next start waits, if it does.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SelfPipelineRoom {
    pub slots: u32,
    pub building: u32,
    pub trees: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub free_gb: Option<f64>,
    pub min_free_gb: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub memory_psi: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub cpu_psi: Option<f64>,
    /// `slots`, `disk`, `memory`, `cpu`, `budget`, or none: a start may go.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub hold: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub why: Option<String>,
}

/// One day of the week's measures.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SelfPipelineDay {
    /// `YYYY-MM-DD`, the local day.
    pub day: String,
    pub started: u32,
    pub built: u32,
    pub accepted: u32,
    pub rejected: u32,
    pub joined: u32,
    pub reverted: u32,
    /// What its built branches and reviews cost.
    pub cost_usd: f64,
}

/// The week's measures (rsi-plan): rows a day, the accept rate, regressions
/// at the next rerun, and the cost of a row against today's.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SelfPipelineWeek {
    /// Oldest first, today last.
    pub days: Vec<SelfPipelineDay>,
    /// Joined rows a day, over the week.
    pub rows_a_day: f64,
    /// Accepted reviews over all verdicts; none before a verdict.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub accept_rate: Option<f64>,
    /// Joins reverted on their own or vetoed (`self.reverted`, `self.vetoed`).
    pub regressions: u32,
    /// The week's cost over its joined rows; none before a join.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub cost_a_row_usd: Option<f64>,
    /// Today's, to set against it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub today_cost_a_row_usd: Option<f64>,
}

/// `self.pipeline`'s answer: the queues, the room, today's spend and rows,
/// and the week's measures when asked.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SelfPipelineResult {
    pub state: SelfState,
    /// The builder in use: `local`.
    pub builder: String,
    pub ready: Vec<SelfPipelineItem>,
    pub building: Vec<SelfPipelineItem>,
    pub reviewing: Vec<SelfPipelineItem>,
    pub joining: Vec<SelfPipelineItem>,
    /// Out of the pipeline, waiting on the owner: rejected twice, at the
    /// keel, or stopped at a join.
    pub held: Vec<SelfPipelineItem>,
    pub room: SelfPipelineRoom,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub budget: Option<SelfBudgetView>,
    /// Today's `self.pipeline.*` rows.
    pub rows_today: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub week: Option<SelfPipelineWeek>,
    /// The ledger's index is still being built: the queues are not read yet.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub building_index: bool,
}
