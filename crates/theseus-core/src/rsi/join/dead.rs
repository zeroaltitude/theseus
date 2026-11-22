//! A start closes a tree the loop opened that a daemon that is gone left open (issue 41; what proves it is
//! `guard`'s doc), with a `self.pipeline.cleared` row for each it closes or could not close: which tree, which
//! phase, and why. A directory the loop did not open is left as it is and said in the log.

use serde_json::json;

use super::guard::{self, Dead, NoSweep, Swept};
use crate::fact::rsi_pipeline::{Cleared, Moved};
use crate::rpc::Core;

/// Why a start closes such a tree, as its row says.
pub const WHY: &str = "the loop opened it (its mark is there), this daemon did not, and this daemon owns the \
     self loop's trees, so no pass is running in it: a daemon that is gone left it open, and it held a pipeline \
     slot";

impl Core {
    /// The start's sweep: every tree with the loop's mark that this daemon did not open is closed, each with its
    /// row. Nothing else is done: no join, install or revert starts or stops. What it found, or why it did not look.
    pub fn close_dead_trees(&self) -> Result<Vec<Dead>, NoSweep> {
        let Some(repo) = self.cfg.self_improve.repo.clone() else {
            return Ok(Vec::new());
        };
        let place = self.tools.self_pr.place(&repo);
        let dead = guard::close_dead(&place).inspect_err(|why| match why {
            NoSweep::NotOwner => tracing::info!(
                "self: {}: this daemon does not sweep the trees",
                crate::rsi::trees_lock::NOT_OWNER
            ),
            NoSweep::Outside => tracing::warn!(trees = %place.trees().display(),
                "self: the trees directory leads out of the state directory: no sweep"),
        })?;
        for d in &dead {
            self.swept_row(d);
        }
        Ok(dead)
    }

    /// The row of one tree the sweep closed or could not close; a cleared mark and a directory the loop did not
    /// open are said in the log only.
    fn swept_row(&self, d: &Dead) {
        let (what, closed) = match &d.swept {
            Swept::Closed => (format!("the {} tree {} is closed", d.phase, d.tree), true),
            Swept::NotClosed(e) => (
                format!("the {} tree {} could not be closed: {e}", d.phase, d.tree),
                false,
            ),
            Swept::MarkCleared => {
                tracing::info!(tree = %d.tree, "self: a mark with no tree any more is cleared");
                return;
            }
            Swept::NotTheLoops => {
                tracing::info!(tree = %d.tree,
                    "self: a directory named like a phase's tree, which the loop did not open: left as it is");
                return;
            }
        };
        let m = Moved::new("", what)
            .with("tree", d.tree.as_str())
            .with("phase", d.phase.as_str())
            .with("reason", "left_by_a_gone_daemon")
            .with("closed", closed)
            .why(WHY)
            .numbers(json!({"trees": usize::from(closed)}));
        self.rec(None).record(&Cleared(m));
    }
}
