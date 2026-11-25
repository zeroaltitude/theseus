//! The core's own fixer and approver: one turn each, in a `"self"` session of its own, as the
//! review's check turn is (`review::CheckTurn`), so the turn's self-budget step reserves and
//! settles each call against the day's self spend exactly as the review's are.
//!
//! The fixer's session is given the item's tree as its own (`Board::roots`, which `own_tree`
//! reads): the file tools write there and nowhere else, its commands run in L1 with the item's
//! build folder, exactly as the builder's do. Nothing more: no other tree, no git of its own (the
//! code commits), and its root is taken back when its turn ends.
//!
//! Each session is registered for its role before its first turn (`rsi::roles`): what it is
//! offered is that role's, and `self.propose_change` refuses it. Each turn has a wall-clock limit,
//! `[self_fix] turn_minutes` ([`within`]), as the review's turn now does.

use std::sync::Arc;
use std::time::Duration;

use theseus_protocol::{SessionOpenParams, TurnSubmitResult};

use super::{fix_of, ApproveAsk, FixAsk, FixFut, Fixed, Fixer};
use crate::rpc::Core;
use crate::rsi::join::review::{self, word_of, Review};
use crate::rsi::roles::Role;
use crate::turn::{Target, TurnRequest};

/// The most of a diff the fixer is given, in characters.
const DIFF_CHARS: usize = 40_000;

/// The fixer's text: the fault, the tree, the rules, and how to answer.
pub fn fix_prompt(a: &FixAsk) -> String {
    let (head, cut) = crate::scrub::safe::head(&a.diff, DIFF_CHARS);
    let mut diff = head.to_string();
    if cut {
        diff.push_str("\n[the diff is cut here: read the rest from the tree]");
    }
    format!(
        "You are the fixer of a change Theseus made to its own source, on branch {branch} for \
         backlog item {item}. Its {point} found a small fault. Repair that fault and nothing \
         else, in the tree at `{tree}` (the branch, checked out): edit it with the file tools by \
         absolute path, and run the tests you touch from that tree with proc.run and sandbox: \
         true.\n\n\
         The rules. Theseus checks each on your change, and a change that breaks one is refused:\n\
         1. Change at most {lines} lines (added and removed together), in at most {files} files.\n\
         2. Make the change right; never change what it is for.\n\
         3. Never edit a test that existed before this branch: remove no test or assertion, add \
         no #[ignore], raise no limit. You may repair the tests the branch itself added.\n\
         4. Touch nothing the keel guards, nor scripts/gate.sh, scripts/keel-guard.py, or the \
         self loop's own judge (crates/theseus-core/src/rsi/join/ and rsi/pipeline/).\n\
         5. Do not commit, branch, stash or otherwise touch git: Theseus commits your change \
         itself, as one commit.\n\
         6. Your commit is then read alone by the model that built the branch, which must accept \
         it, and the gate runs again.\n\n\
         End your reply with one line: `FIX: done` and one sentence of why it repairs the fault, \
         or `FIX: none` and why you will not (a fault that is not small, or not the change's own).\n\n\
         ## What failed\n\n```text\n{failure}\n```\n\n## The item's evidence\n\n{evidence}\n\n\
         ## The branch's diff against main\n\n```diff\n{diff}\n```",
        branch = a.branch,
        item = a.item,
        point = match a.point {
            super::Point::Gate => "gate",
            super::Point::Review => "independent review",
        },
        tree = a.tree,
        lines = a.lines,
        files = a.files,
        failure = a.failure,
        evidence = a.evidence,
    )
}

/// The approver's text: the fix's commit alone, its reason, the fault it answered.
pub fn approve_prompt(a: &ApproveAsk) -> String {
    let (diff, _) = crate::scrub::safe::head(&a.diff, DIFF_CHARS);
    format!(
        "You built branch {branch} of Theseus's own source, for backlog item {item}. Its {point} \
         found a small fault, and another model, {fixer}, repaired it with the one commit below. \
         Nobody approves their own work, so you read that commit alone and judge it: does it \
         repair the fault, and nothing else? It must not change what the change is for, weaken \
         a test, or reach beyond the fault. Theseus refuses a fix that edits a test that was \
         there before the branch, but it cannot tell a repair from a changed value that an \
         existing test reads (a constant or a limit its assertion compares with): look for that, \
         and reject it.\n\n\
         End your reply with one line, `VERDICT: accept` or `VERDICT: reject`, and after it one \
         line per reason.\n\n## The fault it answered\n\n```text\n{failure}\n```\n\n\
         ## The fixer's reason\n\n{reason}\n\n## The commit {head}, alone\n\n```diff\n{diff}\n```",
        branch = a.branch,
        item = a.item,
        point = a.point.words().trim_start_matches("at "),
        fixer = a.fixer,
        failure = a.failure,
        reason = a.reason,
        head = a.head,
    )
}

/// The core's fixer and approver: a turn each.
pub struct Turns;

/// `session` works in the tree named `slug`, as that item's builder does: what `own_tree` reads
/// for a self session's own tree and its build folder.
pub(crate) fn confine(core: &Core, session: &str, slug: &str) {
    core.tools
        .self_pr
        .roots
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(session.to_string(), slug.to_string());
}

/// `session`'s tree taken back.
pub(crate) fn release(core: &Core, session: &str) {
    core.tools
        .self_pr
        .roots
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .remove(session);
}

