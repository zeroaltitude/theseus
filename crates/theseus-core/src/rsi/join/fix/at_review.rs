//! The fix a review asked for (issue 36): the review's `VERDICT: fix` row is written, then the
//! pass reopens the item's tree on the branch at the reviewed head, makes the fix
//! ([`super::make`]), runs main's gate on it, has the builder's profile read the fix's commit
//! alone, and closes the tree; the fixed branch stays in `[self] repo` (issue 53). The accepted fix's `self.fix.verdict` row is the
//! branch's new head (`join::seen`), so the next pass runs the planted revert and a fresh,
//! independent review on it, told a fix was made; a second `fix` there is a reject.
//!
//! Anything else is today's path for a reject: the item goes back to the backlog with a `review:`
//! word (one try, as a reject is), and the words say a fix was tried. A fix cut short by a restart
//! (started, never ended) is ended so at the next pass, never made twice.

use std::sync::Arc;

use anyhow::Result;
use serde_json::Value;

use super::make::Job;
use super::{ends, Point};
use crate::fact::rsi_fix::{Moved, NoFix};
use crate::rsi::join::{blocking, git, remote_of, Run};
use crate::rsi::tree;

impl Run<'_> {
    /// What the review just asked a fix to repair.
    pub(in crate::rsi::join) fn asked_fix(&mut self) -> Vec<String> {
        std::mem::take(&mut self.asked)
    }

    /// The fix the review of this head asked for, once. `Ok(false)` when a halt stopped the pass.
    pub(in crate::rsi::join) async fn fix_at_review(&mut self, reasons: &[String]) -> Result<bool> {
        let rows = super::rows_of(&self.core.store, &self.b.branch, Point::Review)?;
        let Some(rows) = rows else {
            return Ok(true);
        };
        if rows.iter().any(|r| ends(&r.kind)) {
            return Ok(true);
        }
        if rows.iter().any(|r| r.kind == "self.fix.started") {
            let why = "a restart cut the fix short, so it was not made".to_string();
            let m = Moved::new(
                &self.b.item,
                format!("no fix of {} at review", self.b.branch),
            )
            .with("branch", self.b.branch.clone())
            .with("head", self.b.head.clone())
            .with("point", Point::Review.name())
            .why(why.clone());
            self.core.rec(None).record(&NoFix(m));
            // The tree it reopened goes with it, so nothing of the fixer's is built on.
            let (p, item, branch) = (
                self.place.clone(),
                self.b.item.clone(),
                self.b.branch.clone(),
            );
            let closed = blocking(move || match tree::find(&p, &item) {
                Some(t) if t.branch == branch => tree::close(&p, &t),
                _ => Ok(()),
            })
            .await;
            if let Err(e) = closed {
                tracing::warn!(error = %format!("{e:#}"), "self fix: a cut-short fix's tree was not closed");
            }
            self.fix_returned(&format!("a fix was tried at review and {why}"))?;
            return Ok(true);
        }
        if !self.phase("fix") {
            return Ok(false);
        }
        let failure = reasons.join("\n");
        let (p, r) = (self.place.clone(), remote_of(&self.core.cfg));
        let (item, branch, head) = (
            self.b.item.clone(),
            self.b.branch.clone(),
            self.b.head.clone(),
        );
        let opened = blocking(move || {
            git::fetch(&p, &r)?;
            let main = git::main_of(&p, &r)?;
            Ok((main, tree::reopen(&p, &item, &branch, &head)?))
        })
        .await;
        let (main, t) = match opened {
            Ok(o) => o,
            Err(e) => {
                self.stop(
                    "fix",
                    &format!("the tree for the fix could not be made: {e:#}"),
                    Value::Null,
                );
                return Ok(true);
            }
        };
        let core: &Arc<crate::rpc::Core> = self.core;
        let _live = core.tools.self_pr.live_fix(&t.branch);
        let job = Job {
            core: self.core,
            place: &self.place,
            tree: &t,
            item: &self.b.item,
            point: Point::Review,
            builder: self.b.session.as_deref(),
            why: &self.b.why,
            fault: "the review's finding".into(),
            failure,
            main,
            fixer: self.seams.fixer.clone(),
        };
        let out = self.fixed_at_review(&job).await;
        let (p, tr) = (self.place.clone(), t.clone());
        if let Err(e) = blocking(move || tree::close(&p, &tr)).await {
            tracing::warn!(error = %format!("{e:#}"), branch = %t.branch, "self fix: the tree was not closed");
        }
        if let Err(why) = out {
            self.fix_returned(&why)?;
        }
        Ok(true)
    }

    /// Make, gate and approve: the approver, or the words of why the fix did not hold.
    async fn fixed_at_review(&self, job: &Job<'_>) -> Result<String, String> {
        let made = job.make().await?;
        let (p, t) = (self.place.clone(), job.tree.clone());
        let gated = tree::gate(&p, &t).await.and_then(tree::Gated::ran);
        match gated {
            Ok(run) if run.verdict == tree::Verdict::Green => {}
            Ok(run) => return Err(job.red_again(&made, &run.tail)),
            Err(e) => return Err(job.red_again(&made, &format!("the gate did not run: {e:#}"))),
        }
        let approver = job.approve(&made).await?;
        if let Err(e) = self.fix_report(job, &made, &approver) {
            tracing::warn!(error = %format!("{e:#}"), branch = %job.tree.branch, "self fix: its report was not written");
        }
        // The fixed branch stays in `[self] repo` (issue 53): the join reads it there.
        Ok(approver)
    }

    /// The branch's frozen report is never written again, so a fix at review has its own beside it,
    /// written once and read-only as the report is: what was fixed, by which model, how large, and
    /// what approved it.
    fn fix_report(&self, job: &Job<'_>, m: &super::make::Made, approver: &str) -> Result<()> {
        let path = self.place.reports().join(format!(
            "{}-fix-review.md",
            job.tree.branch.replace('/', "-")
        ));
        if path.exists() {
            anyhow::bail!("{} is frozen already", path.display());
        }
        let text = format!(
            "# The fix at review of self branch {branch}\n\n- Item: {item}\n- Reviewed head: {from}\n\
             - Fix: {head}, {lines} lines in {files} files\n- Fixed by: {fixer}\n\
             - Approved by: {approver}\n- Report of the branch: {report}\n\n## What the review \
             asked\n\n{failure}\n\n## The fixer's reason\n\n{reason}\n\n{complete}\n",
            branch = job.tree.branch,
            item = job.item,
            from = m.before,
            head = m.head,
            lines = m.lines,
            files = m.files,
            fixer = m.fixer,
            report = self.b.report.as_deref().unwrap_or("none"),
            failure = job.failure,
            reason = m.reason,
            complete = crate::rsi::pr::COMPLETE,
        );
        let (text, _) = self.core.tools.scrubber.scrub(&text);
        std::fs::create_dir_all(self.place.reports())?;
        let tmp = path.with_extension("md.tmp");
        std::fs::write(&tmp, text)?;
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o444))?;
        std::fs::rename(&tmp, &path)?;
        Ok(())
    }

    /// The item back on the backlog as after a reject, with a fix's words.
    fn fix_returned(&mut self, why: &str) -> Result<()> {
        let identity = format!(
            "review:{}@{}",
            self.b.branch,
            &self.b.head[..self.b.head.len().min(12)]
        );
        let note = format!("rejected at review: {why}");
        let note = crate::scrub::safe::head(&note, 1_000).0.to_string();
        let _one = self
            .core
            .rsi
            .backlog
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(rec) = self.reopened(identity, note)? {
            self.core.store.append(&[rec])?;
        }
        self.pass
            .reviewed
            .push((self.b.branch.clone(), "fix-failed".into()));
        Ok(())
    }
}
