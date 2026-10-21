//! Clearance for a request's data (theseus-a1wvq.3): a request to a model of
//! restricted origin (`cn` or `unknown`) goes out only with a [`DataPass`]
//! minted for that one compiled request, covering its digest. The names here
//! are shared with the rows that build open requests and judge them; keep them.
//!
//! Nothing mints a pass yet: every turn's clearance is
//! `Clearance::closed(DataWhy::Unchecked)`, so the floor (`data_gate.rs`,
//! `turn/data_step.rs`) keeps every request off a restricted model.
//!
//! A session's data state (theseus-a1wvq.5, `SessionRecord.data`): whether
//! it was opened, its open run, and its taint. Structure only: nothing opens
//! a session or computes a run yet; the later rows do.

use serde::{Deserialize, Serialize};

/// Leave to send one compiled request to a model of restricted origin. It
/// holds that request's digest and covers nothing else: minted, never copied
/// into another request (no `Clone`, no `Default`).
#[derive(Debug)]
pub struct DataPass {
    digest: String,
}

impl DataPass {
    /// Whether this pass was minted for the request whose digest is `digest`.
    pub fn covers(&self, digest: &str) -> bool {
        self.digest == digest
    }

    /// A pass for a test's request, by its digest.
    #[cfg(test)]
    pub(crate) fn for_test(digest: &str) -> Self {
        Self {
            digest: digest.into(),
        }
    }
}

/// Why a request is not cleared for a restricted model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataWhy {
    /// Nothing judged the request (every turn, until the open-request row).
    Unchecked,
    /// The request carries the owner's private view.
    PrivateView,
    /// A node of the request is outside the open run.
    NotInRun,
    /// A node no blessed writer vetted.
    Unvetted,
    /// A sealed thread's material.
    Sealed,
    /// An attachment.
    Attachment,
    /// Jev judged the message private.
    VerdictPrivate,
    /// Jev judged the message unclear.
    VerdictUnclear,
    /// Jev's answer is under the act band.
    BelowAct,
    /// Jev gave no verdict.
    NoVerdict,
    /// Jev's verdict came after the wait.
    Late,
    /// The pack that judges data does not act live.
    PackNotLive,
    /// A tool result that did not pass the screen.
    ToolResult,
}

impl DataWhy {
    pub fn as_str(self) -> &'static str {
        match self {
            DataWhy::Unchecked => "unchecked",
            DataWhy::PrivateView => "private_view",
            DataWhy::NotInRun => "not_in_run",
            DataWhy::Unvetted => "unvetted",
            DataWhy::Sealed => "sealed",
            DataWhy::Attachment => "attachment",
            DataWhy::VerdictPrivate => "verdict_private",
            DataWhy::VerdictUnclear => "verdict_unclear",
            DataWhy::BelowAct => "below_act",
            DataWhy::NoVerdict => "no_verdict",
            DataWhy::Late => "late",
            DataWhy::PackNotLive => "pack_not_live",
            DataWhy::ToolResult => "tool_result",
        }
    }
}

/// Whether a turn's requests may go to a restricted model. Opaque: only this
/// module builds a cleared one (the open-request row adds the checked
/// constructor), so anything else holds a closed one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clearance {
    closed: Option<DataWhy>,
}

impl Clearance {
    /// Not cleared, and why.
    pub fn closed(why: DataWhy) -> Self {
        Self { closed: Some(why) }
    }

    pub fn is_cleared(&self) -> bool {
        self.closed.is_none()
    }

    pub fn why(&self) -> Option<DataWhy> {
        self.closed
    }

    /// A pass for the request whose digest is `digest`, when cleared.
    pub fn pass(&self, digest: &str) -> Option<DataPass> {
        self.is_cleared().then(|| DataPass {
            digest: digest.into(),
        })
    }
}

/// A turn starts closed: unchecked, so fail closed.
impl Default for Clearance {
    fn default() -> Self {
        Self::closed(DataWhy::Unchecked)
    }
}

// ------------------------------------------------------------ the session's state

/// How a session came to be open (theseus-a1wvq.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpenHow {
    /// An open task.
    Task,
    /// A China-origin coding draft.
    Draft,
}

impl OpenHow {
    pub fn as_str(self) -> &'static str {
        match self {
            OpenHow::Task => "task",
            OpenHow::Draft => "draft",
        }
    }
}

/// The session was opened, and when.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Opened {
    pub how: OpenHow,
    pub at_ms: u64,
}

/// The current open run: its first and last node, by id. The newest stretch
/// of the conversation in which every node was judged open, ending at the
/// newest person message.
///
/// **The chain of blessed writers.** The run extends over a node only when
/// that node is:
/// - a person's message Jev judged `open` at the act band;
/// - a Theseus reply Jev judged `open` at the act band (asked off the turn's
///   path, by the open-request row);
/// - an open request's tool result that passed the screen.
///
/// Any other node ends it: a private or unclear message, a seal, an
/// attachment, an unscreened tool result, `channel.read` text, a merge, a
/// split, a join steer, a task's report, a wake. So a writer nobody thought
/// of can never extend a run, and a plain turn writes nothing here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenRun {
    pub from: String,
    pub through: String,
}

