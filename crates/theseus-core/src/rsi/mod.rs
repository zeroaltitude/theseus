//! Self-improvement's spine (theseus-pw1q): the one place every self step
//! asks "may I run?", the kill switch it reads, and the ledger's view of
//! what Theseus changed about itself.
//!
//! **The contract** every self step follows (the backlog, the branch
//! builder, the review and the join, the install, auto-revert, the exams,
//! the budget): call [`gate`] before it starts, and again between its
//! phases (after a build, before a push, before a join, before an install),
//! and stop at once on anything but [`Gate::Allowed`], writing nothing more
//! of its own but the row that says where it stopped. [`gate`] is
//!
//! - [`Gate::Off`] while `[self] mode = "off"` (the default), whatever the
//!   switch says: nothing self-directed runs, and nothing says it was asked;
//! - [`Gate::Halted`] while the kill switch is on, whatever the mode. A store
//!   that has never seen the owner's resume is halted, so `mode = "act"`
//!   alone starts nothing; the owner's first `theseus self resume` does;
//! - [`Gate::Allowed`] otherwise.
//!
//! It costs one read of a cached state: the switch's META record
//! (`self.switch`) is read once, at the first ask, and kept current by the
//! halt and the resume that write it (`switch.rs`), never a store scan. A
//! switch that cannot be read is halted.
//!
//! **The switch** (`switch.rs`): halting is open to anyone who may speak to
//! the daemon (the CLI, the web UI, Discord's "halt self", the MCP server
//! is not one) and is idempotent; a resume counts only from the owner in a
//! private place (`judge_act(Act::SelfResume)`, the rule an extension's Load
//! answers to), never from a job's shell, and a refused one is an
//! `approval.refused` row. Each move is one frame: the META record and its
//! `self.halted` or `self.resumed` row. The record survives a restart and an
//! install, as the store does.
//!
//! **The log** (`log.rs`): `self.log` reads the `self.*` rows
//! (`theseus_protocol::rsi::SELF_ROWS`, declared before their writers land)
//! together with today's self-change rows (`TODAY`: the ladder's moves, the
//! learning loop's proposals, the extensions, the owner's routing
//! corrections, consolidation's syntheses), newest first, each with what,
//! why, its numbers and its undo. `digest.rs` makes the week's text of them.

mod digest;
mod log;
mod switch;

use std::sync::{Mutex, PoisonError, RwLock};

use serde::{Deserialize, Serialize};
use theseus_protocol::rsi::{SelfMode, SelfState};
use theseus_store::{kinds, NewRecord};

use crate::store::Store;

pub use digest::{digest_text, week_of, DIGEST_KEY, WEEK_MS};
pub use log::{entry_of, TODAY};

/// The switch's META record.
pub const SWITCH_KEY: &str = "self.switch";

/// What a self step hears when it asks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Gate {
    /// It may run (this phase).
    Allowed,
    /// The kill switch holds every self step: why, and since when (none for
    /// a store that has never been resumed).
    Halted { why: String, at_ms: Option<u64> },
    /// `[self] mode = "off"`: nothing self-directed runs.
    Off,
}

impl Gate {
    /// Its name, as `SelfState::gate` carries it.
    pub fn name(&self) -> &'static str {
        match self {
            Gate::Allowed => "allowed",
            Gate::Halted { .. } => "halted",
            Gate::Off => "off",
        }
    }
}

/// What [`gate`] reads: the mode the daemon started with, and the switch.
pub struct Ctx<'a> {
    pub mode: SelfMode,
    pub switch: &'a Switch,
    pub store: &'a Store,
}

/// May a self step run now? The module's contract.
pub fn gate(ctx: &Ctx<'_>) -> Gate {
    if ctx.mode == SelfMode::Off {
        return Gate::Off;
    }
    match ctx.switch.current(ctx.store) {
        Ok(Some(r)) if !r.halted => Gate::Allowed,
        Ok(Some(r)) => Gate::Halted {
            why: r.why.map_or_else(
                || format!("halted by {}", r.by),
                |w| format!("halted by {}: {w}", r.by),
            ),
            at_ms: Some(r.at_ms),
        },
        Ok(None) => Gate::Halted {
            why: NEVER_RESUMED.into(),
            at_ms: None,
        },
        Err(e) => Gate::Halted {
            why: format!("the kill switch could not be read ({e:#}), so it holds"),
            at_ms: None,
        },
    }
}

/// Why a store that has never been resumed is halted.
pub const NEVER_RESUMED: &str =
    "never resumed: the owner's first `theseus self resume` starts self-improvement";

/// The switch as stored: its last move.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SwitchRecord {
    pub halted: bool,
    pub at_ms: u64,
    pub by: String,
    pub via: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}

impl SwitchRecord {
    pub fn record(&self) -> anyhow::Result<NewRecord> {
        NewRecord::json(kinds::META, Some(SWITCH_KEY), self)
    }
}

/// The switch, cached: read from the store at the first ask, then kept by
/// the moves that write it. `writes` holds a move's read and its frame
/// together, so two moves never cross.
#[derive(Debug, Default)]
pub struct Switch {
    /// `None`: not read yet; `Some(None)`: never moved (halted).
    read: RwLock<Option<Option<SwitchRecord>>>,
    pub(crate) writes: Mutex<()>,
    /// When the weekly digest last went to the owner's DM (`digest.rs`):
    /// `None` until read, while `[self]` posts it.
    pub(crate) digest_at: Mutex<Option<u64>>,
}

impl Switch {
    /// The switch's last move, read once and then from memory.
    pub fn current(&self, store: &Store) -> anyhow::Result<Option<SwitchRecord>> {
        if let Some(r) = &*self.read.read().unwrap_or_else(PoisonError::into_inner) {
            return Ok(r.clone());
        }
        let r = store.get_meta::<SwitchRecord>(SWITCH_KEY)?;
        *self.read.write().unwrap_or_else(PoisonError::into_inner) = Some(r.clone());
        Ok(r)
    }

    /// Keep a move its frame wrote.
    pub(crate) fn set(&self, r: SwitchRecord) {
        *self.read.write().unwrap_or_else(PoisonError::into_inner) = Some(Some(r));
    }

    /// Forget what was read, so the next ask reads the store (a test's
    /// restart in place).
    #[cfg(test)]
    pub fn forget(&self) {
        *self.read.write().unwrap_or_else(PoisonError::into_inner) = None;
    }
}

/// The state every surface shows: the mode, the switch, and the gate.
pub fn state(ctx: &Ctx<'_>) -> SelfState {
    let g = gate(ctx);
    let r = ctx.switch.current(ctx.store).ok().flatten();
    let halted = !matches!(&r, Some(r) if !r.halted);
    SelfState {
        mode: ctx.mode,
        halted,
        never_resumed: r.is_none(),
        at_ms: r.as_ref().map(|r| r.at_ms),
        by: r.as_ref().map(|r| r.by.clone()),
        via: r.as_ref().map(|r| r.via.clone()),
        why: r
            .as_ref()
            .and_then(|r| r.why.clone())
            .or_else(|| r.is_none().then(|| NEVER_RESUMED.to_string())),
        gate: g.name().into(),
    }
}
