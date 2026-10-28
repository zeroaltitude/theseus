//! A first-pass bug review before Claude's (theseus-a1wvq.17; the owner's
//! answer R7, 2026-10-17). With `first_pass = "<profile>"` beside `cn_draft`,
//! a draft that completed and changed something is read once more by a
//! restricted-origin model, on the draft's tree only, before the Claude review
//! opens:
//!
//! - **The task.** After the draft's tree is committed, the core opens a
//!   second task of the session on the `first_pass` profile, in a worktree of
//!   its own at the draft commit (its tree alone, file tools only, no
//!   programs, as a draft: it is a draft session, marked by the META record
//!   `draftfp.<session>` naming the draft it reads, `draft::is_draft`).
//!   Its brief is `FIRST_PASS_HEADER`, the draft's person messages and the
//!   draft's diff. Its report is never posted.
//! - **The findings go to Claude, and nowhere else.** When the first pass
//!   ends, the Claude review opens with its report in the brief, under
//!   "A first-pass reviewer's findings (check each; it may be wrong):". The
//!   reviewer adjudicates; the first pass is never the only or the final
//!   reviewer, and its words reach the owner only as the reviewer repeats them.
//! - **A first pass that fails, stops, or runs past `BOUND`** does not hold the
//!   review: the review opens at once without it, and its brief says in one
//!   line that a first pass gave no findings. The bound is a timer armed when
//!   the pass opens (a `deadline_ms` in the draft's record, so the start sweep
//!   arms it again or cancels the pass that is past it); where it fires the
//!   pass is cancelled, and its cancel is its end like any other.
//! - **The measure.** Its task, profile, model, cost and outcome are on the
//!   `task.draft` row (`first_pass_*`).

use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use theseus_kernel::{micros_to_usd, Opening, SessionKind};
use theseus_store::{kinds, NewRecord};

use super::{outcome_of, Draft};
use crate::node::{Node, Origin};
use crate::session::{title_from, SessionRecord, TaskOf};
use crate::turn::TurnRunner;

/// The first pass's brief, before the open run's person messages and the diff.
pub const FIRST_PASS_HEADER: &str = "Review this diff for bugs; list each finding with file and \
line; do not edit. You can read and search the tree; you cannot run programs. A Claude reviewer \
will check each finding, so say what you are unsure of.";

/// How long a first pass may run before the review stops waiting for it.
pub const BOUND: Duration = Duration::from_secs(10 * 60);

/// The diff and the findings are cut at these many characters.
const DIFF_MAX: usize = 60_000;
const FINDINGS_MAX: usize = 8_000;

/// The first pass of a draft, kept in the draft's record: its profile from the
/// config, then what became of it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FirstPass {
    pub profile: String,
    pub model: String,
    /// Its session, once opened.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    /// When the review stops waiting for it.
    #[serde(default)]
    pub deadline_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<String>,
    #[serde(default)]
    pub cost_usd: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub findings: Option<String>,
}

/// The META record of a first pass's session (`draftfp.<session>`): the draft
/// it reads and the tree it works in.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Mark {
    pub draft: String,
    pub tree: String,
}

fn mark_key(first_pass_session: &str) -> String {
    format!("draftfp.{first_pass_session}")
}

/// The mark of `session`, when it is a first pass.
pub(crate) fn mark_of(store: &crate::store::Store, session: &str) -> Option<Mark> {
    store.get_meta(&mark_key(session)).ok().flatten()
}

/// The first pass was opened: the draft's end waits for its own.
pub(super) fn started(d: &Draft) -> bool {
    d.first_pass.as_ref().is_some_and(|f| f.task.is_some())
}

pub(crate) fn correlation(draft_session: &str) -> String {
    let tail = draft_session.strip_prefix("ses_").unwrap_or(draft_session);
    format!("act_{tail}f")
}

/// The line the review's brief carries about the first pass: its findings, or
/// that it gave none. Nothing when the draft had no first pass.
pub(super) fn note(fp: Option<&FirstPass>) -> String {
    let Some(f) = fp.filter(|f| f.task.is_some()) else {
        return String::new();
    };
    match f.findings.as_deref().filter(|t| !t.trim().is_empty()) {
        Some(t) => {
            format!("\n\nA first-pass reviewer's findings (check each; it may be wrong):\n{t}")
        }
        None => format!(
            "\n\nA first-pass reviewer was asked and gave no findings ({}).",
            f.outcome.as_deref().unwrap_or("it did not finish")
        ),
    }
}

/// The first pass's half of a `task.draft` row.
pub(super) fn passed(d: &Draft) -> Option<crate::fact::draft::Passed<'_>> {
    let f = d.first_pass.as_ref()?;
    Some(crate::fact::draft::Passed {
        task: f.task.as_deref()?,
        profile: &f.profile,
        model: &f.model,
        cost_usd: f.cost_usd,
        outcome: f.outcome.as_deref().unwrap_or("unfinished"),
    })
}