/// For an open session only: the first node that was not open, and why.
/// Bookkeeping only (the ledger, inspect, the benchmark): it never gates a
/// request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Taint {
    pub why: DataWhy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node: Option<String>,
    pub at_ms: u64,
}

/// A session's data state (`SessionRecord.data`). Absent reads as the
/// private view with no open run: every record before it, with no migration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(from = "Stored")]
pub struct DataState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opened: Option<Opened>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<OpenRun>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub taint: Option<Taint>,
    /// The run as this copy was read from the store, which a turn's
    /// write-back compares (`merge`). Never stored.
    #[serde(skip)]
    read: Option<OpenRun>,
}

/// `DataState` as stored: reading it remembers the run it read.
#[derive(Deserialize)]
struct Stored {
    #[serde(default)]
    opened: Option<Opened>,
    #[serde(default)]
    run: Option<OpenRun>,
    #[serde(default)]
    taint: Option<Taint>,
}

impl From<Stored> for DataState {
    fn from(s: Stored) -> Self {
        Self {
            read: s.run.clone(),
            opened: s.opened,
            run: s.run,
            taint: s.taint,
        }
    }
}

/// Equal when what is stored is.
impl PartialEq for DataState {
    fn eq(&self, o: &Self) -> bool {
        (&self.opened, &self.run, &self.taint) == (&o.opened, &o.run, &o.taint)
    }
}

impl Eq for DataState {}

impl DataState {
    pub fn new(opened: Option<Opened>, run: Option<OpenRun>, taint: Option<Taint>) -> Self {
        Self {
            opened,
            run,
            taint,
            read: None,
        }
    }

    /// The turn's write-back merged into the stored state, never over it: a
    /// stored taint always wins, `opened` is never cleared, and the turn's run
    /// is taken only while the stored one is still the run the turn read at
    /// its start (or already the turn's): compare and set.
    pub fn merge(
        stored: Option<Box<DataState>>,
        turn: Option<&DataState>,
    ) -> Option<Box<DataState>> {
        let Some(turn) = turn else {
            return stored;
        };
        let mut s = stored.map(|b| *b).unwrap_or_default();
        if s.run == turn.read || s.run == turn.run {
            s.run.clone_from(&turn.run);
        }
        if s.taint.is_none() {
            s.taint.clone_from(&turn.taint);
        }
        if s.opened.is_none() {
            s.opened.clone_from(&turn.opened);
        }
        (s != DataState::default()).then(|| Box::new(s))
    }

    /// Its line for a person (inspect).
    pub fn line(&self) -> String {
        let opened = self.opened.as_ref().map_or("not opened".to_string(), |o| {
            format!("opened ({}) at {}", o.how.as_str(), o.at_ms)
        });
        let run = self.run.as_ref().map_or("no open run".to_string(), |r| {
            format!("open run {} to {}", r.from, r.through)
        });
        let taint = self.taint.as_ref().map_or(String::new(), |t| {
            format!(
                "; tainted ({}) at {}",
                t.why.as_str(),
                t.node.as_deref().unwrap_or("no node")
            )
        });
        format!("{opened}; {run}{taint}")
    }
}

/// A `session.data` row: the session was opened (callers come with the
/// open-request rows).
pub fn opened_row(session: &str, how: OpenHow) -> anyhow::Result<theseus_store::NewRecord> {
    crate::fact::row(
        &crate::fact::data::SessionDataOpened { how },
        Some(session),
        None,
    )
}

/// Open `rec` as an open task at `at_ms` (theseus-a1wvq.6): its data state
/// says so and nothing more, and the returned row rides the same frame as
/// the record. Both paths that start a task call this one.
pub fn open_task(
    rec: &mut crate::session::SessionRecord,
    at_ms: u64,
) -> anyhow::Result<theseus_store::NewRecord> {
    let opened = Opened {
        how: OpenHow::Task,
        at_ms,
    };
    rec.data = Some(Box::new(DataState::new(Some(opened), None, None)));
    opened_row(&rec.session_id, OpenHow::Task)
}

/// A `session.data` row: an open session's first node that was not open.
pub fn tainted_row(session: &str, taint: &Taint) -> anyhow::Result<theseus_store::NewRecord> {
    let fact = crate::fact::data::SessionDataTainted {
        why: taint.why,
        node: taint.node.as_deref(),
    };
    crate::fact::row(&fact, Some(session), None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_closed_clearance_mints_no_pass() {
        let c = Clearance::default();
        assert!(!c.is_cleared());
        assert_eq!(c.why(), Some(DataWhy::Unchecked));
        assert!(c.pass("abc").is_none());
        assert_eq!(
            serde_json::to_string(&DataWhy::VerdictUnclear).unwrap(),
            "\"verdict_unclear\""
        );
        assert_eq!(DataWhy::PackNotLive.as_str(), "pack_not_live");
    }

    #[test]
    fn a_pass_covers_its_own_digest_alone() {
        let p = DataPass::for_test("abc");
        assert!(p.covers("abc"));
        assert!(!p.covers("abd"));
    }
}
