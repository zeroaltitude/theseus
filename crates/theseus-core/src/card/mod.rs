//! The owner card from the owner's history (theseus-slvl.6; the personality
//! plan's owner card, soul B3's review queue made concrete for it).
//!
//! Knowing the owner today is similarity recall over the imported archive:
//! nothing about him is sure to be in context, and nothing curates what is
//! still true. The card is standing's `fact` and `preference` records
//! (theseus-a7vi); this fills it from evidence, one question at a time.
//!
//! - **The job** (`run.rs`; `theseus card propose`, and nightly at most every
//!   `[card] every_days` until the owner says done, `tender.rs`) reads the
//!   archive's evidence (`evidence.rs`: feedback and SOP notes, USER-style
//!   files, `decision_or_preference` episodes), splits it into lines and
//!   clusters near-duplicates (`cluster.rs`), drops clusters whose episodes
//!   were mostly seen already, and asks `[card] profile` (Haiku) to state
//!   each as one short fact or preference through one tool (`ask.rs`), citing
//!   the episodes it rests on. A statement citing none of its cluster's
//!   episodes, copying a line, or repeating a live standing record or an
//!   answered proposal is dropped.
//! - **The live pass** (`live.rs`, theseus-6f8t0): nightly, for as long as
//!   `[card] observe` is on and whether or not the card is done, the open
//!   observations (`card.obs.<id>`, kept by the exchange-end observer) become
//!   at most `live_per_night` questions: Stale (a record they contradict: keep,
//!   edit or end it, `rpc/card_stale.rs`), Add, and Pattern (the same
//!   preference or decision in `pattern_sessions` conversations). Only the
//!   owner's answer keeps, edits or ends a record.
//! - **Labels are respected, before any call.** An episode labelled
//!   `partner-confidential` (or naming a partner codename) is counted and
//!   never read: its text is in no cluster and no request. Outside text is
//!   never evidence. A `personal` episode may be read: the card is private
//!   (standing's privacy rule shows a shared place that its preferences and
//!   facts are withheld, never their text).
//! - **Budgeted.** Every call reserves its worst case first, on the day's
//!   `[card] day_usd` and the daemon's day ceiling; a call that would pass
//!   either is not made (`stopped: budget`), and the cost is a `card.proposed`
//!   row, a spend kind.
//! - **Each proposal is a question** (`theseus_protocol::card`): "Add to
//!   your card: '…' (from 14 sources)? [Keep] [Edit] [Not this]", a META
//!   record `card.proposal.<id>`, answered by `card.answer`. Keep (or Edit,
//!   the owner's words) writes the standing record through `standing.add`
//!   with `source` `card:<id>`; Not this is remembered, so the sources are
//!   never proposed again; a question nobody answers in `[card] ask_days`
//!   expires to Not this (`answer.rs`, applied when the card is read or the
//!   job runs: no timer). At most `[card] max_open` wait at once.
//! - **Idempotent.** The episodes of every proposal and of every call that
//!   was made are remembered (`card.sent.<call>`), so a second run over the
//!   same history sends nothing.
//!
//! - **Observations** (theseus-6f8t0; `observe.rs`): in a private conversation
//!   the owner is alone in, one cheap call at an exchange's due point (and the
//!   nightly sweep) reads the owner's own lines since its mark and keeps what
//!   they say about the owner that should hold from now on — the owner's
//!   setup and work, and the owner's own life said in passing — as unconfirmed
//!   `card.obs.<id>` records. They are private notes: never a node, never
//!   indexed or recalled, and never in a shared place. A later pass turns
//!   them into questions the owner answers.
//! - **The day's observations ride in the turn's tail** (`noted.rs`,
//!   theseus-6f8t0, the sitting's stop 5): only where the owner is alone, only
//!   for a model of unrestricted origin, after the cached prefix, so a turn
//!   asked what it has learned about the owner today can name what it heard.
//!   Before it, the records were in no prompt at all and the answer could only
//!   list what the daemon had done.
//! - **`card.observed` (`tool.rs`) reaches the rest**: the block carries only
//!   today's open notes, bounded, so a turn asked about a longer span reads
//!   them with the tool instead, in the owner's own places only. The block is
//!   the guarantee; the tool is the reach.
//!
//! Nothing here is added to the card without the owner's answer, and nothing
//! enters a turn's cached prefix: the job is off every turn, and the standing
//! block's cached prefix changes only when the owner keeps one.

pub mod answer;
pub mod ask;
pub mod cluster;
pub mod dm;
pub mod evidence;
pub mod live;
pub mod noted;
pub mod observe;
pub mod run;
pub mod state;
pub mod tender;
pub mod tool;
pub mod write;

#[cfg(test)]
mod fixture;
#[cfg(test)]
mod tests_dm;
#[cfg(test)]
mod tests_dm_race;
#[cfg(test)]
mod tests_job;
#[cfg(test)]
mod tests_live;
#[cfg(test)]
mod tests_pure;
