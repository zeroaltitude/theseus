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
//! - [`activation`]: spreading activation over typed edges (step 32b), and
//!   [`activated`], the `+activation` arm's science;
//! - [`rerank`]: the `+rerank` arm's reorder of the top 20 by Jev's answers,
//!   and the pack it would admit (step 32c);
//! - [`retention`]: the `+retention` arm's science, the fused score weighed
//!   by the node's retrievability (step 32a's wire-in).
//! - [`consolidate`]: consolidation's clusters of nodes recall admits
//!   together, a synthesis's citations and checks, and its shadow score
//!   (step 31b).
//!
//! The core keeps the retention projection (32a's wire-in); 32b's adds the
//! adjacency projection and the `+activation` arm.

pub mod access;
pub mod activated;
pub mod activation;
pub mod consolidate;
pub mod fsrs;
pub mod recall;
pub mod rerank;
pub mod retention;
pub mod science;

pub use access::{Access, AccessEvent, Durability, Label, Outcome};
pub use activated::Activated;
pub use activation::{spread, Adjacency, AdjacencyList, EdgeKind, EdgeWeights, SpreadParams};
pub use fsrs::{Fsrs6, Grade, ParamsError, Retention, FSRS6_DEFAULT};
pub use recall::{
    Admitted, Asker, Candidate, Dropped, Link, LinkKind, Pack, Params, Place, Reason,
};
pub use retention::RetentionRank;
pub use science::{Baseline, MemoryScience, ScienceId, SynthesisAdmit, WithSyntheses};
