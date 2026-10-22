//! China-origin drafts, always reviewed by Claude (theseus-a1wvq.13; the
//! owner, 2026-10-17: "I'd always want to do final review and fixes with a
//! more competent model").
//!
//! - **Who drafts.** A coding class whose `[routing.classes.<class>]` names
//!   `cn_draft` (with `final`, its Claude reviewers) drafts an open, admitted
//!   ask of a conversation working in a git tree under the open roots
//!   (`turn/draft_step.rs` says the rule and its `draft_skip` words). That
//!   turn makes no model call: it opens the draft and says so in one line.
//! - **The draft** is an open task of the session (`OpenHow::Draft`) on the
//!   `cn_draft` profile, in its own git worktree of the session's directory at
//!   `<state>/drafts/<short>`, on branch `draft/<short>`, which joins its open
//!   roots (`open_view::open_roots`) and no other session's. Its brief is the
//!   fixed `DRAFT_HEADER` and the open run's person messages, verbatim. It is
//!   offered the file tools and never a program (`Open::offers`), and it never
//!   moves to the `[model]` default: where the floor would move it, its turn
//!   ends (`draft_step::stopped`). Its report is never posted
//!   (`TaskOf::target_for`, `task::cancelled_report`).
//! - **The review.** However the draft ends, the core commits its tree ("draft
//!   by <profile>") and opens a check of it on the first usable `final`
//!   profile, in the frame after the draft's end (`draft_ended`), under the
//!   parent's view, its `dir` the tree, its brief `REVIEW_HEADER` and the same
//!   messages. Its correlation id is the draft's, so a rerun finds it. With no
//!   usable reviewer nothing is applied and the place is asked once.
//! - **The apply.** When the review ends the core commits the tree again
//!   ("review by <profile>"); a review that completed applies
//!   `git diff <base>..<final>` to the session's directory, `git apply
//!   --check` first, never `--index` or `--3way`; the worktree is removed (the
//!   branch stays), and one frame holds the line that says what happened and
//!   the `task.draft` row (`fact/draft.rs`). The review's report is posted as
//!   a check's is, so the line follows it in the next frame.
//!
//! Every git call is a child through `children::spawn`, on the blocking pool.
//! What a draft is lives in a META record, `draft.<draft session>`: no stored
//! record gains a field.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use theseus_kernel::children::{self, Kind};
use theseus_kernel::{micros_to_usd, ExecState, KernelError, Opening, SessionKind};
use theseus_store::{kinds, NewRecord};

use crate::check::Claim;
use crate::node::{Body, Node, Origin};
use crate::session::{title_from, SessionRecord, TargetRef, TaskOf};
use crate::task::short;
use crate::turn::TurnRunner;

/// The draft's brief, before the open run's person messages.
pub const DRAFT_HEADER: &str = "Draft this change in your tree. You can read, search and edit \
files there; you cannot run programs, so don't try to build or test: a Claude reviewer will run \
the tests, check and finish your work. When you are done, report what you changed and what you \
could not do.";

/// The failure classes of a draft's turn that ends where the floor would
/// move it: a result the screen failed or a taint, and a brief not admitted.
pub const DRAFT_STOPPED: &str = "draft_stopped";
pub const DRAFT_NOT_ADMITTED: &str = "draft_not_admitted";

/// Who writes the core's commits in a draft's tree.
const IDENT: [&str; 4] = [
    "-c",
    "user.name=Theseus",
    "-c",
    "user.email=theseus@localhost",
];

/// The review's brief, before the open run's person messages.
pub fn review_header(base: &str) -> String {
    format!(
        "Review the draft's change against the request below: read the diff (git diff {base}), \
         run the tests, fix what is wrong, and commit. If there is no usable draft, do the work \
         yourself. End with one line: `Changed: <n> lines; tests fixed: <m>`."
    )
}

/// What a draft is, kept as the META record `draft.<draft session>` from the
/// frame that opens it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Draft {
    pub parent_session: String,
    pub parent_execution: String,
    /// The owner's directory, where a reviewed change is applied.
    pub dir: String,
    pub tree: String,
    pub branch: String,
    /// `HEAD` of the owner's directory when the tree was made.
    pub base: String,
    pub profile: String,
    pub model: String,
    pub reviewers: Vec<String>,
    /// The open run's person messages, oldest first.
    pub asks: Vec<String>,
    /// Where the place hears the review and the line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// Set when the review opens: the draft commit (the base when the draft
    /// changed nothing), its outcome and its cost.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draft_commit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<String>,
    #[serde(default)]
    pub draft_cost_usd: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review: Option<String>,
    /// The `task.draft` row is written: nothing more happens.
    #[serde(default)]
    pub settled: bool,
}

