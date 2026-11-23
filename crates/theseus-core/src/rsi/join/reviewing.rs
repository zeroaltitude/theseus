//! The review's phases of one branch (theseus-pw1q.9): the independence
//! check, the planted revert, the reviewer, and the verdict's row.
//!
//! Independence is checked twice. Before the turn, on the model the review asks for, so a review that
//! could not be independent is not paid for. After it, on the model that answered (the turn's own
//! result): a turn is moved by routing, or by a refusal's fallback, and the model it asked for is
//! only a request. The verdict records the model that answered. A review that ran on the model that
//! built the branch, or did not say what ran, is `refused`: it never accepts a branch.

use anyhow::Result;
use serde_json::Value;
use theseus_protocol::tasks::{TaskEvidence, TaskState};
use theseus_store::NewRecord;

use super::planted::{self, Outcome};
use super::review::Ask;
use super::unreviewable::{self, HOLD_AFTER};
use super::{blocking, git, remote_of, scratch, Run, Said};
use crate::fact::rsi_join::SelfReviewVerdict;
use crate::job_model::How;
use crate::turn::Target;

/// Who would review, and whether that review would be independent. The fixer (`fix`) is resolved
/// by the same rule, on the same profile, and refused the same way.
pub(crate) struct Reviewer {
    pub(crate) target: Option<Target>,
    /// `fable (claude-fable-5-2)`.
    pub(crate) name: String,
    /// The model it asks to run on; empty when none resolved.
    pub(crate) model: String,
    /// The builder's model, which the model that answers must not be.
    pub(crate) builder: Option<String>,
    pub(crate) refusal: Option<String>,
}

/// A verdict's words as `who` is named: the reviewer, and the model it asked for.
fn said_of(
    who: &Reviewer,
    verdict: &'static str,
    planted: &'static str,
    reasons: Vec<String>,
    cost_usd: f64,
) -> Said {
    Said {
        reviewer: who.name.clone(),
        model: who.model.clone(),
        asked: who.model.clone(),
        verdict,
        planted,
        checked: None,
        reasons,
        cost_usd,
    }
}

/// A pin is held where it names: the turn is the owner's choice, so routing never moves it
/// (`Target::chosen`, as a message sent with `-P` is). Jev's pick is not held.
fn held(mut t: Target, how: How) -> Target {
    if matches!(how, How::Pinned) {
        t.chosen = Some(format!("profile {}", t.profile));
    }
    t
}

/// Why a review that ran on `ran` does not count against a branch built on `builder`; none when it
/// is another model. A model that is not known cannot be shown to differ.
pub(crate) fn not_independent(builder: &str, ran: &str) -> Option<String> {
    if ran.is_empty() {
        return Some(
            "the review did not say which model answered, so it cannot be shown independent of \
             the model that built the branch"
                .into(),
        );
    }
    crate::catalog::same_model(builder, ran).then(|| {
        format!(
            "the review ran on {ran}, the model that built the branch: it does not count, and an \
             independent review is on another model, so set [self] reviewer_profile to one"
        )
    })
}

/// The model the session `builder` last ran on: the model that built a branch.
pub(crate) fn builder_model(core: &crate::rpc::Core, builder: Option<&str>) -> Option<String> {
    builder_target(core, builder).map(|t| t.model)
}

/// The profile and the model the session `builder` last ran on.
pub(crate) fn builder_target(
    core: &crate::rpc::Core,
    builder: Option<&str>,
) -> Option<crate::session::TargetRef> {
    builder
        .and_then(|s| {
            core.store
                .get_session::<crate::session::SessionRecord>(s)
                .ok()
                .flatten()
        })
        .and_then(|r| r.last_target)
}

impl Run<'_> {
    /// [`reviewer_for`] this branch.
    async fn reviewer(&self) -> Reviewer {
        reviewer_for(self.core, self.b.session.as_deref(), &self.b.why).await
    }
}

