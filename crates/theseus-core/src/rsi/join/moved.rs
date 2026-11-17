//! A finished change that no longer merges is sent back to its builder (issue 20), found before
//! its review is spent.
//!
//! **Where the check sits: the head of the join pass, before a branch's review.** The pass holds
//! `Shared::joins`, so nothing reviews or joins the head while it runs, and it already owns the
//! conflict path (`joining`'s `conflicted`: the card, the held stop, the bound on repeats), which
//! this check enters. A check in the pipeline's tick would run without that lock and could race
//! a review of the same head, and would need a second copy of the card and the bound. The cost is
//! one `git fetch` and one `merge-tree` per unreviewed branch per pass.
//!
//! A branch whose dry run on main's tip conflicts goes back as `conflicted` does: the item is
//! `proposed` again, the head is held so no pass joins it, and the conflict is counted for the
//! bound (`MAX_CONFLICTS`). What differs is what the builder gets: a tree on the branch's own
//! commits with main merged into it ([`crate::rsi::tree_main`]) and a brief that says so
//! (`backlog::brief::main_moved`), where it used to get a fresh branch off main. Nothing of
//! this is a try (`backlog::attempts` reads no `conflict:` or `main-merged:` evidence), so a
//! change that was fine is not failed for main having moved.

use anyhow::Result;
use serde_json::{json, Value};
use theseus_protocol::tasks::TaskEvidence;

use super::joining::tree_not_closed_line;
use super::{blocking, git, Run};
use crate::rsi::tree;
use crate::rsi::tree_main::{self, Brought};

/// The evidence identity of a tree that holds a branch's change with main merged in.
pub(crate) const MAIN_MERGED: &str = "main-merged:";

impl Run<'_> {
    /// Whether the branch no longer merges into main's tip and was sent back to its builder
    /// (then it is not reviewed). A dry run that could not be made is no answer: the review goes
    /// on as it always did, and the join's own dry run asks again.
    pub(super) async fn moved_under(&mut self, remote: &str) -> Result<bool> {
        let (p, r, h) = (self.place.clone(), remote.to_string(), self.b.head.clone());
        let dry = blocking(move || {
            git::fetch(&p, &r)?;
            let base = git::main_of(&p, &r)?;
            if p.git(&p.repo, &["merge-base", "--is-ancestor", &h, &base])?
                .status
                .success()
            {
                return Ok(Ok(()));
            }
            git::dry_run(&p, &base, &h)
        })
        .await;
        match dry {
            Ok(Err(why)) => {
                self.conflicted(remote, &why).await?;
                Ok(true)
            }
            Ok(Ok(())) => Ok(false),
            Err(e) => {
                tracing::warn!(branch = %self.b.branch, error = %format!("{e:#}"),
                    "self-join: main was not compared with the branch before its review");
                Ok(false)
            }
        }
    }

    /// The item's working copy with main merged into it; none when it could not be made (the
    /// item has a tree already, or git said no), and the item is built again off the new main.
    pub(super) async fn main_into_tree(&self, remote: &str) -> Option<Brought> {
        let (p, r, item, branch) = (
            self.place.clone(),
            remote.to_string(),
            self.b.item.clone(),
            self.b.branch.clone(),
        );
        let made = blocking(move || {
            let main = git::main_of(&p, &r)?;
            tree_main::bring_main_in(&p, &item, &branch, &main)
        })
        .await;
        match made {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!(branch = %self.b.branch, error = %format!("{e:#}"),
                    "self-join: main was not merged into a working copy of the branch");
                None
            }
        }
    }

    /// Close the item's tree, so the next build opens a new one off the new main
    /// (`self_tree_open` would find this one); a tree that will not close is said.
    pub(super) async fn close_stale_tree(&self, text: &mut String, numbers: &mut Value) {
        let (p, c) = (self.place.clone(), self.b.item.clone());
        // The item's sessions first (`Core::close_item_trees`): nothing of theirs outlives the tree.
        if let Err(e) = self.core.end_item_sessions(&c, None).await {
            tracing::warn!(error = %format!("{e:#}"), "self-join: the item's sessions were not ended");
        }
        if let Err(e) = blocking(move || {
            if let Some(t) = tree::find(&p, &c) {
                tree::close(&p, &t)?;
            }
            Ok(())
        })
        .await
        {
            // The log keeps the error. The owner gets what it means: the next
            // build may find this stale tree and cut from its old base
            // (theseus-51sfw). The conflict's card and stop still go out.
            tracing::warn!(error = %format!("{e:#}"),
                "self-join: the conflicted tree was not closed");
            text.push('\n');
            text.push_str(&tree_not_closed_line(&self.b.item));
            numbers["tree_closed"] = json!(false);
        }
    }
}

/// The evidence that says the item's tree holds its change with main merged in: the branch the
/// tree is on, and what the merge left (the builder's brief reads it).
pub(super) fn main_merged(b: &Brought, at_ms: u64) -> TaskEvidence {
    let branch = &b.tree.branch;
    let note = if b.conflicts.is_empty() {
        "main merged in without a clash; the merge waits for its commit".to_string()
    } else {
        format!("it conflicts with main in {}", b.conflicts.join(", "))
    };
    TaskEvidence {
        node: None,
        identity: format!("{MAIN_MERGED}{branch}"),
        note: Some(note),
        by: "self-join".into(),
        at_ms,
    }
}

/// The owner's note for a branch that no longer merged and went back to its builder.
pub(crate) fn moved_card(branch: &str, item: &str, why: &str) -> String {
    crate::owner_note::note(
        crate::owner_note::Kind::Fyi,
        &format!("Self branch {branch} no longer merged with main"),
        &format!("its join found a conflict ({item})"),
        &format!(
            "{why}. The change is back with its builder, with main merged into its working copy \
             to resolve what clashes; it is checked again after that, and this is not counted as \
             a failed try. Nothing waits for you."
        ),
    )
}
