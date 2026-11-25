//! The history tool's interface (design §6, rung 3; trim item 5: ONE look-back tool, with a place and a label
//! argument). A public tool (`places::public_tool`), so a shared place's class admits it too; a tool ceiling can
//! still leave it out (a role without the `context` family), and where its family is not loaded by default it is
//! deferred, found by the turn's tool search. Bare, it returns the [`PAGE_EXCHANGES`] exchanges before
//! the oldest one in full view, one line each; called again with the page's cursor, the ones before those,
//! with no horizon. Every page passes the gate, which reads only whether an exchange's place is known: any
//! place, for every reader (anything goes, 2026-11-22). Implemented by the history work (`history_tool`).

use serde::{Deserialize, Serialize};

use super::timeline::Cursor;
use super::{Handle, PointOfView};

/// The tool's name.
pub const HISTORY_TOOL: &str = "context.history";
/// Exchanges on one page.
pub const PAGE_EXCHANGES: usize = 100;
/// Pages a turn may read without asking.
pub const LOOKS_PER_TURN: u32 = 5;

/// The tool's arguments. All optional: bare is "the 100 before what is in view, here".
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryArgs {
    /// Another place, any of the owner's; `"*"`: every place, newest first; absent: here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place: Option<String>,
    /// A label's timeline: a topic or a thing by its id (`topic:…`, `file:…`), or one of Jev's four content
    /// labels (`personal`, `secret`, `confidential`, `injection`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Words to search for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub words: Option<String>,
    /// The `next` of the previous page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<Cursor>,
    /// Open one line to its full text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open: Option<Handle>,
    /// A time span (`"last week"`, `"2026-11-20"`, `"from 3 to 5 November"`): only its exchanges, newest first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<String>,
    /// A span's start, read as `when` is (its start); with `to` or alone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    /// A span's end, read as `when` is (its end, not included); with `from` or alone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
}

/// One line of a page: his words exactly, and a line of what Theseus did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryLine {
    pub handle: Handle,
    pub place: String,
    pub at_ms: u64,
    pub his_words: String,
    /// The `did` line; `None` reads "reply not summarised".
    pub did: Option<String>,
    /// Whose words they are: the owner's mark's person (`owner`, a chat author), none when no mark proves one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub person: Option<String>,
    /// With no `did` line: the final reply's first characters, shown as its beginning.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_begins: Option<String>,
    /// Whether what the line shows of the reply derives from outside text: its exchange read an External
    /// result (its `did` line's mark, or its turn's own nodes), so the reply or its summary may quote it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub outside: bool,
    /// On a content label's page: what the label was on, and whether the owner removed it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub labels: Option<String>,
}

/// How a page ends: told apart, so the model knows whether it may ask or should offer to go on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PageEnd {
    More,
    HistoryExhausted,
    AllowanceUsed,
    /// The walk read its bound of records before its page was full: a limit of the tool, not the end of
    /// history; it resumes from a token the tool keeps for the turn.
    ScanLimit,
    /// A words search in a fork reached the end of its walk with hits the fork's cut held (the source's words
    /// after the point): those may have filled the search's bounded top hits and crowded out earlier ones, so
    /// the list may be incomplete. Never told as the end of history.
    SearchCut,
    /// A words search whose bound was reached with hits no longer on record among those it took (gone, erased,
    /// or stale index entries): a live match below them may have been left out, so the list may be
    /// incomplete. Never told as the end of history.
    SearchStale,
}

/// One page, newest first, with a stable cursor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryPage {
    pub lines: Vec<HistoryLine>,
    pub next: Option<Cursor>,
    pub end: PageEnd,
}

/// The tool.
pub trait History {
    /// `looks` is how many pages this turn has read already.
    fn page(
        &self,
        pov: &PointOfView,
        args: &HistoryArgs,
        looks: u32,
    ) -> anyhow::Result<HistoryPage>;
}
