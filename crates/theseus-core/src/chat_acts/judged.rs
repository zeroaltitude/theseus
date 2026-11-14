//! **Defence in depth behind `chat.allow_held_call`, and not the gate on it**
//! (chat-acts-are-tools, S43-1; the owner, 2026-11-13 23:32, Discord message
//! `1571050236367995101`):
//!
//! > *"ultimately i'm ok with the model deciding once it has been guided about
//! > this -- even without it, openclaw has been very safe as has claude code.
//! > i'm cool with adding an additional layer of special case jev
//! > classification that would defense-in-depth this"*
//!
//! So the order is **guidance first**: the tool is offered only while a hold
//! is open, its description states the held call and says the decision is the
//! operator's, and the model chooses. Only then is `allow_said.v1` asked one
//! Noul — *does the operator's latest message say to run this held call as it
//! was first proposed?* — and only an answer **below the question's act band**
//! takes the call back, with a reason the model can read.
//!
//! **It fails OPEN, deliberately. Do not "improve" this to fail closed.**
//! S17-D6 is the owner's standing condition that he can always say *run what
//! was originally suggested*. A judge that refused the act whenever it could
//! not be reached would let a network fault, a spent day's budget or a pack
//! switched off silently remove the one route past a safety hold — a strictly
//! worse failure than the one it guards against, because the model has already
//! read his words and judged them, because the tool is reachable only from his
//! own message in a private place, and because the hold it releases admits
//! exactly one call and nothing standing. So every not-an-answer is
//! [`Verdict::Unjudged`]: the call stands, and `tool.hold_allowed` carries
//! `judged: false` so the ledger says plainly that no judge saw it.
//!
//! The call itself is `judge::allow_said`, where the judge service's own parts
//! are in scope.

use theseus_judge::builders::AllowSaidInput;
use theseus_judge::{Answer, Judgment, Outcome};
use theseus_protocol::work::undo::HeldCall;

use crate::Core;

pub use crate::judge::allow_said::{PACK, SAYS_RUN, SCOPE};

/// What the second layer made of the operator's message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// At the act band or above: the model's call is confirmed.
    Yes,
    /// Below the act band: the call is refused, with why for the model.
    No(String),
    /// **No judge saw it** — the pack off, the client not built, the day's
    /// budget spent, the call failed, shed or skipped. The model's call stands.
    Unjudged,
}

/// The `says_run` Noul, when Jev answered it.
pub fn noul(j: &Judgment) -> Option<f64> {
    if j.pack != PACK || j.outcome != Outcome::Answered {
        return None;
    }
    j.answers
        .iter()
        .find(|a| a.def == SAYS_RUN)
        .and_then(|a| match a.answer {
            Answer::Noul { noul } => Some(noul),
            _ => None,
        })
}

/// The bar `says_run` must reach, read off the pack rather than written twice.
fn bar() -> Option<f64> {
    theseus_judge::pack::by_name(PACK)?
        .questions
        .iter()
        .find(|q| q.id == SAYS_RUN)
        .map(|q| q.thresholds.act)
}

/// The verdict for `noul` at `bar`: pure, so the fail-open rule and the band
/// are stated somewhere a reader can check them without a judge.
pub fn verdict_of(answer: Option<f64>, bar: Option<f64>) -> Verdict {
    match (answer, bar) {
        (Some(n), Some(b)) if n >= b => Verdict::Yes,
        (Some(n), Some(b)) => Verdict::No(format!(
            "A second reading of their message put the chance that it says to run this call at \
             {n:.2}, under the {b:.2} it needs."
        )),
        // No answer, or a pack whose band cannot be read: the call stands.
        _ => Verdict::Unjudged,
    }
}

/// The second layer's reading of `message` against the hold `h`, with the
/// other calls `held` in the same conversation for context.
pub async fn says_run(core: &Core, message: &str, h: &HeldCall, held: &[HeldCall]) -> Verdict {
    let input = AllowSaidInput {
        message: message.to_string(),
        tool: h.tool.clone(),
        summary: h.summary.clone(),
        id: h.id.clone(),
        others: held
            .iter()
            .filter(|o| o.id != h.id)
            .map(|o| format!("{} — {} ({})", o.tool, o.summary, o.id))
            .collect(),
    };
    let judgment = core
        .runner
        .judge
        .judge_allow_said(&h.session_id, input)
        .await;
    verdict_of(judgment.as_ref().and_then(noul), bar())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The band is read off the pack, and the three verdicts it gives.
    #[test]
    fn the_band_is_the_packs_own_and_the_three_verdicts_are_these() {
        let b = bar().expect("allow_said.v1 is embedded and has its question");
        assert!((0.0..=1.0).contains(&b), "a band: {b}");
        assert_eq!(verdict_of(Some(b), Some(b)), Verdict::Yes, "at the band");
        assert_eq!(verdict_of(Some(1.0), Some(b)), Verdict::Yes);
        assert!(matches!(verdict_of(Some(0.0), Some(b)), Verdict::No(_)));
    }

    /// **The fail-open rule, stated.** Every way of having no answer leaves the
    /// model's call standing; only an answer under the band refuses it.
    #[test]
    fn no_answer_leaves_the_call_standing_and_only_a_low_answer_refuses() {
        assert_eq!(verdict_of(None, bar()), Verdict::Unjudged, "no answer");
        assert_eq!(verdict_of(None, None), Verdict::Unjudged, "no pack either");
        assert_eq!(
            verdict_of(Some(0.0), None),
            Verdict::Unjudged,
            "an answer whose band cannot be read decides nothing"
        );
    }
}
