//! The one-line summaries and the fold nodes of a place's ladder (issue 44,
//! step 4; the design note is `docs/design/ladder.md`). Small records written
//! beside the exchanges (`crate::exchange`), never in them, and nothing here
//! is sent to a model that answers a turn: the renderer reads them.
//!
//! - **A `did` line** (`did.<turn id>`, [`did::DidLine`]): at most
//!   [`did::MAX_CHARS`] characters on what Theseus did in one exchange,
//!   written once after the exchange ends by a small-model job, from the
//!   person's message and the final reply only (never tool results), with the
//!   model that wrote it and the exchange's marks. An exchange with none reads
//!   as [`did::NOT_SUMMARISED`] and its handle.
//! - **A fold node** (`fold.<place>␁<level>␁<first ms>`, the view contract's
//!   key, [`fold::FoldNode`]; shown to the view as `crate::view::fold::FoldNode`): [`FOLD_STEP`] consecutive
//!   exchanges of a place at the first level: the person's words in them by
//!   node id, a title and an account written once by the same kind of job,
//!   the union of their marks, and a handle (its id, `foldh.<id>` naming its
//!   key) that opens them. A level
//!   above the first is built by code from its children's typed fields; no
//!   model writes one. A node never changes once written.
//! - **The rungs** (`rung.<place>␁<level>␁<newest first>`): each node's id
//!   under its place and level, newest first, so a level's standing nodes are
//!   the first keys of its prefix.
//!
//! No fold takes the window's newest [`WINDOW_FLOOR`] exchanges: a settled
//! window holds [`WINDOW_FLOOR`] to [`WINDOW_FLOOR`] + [`FOLD_STEP`] - 1
//! (fewer in a new place or one that grows into the floor, more while the
//! fold job is behind), and at
//! [`WINDOW_FLOOR`] + [`FOLD_STEP`] its oldest [`FOLD_STEP`] fold
//! (`read::fold_step`); a level that reaches 2[`FOLD_STEP`] nodes folds its
//! oldest [`FOLD_STEP`] into one node of the level above. Only what came after the
//! place's latest `/new` is counted. Every model call is held on the day's
//! line [`DAY_USD`] (reserved in `ladder.day` before it is made, [`DayPurse`])
//! and the daemon's day ceiling first (`writer.rs`).
//!
//! New META families need no store-format step; no field is added to any
//! stored record.

use serde::{Deserialize, Serialize};

use crate::scrub::safe;

pub mod build;
pub mod did;
pub mod fold;
pub mod gists;
pub mod read;
pub mod view;
pub mod writer;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_authors;
#[cfg(test)]
mod tests_floor;
#[cfg(test)]
mod tests_gists;
#[cfg(test)]
mod tests_gists_fidelity;
#[cfg(test)]
mod tests_job;
#[cfg(test)]
pub(crate) mod tests_moved;

/// The window's floor: the newest exchanges no fold takes (30 since 2026-11-24; 20 before). A place with fewer
/// has them all; a request's budget can still shorten all but the newest three exchanges' words.
pub const WINDOW_FLOOR: usize = 30;
/// The fold step: a fold takes this many exchanges into one first-level node, or this many nodes of a level
/// into one node above it. Every node, before and after the floor rose, holds this many.
pub const FOLD_STEP: usize = 20;
/// A task's window, never folded: its newest 40 (`read::ladder`), the size it had before the floor rose.
pub const TASK_WINDOW: usize = 40;
/// What the summaries may spend in a day, in dollars, across every place.
pub const DAY_USD: f64 = 2.0;
/// The day's spend so far: `ladder.day`, one record.
pub const DAY_KEY: &str = "ladder.day";

/// One summary job at a time, so the day's spend is read and written by one.
pub(crate) static WRITING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// The marks a line or a node carries: the view's (`crate::view::Marks`),
/// written by Theseus, from its exchanges' place and audience; outside when
/// any tool result in them came from outside. Marks only grow.
pub use crate::view::Marks;

/// An exchange's marks before its tool results are read.
pub fn marks_of(ex: &crate::exchange::Exchange) -> Marks {
    use crate::view::{Audience, Author};
    let audience = match (ex.private, ex.place.as_str()) {
        (_, "unknown") => Audience::Unknown,
        (true, _) => Audience::Private,
        (false, _) => Audience::Shared,
    };
    Marks {
        place: Some(ex.place.clone()),
        audience,
        author: Author::Theseus,
        outside: false,
        sealed: false,
        veiled: false,
    }
}

