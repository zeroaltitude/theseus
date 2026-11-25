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
//! is not one) and is idempotent; a resume counts only from the owner, from
//! any place (`judge_act(Act::SelfResume)`, `places::owner_anywhere`: the
//! CLI, the web UI, or an author holding an owner handle; the owner's call of
//! 2026-10-10), never from a job's shell, and a refused one is an
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
//!
//! **What stops a failing loop** (the cap on tries, the rules the builder is told, formatting
//! before the gate, no start on a red main, a tree that stays under its gate, a stale item settled,
//! set-aside, a join that leaves no tree, the install's reap): `docs/design/self-loop-failure-handling.md`.
//!
//! **The builder's build folder** (`builder_dir`: the one directory a self builder's commands keep, so it can run
//! the tests of what it writes, apart from the gate's build tree and removed with the item):
//! `docs/design/self-builder-build-folder.md`.
//!
//! **Libraries** (`deps`: what the builder may download, the packages the pipeline fetches for a gate from a branch's
//! lock files, what is refused, and the change of dependencies that stops at the owner's card):
//! `docs/design/self-dependencies.md`.

pub mod backlog;
pub mod budget;
pub mod builder_dir;
pub mod capacity;
pub mod deps;
pub(crate) mod digest;
pub mod evidence;
pub mod exams;
pub mod infra;
pub mod jail;
pub mod join;
mod log;
pub mod mine;
pub mod mint;
pub mod narrow;
pub mod outgoing;
pub(crate) mod own_removals;
pub(crate) mod own_tree;
pub mod pause;
pub mod pause_tool;
pub mod pipeline;
pub mod pr;
pub mod reach;
pub mod rerun;
pub mod revert;
pub(crate) mod roles;
pub mod sealed;
mod switch;
#[cfg(test)]
mod tests_budget;
#[cfg(test)]
mod tests_fetch_bound;
#[cfg(test)]
mod tests_fmt;
#[cfg(test)]
mod tests_identity;
#[cfg(test)]
mod tests_jail;
#[cfg(test)]
mod tests_lease;
#[cfg(test)]
mod tests_narrow;
#[cfg(test)]
mod tests_pr;
#[cfg(test)]
mod tests_reach;
#[cfg(test)]
mod tests_reach_within;
#[cfg(test)]
mod tests_rerun;
#[cfg(test)]
pub(crate) mod tests_tree;
#[cfg(test)]
mod tests_tree_git;
pub mod tree;
pub mod tree_fmt;
pub(crate) mod tree_git;
pub mod tree_lease;
pub mod tree_main;
pub mod trees_lock;

use std::sync::{Mutex, PoisonError, RwLock};

use serde::{Deserialize, Serialize};
use theseus_protocol::rsi::{SelfMode, SelfState};
use theseus_store::{kinds, NewRecord};

use crate::store::Store;

pub use digest::{capped as digest_capped, line as digest_line};
pub use digest::{digest_text, week_of, DIGEST_KEY, NOTICE_KEY, WEEK_MS};
pub use log::{entry_of, TODAY};

/// The principal of every self-directed task and its sessions (S1): a reserved
/// string that holds no owner handle and can never ack anything.
pub const SELF_PRINCIPAL: &str = "self";

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
    /// The local day the morning notice was last settled for, `2026-10-18`
    /// (`digest/notice.rs`): `None` until read.
    pub(crate) notice_day: Mutex<Option<String>>,
    /// One backlog move at a time: a night, a drop, a move (`backlog.rs`).
    pub(crate) backlog: Mutex<()>,
    /// The starts no note has told yet: one read and write at a time
    /// (`backlog/folded.rs`). Lock order: `backlog` first, then `folded`,
    /// never the reverse (the night holds `backlog` while it reads and takes
    /// the record).
    pub(crate) folded: Mutex<()>,
    /// Test-only: how many times `folded_read` has read the store on this
    /// `Core`, so a test can assert a count instead of inferring one from an
    /// interleaving seam (theseus-1eemp, B5 L28 residue).
    #[cfg(test)]
    pub(crate) folded_reads: std::sync::atomic::AtomicU64,
    /// The self pipeline's one tick at a time, its one self-join pass at a
    /// time, and its kick (`pipeline.rs`, theseus-pw1q.16).
    pub(crate) pipeline: pipeline::Shared,
}

impl Switch {
    /// The switch's last move, read once and then from memory.
    pub fn current(&self, store: &Store) -> anyhow::Result<Option<SwitchRecord>> {
        self.current_with(|| store.get_meta::<SwitchRecord>(SWITCH_KEY))
    }

    fn current_with(
        &self,
        load: impl FnOnce() -> anyhow::Result<Option<SwitchRecord>>,
    ) -> anyhow::Result<Option<SwitchRecord>> {
        if let Some(r) = &*self.read.read().unwrap_or_else(PoisonError::into_inner) {
            return Ok(r.clone());
        }
        let r = load()?;
        let mut cached = self.read.write().unwrap_or_else(PoisonError::into_inner);
        Ok(cached.get_or_insert(r).clone())
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
        day_spent_usd: None,
        day_line_usd: None,
        sealed: None,
    }
}

#[cfg(test)]
mod switch_cache_tests {
    use super::{Switch, SwitchRecord};

    #[test]
    fn a_cold_read_cannot_overwrite_a_completed_halt() {
        let switch = Switch::default();
        let halted = SwitchRecord {
            halted: true,
            at_ms: 2,
            by: "owner".into(),
            via: "cli".into(),
            why: None,
        };
        let mut resumed = halted.clone();
        resumed.halted = false;
        resumed.at_ms = 1;
        let got = switch
            .current_with(|| {
                // The store read began before the halt, but finishes after it.
                switch.set(halted.clone());
                Ok(Some(resumed))
            })
            .unwrap();
        assert_eq!(got, Some(halted.clone()));
        assert_eq!(
            switch.current_with(|| panic!("cached")).unwrap(),
            Some(halted)
        );
    }
}
