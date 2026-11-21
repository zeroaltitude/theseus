//! The history tool's interface (design §6, rung 3; trim item 5: ONE look-back tool, with a place and a thing
//! argument). Always offered. Bare, it returns the [`PAGE_EXCHANGES`] exchanges before the oldest one in full
//! view, one line each; called again with the page's cursor, the ones before those, with no horizon. Every
//! page passes the gate; in company it reads that place alone. Implemented by the history work.

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
    /// Another private place (where he is alone); absent: here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place: Option<String>,
    /// A thing's timeline, `type:value`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thing: Option<String>,
    /// Words to search for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub words: Option<String>,
    /// The `next` of the previous page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<Cursor>,
    /// Open one line to its full text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open: Option<Handle>,
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
