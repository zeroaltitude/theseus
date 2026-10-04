//! Theseus's memory science, as pure logic with no I/O (design M6 §2.1).
//!
//! Every function here is pure: its inputs are explicit, so each result
//! replays from the record.
//! - [`science`]: the `MemoryScience` trait and its baseline (step 30a);
//! - [`recall`]: recall's filters, each drop with its reason, the science's
//!   rank, and the pack under the budget (step 30a), which the core's recall
//!   step and `memory.search` run;
//! - [`fsrs`]: FSRS-6 retention (step 32a), from the published algorithm,
//!   and its fold over a node's events;
//! - [`access`]: what happened to a node, and the review it is;
//! - [`activation`]: spreading activation over typed edges (step 32b).
//!
//! The wire-ins of 32a and 32b add the retention and adjacency projections
//! and the `+retention` and `+activation` arms.

pub mod access;
pub mod activation;
pub mod fsrs;
pub mod recall;
pub mod science;

pub use access::{Access, AccessEvent, Durability, Label, Outcome};
pub use activation::{spread, Adjacency, AdjacencyList, EdgeKind, EdgeWeights, SpreadParams};
pub use fsrs::{Fsrs6, Grade, ParamsError, Retention, FSRS6_DEFAULT};
pub use recall::{Admitted, Asker, Candidate, Dropped, Pack, Params, Place, Reason};
pub use science::{Baseline, MemoryScience, ScienceId};
