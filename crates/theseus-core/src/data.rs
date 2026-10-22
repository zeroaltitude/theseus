//! Clearance for a request's data (theseus-a1wvq.3): a request to a model of
//! restricted origin (`cn` or `unknown`) goes out only with a [`DataPass`]
//! minted for that one compiled request, covering its digest. The names here
//! are shared with the rows that build open requests and judge them; keep them.
//!
//! Per request (theseus-a1wvq.8): `admits` says whether a turn's input is
//! open and what its run is; `clearance`, over what one compiled request
//! `Carried`, is the only cleared `Clearance`, and so the only source of a
//! pass. The turn's side is `turn/open_step.rs`.
//!
//! The screen (theseus-a1wvq.9): a tool's result joins an open request's run
//! only when `screen` passes it, and a result it fails ends the run there, never
//! the session. The result is the one part of an open request that no person
//! wrote and Jev never judged, so only the file tools under the open roots, a
//! search and a fetch of a public address may pass, and only a result the
//! scrubber found nothing in. A program's result never passes (it can read any
//! path the daemon's user can); a sandboxed program run is a later row. The
//! turn's side is `turn/screen_step.rs`.
//!
//! A session's data state (theseus-a1wvq.5, `SessionRecord.data`): whether
//! it was opened, its open run, and its taint. An open task opens one
//! (theseus-a1wvq.6); an open request's turn stores its run.

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
    /// Nothing judged the request.
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
    /// Whether an open session's refusal for this reason is its taint: what
    /// was judged or carried was not open. A judgment that never came (or
    /// came late, or in shadow) taints nothing: the input was not judged.
    pub fn taints(self) -> bool {
        !matches!(
            self,
            DataWhy::Unchecked | DataWhy::NoVerdict | DataWhy::Late | DataWhy::PackNotLive
        )
    }

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

/// Whether one request may go to a restricted model. Opaque: only
/// [`clearance`] builds a cleared one, so anything else holds a closed one.
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

// ------------------------------------------------------------ admission and the pass

/// What `admits` reads of one input (theseus-a1wvq.8): all of it held by the
/// turn already, so the check is pure and costs nothing on a plain turn.
pub struct Check<'a> {
    /// The input node: a person's message, or a task's brief.
    pub input: &'a str,
    /// The node just before it in the transcript, if any.
    pub before: Option<&'a str>,
    /// The route pack's `data` answer (`Verdict.data`), if one came.
    pub answer: Option<&'a crate::routing::data::DataAnswer>,
    /// The verdict came after the wait.
    pub late: bool,
    /// The route pack acts for the session (canary or live, breaker closed).
    pub live: bool,
    /// The thread is sealed, a seal is pending, or the verdict recommends one.
    pub sealed: bool,
    /// The input carries an attachment.
    pub attachment: bool,
}

/// Whether the input is open, and its run: each check in order, each failure
/// its own reason. The run extends the stored one only when the node just
/// before the input is the stored run's last (the chain); otherwise it is the
/// input alone. A session's taint closes it for good (theseus-a1wvq.8, F1):
/// no node at or after a taint reaches a restricted model.
pub fn admits(s: Option<&DataState>, c: &Check<'_>) -> Result<OpenRun, DataWhy> {
    if let Some(t) = s.and_then(|s| s.taint.as_ref()) {
        return Err(t.why);
    }
    if !c.live {
        return Err(DataWhy::PackNotLive);
    }
    if c.attachment {
        return Err(DataWhy::Attachment);
    }
    if c.sealed {
        return Err(DataWhy::Sealed);
    }
    if c.late {
        return Err(DataWhy::Late);
    }
    let a = c.answer.ok_or(DataWhy::NoVerdict)?;
    match a.choice.as_str() {
        "open" => {}
        "private" => return Err(DataWhy::VerdictPrivate),
        _ => return Err(DataWhy::VerdictUnclear),
    }
    if a.band != theseus_judge::band::Band::Act {
        return Err(DataWhy::BelowAct);
    }
    let from = match s.and_then(|s| s.run.as_ref()) {
        Some(r) if c.before == Some(r.through.as_str()) => r.from.clone(),
        _ => c.input.to_string(),
    };
    Ok(OpenRun {
        from,
        through: c.input.to_string(),
    })
}

/// A turn with no new input in an open session (an open task's later turn):
/// admitted with the stored run while its last node is the run's last, else
/// `unvetted`; with no run, nothing was judged open (`no_verdict`).
pub fn admits_again(s: Option<&DataState>, last: Option<&str>) -> Result<OpenRun, DataWhy> {
    if let Some(t) = s.and_then(|s| s.taint.as_ref()) {
        return Err(t.why);
    }
    let run = s.and_then(|s| s.run.as_ref()).ok_or(DataWhy::NoVerdict)?;
    match last == Some(run.through.as_str()) {
        true => Ok(run.clone()),
        false => Err(DataWhy::Unvetted),
    }
}

/// Who wrote a node a request carries, as `clearance` reads one past the
/// run's last node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Writer {
    /// This request's own turn: its answer, its calls, its recall.
    Loop,
    /// A tool's result the turn did not screen (`screen`): a screened one
    /// is carried as the loop's own.
    ToolResult,
    /// Anyone else: a steer, a late result, a wake, a report.
    Other,
}

