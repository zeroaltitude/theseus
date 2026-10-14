//! Watches (theseus-f2r2b; the horizon report's §5.1, §5.5): waiting on the
//! world with no model. "Tell me when PR 123 merges" was a repeating wake: a
//! model turn per check, at 5 minutes the most often. A watch is a check
//! (`check.rs`: a command in L1, a fetch, a file) run on a timer until a
//! condition holds (`cond.rs`), and then one thing happens: a notice, a turn
//! of the conversation that set it, or a task's brief handed to that turn.
//! Waiting costs nothing: three days at 5 minutes is 864 checks, a handful of
//! frames, and $0 of model.
//!
//! - **The record.** A watch is a pending wake, purpose `Watch`, run
//!   `Probe` (`spec.rs` says where each field rides; no stored field was
//!   added, no store format bumped). A wake already has `wake.list` and
//!   `wake.cancel`, `theseus cancel`, and the cap `[kernel] max_watches`, and
//!   a conversation that ends takes its watches with it.
//! - **The timer.** There is none of its own. The driver's tick (500 ms,
//!   already there) asks the `Board`, which holds each open watch's next
//!   check in memory: at most 4 run at once, each bounded (30 s, 64 KiB of
//!   output, scrubbed), off every turn's thread. After a restart the board is
//!   filled once from the open wakes, and each watch checks once at once: a
//!   read, safe to repeat.
//! - **Frames only on news** (`step.rs`, `theseus_kernel::watch`). The first
//!   value, a different value, a met condition, an expiry, and a third failure
//!   in a row each write one frame; an unchanged value writes nothing (the
//!   check count and the next time stay in memory, and the next frame carries
//!   the count). A met watch is an Interrupt with sound, one that gave up or
//!   keeps failing an Inform (`theseus_protocol::notices`).
//! - **Bounds.** A check every minute at the most often (5 by default), a
//!   watch for 30 days at the most, `[kernel] max_watches` (20) open
//!   daemon-wide, failures backing off ×2 to an hour.
//! - **Who sets one.** The model's `watch` tool (`watch_tool.rs`: a command's
//!   watch is asked about once, at its creation, with the command shown, and
//!   nothing can change it afterwards), or the owner's `watch.create`
//!   (`rpc/watch.rs`: the owner typed the command, so no card stands in front
//!   of it); `theseus watches add|list|rm`.

pub mod board;
pub mod check;
pub mod cond;
pub mod run;
pub mod spec;
pub mod step;

#[cfg(test)]
mod tests_watch;
#[cfg(test)]
mod tests_watch_tool;

pub use board::Board;
