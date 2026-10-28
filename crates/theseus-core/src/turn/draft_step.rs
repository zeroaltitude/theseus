//! The draft step (theseus-a1wvq.13; `draft.rs` holds the rest): after the
//! first loop's route decision, a turn whose class has `cn_draft` drafts only
//! when all of these hold, each failure its own `draft_skip` word on the
//! route row, the first that fails:
//!
//! - `labelled`: no label keeps the session off a model of restricted origin
//!   (`RouteState.cn`: allowed, or its items not yet judged; the draft's own
//!   brief is judged on its own session);
//! - `in_task`: the session is no task;
//! - `wake`: the turn is a person's message, not a wake's, a report's or a
//!   continuation;
//! - `not_git`: the session has a directory, and it is a git work tree (and
//!   its worktree could be made);
//! - `draft_unusable`: the `cn_draft` profile is usable for a session whose
//!   labels allow it;
//! - `no_reviewer`: a `final` profile is usable.
//!
//! With `[routing] draft_by_jev` on, Jev's `draft` answer comes before all of
//! these (theseus-kqeih): `jev_unasked`, `jev_declined` or `jev_unsure` keep the
//! turn off a draft before a tree is opened, a sure `cheap` waives none of them,
//! and `draft_choice` on the route row says what Jev answered.
//!
//! A drafting turn makes no model call: its first compile is dropped, the
//! draft opens in the turn's own frame (`Kernel::open_task_at`, the carve,
//! the depth rule and the rows), and the turn ends with one line, posted as a
//! notice, as a retry that found nothing to do posts its line
//! (`noop_step.rs`). A class without `cn_draft` pays one branch.

use super::*;
use crate::draft::{self, Draft};
use crate::routing::Verdict;
use std::path::{Path, PathBuf};

/// What a drafting turn has made ready before its route row.
pub(super) struct Drafting {
    draft: Draft,
    correlation_id: String,
    profile: TargetRef,
    reviewer: String,
}

impl TurnRunner {
    /// Whether `profile` is usable now, for a session whose labels allow a
    /// model of restricted origin or not.
    pub(crate) fn profile_usable(&self, profile: &str, cn_ok: bool) -> bool {
        let floor = super::data_step::Floor {
            sensitive: false,
            cn_ok,
        };
        self.route_profiles(floor)
            .get(profile)
            .is_some_and(|p| p.unusable.is_none())
    }

    /// The draft rule for the verdict the turn routes by: none for a class
    /// without `cn_draft`; else the draft made ready (its tree), or the first
    /// word that kept it from drafting, kept for the route row.
    pub(super) async fn draft_check(
        &self,
        t: &mut Turn<'_>,
        session: &SessionRecord,
        verdict: Option<&Verdict>,
    ) -> Option<Drafting> {
        let table = self.cfg.routing.classes.table(&verdict?.mode);
        let name = table.cn_draft.clone()?;
        if self.cfg.routing.draft_by_jev {
            // Jev's say comes before a tree is opened: declining costs nothing,
            // opening one does (theseus-kqeih). The labels and the rest of the
            // rule still come after it, so a `cheap` answer waives none of them.
            let answer = verdict?.draft.as_deref();
            t.route.draft_choice = answer.map(crate::routing::draft_choice::DraftAnswer::word);
            if let Err(word) = crate::routing::draft_choice::allows(answer) {
                t.route.draft_skip = Some(word);
                return None;
            }
        }
        match self.draft_ready(t, session, &name, table).await {
            Ok(d) => Some(d),
            Err(word) => {
                t.route.draft_skip = Some(word);
                None
            }
        }
    }

