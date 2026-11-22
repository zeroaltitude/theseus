//! The owner is told when the self loop cannot start anything (issue 41). A hold on the slots that
//! has lasted [`TELL_AFTER`] is said where the owner looks: the morning notice's "Waits for you" and
//! the Ops page (`health`'s `self_held`), with each tree that holds a slot and the one act that clears
//! it. Before, the reason was only in `theseus self pipeline`, and nine hours went by unseen.

use std::time::Duration;

use theseus_protocol::rsi::SelfMode;

use crate::rpc::Core;
use crate::rsi::capacity;
use crate::rsi::join::{guard, rows_of};

/// How long a hold on the slots lasts before the owner is told. Past an install's longest build
/// (two hours, `join::install`), so a live install is never told as a stuck one.
pub const TELL_AFTER: Duration = Duration::from_secs(3 * 3600);

/// The one act that clears `tree`, a tree under the trees directory of the clone `repo`.
fn act(tree: &str, trees: &std::path::Path, repo: &std::path::Path) -> String {
    if guard::is_phase_tree(tree) {
        "restart the daemon (`systemctl --user restart theseusd`): its start closes a phase's tree \
         that no pass of its own holds"
            .into()
    } else if tree.starts_with("join-") {
        format!(
            "remove the merge's tree: `git -C {} worktree remove --force {}`",
            repo.display(),
            trees.join(tree).display()
        )
    } else {
        "`theseus self drop` its item, which `theseus self pipeline` names".into()
    }
}

impl Core {
    /// The line for the owner when the loop has been held on its slots for [`TELL_AFTER`] or more.
    pub fn self_held(&self) -> Option<String> {
        self.self_held_at(theseus_protocol::now_unix_ms())
    }

    /// The same, at `now_ms`. None with the mode off, with no `[self] repo`, while the slots are not
    /// all taken by trees, while the last thing the pipeline said is not a hold on the slots, or
    /// before [`TELL_AFTER`] has gone by since it said so.
    pub(crate) fn self_held_at(&self, now_ms: u64) -> Option<String> {
        let cfg = &self.cfg.self_improve;
        if cfg.mode != SelfMode::Act {
            return None;
        }
        let place = self.tools.self_pr.place(cfg.repo.as_deref()?);
        let open = capacity::trees_named(&place.trees());
        if open.is_empty() || open.len() < cfg.pipeline_builds as usize {
            return None;
        }
        let rows = rows_of(
            &self.store,
            &["self.pipeline.held", "self.pipeline.started"],
        )
        .ok()??;
        let last = rows.iter().max_by_key(|r| r.at_unix_ms)?;
        if last.kind != "self.pipeline.held" || last.data["hold"] != "slots" {
            return None;
        }
        let held = now_ms.saturating_sub(last.at_unix_ms);
        if held < TELL_AFTER.as_millis() as u64 {
            return None;
        }
        let trees = place.trees();
        let acts: Vec<String> = open
            .iter()
            .map(|t| format!("{t}: {}", act(t, &trees, &place.repo)))
            .collect();
        Some(format!(
            "the self loop has started nothing for {} h: every slot is taken by an open tree. To \
             clear it, {}",
            held / 3_600_000,
            acts.join("; ")
        ))
    }
}
