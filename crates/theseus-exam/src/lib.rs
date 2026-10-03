//! M6's memory exam and the headroom test (step 34a; design §2.9): how much
//! could memory possibly help, before any memory is built?
//!
//! - `item`: the exam as versioned data, `exam/exam-v2.toml`
//!   (theseus-zaz.11): ten families at scale, and four that make retrieval
//!   hard (paraphrase, scale, time, tool output), half of each held out. Each
//!   item is a past, a present, and a check.
//! - `generate`: the exam's sessions made from templates and seeds, with dates
//!   spread over months.
//! - `fixture`: the fixture writer, which puts every item's past into a
//!   scratch store as the product writes sessions, and a manifest of where.
//! - `check`: the check language that scores an answer, deterministically.
//! - `render`: the oracle arm's note, in the recall note's format (§2.4).
//! - `drive`: the run, arms × items × runs, through a scratch daemon's socket.
//! - `stats` and `report`: paired by item, clustered by item, with intervals.
//! - `words`: a text's content words, which the paraphrase and scale
//!   families' definitions are checked with.
//! - `probe`: where the gold ranked, per item, and recall per family.
//! - `tender`: each task asked of a running index tender over its socket,
//!   per arm of sources and fusion weights, so the exam can judge BM25,
//!   vectors and fusion without building a model (theseus-emc).
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
pub mod tender;
pub mod time;
pub mod words;
