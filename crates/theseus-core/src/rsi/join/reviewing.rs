//! The review's phases of one branch (theseus-pw1q.9): the independence
//! check, the planted revert, the reviewer, and the verdict's row.

use anyhow::Result;
use serde_json::Value;
use theseus_protocol::tasks::{TaskEvidence, TaskState};
use theseus_store::NewRecord;

use super::planted::{self, Outcome};
use super::review::Ask;
use super::{blocking, git, remote_of, Run, Said};
use crate::fact::rsi_join::SelfReviewVerdict;
use crate::turn::Target;

/// Who would review, and whether that review would be independent.
struct Reviewer {
    target: Option<Target>,
    /// `fable (claude-fable-5-1)`.
    name: String,
    model: String,
    refusal: Option<String>,
}

impl Run<'_> {
    /// The reviewer's target against the builder's model: `[self]
    /// reviewer_profile` when it is set (a pin), else Jev's choice for the
    /// review, told the builder's model (route.v10, theseus-ej6bj). Review
    /// independence holds either way: a reviewer on the builder's model is
    /// refused, as it always was, and the rule is not a cap.
    async fn reviewer(&self) -> Reviewer {
        let core = self.core;
        let builder = self
            .b
            .session
            .as_deref()
            .and_then(|s| {
                core.store
                    .get_session::<crate::session::SessionRecord>(s)
                    .ok()
                    .flatten()
            })
            .and_then(|r| r.last_target)
            .map(|t| t.model);
        let live = core.live_profile().0;
        let site = crate::job_model::Site {
            name: "self reviewer",
            pin: core.cfg.self_improve.reviewer_profile.as_deref(),
            legacy: Some(crate::config::legacy::SELF_REVIEWER),
            job: theseus_judge::builders::JobKind::SelfReviewer {
                diff_summary: self.b.why.clone(),
                builder_model: builder.clone(),
                // The change is the assistant's own code, written from the
                // owner's sessions: owner material, US options only.
                private: true,
            },
            restricted: theseus_judge::builders::RestrictedModels::NotAllowed,
        };
        let target = core.runner.job_target(&live, site).await.map(|(t, _)| t);
        let profile = match &target {
            Ok(t) => t.profile.clone(),
            Err(e) => e.profile.clone(),
        };
        let refusal = match (&target, &builder) {
            (Err(e), _) => Some(format!("[self] reviewer_profile {e}")),
            (_, None) => Some(
                "the builder's model is not known, so no review can be shown independent of it"
                    .into(),
            ),
            (Ok(t), Some(m)) if t.model == *m => Some(format!(
                "the reviewer would run on {m}, the model that built the branch: an independent \
                 review is on another model, so set [self] reviewer_profile to one"
            )),
            _ => None,
        };
        let target = target.ok();
        Reviewer {
            name: target
                .as_ref()
                .map_or_else(|| profile.clone(), |t| format!("{profile} ({})", t.model)),
            model: target.as_ref().map(|t| t.model.clone()).unwrap_or_default(),
            target,
            refusal,
        }
    }

    /// The review: the verdict written, or none when a halt stopped it.
    pub(super) async fn review(&mut self) -> Result<Option<String>> {
        let who = self.reviewer().await;
        let said = |verdict, planted, reasons, cost_usd| Said {
            reviewer: who.name.clone(),
            model: who.model.clone(),
            verdict,
            planted,
            reasons,
            cost_usd,
        };
        if let Some(why) = who.refusal.clone() {
            self.verdict(said("refused", "not-run", vec![why], 0.0))?;
            return Ok(Some("refused".into()));
        }
        let Some(target) = who.target.clone() else {
            return Ok(None);
        };
        let Some(planted) = self.planted().await else {
            return Ok(None);
        };
        if let Some(why) = planted.reason() {
            self.verdict(said("reject", planted.name(), vec![why], 0.0))?;
            return Ok(Some("reject".into()));
        }
        if !self.phase("review") {
            return Ok(None);
        }
        let remote = remote_of(&self.core.cfg);
        let ask = self.ask(&remote, target).await?;
        let core = self.core.clone();
        let r = match self.seams.reviewer.review(&core, &ask).await {
            Ok(r) => r,
            Err(e) => {
                let why = format!("the reviewer did not answer: {e:#}");
                self.stop("review", &why, Value::Null);
                return Ok(None);
            }
        };
        let v = if r.accept { "accept" } else { "reject" };
        self.verdict(said(v, planted.name(), r.reasons, r.cost_usd))?;
        Ok(Some(v.into()))
    }

    /// The planted revert on the remote's main; none when it stopped.
    async fn planted(&mut self) -> Option<Outcome> {
        if !self.phase("planted") {
            return None;
        }
        let (p, t, b, h, r) = (
            self.place.clone(),
            self.seams.tests.clone(),
            self.b.branch.clone(),
            self.b.head.clone(),
            remote_of(&self.core.cfg),
        );
        let planted = blocking(move || {
            git::fetch(&p, &r)?;
            let base = git::main_of(&p, &r)?;
            planted::run(&p, &*t, &b, &base, &h)
        })
        .await;
        match planted {
            Ok(o) => Some(o),
            Err(e) => {
                let why = format!("the planted revert could not run: {e:#}");
                self.stop("planted", &why, Value::Null);
                None
            }
        }
    }

    /// What the reviewer is given: the diff, the report, the evidence.
    async fn ask(&self, remote: &str, target: Target) -> Result<Ask> {
        let (p, r, h) = (self.place.clone(), remote.to_string(), self.b.head.clone());
        let diff = blocking(move || {
            let base = git::main_of(&p, &r)?;
            git::diff(&p, &base, &h, 3)
        })
        .await?;
        let report = self
            .b
            .report
            .as_deref()
            .and_then(|r| std::fs::read_to_string(r).ok())
            .unwrap_or_else(|| "(no frozen report)".into());
        let evidence = crate::task_graph::get(&self.core.store, &self.b.item)
            .ok()
            .flatten()
            .map(|t| {
                let mut e = format!("{}\n\n{}\n", t.title, t.objective);
                for ev in &t.evidence {
                    let note = ev.note.as_deref().map(|n| format!(": {n}"));
                    e.push_str(&format!("- {}{}\n", ev.identity, note.unwrap_or_default()));
                }
                e
            })
            .unwrap_or_else(|| format!("(item {} is not on the backlog)", self.b.item));
        Ok(Ask {
            branch: self.b.branch.clone(),
            head: self.b.head.clone(),
            item: self.b.item.clone(),
            diff,
            report,
            evidence,
            target,
            built_ms: self.b.at_ms,
        })
    }

    /// The item back on the backlog, `proposed` again with `note` among its
    /// evidence under `identity`: its record, for the caller's frame; none
    /// when the item is not on the backlog.
    pub(super) fn reopened(&self, identity: String, note: String) -> Result<Option<NewRecord>> {
        let Some(mut t) = crate::task_graph::get(&self.core.store, &self.b.item)? else {
            return Ok(None);
        };
        let now = theseus_protocol::now_unix_ms();
        t.state = TaskState::Proposed;
        t.session = None;
        t.version += 1;
        t.updated_at_ms = now;
        t.evidence.push(TaskEvidence {
            node: None,
            identity,
            note: Some(note),
            by: "self-join".into(),
            at_ms: now,
        });
        Ok(Some(crate::task_graph::record(&t)?))
    }

    /// The verdict's row; a reject puts the item back on the backlog in its
    /// frame.
    fn verdict(&mut self, v: Said) -> Result<()> {
        let reasons = v.reasons.clone();
        let fact = SelfReviewVerdict {
            item: self.b.item.clone(),
            branch: self.b.branch.clone(),
            head: self.b.head.clone(),
            reviewer: v.reviewer.clone(),
            model: v.model,
            verdict: v.verdict.into(),
            planted_revert: v.planted.into(),
            reasons: v.reasons,
            cost_usd: v.cost_usd,
        };
        let rec = self.core.rec(None);
        let mut frame = vec![rec.row(&fact)?];
        if v.verdict == "reject" {
            let identity = format!(
                "review:{}@{}",
                self.b.branch,
                &self.b.head[..self.b.head.len().min(12)]
            );
            let note: String = reasons.join("; ").chars().take(1_000).collect();
            frame.extend(self.reopened(identity, format!("rejected by {}: {note}", v.reviewer))?);
        }
        self.core.store.append(&frame)?;
        rec.announce(&fact);
        self.pass
            .reviewed
            .push((self.b.branch.clone(), v.verdict.into()));
        Ok(())
    }
}