    async fn draft_ready(
        &self,
        t: &Turn<'_>,
        session: &SessionRecord,
        name: &str,
        table: &crate::config::routing::ClassProfiles,
    ) -> Result<Drafting, &'static str> {
        let reviewers = &table.final_review;
        if let super::data_step::Cn::No(word) = t.route.cn {
            if crate::content::Flag::parse(word).is_some() {
                return Err("labelled");
            }
        }
        if t.tc.task.is_some() || session.task.is_some() {
            return Err("in_task");
        }
        if t.continuation || !t.wakes.is_empty() || !t.woke_by.is_empty() {
            return Err("wake");
        }
        let dir = session.dir.as_deref().map(PathBuf::from).ok_or("not_git")?;
        let at = theseus_tools::paths::canonical_best_effort(&dir);
        let top = tokio::task::spawn_blocking(move || draft::work_tree(&at))
            .await
            .ok()
            .flatten()
            .ok_or("not_git")?;
        let profile = self
            .tools
            .profiles
            .get(name)
            .filter(|_| self.profile_usable(name, true) && !t.route.sensitive)
            .cloned()
            .ok_or("draft_unusable")?;
        let reviewer = reviewers
            .iter()
            .find(|p| self.profile_usable(p, false))
            .ok_or("no_reviewer")?
            .clone();
        let asks = Self::person_messages(t).map_err(|_| "wake")?;
        let correlation_id = theseus_kernel::new_id("act");
        let (_, task_session) = theseus_kernel::task_ids(&correlation_id);
        let tree = draft::tree_of(&self.cfg, &task_session);
        let branch = draft::branch_of(&task_session);
        let (at, into, b) = (top.clone(), tree.clone(), branch.clone());
        let base = tokio::task::spawn_blocking(move || draft::open_tree(&at, &into, &b)).await;
        let (base, git_dir) = match base {
            Ok(Ok(opened)) => opened,
            Ok(Err(e)) => {
                tracing::warn!(error = %format!("{e:#}"), dir = %top.display(), "a draft's tree could not be made");
                return Err("not_git");
            }
            Err(_) => return Err("not_git"),
        };
        let target = self
            .outbox
            .report_target(self.outbox.target(t.tc.session_id))
            .0;
        Ok(Drafting {
            draft: Draft {
                parent_session: t.tc.session_id.into(),
                parent_execution: t.tc.execution_id.into(),
                dir: top.to_string_lossy().into_owned(),
                tree: tree.to_string_lossy().into_owned(),
                branch,
                base,
                profile: name.into(),
                model: profile.model.clone(),
                reviewers: reviewers.to_vec(),
                asks,
                target,
                git_dir,
                first_pass: table.first_pass.as_ref().and_then(|p| {
                    Some(draft::first_pass::FirstPass {
                        profile: p.clone(),
                        model: self.tools.profiles.get(p)?.model.clone(),
                        ..Default::default()
                    })
                }),
                ..Draft::default()
            },
            correlation_id,
            profile,
            reviewer,
        })
    }

    /// The turn's own person message, verbatim: the draft's brief is the ask
    /// of this turn alone, since no run of earlier messages was judged open.
    fn person_messages(t: &Turn<'_>) -> Result<Vec<String>> {
        let nodes = t.tc.store.transcript(t.tc.session_id)?;
        let turn = t.tc.turn_id;
        let ask = nodes.iter().rev().find_map(|(_, n)| match &n.body {
            Body::UserMessage { text, .. }
                if n.kind == crate::stub::Kind::UserMessage
                    && n.turn_id.as_deref() == Some(turn) =>
            {
                Some(text.clone())
            }
            _ => None,
        });
        ask.map(|a| vec![a])
            .ok_or_else(|| anyhow::anyhow!("the turn has no message of its own"))
    }

    /// Open the draft in the turn's frame and end the turn with its line; a
    /// draft that cannot open runs the turn as before, its tree removed.
    pub(super) fn draft_open(&self, t: &mut Turn<'_>, d: Option<Drafting>) -> bool {
        let Some(d) = d else {
            return false;
        };
        match self.open_draft(t, &d) {
            Ok(()) => {
                let line = format!(
                    "Drafting on {} ({}); {} ({}) reviews and finishes it.",
                    d.draft.profile,
                    d.draft.model,
                    d.reviewer,
                    self.tools
                        .profiles
                        .get(&d.reviewer)
                        .map_or("", |p| p.model.as_str())
                );
                t.output.clone_from(&line);
                t.stop_reason = "drafted".into();
                t.route.drafted = Some(line);
                true
            }
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), "a draft did not open; the turn runs as before");
                let (dir, tree, branch) = (
                    d.draft.dir.clone(),
                    d.draft.tree.clone(),
                    d.draft.branch.clone(),
                );
                tokio::task::spawn_blocking(move || {
                    draft::drop_draft(Path::new(&dir), Path::new(&tree), &branch)
                });
                false
            }
        }
    }

    fn open_draft(&self, t: &Turn<'_>, d: &Drafting) -> Result<()> {
        let tc = &t.tc;
        let parent = tc
            .kernel
            .execution(tc.execution_id)?
            .ok_or_else(|| anyhow::anyhow!("the conversation has no execution"))?;
        let pool = match tc.kernel.overdraws(&parent) {
            true => parent.budget.available().max(parent.budget.limit_micros),
            false => parent.budget.available(),
        };
        let want = pool / crate::task::DEFAULT_SHARE.1 * crate::task::DEFAULT_SHARE.0;
        let how = theseus_kernel::Opening {
            reports_to: d.draft.target.clone(),
            wake_parent: false,
            start_at_ms: None,
            helper: None,
        };
        let hold = session_hold(tc)?;
        // The brief is an item like any other, and the draft's session is
        // judged by it (theseus-9ymz0). It is kept out here so it can be queued
        // after its frame is written: without that the draft's own session is
        // never judged, so its first request finds an unjudged item, waits,
        // times out and ends `draft_not_admitted` — no draft would ever run.
        let briefed: std::sync::Mutex<Option<Node>> = std::sync::Mutex::new(None);
        let opened = tc
            .kernel
            .open_task_at(tc.guard, &d.correlation_id, want, how, |task| {
                let sid = &task.session_id;
                let mut rec = SessionRecord::with_id(
                    sid.clone(),
                    theseus_kernel::SessionKind::Task,
                    Some("task".into()),
                );
                rec.title = Some(format!(
                    "Draft: {}",
                    crate::session::title_from(&d.draft.asks.join(" "))
                ));
                rec.execution_id = Some(task.id.clone());
                rec.dir = Some(d.draft.tree.clone());
                rec.last_target = Some(d.profile.clone());
                rec.task = Some(crate::session::TaskOf {
                    parent_session: tc.session_id.into(),
                    parent_execution: tc.execution_id.into(),
                    by: d.correlation_id.clone(),
                    target: d.draft.target.clone(),
                    arrangement: None,
                    check: None,
                    notify_done: false,
                });
                let author = format!("session:{}", tc.session_id);
                let text = draft::brief(draft::DRAFT_HEADER, &d.draft.asks);
                let brief = Node::relayed(sid, None, crate::node::Origin::Agent, &author, &text);
                let mut records = match &hold {
                    Some(h) => {
                        let taken = crate::external::taken(
                            h,
                            tc.session_id,
                            crate::external::VIA_TASK,
                            &brief.id,
                            theseus_protocol::now_unix_ms(),
                        );
                        crate::external::hold(rec, crate::external::bare(taken), None)?
                            .unwrap_or_default()
                    }
                    None => vec![NewRecord::json(
                        theseus_store::kinds::SESSION,
                        Some(sid),
                        &rec,
                    )?],
                };
                records.push(brief.record()?);
                *briefed
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(brief);
                // A draft of a private session is private from its first call
                // (theseus-b0ozi, LOW-5).
                records.extend(crate::google::private::inherited(
                    tc.store,
                    &self.cfg,
                    tc.session_id,
                    sid,
                )?);
                if let Some(target) = &d.draft.target {
                    records.push(tc.outbox.task_record(sid, target)?);
                }
                records.push(NewRecord::json(
                    theseus_store::kinds::META,
                    Some(&draft::meta_key(sid)),
                    &d.draft,
                )?);
                Ok(records)
            })?;
        if let Some(target) = &d.draft.target {
            tc.outbox.task_bound(&opened.task.session_id, target);
        }
        let briefed = briefed
            .into_inner()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(node) = briefed {
            self.content_message(
                &opened.task.session_id,
                &node,
                ItemKind::TaskBrief,
                "draft brief",
            );
        }
        Ok(())
    }

    /// A drafting turn makes no model call: its loop is not counted.
    pub(super) fn drafted(t: &mut Turn<'_>) -> bool {
        let drafted = t.route.drafted.is_some();
        if drafted {
            t.loops = t.loops.saturating_sub(1);
        }
        drafted
    }

    /// A draft never moves to the `[model]` default: where the floor would
    /// move it, its turn ends instead, with no model call, and its end opens
    /// its review. `first` is the first loop's floor: a draft that never got
    /// to use its model (its brief was labelled, or not judged in time) is
    /// `draft_not_admitted`; one a later label stopped is `draft_stopped`.
    /// None for every other session.
    pub(super) fn draft_stopped(t: &Turn<'_>, why: &str, first: bool) -> Option<Failure> {
        t.tc.draft?;
        let class = match first {
            true => draft::DRAFT_NOT_ADMITTED,
            false => draft::DRAFT_STOPPED,
        };
        let said = format!("the draft ends here rather than leave its model: {why}");
        Some(Failure {
            class: class.into(),
            transient: false,
            usage_unknown: false,
            reason: format!("{class}: {said}"),
            source: anyhow::anyhow!("{said}"),
        })
    }

    /// The drafting turn's one line, where its reply would have gone.
    pub(super) fn stage_drafted(&self, t: &Turn<'_>, line: &str) -> Result<()> {
        let Some(target) = t
            .answer_at
            .clone()
            .or_else(|| self.outbox.target(t.tc.session_id))
        else {
            return Ok(());
        };
        let (post, records) = self.outbox.stage(
            t.tc.session_id,
            t.tc.execution_id,
            &target,
            json!({"kind": "notice", "text": line}),
        )?;
        for r in records {
            t.tc.store.defer(r)?;
        }
        t.tc.posts.lock().unwrap().push(post);
        Ok(())
    }
}

/// The parent's external-text hold, which a draft takes bare.
fn session_hold(tc: &TurnCtx<'_>) -> Result<Option<theseus_protocol::ExternalText>> {
    Ok(tc
        .store
        .get_session::<SessionRecord>(tc.session_id)?
        .and_then(|r| r.external))
}