pub fn meta_key(draft_session: &str) -> String {
    format!("draft.{draft_session}")
}

/// Whether `rec` is a draft's session.
pub fn is_draft(rec: &SessionRecord) -> bool {
    rec.data
        .as_ref()
        .and_then(|d| d.opened.as_ref())
        .is_some_and(|o| o.how == crate::data::OpenHow::Draft)
}

/// Whether the session `id` is a draft's, read from the store: a session
/// that does not read is not.
pub fn is_draft_session(store: &crate::store::Store, id: &str) -> bool {
    store
        .get_session::<SessionRecord>(id)
        .ok()
        .flatten()
        .is_some_and(|r| is_draft(&r))
}

/// The tree a draft, or its review, works in while it is open: the file
/// tools there are not held by the approve list's hold on the state dir, as a
/// self task's own tree is not (`rsi::own_tree`). Any other session: none.
pub(crate) fn own_tree(rt: &crate::toolrun::ToolRuntime, session: &str) -> Option<PathBuf> {
    let core = rt.self_pr.core()?;
    let rec: SessionRecord = core.store.get_session(session).ok().flatten()?;
    let of = match is_draft(&rec) {
        true => rec.session_id,
        false => rec.task?.check?.checked_task,
    };
    let d: Draft = core.store.get_meta(&meta_key(&of)).ok().flatten()?;
    let mine = d.review.as_deref() == Some(session) || of == session;
    (mine && !d.settled).then(|| theseus_tools::paths::canonical_best_effort(Path::new(&d.tree)))
}

/// A draft's tree, from its session's id.
pub fn tree_of(cfg: &crate::Config, draft_session: &str) -> PathBuf {
    cfg.state_dir().join("drafts").join(short(draft_session))
}

pub fn branch_of(draft_session: &str) -> String {
    format!("draft/{}", short(draft_session))
}

/// The review's correlation id, derived from the draft's session: its open
/// is the kernel's, so a second open finds it and writes nothing.
fn review_correlation(draft_session: &str) -> String {
    let tail = draft_session.strip_prefix("ses_").unwrap_or(draft_session);
    format!("act_{tail}r")
}

/// The draft's brief, built by the core from the open run's person messages.
pub fn brief(header: &str, asks: &[String]) -> String {
    let mut out = header.to_string();
    for a in asks {
        out.push_str("\n\n");
        out.push_str(a);
    }
    out
}

/// The reviewer's count of the tests it fixed, from its last line
/// (`Changed: <n> lines; tests fixed: <m>`): none when the line is missing.
pub fn tests_fixed(report: &str) -> Option<u64> {
    let last = report.lines().rev().find(|l| !l.trim().is_empty())?;
    let (_, m) = last.split_once("tests fixed:")?;
    let digits: String = m
        .trim()
        .trim_start_matches('`')
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok()
}

// ------------------------------------------------------------ git

/// `git -C <dir> <args>`, a child of the daemon's, never prompting, with
/// `input` on its stdin when given. Blocking: call it on the blocking pool.
fn git_in(dir: &Path, args: &[&str], input: Option<&[u8]>) -> Result<Output> {
    use std::io::Write;
    let mut c = Command::new("git");
    c.arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = children::spawn(Kind::Owned, || c.spawn(), |c| Some(c.id()))
        .context("git could not start")?;
    if let (Some(bytes), Some(mut stdin)) = (input, child.stdin.take()) {
        stdin.write_all(bytes)?;
    }
    Ok(child.wait_with_output()?)
}

/// `git` that must succeed: its stdout, trimmed.
fn git(dir: &Path, args: &[&str]) -> Result<String> {
    let o = git_in(dir, args, None)?;
    if !o.status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&o.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&o.stdout).trim().to_string())
}

/// Whether `dir` is inside a git work tree.
pub fn work_tree(dir: &Path) -> bool {
    git(dir, &["rev-parse", "--is-inside-work-tree"]).is_ok_and(|o| o == "true")
}

