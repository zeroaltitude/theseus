//! The footing an approved answer went through (issue 22): said on the settled
//! card and in the echo, so the owner reads whether a code was taken or an
//! earlier proof of the same place covered it.

use super::proofs::{held, Proofs};
use super::windows::{span, Proof};

/// The most footings kept until the answer's echo reads them.
const KEPT: usize = 64;

/// What an approved answer rested on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Footing {
    /// A code came with this answer and counted.
    Code,
    /// No code: an open window of the same place covered it, given this long
    /// before (a span in words).
    Earlier(String),
}

impl Footing {
    /// Said on the settled card after the approver: "on the second proof given
    /// here 3 minutes earlier", "with a fresh code".
    pub fn note(&self) -> String {
        match self {
            Footing::Code => "with a fresh code".into(),
            Footing::Earlier(ago) => format!("on the second proof given here {ago} earlier"),
        }
    }

    /// The answer's echo; `fresh` is a question that takes a code every time.
    pub fn detail(&self, fresh: bool) -> String {
        match (self, fresh) {
            (Footing::Code, true) => "second proof: a fresh code was needed and given".into(),
            (Footing::Code, false) => "second proof: given here with a fresh code".into(),
            (Footing::Earlier(ago), _) => {
                format!("second proof: given here {ago} earlier, so no code was asked")
            }
        }
    }
}

impl Proofs {
    /// The footing a judged answer went through; none when it needed no proof
    /// or did not get it.
    pub fn footing_of(&self, proof: &Proof) -> Option<Footing> {
        match proof {
            Proof::Given => Some(Footing::Code),
            Proof::GivenEarlier { at_ms, .. } => {
                Some(Footing::Earlier(span(self.now().saturating_sub(*at_ms))))
            }
            _ => None,
        }
    }

    /// Keep `question`'s footing until its echo is built.
    pub(crate) fn keep_footing(&self, question: &str, footing: Footing) {
        let mut v = held(&self.footings);
        v.retain(|(q, _)| q != question);
        v.push((question.to_string(), footing));
        if v.len() > KEPT {
            v.remove(0);
        }
    }

    /// `question`'s footing, if one was kept, left in place.
    pub(crate) fn footing_kept(&self, question: &str) -> Option<Footing> {
        held(&self.footings)
            .iter()
            .find(|(q, _)| q == question)
            .map(|(_, f)| f.clone())
    }

    /// `question`'s footing, taken.
    pub(crate) fn take_footing(&self, question: &str) -> Option<Footing> {
        let mut v = held(&self.footings);
        let at = v.iter().position(|(q, _)| q == question)?;
        Some(v.remove(at).1)
    }
}
