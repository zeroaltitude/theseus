//! The independent review (theseus-pw1q.9): a check in a `"self"` session
//! of its own, on `[self] reviewer_profile`, whose model must differ from
//! the builder's. It checks the branch by its claim, never its working
//! (`crate::check`): it is given the diff, the frozen report and the
//! item's evidence, rendered as a check's claim (`check::render`), and
//! nothing of the builder's transcript. Its verdict is the reply's last
//! `VERDICT: accept` or `VERDICT: reject` line, with the lines after it as
//! its reasons; a reply without one is a reject.

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
    /// What it runs on: `[self] reviewer_profile`'s target.
    pub target: Target,
    /// When the branch was built (the claim's time).
    pub built_ms: u64,
}

/// The reviewer's verdict.
#[derive(Debug, Clone, PartialEq)]
pub struct Review {
    pub accept: bool,
    pub reasons: Vec<String>,
    pub cost_usd: f64,
    /// The session it ran in.
    pub session: Option<String>,
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
        "{}\n\nYou are the independent reviewer of a change Theseus made to its own source. Judge \
         whether the diff does what the report claims, fixes what the evidence shows, and breaks \
         nothing else; a test that would pass without the fix proves nothing. Then end your reply \
         with one line, `VERDICT: accept` or `VERDICT: reject`, and after it one line per reason.",
        crate::check::render(&[], Some(&claim))
    )
}

/// The verdict a reply gives: its last `VERDICT:` line and the lines after.
pub fn verdict_of(reply: &str) -> (bool, Vec<String>) {
    let lines: Vec<&str> = reply.lines().collect();
    let Some(at) = lines.iter().rposition(|l| {
        l.trim()
            .trim_matches('`')
            .to_ascii_lowercase()
            .starts_with("verdict:")
    }) else {
        return (false, vec!["the reviewer gave no verdict line".into()]);
    };
    let word = lines[at]
        .trim()
        .trim_matches('`')
        .split_once(':')
        .map(|(_, w)| w.trim().to_ascii_lowercase())
        .unwrap_or_default();
    let accept = word.starts_with("accept");
    let mut reasons: Vec<String> = lines[at + 1..]
        .iter()
        .map(|l| l.trim().trim_start_matches(['-', '*', ' ']).to_string())
        .filter(|l| !l.is_empty())
        .take(20)
        .collect();
    if reasons.is_empty() && !accept {
        reasons.push("the reviewer rejected it and gave no reason".into());
    }
    (accept, reasons)
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
            let sink = crate::bus::EventSink::new(core.bus.clone(), &session, None);
            let r = core
                .runner
                .run(TurnRequest {
                    session: rec,
                    input: Some(prompt(ask)),
                    target: ask.target.clone(),
                    sink,
                    author: "self:review".into(),
                    recompile: None,
                    attachments: vec![],
                    arrived: None,
                    reply_to: None,
                    answer_at: None,
                    prompt: None,
                })
                .await?;
            let (accept, reasons) = verdict_of(&r.output);
            Ok(Review {
                accept,
                reasons,
                cost_usd: r.cost_usd.unwrap_or(0.0),
                session: Some(session),
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
}