/// A worktree of `dir` at `HEAD` in `tree`, on a new `branch`: `HEAD`'s sha.
pub fn open_tree(dir: &Path, tree: &Path, branch: &str) -> Result<String> {
    let base = git(dir, &["rev-parse", "HEAD"])?;
    if let Some(up) = tree.parent() {
        std::fs::create_dir_all(up)?;
    }
    let at = tree.to_string_lossy();
    git(dir, &["worktree", "add", "-b", branch, &at, &base])?;
    Ok(base)
}

/// Commit everything in `tree` as `message`, when anything changed: the
/// tree's `HEAD` after.
fn commit(tree: &Path, message: &str) -> Result<String> {
    if !git(tree, &["status", "--porcelain"])?.is_empty() {
        git(tree, &["add", "-A"])?;
        let mut args = IDENT.to_vec();
        args.extend(["commit", "-q", "--no-verify", "-m", message]);
        git(tree, &args)?;
    }
    git(tree, &["rev-parse", "HEAD"])
}

/// Added plus deleted lines from `from` to `to` (`--numstat`; a binary
/// file counts none).
fn numstat(tree: &Path, from: &str, to: &str) -> Result<u64> {
    let out = git(tree, &["diff", "--numstat", from, to])?;
    Ok(out
        .lines()
        .filter_map(|l| {
            let mut f = l.split('\t');
            let added = f.next()?.parse::<u64>().ok()?;
            let deleted = f.next()?.parse::<u64>().ok()?;
            Some(added + deleted)
        })
        .sum())
}

/// What the apply did.
enum Apply {
    Done,
    NoChange,
    Refused(String),
}

/// `git diff <base>..<fin>` of the tree applied to `dir`: checked first, and
/// never staged in its index nor merged with conflict markers.
fn apply(dir: &Path, tree: &Path, base: &str, fin: &str) -> Result<Apply> {
    let o = git_in(tree, &["diff", "--binary", base, fin], None)?;
    if !o.status.success() {
        bail!(
            "git diff failed: {}",
            String::from_utf8_lossy(&o.stderr).trim()
        );
    }
    if o.stdout.is_empty() {
        return Ok(Apply::NoChange);
    }
    let check = git_in(dir, &["apply", "--check", "-"], Some(&o.stdout))?;
    if !check.status.success() {
        let why = String::from_utf8_lossy(&check.stderr);
        let first = why.lines().next().unwrap_or("it does not apply").trim();
        return Ok(Apply::Refused(first.to_string()));
    }
    let done = git_in(dir, &["apply", "-"], Some(&o.stdout))?;
    if !done.status.success() {
        bail!(
            "git apply failed: {}",
            String::from_utf8_lossy(&done.stderr).trim()
        );
    }
    Ok(Apply::Done)
}

/// Remove the tree; its branch stays.
pub(crate) fn close_tree(dir: &Path, tree: &Path) -> Result<()> {
    git(
        dir,
        &["worktree", "remove", "--force", &tree.to_string_lossy()],
    )
    .map(drop)
}

/// The commits and the apply after a review, on the blocking pool.
struct Settled {
    fin: String,
    lines: u64,
    applied: Result<Apply>,
}

fn settle_tree(d: &Draft, review: &str, completed: bool) -> Result<Settled> {
    let (dir, tree) = (Path::new(&d.dir), Path::new(&d.tree));
    let fin = commit(tree, &format!("review by {review}"))?;
    let from = d.draft_commit.as_deref().unwrap_or(&d.base);
    let lines = numstat(tree, from, &fin)?;
    let applied = match completed {
        true => apply(dir, tree, &d.base, &fin),
        false => Ok(Apply::NoChange),
    };
    if let Err(e) = close_tree(dir, tree) {
        tracing::warn!(error = %format!("{e:#}"), tree = %d.tree, "a reviewed draft's tree was not removed");
    }
    Ok(Settled {
        fin,
        lines,
        applied,
    })
}

// ------------------------------------------------------------ the core's side

/// A task's outcome word, from its report.
fn outcome_of(r: &crate::task::Report) -> &'static str {
    let reason = r.reason.as_deref().unwrap_or("");
    match r.outcome {
        "failed" if reason.starts_with(DRAFT_STOPPED) => "stopped",
        "failed" if reason.starts_with(DRAFT_NOT_ADMITTED) => "not_admitted",
        o => o,
    }
}