/// How narrow an audience is: an aggregate takes the narrowest of its
/// sources', and an unknown one, which denies, is narrowest of all.
fn narrowness(a: crate::view::Audience) -> u8 {
    use crate::view::Audience;
    match a {
        Audience::Open => 0,
        Audience::Shared => 1,
        Audience::Private => 2,
        Audience::Unknown => 3,
    }
}

/// `a` grown by `b`: its flags, the narrowest audience of the two (private
/// wins over shared and open; unknown over all), and no place when the two
/// name different ones (a missing mark, which denies).
pub fn union(mut a: Marks, b: &Marks) -> Marks {
    a.grow(b);
    if narrowness(b.audience) > narrowness(a.audience) {
        a.audience = b.audience;
    }
    if a.place != b.place {
        a.place = None;
    }
    a
}

/// Between a key's parts: a place that holds it gets no ladder.
pub const SEP: char = '\u{1}';

/// Whether `place` may have a ladder: its name cannot break a key's parts.
pub fn laddered(place: &str) -> bool {
    !place.is_empty() && !place.contains(SEP)
}

/// A message the judge's `classify` called a correction:
/// `ladder.correction.<turn>`, for the message that opened that turn.
pub const CORRECTION_PREFIX: &str = "ladder.correction.";

/// The flag of judgment `j`, when it is `classify`'s answer that the
/// message opening `turn` is a correction (its `kind` question, the pack's
/// typed choice): written in the judge's own frame by its sink.
pub fn correction_record(
    j: &theseus_judge::Judgment,
    turn: Option<&str>,
) -> Option<theseus_store::NewRecord> {
    let turn = turn?;
    let corrects = j.pack.starts_with("classify.")
        && j.answers.iter().any(|a| {
            a.question == "kind"
                && matches!(&a.answer, theseus_judge::Answer::Choice { choice, .. } if choice == "correction")
        });
    corrects
        .then(|| {
            theseus_store::NewRecord::json(
                theseus_store::kinds::META,
                Some(&format!("{CORRECTION_PREFIX}{turn}")),
                &true,
            )
            .ok()
        })
        .flatten()
}

/// Whether the message that opened `turn` was called a correction.
pub fn corrected(store: &crate::store::Store, turn: &str) -> anyhow::Result<bool> {
    Ok(store
        .get_meta::<bool>(&format!("{CORRECTION_PREFIX}{turn}"))?
        .unwrap_or(false))
}

/// A place's `/new` marks: `ladder.new.<place>`, the latest one.
pub const NEW_PREFIX: &str = "ladder.new.";

/// The place's latest `/new`, as the view's position: its time and the
/// turn it came after (a tie at one millisecond is broken by the turn id).
/// None: no `/new` was ever said there. A failed read is an error, never
/// none: what came before a reset is not brought back by a bad read.
pub fn new_mark(
    store: &crate::store::Store,
    place: &str,
) -> anyhow::Result<Option<crate::view::Pos>> {
    store.get_meta(&format!("{NEW_PREFIX}{place}"))
}

/// The record of a `/new` in `place` at `at`.
pub fn new_mark_record(
    place: &str,
    at: &crate::view::Pos,
) -> anyhow::Result<theseus_store::NewRecord> {
    theseus_store::NewRecord::json(
        theseus_store::kinds::META,
        Some(&format!("{NEW_PREFIX}{place}")),
        at,
    )
}

/// The day's spend on summaries (`ladder.day`): what was settled, and the
/// calls reserved and not yet settled, each at its worst case. A
/// reservation a crash left is counted as spent until the day ends.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Day {
    /// The local day, `YYYY-MM-DD`.
    pub day: String,
    pub spent_usd: f64,
    #[serde(default)]
    pub reserved: Vec<Reserved>,
}

/// One call's reservation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Reserved {
    pub id: String,
    pub usd: f64,
}

impl Day {
    /// The record as it stands on `today`: a past day's is a fresh one.
    pub fn on(self, today: &str) -> Day {
        match self.day == today {
            true => self,
            false => Day {
                day: today.to_string(),
                ..Day::default()
            },
        }
    }

    /// Settled and reserved together.
    pub fn used(&self) -> f64 {
        self.spent_usd + self.reserved.iter().map(|r| r.usd).sum::<f64>()
    }

    /// What is left of [`DAY_USD`] on `today`.
    pub fn left(&self, today: &str) -> f64 {
        (DAY_USD - self.clone().on(today).used()).max(0.0)
    }
}

/// Below this a difference of dollars is the float's, not the money's.
const EPSILON_USD: f64 = 1e-6;

