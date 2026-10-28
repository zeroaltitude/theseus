//! How deep a task sits (theseus-tecj5): the number of executions above it,
//! read by walking the `parent` links of records that already exist, so
//! nothing about depth is stored. A conversation is depth 0, its task 1, that
//! task's task 2.
//!
//! The walk follows at most [`WALK_MAX`] parent links (so a chain holds up to
//! `WALK_MAX + 1` executions) and never visits one execution twice, so a
//! corrupt store's cycle ends it. A parent link to a record that is not there
//! ends it too: no path writes an execution out of a store, so an absent
//! ancestor is damage, not a shorter tree. A cut walk reports the bound itself
//! as its depth: it errs toward refusing a task, never toward opening one.

use anyhow::Result;

use crate::kernel::Kernel;
use crate::types::*;

/// The most parent links one walk follows. Above any depth the config allows
/// (`DEPTH_LIMIT_MAX`).
pub const WALK_MAX: usize = 64;
/// The largest depth limit the config may name.
pub const DEPTH_LIMIT_MAX: u32 = 32;
/// How deep tasks may go when the config names none: the owner's "lets start
/// with 10 deep, and observe" (2026-10-25).
pub const DEFAULT_DEPTH_LIMIT: u32 = 10;

/// An execution and the executions above it.
#[derive(Debug, Clone)]
pub struct Ancestry {
    /// The execution asked about first, then its parent, and so on up.
    pub chain: Vec<Execution>,
    /// The walk ended at [`WALK_MAX`], at a repeat, or at an ancestor whose
    /// record is missing, not at a top.
    pub cut: bool,
}

impl Ancestry {
    /// Executions above the first: its depth, or [`WALK_MAX`] when cut. A
    /// task whose parent link is missing still counts one.
    pub fn depth(&self) -> usize {
        if self.cut {
            return WALK_MAX;
        }
        let above = self.chain.len().saturating_sub(1);
        match self.chain.first() {
            Some(e) if e.kind == SessionKind::Task => above.max(1),
            _ => above,
        }
    }
}

impl Kernel {
    /// The depth limit tasks may reach: `KernelConfig::task_depth_limit`,
    /// at least 1 and at most [`DEPTH_LIMIT_MAX`].
    pub fn task_depth_limit(&self) -> u32 {
        self.config().task_depth_limit.clamp(1, DEPTH_LIMIT_MAX)
    }

    /// `id` and its ancestors, nearest first. Plain reads, no locks: an
    /// execution's parent never changes.
    pub fn ancestry(&self, id: &str) -> Result<Ancestry> {
        let mut chain: Vec<Execution> = Vec::new();
        let mut at = Some(id.to_string());
        while let Some(cur) = at {
            if chain.len() > WALK_MAX || chain.iter().any(|e| e.id == cur) {
                return Ok(Ancestry { chain, cut: true });
            }
            let Some(e) = self.execution(&cur)? else {
                if chain.is_empty() {
                    return Err(crate::kernel::KernelError::UnknownExecution(cur).into());
                }
                return Ok(Ancestry { chain, cut: true });
            };
            at = e.parent.clone();
            chain.push(e);
        }
        Ok(Ancestry { chain, cut: false })
    }

    /// How deep `id` sits (see the module doc).
    pub fn task_depth(&self, id: &str) -> Result<usize> {
        Ok(self.ancestry(id)?.depth())
    }
}