impl TurnRunner {
    /// A task session ended (its end's frame, or a cancel's, is written): a
    /// draft's end opens its review, and a review's end applies and settles.
    /// Any other session pays one record read.
    pub async fn draft_ended(&self, session_id: &str) {
        let Ok(Some(rec)) = self.store.get_session::<SessionRecord>(session_id) else {
            return;
        };
        let done = if is_draft(&rec) {
            self.open_review(&rec).await
        } else if let Some(c) = rec.task.as_ref().and_then(|t| t.check.as_ref()) {
            let checked = c.checked_task.clone();
            self.finish_review(&rec, &checked).await
        } else {
            return;
        };
        if let Err(e) = done {
            tracing::warn!(error = %format!("{e:#}"), session_id, "a draft's next step failed");
        }
    }

    fn draft_of(&self, draft_session: &str) -> Result<Option<Draft>> {
        self.store.get_meta(&meta_key(draft_session))
    }

    /// The first usable `final` profile, for a review that is no open request.
    fn reviewer(&self, d: &Draft) -> Option<TargetRef> {
        d.reviewers
            .iter()
            .find(|p| self.profile_usable(p, false))
            .and_then(|p| self.tools.profiles.get(p).cloned())
    }

    /// The draft ended: commit its tree, then open its review, or tell the
    /// place that none can run.
    async fn open_review(&self, rec: &SessionRecord) -> Result<()> {
        let sid = rec.session_id.clone();
        let Some(mut d) = self.draft_of(&sid)? else {
            bail!("draft {sid} has no record");
        };
        if d.settled || d.review.is_some() {
            return Ok(());
        }
        let exec = rec.execution_id.clone().unwrap_or_default();
        let report = crate::task::load_report(&self.store, &self.kernel, &exec)?
            .context("the draft has no execution")?;
        let outcome = outcome_of(&report);
        let tree = PathBuf::from(&d.tree);
        let message = format!("draft by {}", d.profile);
        let made = tokio::task::spawn_blocking(move || commit(&tree, &message)).await??;
        d.draft_commit = Some(made);
        d.outcome = Some(outcome.into());
        d.draft_cost_usd = micros_to_usd(report.spent_micros);
        let claim_text = match (&report.text, outcome) {
            (Some(t), "complete") => t.clone(),
            _ => format!(
                "no usable draft: {outcome} ({})",
                report.reason.as_deref().unwrap_or("no report")
            ),
        };
        let Some(reviewer) = self.reviewer(&d) else {
            return self.unreviewed(&sid, d, "no final profile is usable");
        };
        let corr = review_correlation(&sid);
        let parent = self
            .kernel
            .execution(&d.parent_execution)?
            .context("the draft's conversation has no execution")?;
        let want =
            parent.budget.available() / crate::task::DEFAULT_SHARE.1 * crate::task::DEFAULT_SHARE.0;
        let how = Opening {
            reports_to: d.target.clone(),
            wake_parent: false,
            start_at_ms: None,
        };
        let claim = Claim {
            task: sid.clone(),
            short: short(&sid),
            node: report.node.clone().unwrap_or_default(),
            at_ms: theseus_protocol::now_unix_ms(),
            text: claim_text,
        };
        let mut basis = None;
        let opened =
            self.kernel
                .open_task_owned(&d.parent_execution, &corr, want.max(1), how, |task| {
                    let (records, b) = self.review_records(task, &d, &reviewer, &claim)?;
                    basis = Some(b);
                    Ok(records)
                });
        let opened = match opened {
            Ok(o) => o,
            Err(e) if matches!(e.downcast_ref(), Some(KernelError::NothingToCarve { .. })) => {
                return self.unreviewed(
                    &sid,
                    d,
                    "the conversation's spend limit leaves nothing to carve",
                );
            }
            Err(e) => return Err(e),
        };
        if !opened.opened {
            return Ok(());
        }
        let review = opened.task.session_id;
        if let Some(t) = &d.target {
            self.outbox.task_bound(&review, t);
        }
        if let Some(b) = &basis {
            self.draft_rec(&review)
                .announce(&crate::fact::check::TaskCheckOpened {
                    short: &short(&review),
                    session_id: &review,
                    basis: b,
                });
        }
        Ok(())
    }

