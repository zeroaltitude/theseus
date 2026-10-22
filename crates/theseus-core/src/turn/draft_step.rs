//! The draft step (theseus-a1wvq.13; `draft.rs` holds the rest): after the
//! first loop's route decision, a turn whose class has `cn_draft` drafts only
//! when all of these hold, each failure its own `draft_skip` word on the
//! route row, the first that fails:
//!
//! - `not_admitted`: the input is open and admitted (`RouteState.admitted`);
//! - `in_task`: the session is no task;
//! - `wake`: the turn is a person's message, not a wake's, a report's or a
//!   continuation;
//! - `dir_not_open`: the session's directory, and its repository's top (the draft
//!   tree is the whole repository), are under the open roots;
//! - `not_git`: it is a git work tree (and its worktree could be made);
//! - `draft_unusable`: the `cn_draft` profile is usable for a cleared ask;
//! - `no_reviewer`: a `final` profile is usable.
//!
//! A drafting turn makes no model call: its first compile is dropped, the
//! draft opens in the turn's own frame (`Kernel::open_task_at`, the carve,
//! the depth rule and the rows), and the turn ends with one line, posted as a
//! notice, as a retry that found nothing to do posts its line
//! (`noop_step.rs`). A class without `cn_draft` pays one branch.

use super::*;
use crate::data::OpenHow;
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
    /// Whether `profile` is usable now, for a cleared ask or not.
    pub(crate) fn profile_usable(&self, profile: &str, cleared: bool) -> bool {
        let floor = super::data_step::Floor {
            sensitive: false,
            cleared,
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
        match self
            .draft_ready(t, session, &name, &table.final_review)
            .await
        {
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
        reviewers: &[String],
    ) -> Result<Drafting, &'static str> {
        let Ok(run) = &t.route.admitted.0 else {
            return Err("not_admitted");
        };
        if t.tc.task.is_some() || session.task.is_some() {
            return Err("in_task");
        }
        if t.continuation || !t.wakes.is_empty() || !t.woke_by.is_empty() {
            return Err("wake");
        }
        let dir = session
            .dir
            .as_deref()
            .map(PathBuf::from)
            .ok_or("dir_not_open")?;
        let roots = crate::open_view::open_roots(&self.cfg, session);
        let canonical = theseus_tools::paths::canonical_best_effort(&dir);
        if !roots
            .iter()
            .any(|r| theseus_tools::paths::within(&canonical, r))
        {
            return Err("dir_not_open");
        }
        let at = canonical.clone();
        let top = tokio::task::spawn_blocking(move || draft::work_tree(&at))
            .await
            .ok()
            .flatten()
            .ok_or("not_git")?;
        // The tree is the whole repository: all of it must be open.
        if !roots.iter().any(|r| theseus_tools::paths::within(&top, r)) {
            return Err("dir_not_open");
        }
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
        let asks = Self::person_messages(t, run).map_err(|_| "not_admitted")?;
        let correlation_id = theseus_kernel::new_id("act");
        let (_, task_session) = theseus_kernel::task_ids(&correlation_id);
        let tree = draft::tree_of(&self.cfg, &task_session);
        let branch = draft::branch_of(&task_session);
        let (at, into, b) = (top.clone(), tree.clone(), branch.clone());
        let base = tokio::task::spawn_blocking(move || draft::open_tree(&at, &into, &b)).await;
        let base = match base {
            Ok(Ok(base)) => base,
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
                ..Draft::default()
            },
            correlation_id,
            profile,
            reviewer,
        })
    }

    /// The open run's person messages, oldest first, verbatim.
    fn person_messages(t: &Turn<'_>, run: &crate::data::OpenRun) -> Result<Vec<String>> {
        let nodes = t.tc.store.transcript(t.tc.session_id)?;
        let from = nodes.iter().position(|(_, n)| n.id == run.from);
        let through = nodes.iter().position(|(_, n)| n.id == run.through);
        let (Some(from), Some(through)) = (from, through) else {
            anyhow::bail!("the open run is not in the transcript");
        };
        Ok(nodes[from..=through]
            .iter()
            .filter(|(_, n)| n.kind == crate::stub::Kind::UserMessage)
            .filter_map(|(_, n)| match &n.body {
                Body::UserMessage { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect())
    }

    /// Open the draft in the turn's frame and end the turn with its line; a
    /// draft that cannot open runs the turn as before, its tree removed.
    pub(super) fn draft_open(&self, t: &mut Turn<'_>, d: Option<Drafting>) -> bool {
        let Some(d) = d else {
            return false;
        };
        match Self::open_draft(t, &d) {
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
                let (dir, tree) = (d.draft.dir.clone(), d.draft.tree.clone());
                tokio::task::spawn_blocking(move || {
                    draft::close_tree(Path::new(&dir), Path::new(&tree))
                });
                false
            }
        }
    }

    fn open_draft(t: &Turn<'_>, d: &Drafting) -> Result<()> {
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
        };
        let hold = session_hold(tc)?;
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
                let opened = crate::data::open_task(&mut rec, OpenHow::Draft, tc.kernel.now_ms())?;
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
                records.push(opened);
                records.push(brief.record()?);
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
    /// its review. None for every other session.
    pub(super) fn draft_stopped(t: &Turn<'_>, why: &str) -> Option<Failure> {
        t.tc.open.filter(|o| o.draft)?;
        let class = match t.route.admitted.0 {
            Ok(_) => draft::DRAFT_STOPPED,
            Err(_) => draft::DRAFT_NOT_ADMITTED,
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

/// The parent's external-text hold, which an open task takes bare.
fn session_hold(tc: &TurnCtx<'_>) -> Result<Option<theseus_protocol::ExternalText>> {
    Ok(tc
        .store
        .get_session::<SessionRecord>(tc.session_id)?
        .and_then(|r| r.external))
}
