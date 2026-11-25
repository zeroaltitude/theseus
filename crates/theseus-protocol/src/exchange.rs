//! The exchange ledger (issue 44, step 1): `exchange.report` sums the
//! measuring rows (`context.turn`, one per turn in a private place) into
//! counts and shares; `exchange.rebuild` writes the exchanges and touches of
//! history from before them, from the nodes, with a dry run that only counts.
//! Both carry ids, kinds and counts, never a message's text.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// `exchange.report`: the newest `limit` rows (default all), none older than `since_ms`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct ExchangeReportParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub limit: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub since_ms: Option<u64>,
}

/// The rows' answers to the measuring questions (issue 44, step 1), counts only. The previous exchange and
/// the things named are counted over the turns his own message opened; the context over the turns whose
/// first call was measured.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct ExchangeAnswers {
    /// Rows whose exchange a message of the owner's opened, by the exchange's own person.
    pub his_turns: u64,
    /// Rows whose exchange does not read (erased, or never written).
    pub unreadable: u64,
    /// The place's previous exchange in the context sent: `shown`, `missed` (held by a seal or a gate, or
    /// a real miss), `after_new` (before the place's `/new`, which the view leaves out), `first_here`,
    /// `not_measured` (the first call was never measured).
    pub previous: BTreeMap<String, u64>,
    /// Of the `missed`, where the previous exchange ran: `same` session, `other`.
    pub missed_by_session: BTreeMap<String, u64>,
    /// Turns whose message named a thing the index recognises: `some`, `none`, `unknown` (the index was
    /// late).
    pub named: BTreeMap<String, u64>,
    /// For each kind of thing, the turns that named it at least once.
    pub named_kinds: BTreeMap<String, u64>,
    /// Rows whose first call was measured, and each part of what it sent, summed (tokens, est.).
    pub context_turns: u64,
    pub context: BTreeMap<String, u64>,
    /// The provider's cache, read and written, summed over the same rows (tokens).
    pub cache_read: u64,
    pub cache_write: u64,
}

/// The rows, summed. `counts` is a group's values and how many rows had
/// each (`prev`: `transcript`, `place_tail`, `missed`, `none`; `peer`;
/// `kind`; …); `tokens` is each part of the sent context, added up over the
/// rows, est. at 3.6 bytes a token. `answers` is none from a daemon that
/// does not count them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct ExchangeReportResult {
    pub rows: u64,
    /// The first and last row's time.
    pub from_ms: Option<u64>,
    pub to_ms: Option<u64>,
    pub counts: BTreeMap<String, BTreeMap<String, u64>>,
    pub tokens: BTreeMap<String, u64>,
    /// The places bound now, by kind (`dm`, `channel_private`, …): counts.
    pub places_bound: BTreeMap<String, u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub answers: Option<ExchangeAnswers>,
}

/// `exchange.rebuild`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct ExchangeRebuildParams {
    /// Count what it would write, and write nothing.
    pub dry_run: bool,
}

/// What a rebuild found and wrote.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct ExchangeRebuildResult {
    pub dry_run: bool,
    pub sessions: u64,
    /// Turns found in the nodes, and of them those that already had an
    /// exchange (written as they happened; kept as they are).
    pub turns: u64,
    pub kept: u64,
    pub exchanges: u64,
    pub touches: u64,
    /// Of the new exchanges, those whose place, and whose person, the owner's
    /// mark of their message proved; the rest say `unknown` and name no person.
    pub place_proven: u64,
    pub person_proven: u64,
    /// Touches keyed off their exchange's start (an older build's reply
    /// touches), moved to it so that one exchange holds one touch of a root.
    pub repaired: u64,
    /// Records under the item labels' prefix (`labels.`) that could not be
    /// read as an item's labels: passed over, and left as they are.
    pub labels_passed_over: u64,
    /// Why the things named in them were not looked for, if they were not.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub things_skipped: Option<String>,
}

