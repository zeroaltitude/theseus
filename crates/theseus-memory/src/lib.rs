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
//! - [`gate`]: the gate's types and its one rule left: a piece whose place
//!   cannot be read is not shown (issue 44; anything goes since 2026-11-22).
//!
//! The research arms `+retention` (FSRS-6, 32a), `+activation` (spreading
//! activation, 32b) and `+synthesis` (31b) never ran live and are gone, with
//! the nightly synthesis writer; `baseline` and `bm25` are what recall runs.

pub mod floor;
pub mod gate;
pub mod recall;
pub mod rerank;
pub mod science;

#[cfg(test)]
mod tests_category;
#[cfg(test)]
mod tests_persona;

pub use floor::Floor;
pub use recall::{
    Admitted, Asker, Candidate, CategoryBoost, Dropped, Link, LinkKind, Pack, Params, PersonaBoost,
    Place, Reason,
};
pub use science::{Baseline, MemoryScience, ScienceId};