    /// The review's records in its open's frame: its session (a check of the
    /// draft, on the reviewer, in the tree), its brief, the claim's
    /// arrangement, its row, and the draft's record naming it.
    fn review_records(
        &self,
        task: &theseus_kernel::Execution,
        d: &Draft,
        reviewer: &TargetRef,
        claim: &Claim,
    ) -> Result<(Vec<NewRecord>, theseus_protocol::TaskCheck)> {
        let sid = &task.session_id;
        let author = format!("session:{}", d.parent_session);
        let text = brief(&review_header(&d.base), &d.asks);
        let brief_node = Node::relayed(sid, None, Origin::Agent, &author, &text);
        let mut arrangement = Node::arrangement(sid, &author, vec![], false);
        if let Body::Arrangement { claim: c, .. } = &mut arrangement.body {
            *c = Some(claim.clone());
        }
        let basis = theseus_protocol::TaskCheck {
            checked_task: claim.task.clone(),
            checked_short: claim.short.clone(),
            report_node: claim.node.clone(),
            report_at_ms: claim.at_ms,
            excluded_sessions: vec![claim.task.clone()],
            admitted: vec![theseus_protocol::CheckPiece {
                node_id: claim.node.clone(),
                session_id: claim.task.clone(),
                role: "claim".into(),
                from: "claim".into(),
            }],
            profile: reviewer.profile.clone(),
            provider: reviewer.provider.clone(),
            model: reviewer.model.clone(),
            overlaps: vec![],
            at_ms: theseus_protocol::now_unix_ms(),
        };
        let mut rec = SessionRecord::with_id(sid.clone(), SessionKind::Task, Some("task".into()));
        rec.title = Some(format!("Review: {}", title_from(&d.asks.join(" "))));
        rec.execution_id = Some(task.id.clone());
        rec.dir = Some(d.tree.clone());
        rec.last_target = Some(reviewer.clone());
        rec.task = Some(TaskOf {
            parent_session: d.parent_session.clone(),
            parent_execution: d.parent_execution.clone(),
            by: review_correlation(&claim.task),
            target: d.target.clone(),
            arrangement: Some(arrangement.id.clone()),
            check: Some(basis.clone()),
            notify_done: false,
        });
        let mut records = vec![
            NewRecord::json(kinds::SESSION, Some(sid), &rec)?,
            brief_node.record()?,
            arrangement.record()?,
        ];
        if !claim.node.is_empty() {
            records.push(
                crate::graph::Edge::new(
                    crate::graph::EdgeKind::DerivedFrom,
                    &arrangement.id,
                    &claim.node,
                    crate::graph::VIA_CLAIM,
                )
                .record()?,
            );
        }
        if let Some(t) = &d.target {
            records.push(self.outbox.task_record(sid, t)?);
        }
        records.push(
            self.draft_rec(sid)
                .row(&crate::fact::check::TaskCheckOpened {
                    short: &short(sid),
                    session_id: sid,
                    basis: &basis,
                })?,
        );
        let named = Draft {
            review: Some(sid.clone()),
            ..d.clone()
        };
        records.push(NewRecord::json(
            kinds::META,
            Some(&meta_key(&claim.task)),
            &named,
        )?);
        Ok((records, basis))
    }

    /// No reviewer can run: nothing is applied, the tree stays, and the
    /// place is asked once, with the `task.draft` row in the post's frame.
    fn unreviewed(&self, draft_session: &str, mut d: Draft, why: &str) -> Result<()> {
        let text = format!(
            "The draft by {} is on branch {} (not reviewed: {why}), so it wasn't applied. Ask me \
             to review it when that clears.",
            d.profile, d.branch
        );
        let row = crate::fact::draft::TaskDraft {
            draft_task: draft_session,
            draft_profile: &d.profile,
            draft_model: &d.model,
            draft_cost_usd: d.draft_cost_usd,
            draft_outcome: d.outcome.as_deref().unwrap_or("failed"),
            review: None,
            lines_changed: 0,
            tests_fixed_reported: None,
            applied: false,
            why,
        };
        d.settled = true;
        self.settle(draft_session, &d, &text, &row)
    }