/// `exchange.ladder` (issue 44, step 4): build a place's ladder from its
/// history since its latest `/new`: the missing `did` lines, then the folds.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct ExchangeLadderParams {
    /// The place, as the ledger names it (`discord:channel:<id>`, `local:<dir>`).
    pub place: String,
    /// Count and price what it would do, and write nothing.
    pub dry_run: bool,
    /// The most this run may spend, in dollars: the smaller of it and what is
    /// left of the day's line for summaries (none: that line alone).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub cap_usd: Option<f64>,
    /// Leave to pass the day's line, up to `cap_usd` (which it needs).
    pub over_day_line: bool,
}

/// What a ladder build found, would do or did.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct ExchangeLadderResult {
    pub dry_run: bool,
    pub place: String,
    /// The place's latest `/new`; 0 for a place that has had none.
    pub since_ms: u64,
    /// The place's name holds a character no ladder key may: nothing done.
    pub skipped: bool,
    /// What is left of the day's line, and whether this run would need
    /// `over_day_line` to finish.
    pub day_left_usd: f64,
    pub needs_over_day_line: bool,
    pub exchanges: u64,
    /// Lines there already, and lines it would write (wrote).
    pub lines_had: u64,
    pub lines: u64,
    /// First-level nodes it would make (made), and the nodes above made by code.
    pub folds: u64,
    pub above: u64,
    pub estimated_usd: f64,
    pub spent_usd: f64,
    /// Why it stopped before it was done.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub stopped: Option<String>,
}

/// `exchange.gists` (issue 44, step 4): a reading sheet of `did` lines and
/// first-level fold nodes, each beside what its writer read, drawn by a
/// seed so a reading can be repeated. A read for the owner in person; it
/// stores nothing and calls no model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct ExchangeGistsParams {
    /// One place, as the ledger names it; none: every place.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub place: Option<String>,
    /// How many lines, and how many nodes, to draw.
    pub lines: u32,
    pub nodes: u32,
    /// The draw's seed; none: one is chosen and returned.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub seed: Option<u64>,
}

impl Default for ExchangeGistsParams {
    fn default() -> Self {
        ExchangeGistsParams {
            place: None,
            lines: 50,
            nodes: 10,
            seed: None,
        }
    }
}

/// A reading sheet: how many lines and first-level nodes exist where it
/// looked, the seed it drew with, and what it drew, oldest first.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct ExchangeGistsResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub place: Option<String>,
    pub seed: u64,
    pub lines_total: u64,
    pub nodes_total: u64,
    pub lines: Vec<GistLine>,
    pub nodes: Vec<GistNode>,
}

/// A `did` line beside its exchange.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct GistLine {
    pub turn_id: String,
    pub place: String,
    /// The exchange's start (the line's own time when it cannot be read).
    pub at_ms: u64,
    pub model: String,
    pub writer: u32,
    /// The line's writer's material for its exchange, rebuilt now: the
    /// message and the final reply at its own cuts, each author as it is
    /// told, with the values the writer substitutes substituted; none when the
    /// exchange cannot be read. Not a kept copy of what it was given.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub read: Option<String>,
    /// The line as the view shows it.
    pub line: String,
    /// Why the material may not be what its writer was given: an older
    /// writer, sources no longer in the store. Read before judging.
    pub notes: Vec<String>,
}

/// A first-level fold node beside its exchanges.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct GistNode {
    pub id: String,
    pub place: String,
    pub first_ms: u64,
    pub last_ms: u64,
    /// The exchanges it stands for, and how many of them can still be read.
    pub exchanges: u32,
    pub readable: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub model: Option<String>,
    pub writer: u32,
    /// Its writer's material for those still readable, rebuilt now at its own
    /// cuts, each with the `did` line it had when the node was written, with
    /// the values the writer substitutes substituted.
    pub read: String,
    /// The node as the view shows it: its heading, then its text.
    pub shown: String,
    /// Why the material may not be what its writer was given: an older
    /// writer, sources no longer in the store. Read before judging.
    pub notes: Vec<String>,
}

