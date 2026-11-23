//! The independent review (theseus-pw1q.9): a check in a `"self"` session
//! of its own, on `[self] reviewer_profile`, whose model must differ from
//! the builder's. It checks the branch by its claim, never its working
//! (`crate::check`): it is given the diff, the frozen report and the
//! item's evidence, rendered as a check's claim (`check::render`), and
//! nothing of the builder's transcript. Its verdict is the reply's last
//! `VERDICT: accept` or `VERDICT: reject` line, with the lines after it as
//! its reasons; a reply without one is a reject. While `[self_fix]` is on, it
//! may also answer `VERDICT: fix` (issue 36: a small, local repair would make
//! the change acceptable; `fix`), and a head a fix already repaired is told so
//! and is offered no second one.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use anyhow::Result;
use theseus_protocol::SessionOpenParams;

use crate::rpc::Core;
use crate::turn::{Target, TurnRequest};

/// The most of a diff the reviewer is given, in characters.
pub const DIFF_CHARS: usize = 60_000;

/// What the reviewer is given.
#[derive(Debug, Clone)]
pub struct Ask {
    pub branch: String,
    pub head: String,
    pub item: String,
    pub diff: String,
    pub report: String,
    pub evidence: String,
    /// How the planted revert proved the change and what that asks of the reviewer
    /// (`Outcome::proof_note`, then a blank line), or empty. Not part of the claim, so the diff's
    /// cut never reaches it.
    pub proof: String,
    /// What it asks to run on: `[self] reviewer_profile`'s target, held as a pin when that is set
    /// (`Target::chosen`), else the reviewer's pick. What ran is `Review::model`.
    pub target: Target,
    /// When the branch was built (the claim's time).
    pub built_ms: u64,
    /// The directory the review may write in (`scratch::dir`): removed when it has answered.
    pub scratch: String,
    /// The self repo, which holds the branch: where a checkout for the review is taken from.
    pub repo: String,
    /// A fix may be asked for (`[self_fix]` is on and this head was not fixed at review yet):
    /// the most lines and files it may change.
    pub offer_fix: Option<(u64, u64)>,
    /// This head is a fix's: what was repaired, by whom, and what approved it.
    pub fixed: Option<String>,
}

/// The reviewer's verdict.
#[derive(Debug, Clone, PartialEq)]
pub struct Review {
    pub accept: bool,
    /// It answered `VERDICT: fix`: a small repair, said in its reasons (`fix`).
    pub fix: bool,
    pub reasons: Vec<String>,
    pub cost_usd: f64,
    /// The session it ran in.
    pub session: Option<String>,
    /// The profile and the model that answered, from the turn's own result: the target the review
    /// asked for is a request, and a turn can end up elsewhere. Empty when it is not known.
    pub profile: String,
    pub model: String,
}

pub type Fut<'a> = Pin<Box<dyn Future<Output = Result<Review>> + Send + 'a>>;

/// Who reviews: the core's own check turn, or a test's stand-in.
pub trait Reviewer: Send + Sync {
    fn review<'a>(&'a self, core: &'a Arc<Core>, ask: &'a Ask) -> Fut<'a>;
}

/// The check's text: the claim (the report, the diff and the evidence) as
/// `check::render` puts a check task's, and how to answer.
pub fn prompt(a: &Ask) -> String {
    let mut diff: String = a.diff.chars().take(DIFF_CHARS).collect();
    if diff.len() < a.diff.len() {
        diff.push_str("\n[the diff is cut here: read the rest from the branch]");
    }
    let claim = crate::check::Claim {
        task: a.branch.clone(),
        short: a.branch.clone(),
        node: a.head.clone(),
        at_ms: a.built_ms,
        text: format!(
            "Self branch {} at {}, for backlog item {}.\n\n## Its frozen report\n\n{}\n\n## The \
             item's evidence\n\n{}\n\n## Its diff against main\n\n```diff\n{diff}\n```",
            a.branch, a.head, a.item, a.report, a.evidence
        ),
    };
    format!(
        "{}\n\n{proof}You are the independent reviewer of a change Theseus made to its own source. Judge \
         whether the diff does what the report claims, fixes what the evidence shows, and breaks \
         nothing else; a test that would pass without the fix proves nothing. Then end your reply \
         with one line, {verdicts}, and after it one line per reason.{offer}\n\n\
         If you need a checkout or a build to check this, make them under `{scratch}`. That \
         directory is yours for this review and is removed when you have answered, so there is \
         nothing to clean up. Take the checkout from the self repository (`git -C {repo} worktree \
         add --detach {scratch}/tree {head}`) and point any build at `{scratch}/target`. Your commands' `TMPDIR` is `{scratch}/tmp`. Add no \
         worktree to any other checkout, and write nothing under the self trees directory.",
        crate::check::render(&[], Some(&claim)),
        proof = a.proof,
        verdicts = match a.offer_fix {
            Some(_) => "`VERDICT: accept`, `VERDICT: reject` or `VERDICT: fix`",
            None => "`VERDICT: accept` or `VERDICT: reject`",
        },
        offer = offer(a),
        scratch = a.scratch,
        repo = a.repo,
        head = a.head,
    )
}