/// The reviewer's target against the builder's model: `[self]
/// reviewer_profile` when it is set (a pin), else Jev's choice for the
/// review, told the builder's model (route.v10, theseus-ej6bj). Review
/// independence holds either way: a reviewer on the builder's model is
/// refused, as it always was, and the rule is not a cap. `builder` is the
/// session that built the branch, `why` the change's own words.
pub(crate) async fn reviewer_for(
    core: &std::sync::Arc<crate::rpc::Core>,
    builder: Option<&str>,
    why: &str,
) -> Reviewer {
    let builder = builder_model(core, builder);
    let live = core.live_profile().0;
    let site = crate::job_model::Site {
        name: "self reviewer",
        pin: core.cfg.self_improve.reviewer_profile.as_deref(),
        job: theseus_judge::builders::JobKind::SelfReviewer {
            diff_summary: why.to_string(),
            builder_model: builder.clone(),
            // The change is the assistant's own code, written from the
            // owner's sessions: owner material, US options only.
            private: true,
        },
    };
    let target = core
        .runner
        .job_target(&live, site)
        .await
        .map(|(t, how)| held(t, how));
    let profile = match &target {
        Ok(t) => t.profile.clone(),
        Err(e) => e.profile.clone(),
    };
    let refusal = match (&target, &builder) {
        (Err(e), _) => Some(format!("[self] reviewer_profile {e}")),
        (_, None) => Some(
            "the builder's model is not known, so no review can be shown independent of it".into(),
        ),
        (Ok(t), Some(m)) if crate::catalog::same_model(m, &t.model) => Some(format!(
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
        builder,
        target,
        refusal,
    }
}

impl Run<'_> {
    /// The review: the verdict written, or none when a halt stopped it.
    pub(super) async fn review(&mut self) -> Result<Option<String>> {
        let who = self.reviewer().await;
        let said = |verdict, planted, reasons, cost_usd| {
            said_of(&who, verdict, planted, reasons, cost_usd)
        };
        if let Some(v) = self.ended_early(&who).await? {
            return Ok(Some(v.into()));
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
        // The reviewer's own place to write, and what it leaves is removed whatever it says:
        // a tree it left under the self trees held the pipeline's slot for good (`scratch`).
        let (p, b) = (self.place.clone(), self.b.branch.clone());
        let own = blocking(move || Ok(scratch::open(&p, &b))).await?;
        let place = self.place.clone();
        let closed = |own: scratch::Scratch| {
            let p = place.clone();
            blocking(move || Ok(scratch::close(&p, &own)))
        };
        let ask = match self.ask(&remote, target, &own.dir).await {
            Ok(a) => Ask {
                proof: planted.ask_text(&a.scratch),
                ..a
            },
            Err(e) => {
                let _ = closed(own).await;
                return Err(e);
            }
        };
        let core = self.core.clone();
        let reviewed = self.seams.reviewer.review(&core, &ask).await;
        let left = closed(own).await.unwrap_or_default();
        if !left.is_empty() {
            tracing::warn!(
                branch = %self.b.branch,
                trees = ?left,
                "self-review: the reviewer left a tree under the self trees; it was removed, so it holds no slot"
            );
        }
        let r = match reviewed {
            Ok(r) => r,
            Err(e) => {
                let why = format!("the reviewer did not answer: {e:#}");
                self.stopped_review(&why, unreviewable::Cause::of(&e));
                return Ok(None);
            }
        };
        // What the turn says answered, not what it was asked for.
        let ran = Said {
            reviewer: match r.profile.as_str() {
                "" => r.model.clone(),
                p => format!("{p} ({})", r.model),
            },
            model: r.model.clone(),
            ..said("", planted.name(), vec![], r.cost_usd)
        };
        if !r.model.is_empty() && r.model != who.model {
            tracing::warn!(
                branch = %self.b.branch,
                asked = %who.model,
                ran = %r.model,
                "self-review: the review ran on another model than it asked for"
            );
        }
        let builder = who.builder.as_deref().unwrap_or_default();
        if let Some(why) = not_independent(builder, &r.model) {
            self.verdict(Said {
                verdict: "refused",
                reasons: vec![why],
                ..ran
            })?;
            return Ok(Some("refused".into()));
        }
        let (v, reasons, checked) = self.judged(&planted, &r);
        self.verdict(Said {
            verdict: v,
            reasons,
            checked,
            ..ran
        })?;
        Ok(Some(v.into()))
    }

    /// A review that ends before its planted revert runs, its verdict written: no independent
    /// reviewer could run (`refused`), or, with a reviewer, a head that holds a fixer's commit no
    /// other model approved or was not built by a builder (`not_a_builders`, issue 36, round 2),
    /// which every head that reaches a review is checked for, whatever path led to it.
    async fn ended_early(&mut self, who: &Reviewer) -> Result<Option<&'static str>> {
        if let Some(why) = who.refusal.clone() {
            self.verdict(said_of(who, "refused", "not-run", vec![why], 0.0))?;
            return Ok(Some("refused"));
        }
        if who.target.is_none() {
            return Ok(None);
        }
        if let Some(why) = self.not_a_builders().await {
            self.verdict(said_of(who, "reject", "not-run", vec![why], 0.0))?;
            return Ok(Some("reject"));
        }
        Ok(None)
    }

    /// The verdict a reply comes to: the proof step's reading of it (`proof_check::judge`), which
    /// may make an accept a reject, and of a reviewer's reject, whether it is the fix it may ask
    /// for (`word`, issue 36).
    fn judged(
        &self,
        planted: &Outcome,
        r: &super::review::Review,
    ) -> (&'static str, Vec<String>, Option<&'static str>) {
        let (v, reasons, checked) = super::proof_check::judge(planted, r.accept, r.reasons.clone());
        if r.accept {
            return (v, reasons, checked);
        }
        let (v, reasons) = self.word(r);
        (v, reasons, checked)
    }

    /// The verdict's word (issue 36): a `fix` is one only while `[self_fix]` is on and this head
    /// was not fixed at review already; a second `fix` is a reject, and says so.
    fn word(&self, r: &super::review::Review) -> (&'static str, Vec<String>) {
        match (r.accept, r.fix) {
            (true, _) => ("accept", r.reasons.clone()),
            (false, true) if self.b.fixed.is_some() => {
                let mut why = vec![
                    "it asked for a second fix at review, which is a reject: a branch is fixed at \
                     review once"
                        .to_string(),
                ];
                why.extend(r.reasons.iter().cloned());
                ("reject", why)
            }
            (false, true) if self.core.cfg.self_fix.on() => ("fix", r.reasons.clone()),
            _ => ("reject", r.reasons.clone()),
        }
    }

    /// A review whose turn failed. A failure that will not clear by itself, the same one for the
    /// [`HOLD_AFTER`]th time in a row, holds the change for the owner with one card; any other is
    /// a stop the next pass tries again, after a wait that grows while the reason is transient
    /// (`unreviewable`).
    fn stopped_review(&mut self, why: &str, cause: Option<unreviewable::Cause>) {
        let Some(cause) = cause else {
            return self.stop("review", why, Value::Null);
        };
        let repeats = unreviewable::run_with(&self.earlier, "review", &cause);
        let numbers = cause.numbers(repeats);
        if cause.transient || repeats < HOLD_AFTER {
            return self.stop("review", why, numbers);
        }
        self.card(unreviewable::card(
            &self.b.branch,
            &self.b.item,
            &cause.class,
            repeats,
            why,
        ));
        self.hold(
            "review",
            &format!(
                "the review failed the same way {repeats} times ({}), so it waits on the owner's card",
                cause.class
            ),
            numbers,
        );
    }

    /// Why this head may not be reviewed: a fixer's commit in it that no other model approved
    /// (`fix::unapproved`), or a build by a session the loop opened for another job
    /// (`rsi::roles`). None when it may.
    async fn not_a_builders(&self) -> Option<String> {
        if let Some(role) = self
            .b
            .session
            .as_deref()
            .and_then(|s| self.core.tools.self_pr.role(s))
        {
            return Some(format!(
                "it was built by a session the self loop opened as {}, not by a builder",
                role.words()
            ));
        }
        let (s, p, r, h) = (
            self.core.store.clone(),
            self.place.clone(),
            remote_of(&self.core.cfg),
            self.b.head.clone(),
        );
        let found = blocking(move || {
            let main = git::main_of(&p, &r)?;
            super::fix::unapproved(&s, &p, &p.repo, &format!("{main}..{h}"))
        })
        .await;
        match found {
            Ok(w) => w,
            Err(e) => Some(format!(
                "whether the branch holds an unapproved fix could not be read: {e:#}"
            )),
        }
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
                self.stopped_planted(&why, planted::Undecided::of(&e));
                None
            }
        }
    }

    /// What the reviewer is given: the diff, the report, the evidence.
    async fn ask(&self, remote: &str, target: Target, scratch: &std::path::Path) -> Result<Ask> {
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
            proof: String::new(),
            target,
            built_ms: self.b.at_ms,
            scratch: scratch.display().to_string(),
            repo: self.place.repo.display().to_string(),
            offer_fix: (self.core.cfg.self_fix.on() && self.b.fixed.is_none())
                .then_some((self.core.cfg.self_fix.lines, self.core.cfg.self_fix.files)),
            fixed: self.b.fixed.clone(),
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
        if v.verdict == "fix" {
            self.asked.clone_from(&reasons);
        }
        let fact = SelfReviewVerdict {
            item: self.b.item.clone(),
            branch: self.b.branch.clone(),
            head: self.b.head.clone(),
            reviewer: v.reviewer.clone(),
            model: v.model,
            asked: v.asked,
            verdict: v.verdict.into(),
            planted_revert: v.planted.into(),
            proof_check: v.checked.map(Into::into),
            reasons: v.reasons,
            cost_usd: v.cost_usd,
        };
        let rec = self.core.rec(None);
        let _one = self
            .core
            .rsi
            .backlog
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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

#[cfg(test)]
mod tests {
    use super::not_independent;

    #[test]
    fn a_review_is_independent_only_of_another_model() {
        let builder = "claude-sonnet-5-5";
        assert_eq!(not_independent(builder, "claude-opus-5-5"), None);
        assert_eq!(not_independent(builder, "claude-sonnet-5-50"), None);
        assert!(not_independent(builder, builder).is_some());
        assert!(not_independent(builder, "claude-sonnet-5-5-20261101").is_some());
        assert!(
            not_independent(builder, "").is_some(),
            "unknown is not independent"
        );
    }
}