fn cut(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((i, _)) => format!("{}\n[cut at {max} characters]", &text[..i]),
        None => text.to_string(),
    }
}

impl TurnRunner {
    /// Open the first pass of the draft `sid` when it has one and the draft
    /// completed with a change: true when it is open, so the review waits.
    /// Any other draft drops the first pass from its record.
    pub(super) async fn start_first_pass(
        &self,
        sid: &str,
        d: &mut Draft,
        completed: bool,
    ) -> Result<bool> {
        let Some(fp) = d.first_pass.clone() else {
            return Ok(false);
        };
        let changed = d.draft_commit.as_deref().is_some_and(|c| c != d.base);
        let parent_open = self
            .kernel
            .execution(&d.parent_execution)?
            .is_some_and(|p| !p.state.is_terminal());
        let profile = self
            .tools
            .profiles
            .get(&fp.profile)
            .filter(|_| self.profile_usable(&fp.profile, true))
            .cloned();
        let (true, true, true, Some(profile)) = (completed, changed, parent_open, profile) else {
            d.first_pass = None;
            return Ok(false);
        };
        // A failure once the tree is made would leave it on disk, and every
        // later end of the draft fail at `worktree add`, the review never
        // opening: the review opens without a first pass instead, and the
        // tree goes (best effort).
        match self.open_first_pass(sid, d, fp, profile).await {
            Ok(opened) => {
                if opened {
                    self.arm_first_pass(sid, BOUND);
                }
                Ok(true)
            }
            Err(e) => {
                self.first_pass_not_opened(sid, d, &e).await;
                Ok(false)
            }
        }
    }

    /// The first pass's task, on a tree of its own at the draft's commit:
    /// true when this call opened it.
    async fn open_first_pass(
        &self,
        sid: &str,
        d: &Draft,
        fp: FirstPass,
        profile: crate::session::TargetRef,
    ) -> Result<bool> {
        let corr = correlation(sid);
        let (_, fp_sid) = theseus_kernel::task_ids(&corr);
        let tree = super::tree_of(&self.cfg, &fp_sid);
        let diff = Self::first_pass_tree(d, &tree).await?;
        let parent = self
            .kernel
            .execution(&d.parent_execution)?
            .context("the draft's conversation has no execution")?;
        let want =
            parent.budget.available() / crate::task::DEFAULT_SHARE.1 * crate::task::DEFAULT_SHARE.0;
        let how = Opening {
            reports_to: None,
            wake_parent: false,
            start_at_ms: None,
            helper: None,
        };
        let now = theseus_protocol::now_unix_ms();
        let mut staged = d.clone();
        staged.first_pass = Some(FirstPass {
            task: Some(fp_sid.clone()),
            deadline_ms: now + BOUND.as_millis() as u64,
            ..fp
        });
        let mark = Mark {
            draft: sid.to_string(),
            tree: tree.to_string_lossy().into_owned(),
        };
        let briefed: std::sync::Mutex<Option<Node>> = std::sync::Mutex::new(None);
        let opened =
            self.kernel
                .open_task_owned(&d.parent_execution, &corr, want.max(1), how, |task| {
                    let id = &task.session_id;
                    let mut rec =
                        SessionRecord::with_id(id.clone(), SessionKind::Task, Some("task".into()));
                    rec.title = Some(format!("First pass: {}", title_from(&d.asks.join(" "))));
                    rec.execution_id = Some(task.id.clone());
                    rec.dir = Some(mark.tree.clone());
                    rec.last_target = Some(profile.clone());
                    rec.task = Some(TaskOf {
                        parent_session: d.parent_session.clone(),
                        parent_execution: d.parent_execution.clone(),
                        by: corr.clone(),
                        target: None,
                        arrangement: None,
                        check: None,
                        notify_done: false,
                    });
                    let author = format!("session:{}", d.parent_session);
                    let text = format!(
                        "{}\n\n```diff\n{}\n```",
                        super::brief(FIRST_PASS_HEADER, &d.asks),
                        cut(&diff, DIFF_MAX)
                    );
                    let brief = Node::relayed(id, None, Origin::Agent, &author, &text);
                    *briefed
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(brief.clone());
                    let mut records = vec![
                        NewRecord::json(kinds::SESSION, Some(id), &rec)?,
                        brief.record()?,
                    ];
                    records.extend(crate::google::private::inherited(
                        &self.store,
                        &self.cfg,
                        &d.parent_session,
                        id,
                    )?);
                    records.push(NewRecord::json(kinds::META, Some(&mark_key(id)), &mark)?);
                    records.push(NewRecord::json(
                        kinds::META,
                        Some(&super::meta_key(sid)),
                        &staged,
                    )?);
                    Ok(records)
                })?;
        // The first pass's brief is an item too, and its session is judged by
        // it (theseus-9ymz0): see the note at the draft's own brief.
        let briefed = briefed
            .into_inner()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(node) = briefed {
            self.content_message(
                &opened.task.session_id,
                &node,
                crate::content::ItemKind::TaskBrief,
                "first-pass brief",
            );
        }
        Ok(opened.opened)
    }