/// The wall-clock limit of a fixer's, an approver's and a reviewer's turn.
pub(crate) fn limit(core: &Core) -> Duration {
    Duration::from_secs(core.cfg.self_fix.turn_minutes.max(1) * 60)
}

/// The limit of a reviewer's turn: [`limit`] while `[self_fix]` is on; none with it off, where a
/// review runs as long as it did before issue 36.
pub(crate) fn review_limit(core: &Core) -> Option<Duration> {
    core.cfg.self_fix.on().then(|| limit(core))
}

/// The class of a turn stopped at its limit: a failure that does not clear by itself, so a review
/// stopped there three times in a row holds the change for the owner (`join::unreviewable`), and a
/// fixer or an approver stopped there is a fix that did not hold.
pub const LIMIT_CLASS: &str = "turn_limit";

/// `turn` (of `session`, whose execution is `exec`) to its end, or stopped once it has run for
/// `limit`: its execution is stopped, the turn is let reach its next step, and the answer is a
/// failed turn of [`LIMIT_CLASS`] that says it ran past its limit.
pub(crate) async fn within(
    core: &Core,
    session: &str,
    exec: Option<&str>,
    limit: Duration,
    turn: impl std::future::Future<Output = anyhow::Result<TurnSubmitResult>>,
) -> anyhow::Result<TurnSubmitResult> {
    tokio::pin!(turn);
    tokio::select! {
        r = &mut turn => r,
        () = tokio::time::sleep(limit) => {
            // Its execution is stopped, and the turn is given a minute to reach its next step and
            // settle what it holds; a turn with no execution has nothing to stop or wait for.
            if let Some(e) = exec {
                if let Err(err) = core.stop_execution(e, "self loop: the turn ran past its limit").await {
                    tracing::warn!(error = %format!("{err:#}"), "a self turn past its limit was not stopped");
                }
                let _ = tokio::time::timeout(Duration::from_secs(60), &mut turn).await;
            }
            Err(anyhow::Error::new(crate::turn::TurnError {
                class: LIMIT_CLASS.into(),
                transient: false,
                usage_unknown: true,
                turn_id: session.to_string(),
                session_id: session.to_string(),
                elapsed_ms: u64::try_from(limit.as_millis()).unwrap_or(u64::MAX),
                trace: None,
                usage: theseus_protocol::Usage::default(),
                cost_usd: None,
                tool_calls: 0,
                source: anyhow::anyhow!(
                    "it ran past its limit of {} minutes, [self_fix] turn_minutes, and was stopped",
                    limit.as_secs() / 60
                ),
            }))
        }
    }
}

/// One turn in a new `"self"` session opened for `role`: its output, cost, profile and model.
async fn turn(
    core: &Arc<Core>,
    role: Role,
    label: String,
    author: &str,
    input: String,
    target: Target,
    root: Option<&str>,
) -> anyhow::Result<(String, TurnSubmitResult)> {
    let rec = core.open_session_as(
        SessionOpenParams {
            label: Some(label),
            ..SessionOpenParams::default()
        },
        theseus_kernel::Authority {
            principal: crate::rsi::SELF_PRINCIPAL.into(),
            ..Default::default()
        },
        None,
    )?;
    let session = rec.session_id.clone();
    let exec = rec.execution_id.clone();
    core.tools.self_pr.set_role(&session, role)?;
    if let Some(slug) = root {
        confine(core, &session, slug);
    }
    let sink = crate::bus::EventSink::new(core.bus.clone(), &session, None);
    let run = core.runner.run(TurnRequest {
        session: rec,
        input: Some(input),
        target,
        sink,
        author: author.into(),
        attachments: vec![],
        arrived: None,
        reply_to: None,
        surface: None,
        answer_at: None,
        prompt: None,
    });
    let r = within(core, &session, exec.as_deref(), limit(core), run).await;
    if root.is_some() {
        release(core, &session);
    }
    Ok((session, r?))
}

impl Fixer for Turns {
    fn fix<'a>(&'a self, core: &'a Arc<Core>, ask: &'a FixAsk) -> FixFut<'a> {
        Box::pin(async move {
            let (session, r) = turn(
                core,
                Role::Fixer,
                format!("self fix: {}", ask.branch),
                "self:fix",
                fix_prompt(ask),
                ask.target.clone(),
                Some(&ask.slug),
            )
            .await?;
            let (done, why) = fix_of(&r.output);
            Ok(Fixed {
                done,
                why,
                cost_usd: r.cost_usd.unwrap_or(0.0),
                session: Some(session),
                profile: r.profile,
                model: r.model,
            })
        })
    }

    fn approve<'a>(&'a self, core: &'a Arc<Core>, ask: &'a ApproveAsk) -> review::Fut<'a> {
        Box::pin(async move {
            let (session, r) = turn(
                core,
                Role::Approver,
                format!("self fix check: {}", ask.branch),
                "self:fix-check",
                approve_prompt(ask),
                ask.target.clone(),
                None,
            )
            .await?;
            let (word, reasons) = word_of(&r.output);
            Ok(Review {
                accept: word == "accept",
                fix: false,
                reasons,
                cost_usd: r.cost_usd.unwrap_or(0.0),
                session: Some(session),
                profile: r.profile,
                model: r.model,
            })
        })
    }
}