/// `exchange.topics` (issue 46): the exchanges from before topics were filed
/// per exchange, each one's opening message asked of the ontology's judge
/// once, and touches written for the topics it names. The owner's, in person.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct ExchangeTopicsParams {
    /// Count and price what it would ask, and ask nothing.
    pub dry_run: bool,
    /// The most this run may spend, in dollars (none: the default, $10).
    /// [`topics_cap`] holds what it may be, at the CLI and at the daemon.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub cap_usd: Option<f64>,
    /// His declaration (issue 51): count as his the old exchanges no mark
    /// proves whose opening message arrived in one of his own places, by
    /// the place stored on the message (his terminal, the web UI, his DM, a
    /// channel bound private). Their stored author is not changed.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub include_unmarked: bool,
    /// His declaration undone (issue 51): end every touch an answer it
    /// filed came to, and nothing else. Asks nothing and spends nothing.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub undo_declared: bool,
}

/// A run's cap of `exchange.topics` (issue 46), as the CLI reads it before
/// it builds a request and the daemon again: a finite number of dollars
/// that is at least $0.000001 (one micro-dollar, as budgets count), the
/// number itself, not as it rounds. NaN, an infinity, zero, a negative
/// number and any sum under a micro-dollar are refused; a large one is the
/// owner's to name.
pub fn topics_cap(usd: f64) -> Result<f64, String> {
    match usd.is_finite() && usd >= 0.000_001 {
        true => Ok(usd),
        false => Err(format!(
            "--cap-usd is a number of dollars of at least $0.000001, not {usd}"
        )),
    }
}

/// What a filing of the history found, would do or did: counts and dollars,
/// never a message's text.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct ExchangeTopicsResult {
    pub dry_run: bool,
    pub cap_usd: f64,
    /// Every exchange in the ledger.
    pub exchanges: u64,
    /// Those that already touch a topic, and those this job asked before.
    pub touched_before: u64,
    pub asked_before: u64,
    /// Those a person's typed message opened that is not the owner's: a
    /// guest's, an agent's, or one whose author no record proves. His
    /// decision was about his own history, so they are never sent.
    pub not_owner: u64,
    /// Of those, the ones whose author no record proves (no person, and no
    /// mark on the message), and of these, the ones in places the
    /// ontology's judge would not act on now.
    pub unproven: u64,
    pub unproven_not_acting: u64,
    /// Of the unproven, those whose opening message arrived in one of his
    /// own places (what his declaration counts as his, whether or not this
    /// run makes it), and those whose message has no recorded place (from
    /// before places were recorded), which no declaration counts.
    pub declared: u64,
    pub unplaced: u64,
    /// The run counted his declaration (`include_unmarked`).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub include_unmarked: bool,
    /// The run undid his declaration (`undo_declared`): the answers his
    /// declaration filed, the exchanges whose touches end, and the touches.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub undo_declared: bool,
    pub declared_rows: u64,
    pub undone: u64,
    pub ended: u64,
    /// Those opened by no person's message that can be read: a wake, a
    /// task's brief, Theseus's own turn among them.
    pub no_message: u64,
    /// Those the ontology's judge would not act on now (off, or in shadow).
    pub not_acting: u64,
    /// Those left to ask, and their worst case together.
    pub to_ask: u64,
    pub worst_usd: f64,
    /// Asked this run, of them answered, and the exchanges and touches the
    /// answers came to.
    pub asked: u64,
    pub answered: u64,
    pub filed: u64,
    pub touches: u64,
    pub spent_usd: f64,
    /// Exchanges still to ask when the run stopped.
    pub left: u64,
    /// Why it stopped before it was done.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub stopped: Option<String>,
}
