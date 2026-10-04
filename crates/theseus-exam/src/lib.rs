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
//! - `render`: the oracle arm's note, the core's own render of a `Recall`
//!   node of the item's gold (§2.4, step 30b).
//! - `drive`: the run, arms × items × runs, each cell through its arm's
//!   scratch daemon's socket.
//! - `daemon` and `arms`: one scratch daemon per arm of the real pipeline
//!   (`none`, `bm25`, `baseline`), each on its own copy of the exam's store,
//!   started, waited for until its tender holds the store, and stopped
//!   (row 55, step 34b's wire-in).
//! - `stats` and `report`: paired by item, clustered by item, with intervals,
//!   and the decision per feature under the plan (`docs/m6-ablation-plan.md`,
//!   whose digest every report names).
//! - `words`: a text's content words, which the paraphrase and scale
//!   families' definitions are checked with.
//! - `probe`: where the gold ranked, per item, and recall per family.
//! - `tender`: each task asked of a running index tender over its socket,
//!   per arm of sources and fusion weights, so the exam can judge BM25,
//!   vectors and fusion without building a model (theseus-emc).
//!
//! The crate is a tool of its own (its manifest's `tool` marker): the
//! `theseus-exam` binary, which a maintainer runs by hand, beside
//! `theseusd`. 34a planned to move its driver into `theseus-sim exam` and its
//! scoring into `theseus-memory` at 34b's join; 34b kept both here instead:
//! the exam needs no code in the shipped binaries, and `theseus-sim` stays
//! the gate's tool.
//!
//! **Memory arms (row 55).** An arm of the real memory pipeline is chosen by
//! the scratch daemon's config key `[memory] arm`, never by a field of
//! `turn.submit`: the exam runs one daemon per arm, each in `live` mode, and
//! every client's submit stays as it is. `oracle` is the driver's own: the
//! gold, sent after the task to the `none` daemon.

pub mod arms;
pub mod check;
pub mod client;
pub mod daemon;
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
