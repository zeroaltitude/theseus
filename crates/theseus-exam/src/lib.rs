//! M6's memory exam and the headroom test (step 34a; design §2.9): how much
//! could memory possibly help, before any memory is built?
//!
//! - `item`: the exam as versioned data (`exam/exam-v1.toml`): 40 items, ten
//!   families, half held out. Each is a past, a present, and a check.
//! - `fixture`: the fixture writer, which puts every item's past into a
//!   scratch store as the product writes sessions, and a manifest of where.
//! - `check`: the check language that scores an answer, deterministically.
//! - `render`: the oracle arm's note, in the recall note's format (§2.4).
//! - `drive`: the run, arms × items × runs, through a scratch daemon's socket.
//! - `stats` and `report`: paired by item, clustered by item, with intervals.
//!
//! The crate is a lane (34a): it changes nothing in the core. At the join
//! (34b) its driver becomes `theseus-sim exam`, and its scoring moves to
//! `theseus-memory` beside the arms.

pub mod check;
pub mod client;
pub mod drive;
pub mod fixture;
pub mod item;
pub mod render;
pub mod report;
pub mod stats;
pub mod time;
