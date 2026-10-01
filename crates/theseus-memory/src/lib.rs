//! Theseus's memory science, as pure logic with no I/O (design M6 §2.1).
//!
//! Every function here is pure: its inputs are explicit, so each result
//! replays from the record. This first code is the math of the native
//! science's two arms, built ahead of their wire-ins:
//! - [`fsrs`]: FSRS-6 retention (step 32a), from the published algorithm,
//!   and its fold over a node's events;
//! - [`access`]: what happened to a node, and the review it is;
//! - [`activation`]: spreading activation over typed edges (step 32b).
//!
//! Step 30a adds the `MemoryScience` trait and the baseline science. The
//! wire-ins of 32a and 32b add the retention and adjacency projections and
//! the `+retention` and `+activation` arms.

pub mod access;
pub mod activation;
pub mod fsrs;

pub use access::{Access, AccessEvent, Durability, Label, Outcome};
pub use activation::{spread, Adjacency, AdjacencyList, EdgeKind, EdgeWeights, SpreadParams};
pub use fsrs::{Fsrs6, Grade, ParamsError, Retention, FSRS6_DEFAULT};
