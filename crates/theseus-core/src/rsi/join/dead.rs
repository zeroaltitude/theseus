//! A start closes a phase's tree that a daemon that is gone left open (issue 41; the evidence is
//! `guard`'s doc), with a `self.pipeline.cleared` row for each: which tree, which phase, and why.

use serde_json::json;

use super::guard;
use crate::fact::rsi_pipeline::{Cleared, Moved};
use crate::rpc::Core;

/// Why a start closes such a tree, as its row says.
pub const WHY: &str = "a daemon that is gone left it open: this daemon did not open it, and only \
     this daemon serves this state directory, so no pass of its own is running in it; it held a \
     pipeline slot";

impl Core {
    /// The start's sweep: every phase tree no pass of this daemon opened is closed, each with its
    /// row. Nothing else is done: no join, install or revert starts or stops. The trees it closed.
    pub fn close_dead_trees(&self) -> Vec<guard::Dead> {
        let Some(repo) = self.cfg.self_improve.repo.clone() else {
            return Vec::new();
        };
        let place = self.tools.self_pr.place(&repo);
        let dead = guard::close_dead(&place);
        for d in &dead {
            let m = match &d.closed {
                Ok(()) => Moved::new("", format!("the {} tree {} is closed", d.phase, d.tree)),
                Err(e) => Moved::new(
                    "",
                    format!("the {} tree {} could not be closed: {e}", d.phase, d.tree),
                ),
            };
            let m = m
                .with("tree", d.tree.as_str())
                .with("phase", d.phase.as_str())
                .with("reason", "left_by_a_gone_daemon")
                .with("closed", d.closed.is_ok())
                .why(WHY)
                .numbers(json!({"trees": usize::from(d.closed.is_ok())}));
            self.rec(None).record(&Cleared(m));
            match &d.closed {
                Ok(()) => tracing::info!(tree = %d.tree, phase = %d.phase,
                    "self: a tree a gone daemon left is closed, and its slot is free"),
                Err(e) => tracing::warn!(tree = %d.tree, phase = %d.phase, error = %e,
                    "self: a tree a gone daemon left could not be closed"),
            }
        }
        dead
    }
}