/// What a summary call may spend: the day's line, and a run's own allowance.
pub trait Purse: Send + Sync {
    /// Reserve `usd`, a call's worst case: an id to settle it by, or why not.
    fn reserve(&self, usd: f64) -> Result<String, String>;
    /// Settle reservation `id` at what the call cost (its worst case when it
    /// failed after it was sent).
    fn settle(&self, id: &str, cost_usd: f64);
}

/// The store's purse: each reservation written to `ladder.day` before the
/// call is made, each settlement after, under one lock. A run's allowance
/// is the smaller of its own and what is left of the day's line, unless the
/// run was given leave to pass the line (`over_line`).
pub struct DayPurse<'a> {
    pub store: &'a crate::store::Store,
    run: std::sync::Mutex<RunPurse>,
    over_line: bool,
}

#[derive(Default)]
struct RunPurse {
    /// What the run may still reserve; none: the day's line alone.
    left: Option<f64>,
    spent: f64,
    out: std::collections::HashMap<String, f64>,
}

impl<'a> DayPurse<'a> {
    pub fn new(store: &'a crate::store::Store, run_usd: Option<f64>, over_line: bool) -> Self {
        DayPurse {
            store,
            run: std::sync::Mutex::new(RunPurse {
                left: run_usd,
                ..Default::default()
            }),
            over_line,
        }
    }

    /// What this run has spent, failed calls included.
    pub fn spent(&self) -> f64 {
        self.run.lock().unwrap_or_else(|e| e.into_inner()).spent
    }

    /// What this run may still reserve: its own allowance and, unless it may
    /// pass it, the day's line.
    pub fn left(&self) -> anyhow::Result<f64> {
        let run = self.run.lock().unwrap_or_else(|e| e.into_inner()).left;
        let day = match self.over_line {
            true => f64::INFINITY,
            false => self.day()?.1.left(&today()),
        };
        Ok(run.map_or(day, |r| r.min(day)).max(0.0))
    }

    fn day(&self) -> anyhow::Result<(String, Day)> {
        let today = today();
        let day: Day =
            theseus_store::blocking(|| self.store.get_meta(DAY_KEY))?.unwrap_or_default();
        Ok((today.clone(), day.on(&today)))
    }

    fn write(&self, day: &Day) -> anyhow::Result<()> {
        let rec = theseus_store::NewRecord::json(theseus_store::kinds::META, Some(DAY_KEY), day)?;
        theseus_store::blocking(|| self.store.append(&[rec]))?;
        Ok(())
    }
}

fn today() -> String {
    crate::judge::spend::local_day(theseus_protocol::now_unix_ms())
}

impl Purse for DayPurse<'_> {
    fn reserve(&self, usd: f64) -> Result<String, String> {
        let left = self
            .left()
            .map_err(|e| format!("the day's spend cannot be read: {e:#}"))?;
        // Sums of cents in floating point: a call that fits to a millionth fits.
        if usd > left + EPSILON_USD {
            return Err(format!(
                "the summaries' allowance leaves ${left:.2}, under a call's worst case (${usd:.2})"
            ));
        }
        let (_, mut day) = self.day().map_err(|e| format!("{e:#}"))?;
        let id = crate::new_id("lres");
        day.reserved.push(Reserved {
            id: id.clone(),
            usd,
        });
        self.write(&day)
            .map_err(|e| format!("the reservation was not written: {e:#}"))?;
        let mut run = self.run.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(l) = &mut run.left {
            *l -= usd;
        }
        run.out.insert(id.clone(), usd);
        Ok(id)
    }

    fn settle(&self, id: &str, cost_usd: f64) {
        let settled = self.day().and_then(|(_, mut day)| {
            day.reserved.retain(|r| r.id != id);
            day.spent_usd += cost_usd;
            self.write(&day)
        });
        if let Err(e) = settled {
            // The reservation stays, and counts as spent: the safe side.
            tracing::warn!(error = %format!("{e:#}"), "ladder: a summary call's settlement was not written");
        }
        let mut run = self.run.lock().unwrap_or_else(|e| e.into_inner());
        let held = run.out.remove(id).unwrap_or(0.0);
        if let Some(l) = &mut run.left {
            *l += held - cost_usd;
        }
        run.spent += cost_usd;
    }
}

/// `text` cut by code to at most `max` characters, an ellipsis marking a cut.
pub fn cut(text: &str, max: usize) -> String {
    let text = text.trim();
    if text.chars().count() <= max {
        return text.to_string();
    }
    safe::head_marked(text, max.saturating_sub(1))
}
