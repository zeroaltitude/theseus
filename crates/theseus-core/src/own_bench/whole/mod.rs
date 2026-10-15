//! `turn.replay {scope: "whole_turn"}` (own-benchmark part (c), theseus-pw1q.20):
//! the whole turn run again on another profile, tools and all, and judged by
//! what it did.
//!
//! The turn is rebuilt as `first_loop` rebuilds it (`rebuild.rs`: the same
//! bytes or a refusal), then:
//!
//! - `scratch.rs`: a copy of the turn's directory as it was (`undo.rs` puts
//!   back what the session's recorded edits changed; a turn whose writes
//!   cannot be put back is refused, naming the file), in a directory of its
//!   own that is removed at the end. Nothing is written outside it.
//! - `hands.rs`: the model's calls. File and repository tools run on the copy
//!   and refuse any path outside it; `proc.run` runs in L1 over the copy
//!   (`theseus_kernel::probe_run`: its writes go to L1's own scratch and are
//!   discarded, its egress is `[sandbox] egress`, its environment is a bare
//!   `PATH` and `HOME`); every other tool returns a stub, the original turn's
//!   own recorded result for that tool when it called it, and a plain
//!   sentence otherwise. The row names each.
//! - `runloop.rs`: the loop, under `[route] whole_loops|whole_seconds|
//!   whole_usd`, each call reserved and held on the day ceiling as a
//!   first-loop replay's is.
//! - `compare.rs`: the words (`compare.v1` on the final replies) and the
//!   facts (the same last test run, the same paths changed, the same exit
//!   codes), both in the row.
//! - `label.rs`: a judged match at `[route] whole_label_bar` that the facts
//!   do not contradict becomes a machine label on the turn's route judgment,
//!   source `replay`, train split only (`learning::without_machine`).
//!
//! Only the owner's `turn.replay` runs it today; the nightly sampler still
//! replays first requests.

pub mod compare;
pub mod hands;
pub mod label;
pub mod runloop;
pub mod scratch;
pub mod undo;

#[cfg(test)]
mod tests;

/// The scope that runs the whole turn.
pub const WHOLE_TURN: &str = "whole_turn";