    /// The review ended: commit, apply when it completed, close the tree,
    /// and say what happened, with the row.
    async fn finish_review(&self, rec: &SessionRecord, draft_session: &str) -> Result<()> {
        let Some(mut d) = self.draft_of(draft_session)? else {
            return Ok(());
        };
        if d.settled || d.review.as_deref() != Some(rec.session_id.as_str()) {
            return Ok(());
        }
        let exec = rec.execution_id.clone().unwrap_or_default();
        let report = crate::task::load_report(&self.store, &self.kernel, &exec)?
            .context("the review has no execution")?;
        let reviewer = rec.last_target.clone().context("the review has no model")?;
        let completed = report.outcome == "complete"
            && self
                .kernel
                .execution(&exec)?
                .is_some_and(|e| e.state == ExecState::Complete);
        let (held, profile) = (d.clone(), reviewer.profile.clone());
        let settled =
            tokio::task::spawn_blocking(move || settle_tree(&held, &profile, completed)).await?;
        let (lines, applied) = match settled {
            Ok(s) => {
                tracing::debug!(fin = %s.fin, "a draft's review committed");
                (s.lines, s.applied)
            }
            Err(e) => (0, Err(e)),
        };
        let at = format!("branch {}", d.branch);
        let (applied, why) = match (completed, applied) {
            (false, _) => (false, format!("the review {}", report.outcome)),
            (true, Ok(Apply::Done)) => (true, "applied".to_string()),
            (true, Ok(Apply::NoChange)) => (false, "no change".to_string()),
            (true, Ok(Apply::Refused(r))) => (false, format!("it does not apply ({r})")),
            (true, Err(e)) => (false, format!("{e:#}")),
        };
        let text = match applied {
            true => format!(
                "Drafted by {}, reviewed and fixed by {}; applied to {}.",
                d.profile, reviewer.profile, d.dir
            ),
            false => format!(
                "Drafted by {}, reviewed and fixed by {}; not applied: {why} ({at}).",
                d.profile, reviewer.profile
            ),
        };
        let row = crate::fact::draft::TaskDraft {
            draft_task: draft_session,
            draft_profile: &d.profile,
            draft_model: &d.model,
            draft_cost_usd: d.draft_cost_usd,
            draft_outcome: d.outcome.as_deref().unwrap_or("failed"),
            review: Some(crate::fact::draft::Reviewed {
                task: &rec.session_id,
                profile: &reviewer.profile,
                model: &reviewer.model,
                cost_usd: micros_to_usd(report.spent_micros),
            }),
            lines_changed: lines,
            tests_fixed_reported: report.text.as_deref().and_then(tests_fixed),
            applied,
            why: &why,
        };
        d.settled = true;
        self.settle(draft_session, &d, &text, &row)
    }

    /// One frame: the line to the place (when it has one), the row, and the
    /// draft's record settled.
    fn settle(
        &self,
        draft_session: &str,
        d: &Draft,
        text: &str,
        row: &crate::fact::draft::TaskDraft<'_>,
    ) -> Result<()> {
        let rec = self.draft_rec(draft_session);
        let mut records = vec![
            rec.row(row)?,
            NewRecord::json(kinds::META, Some(&meta_key(draft_session)), d)?,
        ];
        let post = match &d.target {
            Some(t) => {
                let (post, more) = self.outbox.stage(
                    &d.parent_session,
                    &d.parent_execution,
                    t,
                    serde_json::json!({"kind": "notice", "text": text}),
                )?;
                records.extend(more);
                Some(post)
            }
            None => None,
        };
        self.store.append(&records)?;
        if let Some(p) = &post {
            self.outbox.posted(p);
        }
        rec.announce(row);
        Ok(())
    }

    fn draft_rec<'a>(&'a self, session: &'a str) -> crate::fact::Rec<'a> {
        crate::fact::Rec {
            narrator: &self.narrator,
            session: Some(session),
            turn: None,
            to: crate::fact::To::Session(&self.bus, session),
            store: &self.store,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_reviewers_last_line_gives_its_count() {
        assert_eq!(
            tests_fixed("Fixed the parser.\nChanged: 12 lines; tests fixed: 3\n"),
            Some(3)
        );
        assert_eq!(tests_fixed("`Changed: 4 lines; tests fixed: 7`"), Some(7));
        assert_eq!(tests_fixed("Changed: 4 lines; tests fixed: 0"), Some(0));
        assert_eq!(tests_fixed("All done."), None);
        assert_eq!(
            review_correlation("ses_abc123"),
            "act_abc123r",
            "a rerun finds the same review"
        );
    }
}