    /// A first pass that did not open: its tree goes (best effort), and the
    /// draft drops it, so the review opens without it.
    async fn first_pass_not_opened(&self, sid: &str, d: &mut Draft, e: &anyhow::Error) {
        let (_, fp_sid) = theseus_kernel::task_ids(&correlation(sid));
        let tree = super::tree_of(&self.cfg, &fp_sid);
        tracing::warn!(error = %format!("{e:#}"), draft = %sid, "a first pass did not open: the review opens without it");
        let (dir, at) = (d.dir.clone(), tree.clone());
        let closed = tokio::task::spawn_blocking(move || {
            !at.exists() || super::close_tree(Path::new(&dir), &at).is_ok()
        })
        .await;
        if !matches!(closed, Ok(true)) {
            tracing::warn!(tree = %tree.display(), "a first pass that did not open: its tree was not removed");
        }
        d.first_pass = None;
    }

    /// The first pass's worktree at the draft's commit, and the draft's diff.
    async fn first_pass_tree(d: &Draft, tree: &Path) -> Result<String> {
        let (dir, from, made) = (
            d.dir.clone(),
            d.base.clone(),
            d.draft_commit.clone().unwrap_or_default(),
        );
        let (draft_tree, git_dir, at) = (d.tree.clone(), d.git_dir.clone(), tree.to_path_buf());
        tokio::task::spawn_blocking(move || {
            super::detach_tree(Path::new(&dir), &at, &made)?;
            let pinned = super::Pinned {
                tree: Path::new(&draft_tree),
                git_dir: &git_dir,
            };
            let mut args = vec!["diff"];
            args.extend(super::DIFF);
            args.extend([from.as_str(), made.as_str()]);
            pinned.git(&args)
        })
        .await?
    }

    /// Stop waiting for the first pass of the draft `sid` after `delay`.
    pub(crate) fn arm_first_pass(&self, sid: &str, delay: Duration) {
        let (board, sid) = (self.tools.self_pr.clone(), sid.to_string());
        tokio::spawn(async move {
            tokio::time::sleep(delay).await;
            if let Some(core) = board.core() {
                core.runner.expire_first_pass(&sid).await;
            }
        });
    }

    /// The first pass of the draft `sid` is past its bound, or a timer says it
    /// may be: cancel it, whose end opens the review without it; not yet, arm
    /// the rest. No first pass running: nothing.
    pub(crate) async fn expire_first_pass(&self, sid: &str) {
        let Some(core) = self.tools.self_pr.core() else {
            return;
        };
        let Ok(Some(d)) = self.draft_of(sid) else {
            return;
        };
        let Some(f) = d
            .first_pass
            .as_ref()
            .filter(|_| !d.settled && d.review.is_none())
        else {
            return;
        };
        let Some(task) = f.task.as_deref() else {
            return;
        };
        let Ok(Some(rec)) = self.store.get_session::<SessionRecord>(task) else {
            return;
        };
        let exec = rec.execution_id.unwrap_or_default();
        if !matches!(self.kernel.execution(&exec), Ok(Some(e)) if !e.state.is_terminal()) {
            return;
        }
        let left = f
            .deadline_ms
            .saturating_sub(theseus_protocol::now_unix_ms());
        if left > 0 {
            return self.arm_first_pass(sid, Duration::from_millis(left));
        }
        // Boxed: a cancel's own sweep of drafts can come back here.
        if let Err(e) = Box::pin(core.cancel_execution(&exec, "first pass: past its bound")).await {
            tracing::warn!(error = %format!("{e:#}"), sid, "a first pass was not cancelled");
        }
    }

    /// The first pass ended, however: take its report, remove its tree, and
    /// open the review with what it found.
    pub(super) async fn first_pass_ended(&self, rec: &SessionRecord, mark: &Mark) -> Result<()> {
        let Some(mut d) = self.draft_of(&mark.draft)? else {
            return Ok(());
        };
        let tree = std::path::PathBuf::from(&mark.tree);
        let (dir, at) = (d.dir.clone(), tree.clone());
        let gone = tokio::task::spawn_blocking(move || {
            !at.exists() || super::close_tree(Path::new(&dir), &at).is_ok()
        })
        .await?;
        if !gone {
            tracing::warn!(tree = %mark.tree, "a first pass's tree was not removed");
        }
        if d.settled || d.review.is_some() {
            return Ok(());
        }
        let exec = rec.execution_id.clone().unwrap_or_default();
        let report = crate::task::load_report(&self.store, &self.kernel, &exec)?
            .context("the first pass has no execution")?;
        let outcome = outcome_of(&report);
        if let Some(f) = d.first_pass.as_mut() {
            f.outcome = Some(outcome.into());
            f.cost_usd = micros_to_usd(report.spent_micros);
            f.findings = report
                .text
                .as_deref()
                .filter(|_| outcome == "complete")
                .map(|t| cut(t, FINDINGS_MAX));
        }
        self.launch_review(&mark.draft, d)
    }
}
