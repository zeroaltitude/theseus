//! M6's memory exam and the headroom test (step 34a; design §2.9): how much
//! could memory possibly help, before any memory is built?
//!
//! - `item`: the exam as versioned data. `exam/exam-v1.toml`: 40 items, ten
//!   families, half held out. `exam/exam-v2.toml` (theseus-zaz.11): those
//!   families at scale, and four that make retrieval hard (paraphrase, scale,
//!   time, tool output). Each item is a past, a present, and a check.
//! - `generate`: exam-v2's sessions made from templates and seeds, with dates
//!   spread over months.
//! - `fixture`: the fixture writer, which puts every item's past into a
//!   scratch store as the product writes sessions, and a manifest of where.
//! - `check`: the check language that scores an answer, deterministically.
//! - `render`: the oracle arm's note, in the recall note's format (§2.4).
//! - `drive`: the run, arms × items × runs, through a scratch daemon's socket.
//! - `stats` and `report`: paired by item, clustered by item, with intervals.
//! - `probe`: BM25's recall of the gold, per family, at no model cost: how
//!   hard an exam is for lexical retrieval.
//!
//! The crate is a lane (34a): it changes nothing in the core. At the join
//! (34b) its driver becomes `theseus-sim exam`, and its scoring moves to
//! `theseus-memory` beside the arms.

pub mod check;
pub mod client;
pub mod drive;
pub mod fixture;
pub mod generate;
pub mod item;
pub mod probe;
pub mod render;
pub mod report;
pub mod rng;
pub mod stats;
pub mod time;