/// One node a request carries: its id, its transcript position, its writer.
#[derive(Debug, Clone)]
pub struct Carry {
    pub id: String,
    pub at: u64,
    pub writer: Writer,
}

/// What one compiled request carries (theseus-a1wvq.8): whether it was
/// compiled in the open view, `run.from`'s position (none when the
/// transcript does not hold it), and every transcript node it renders.
#[derive(Debug, Clone)]
pub struct Carried {
    pub open_view: bool,
    pub from: Option<u64>,
    pub nodes: Vec<Carry>,
}

/// One request's clearance: the open view, every node at or after the run's
/// first, the run's last among them, and every node past it written by this
/// request's own loop and no tool's result. The only cleared `Clearance`.
pub fn clearance(run: &OpenRun, c: &Carried) -> Clearance {
    match refused(run, c) {
        Some(why) => Clearance::closed(why),
        None => Clearance { closed: None },
    }
}

fn refused(run: &OpenRun, c: &Carried) -> Option<DataWhy> {
    if !c.open_view {
        return Some(DataWhy::PrivateView);
    }
    // The run's first and last must be in the transcript and the request.
    let at = |id: &str| c.nodes.iter().find(|n| n.id == id).map(|n| n.at);
    let (Some(from), Some(through)) = (c.from, at(&run.through)) else {
        return Some(DataWhy::NotInRun);
    };
    if c.nodes.iter().any(|n| n.at < from) {
        return Some(DataWhy::NotInRun);
    }
    c.nodes
        .iter()
        .filter(|n| n.at > through)
        .find_map(|n| match n.writer {
            Writer::Loop => None,
            Writer::ToolResult => Some(DataWhy::ToolResult),
            Writer::Other => Some(DataWhy::Unvetted),
        })
}

// ------------------------------------------------------------ the screen

/// Why a tool's result did not pass the screen (`screen`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unscreened {
    /// A program's output: it can print any file the daemon's user reads.
    Program,
    /// A tool the screen does not list.
    Tool,
    /// The scrubber found a secret in it.
    Secret,
    /// A path outside the open roots, or a private address.
    Outside,
}

impl Unscreened {
    pub fn as_str(self) -> &'static str {
        match self {
            Unscreened::Program => "program",
            Unscreened::Tool => "tool",
            Unscreened::Secret => "secret",
            Unscreened::Outside => "outside",
        }
    }
}

/// Whether one tool result may join an open request's run: `plan` is the call's
/// own plan (none when it does not plan), `redactions` the result node's
/// `meta.redactions`, and `open` the request's reach. In order: a program
/// fails; only the file tools (`places::files_tool`), `web.search` and
/// `http.fetch` are listed, every other tool's result fails (a task list names
/// the owner's other work, `tools.find` the owner's tools); a scrubbed secret
/// fails; and, checked again here after the gate, a file tool must name a
/// resource and every one canonical within an open root, and a fetch must name
/// a public URL and, when the result names one (`meta.final_url`), end at one:
/// a redirect may lead into the owner's network (theseus-lm38z).
pub fn screen(
    tool: &str,
    plan: Option<&theseus_protocol::Plan>,
    redactions: u64,
    final_url: Option<&str>,
    open: &crate::open_view::Open,
) -> Result<(), Unscreened> {
    use theseus_tools::paths;
    if crate::open_view::runs_tool(tool) {
        return Err(Unscreened::Program);
    }
    let file = crate::places::files_tool(tool);
    if !(file || matches!(tool, "web.search" | "http.fetch")) {
        return Err(Unscreened::Tool);
    }
    if redactions > 0 {
        return Err(Unscreened::Secret);
    }
    let inside = |r: &theseus_protocol::Resource| {
        let p = paths::canonical_best_effort(&r.path);
        open.roots.iter().any(|root| paths::within(&p, root))
    };
    let outside = match (file, tool) {
        (true, _) => {
            !plan.is_some_and(|p| !p.resources.is_empty() && p.resources.iter().all(inside))
        }
        (_, "http.fetch") => {
            let not_public = |u: &str| {
                reqwest::Url::parse(u).is_err() || crate::web::net::private_url(u).is_some()
            };
            plan.and_then(|p| p.url.as_deref()).is_none_or(not_public)
                || final_url.is_some_and(not_public)
        }
        _ => false,
    };
    match outside {
        true => Err(Unscreened::Outside),
        false => Ok(()),
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
/// - a person's message Jev judged `open` at the act band (`admits`);
/// - an open request's own answer and calls, every request of its turn
///   cleared (`turn/open_step.rs`, `data_end`);
/// - an open request's tool result that passed the screen (theseus-a1wvq.9,
///   `screen`).
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

/// Open `rec` as an open task (theseus-a1wvq.6) or a draft (theseus-a1wvq.13)
/// at `at_ms`: its data state says so and nothing more, and the returned row
/// rides the same frame as the record. Every path that starts one calls this.
pub fn open_task(
    rec: &mut crate::session::SessionRecord,
    how: OpenHow,
    at_ms: u64,
) -> anyhow::Result<theseus_store::NewRecord> {
    let opened = Opened { how, at_ms };
    rec.data = Some(Box::new(DataState::new(Some(opened), None, None)));
    opened_row(&rec.session_id, how)
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
