//! route.v9's `draft` question (theseus-kqeih; the owner, 2026-10-27 23:05 and
//! 23:15: "let it be routed by jev with decent standing orders"): for a coding
//! turn whose class has a `cn_draft`, does a cheaper drafting model make the
//! first pass, which a Claude profile then reviews and fixes? It is asked only
//! when `[routing] draft_by_jev` is on, and anything but a sure `cheap` is the
//! stronger model: the standing orders are the question's words, not code.

use theseus_judge::band::Band;

/// The question, by its id in the pack.
pub const QUESTION: &str = "draft";

/// The answer that lets a draft be made.
pub const CHEAP: &str = "cheap";

/// Jev's answer: `cheap` or `strong`, how sure, and its band.
#[derive(Debug, Clone, PartialEq)]
pub struct DraftAnswer {
    pub choice: String,
    pub confidence: f64,
    pub band: Band,
}

impl DraftAnswer {
    /// The word the route row keeps: `cheap` or `strong`, whatever Jev's
    /// free-form answer was.
    pub fn word(&self) -> &'static str {
        match self.choice == CHEAP {
            true => "cheap",
            false => "strong",
        }
    }
}

/// Whether Jev's answer lets a draft be made, or the `draft_skip` word that
/// says why not: `jev_unasked` (the verdict carries no answer), `jev_declined`
/// (it said `strong`) or `jev_unsure` (it said `cheap` under the pack's confirm
/// bar).
pub fn allows(answer: Option<&DraftAnswer>) -> Result<(), &'static str> {
    match answer {
        None => Err("jev_unasked"),
        Some(a) if a.choice != CHEAP => Err("jev_declined"),
        Some(a) if a.band == Band::Escalate => Err("jev_unsure"),
        Some(_) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(choice: &str, band: Band) -> DraftAnswer {
        DraftAnswer {
            choice: choice.into(),
            confidence: 0.9,
            band,
        }
    }

    /// Only a sure `cheap` drafts; the words are the ones the bench report counts.
    #[test]
    fn only_a_sure_cheap_answer_lets_a_draft_be_made() {
        assert_eq!(allows(Some(&a("cheap", Band::Act))), Ok(()));
        assert_eq!(allows(Some(&a("cheap", Band::Confirm))), Ok(()));
        assert_eq!(allows(Some(&a("cheap", Band::Escalate))), Err("jev_unsure"));
        assert_eq!(allows(Some(&a("strong", Band::Act))), Err("jev_declined"));
        assert_eq!(allows(Some(&a("anything", Band::Act))), Err("jev_declined"));
        assert_eq!(allows(None), Err("jev_unasked"));
        assert_eq!(a("cheap", Band::Act).word(), "cheap");
        assert_eq!(a("strong", Band::Act).word(), "strong");
    }
}
