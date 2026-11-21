//! Theseus's memory science, as pure logic with no I/O (design M6 §2.1).
//!
//! Every function here is pure: its inputs are explicit, so each result
//! replays from the record.
//! - [`science`]: the `MemoryScience` trait and its baseline (step 30a);
//! - [`recall`]: recall's filters, each drop with its reason, the science's
//!   rank, and the pack under the budget (step 30a), which the core's recall
//!   step and `memory.search` run;
//! - [`rerank`]: the `+rerank` arm's reorder of the top 20 by Jev's answers,
//!   and the pack it would admit (step 32c);
//! - [`gate`]: the gate's rule over a point of view and a piece's marks
//!   (issue 44, step 2), which the core's gate runs.
//!
//! The research arms `+retention` (FSRS-6, 32a), `+activation` (spreading
//! activation, 32b) and `+synthesis` (31b) never ran live and are gone, with
//! the nightly synthesis writer; `baseline` and `bm25` are what recall runs.

pub mod floor;
pub mod gate;
pub mod recall;
pub mod rerank;
pub mod science;
pub mod threads;

#[cfg(test)]
mod tests_category;
#[cfg(test)]
mod tests_persona;
#[cfg(test)]
mod tests_threads;

pub use floor::Floor;
pub use recall::{
    Admitted, Asker, Candidate, CategoryBoost, Dropped, Link, LinkKind, Pack, Params, PersonaBoost,
    Place, Reason,
};
pub use science::{Baseline, MemoryScience, ScienceId};
pub use threads::{
    Cause as ThreadCause, Distant, SessionThread, ThreadLens, Verdict as ThreadVerdict,
};
