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
//!   key, [`fold::FoldNode`]; shown to the view as `crate::view::fold::FoldNode`): [`N`] consecutive
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
//! The window holds [`N`] to 2[`N`] exchanges; at 2[`N`] its oldest [`N`]
//! fold (`read::fold_step`), and a level that reaches 2[`N`] nodes folds its
//! oldest [`N`] into one node of the level above. Only what came after the
//! place's latest `/new` is counted. Every model call is held on the day's
//! line [`DAY_USD`] and the daemon's day ceiling first (`writer.rs`).
//!
//! New META families need no store-format step; no field is added to any
//! stored record.

use serde::{Deserialize, Serialize};

pub mod build;
pub mod did;
pub mod fold;
pub mod read;
pub mod view;
pub mod writer;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_job;

/// The window's grain: it holds `N` to `2N` exchanges, and a fold takes `N`.
pub const N: usize = 20;
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

/// `a` grown by `b`: its flags, and an audience no more private than either.
pub fn union(mut a: Marks, b: &Marks) -> Marks {
    use crate::view::Audience;
    a.grow(b);
    if a.audience != b.audience {
        a.audience = match (a.audience, b.audience) {
            (Audience::Unknown, _) | (_, Audience::Unknown) => Audience::Unknown,
            (Audience::Open, _) | (_, Audience::Open) => Audience::Open,
            _ => Audience::Shared,
        };
    }
    a
}

/// The day's spend on summaries (`ladder.day`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Day {
    /// The local day, `YYYY-MM-DD`.
    pub day: String,
    pub spent_usd: f64,
}

impl Day {
    /// What is left of [`DAY_USD`] on `today`.
    pub fn left(&self, today: &str) -> f64 {
        let spent = if self.day == today {
            self.spent_usd
        } else {
            0.0
        };
        (DAY_USD - spent).max(0.0)
    }

    /// The day after `usd` more was spent on `today`.
    pub fn spend(&self, today: &str, usd: f64) -> Day {
        let spent = if self.day == today {
            self.spent_usd
        } else {
            0.0
        };
        Day {
            day: today.to_string(),
            spent_usd: spent + usd,
        }
    }
}

/// `text` cut by code to at most `max` characters, an ellipsis marking a cut.
pub fn cut(text: &str, max: usize) -> String {
    let text = text.trim();
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}