/// What the reviewer is told of a fix: when it may ask for one, or the one already made.
fn offer(a: &Ask) -> String {
    if let Some(f) = &a.fixed {
        return format!(
            "\n\nThis branch was repaired in place after an earlier review asked for it: {f}. \
             Judge the whole branch as it is now. A fix is not offered a second time: answer \
             accept or reject."
        );
    }
    let Some((lines, files)) = a.offer_fix else {
        return String::new();
    };
    format!(
        "\n\nAnswer fix only when the change is right in what it does and a small, local repair \
         would make it acceptable: at most {lines} lines added and removed, in at most {files} \
         files, touching no test that existed before the branch. Never ask a fix to change what \
         the change is for. After `VERDICT: fix`, say exactly what to repair, one line each: \
         another model makes that repair as one commit, the model that built the branch must \
         accept it, the gate and the planted revert run again, and a fresh review reads the whole."
    )
}

/// The verdict a reply gives: its last `VERDICT:` line and the lines after.
pub fn verdict_of(reply: &str) -> (bool, Vec<String>) {
    let (word, reasons) = word_of(reply);
    (word == "accept", reasons)
}

/// The verdict's word (`accept`, `reject` or `fix`) and the lines after it. Anything else, or no
/// `VERDICT:` line at all, is a reject.
pub fn word_of(reply: &str) -> (&'static str, Vec<String>) {
    let lines: Vec<&str> = reply.lines().collect();
    let Some(at) = lines.iter().rposition(|l| {
        l.trim()
            .trim_matches('`')
            .to_ascii_lowercase()
            .starts_with("verdict:")
    }) else {
        return ("reject", vec!["the reviewer gave no verdict line".into()]);
    };
    let word = lines[at]
        .trim()
        .trim_matches('`')
        .split_once(':')
        .map(|(_, w)| w.trim().to_ascii_lowercase())
        .unwrap_or_default();
    let word = match word.as_str() {
        "accept" => "accept",
        "fix" => "fix",
        _ => "reject",
    };
    let accept = word == "accept";
    let mut reasons: Vec<String> = lines[at + 1..]
        .iter()
        .map(|l| l.trim().trim_start_matches(['-', '*', ' ']).to_string())
        .filter(|l| !l.is_empty())
        .take(20)
        .collect();
    if reasons.is_empty() && !accept {
        reasons.push(match word {
            "fix" => "the reviewer asked for a fix and said nothing of what to repair".into(),
            _ => "the reviewer rejected it and gave no reason".into(),
        });
    }
    (word, reasons)
}

/// The core's own reviewer: one check turn in a new `"self"` session.
pub struct CheckTurn;

impl Reviewer for CheckTurn {
    fn review<'a>(&'a self, core: &'a Arc<Core>, ask: &'a Ask) -> Fut<'a> {
        Box::pin(async move {
            let rec = core.open_session_as(
                SessionOpenParams {
                    label: Some(format!("self review: {}", ask.branch)),
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
            // A reviewer's session is offered a reviewer's tools, and builds nothing (`rsi::roles`).
            core.tools
                .self_pr
                .set_role(&session, crate::rsi::roles::Role::Reviewer)?;
            // Its scratch is what it may remove while it runs (`rsi::own_removals`).
            core.tools
                .self_pr
                .review_opened(&session, std::path::Path::new(&ask.scratch));
            let sink = crate::bus::EventSink::new(core.bus.clone(), &session, None);
            let run = core.runner.run(TurnRequest {
                session: rec,
                input: Some(prompt(ask)),
                target: ask.target.clone(),
                sink,
                author: "self:review".into(),
                attachments: vec![],
                arrived: None,
                reply_to: None,
                surface: None,
                answer_at: None,
                prompt: None,
            });
            // Bounded while `[self_fix]` is on; with it off a review runs as long as it did before.
            let r = match crate::rsi::join::fix::turns::review_limit(core) {
                Some(limit) => {
                    let within = crate::rsi::join::fix::turns::within;
                    within(core, &session, exec.as_deref(), limit, run).await
                }
                None => run.await,
            };
            core.tools.self_pr.review_closed(&session);
            let r = r?;
            let (word, reasons) = word_of(&r.output);
            Ok(Review {
                accept: word == "accept",
                fix: word == "fix",
                reasons,
                cost_usd: r.cost_usd.unwrap_or(0.0),
                session: Some(session),
                profile: r.profile,
                model: r.model,
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_last_verdict_line_decides_and_none_is_a_reject() {
        assert_eq!(
            verdict_of("Looks right.\nVERDICT: accept\n- the test fails without it"),
            (true, vec!["the test fails without it".to_string()])
        );
        let (a, r) =
            verdict_of("verdict: accept? no.\n`VERDICT: reject`\n- it skips the lock\n- no test");
        assert!(!a);
        assert_eq!(r, ["it skips the lock", "no test"]);
        let (a, r) = verdict_of("I think it is fine.");
        assert!(!a);
        assert_eq!(r, ["the reviewer gave no verdict line"]);
    }

    #[test]
    fn a_conditional_verdict_is_not_approval() {
        for text in [
            "accept? no.",
            "acceptable only after fixing the gate",
            "fix later",
        ] {
            assert_eq!(word_of(&format!("VERDICT: {text}")).0, "reject");
        }
    }

    /// `fix` is a word of its own (issue 36), never an accept; anything else is a reject.
    #[test]
    fn fix_is_its_own_word_and_no_accept() {
        let (w, r) = word_of("Close.\nVERDICT: fix\n- the new test asserts the wrong field");
        assert_eq!(w, "fix");
        assert_eq!(r, ["the new test asserts the wrong field"]);
        assert!(!verdict_of("VERDICT: fix\n- x").0, "a fix is no accept");
        assert_eq!(word_of("VERDICT: fixed it myself").0, "reject");
        assert_eq!(word_of("VERDICT: maybe").0, "reject");
    }
}
