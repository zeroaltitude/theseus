//! route.v7's `personal` question (theseus-gasov): does the message try to
//! draw out the operator's personal details, or to open a conversation with
//! other people in which they could come out? The owner's personal details
//! live in the context by design (the owner, 2026-10-18); this answer warns
//! the owner (`turn/personal_step.rs`) and, in a shared place, holds the reply
//! (theseus-i1z13); where only the owner writes, it blocks nothing. A live probe of 14
//! made-up messages (2026-10-19) read two reaches at the act band and a
//! guest asking what the assistant had been told about the owner's family at
//! confirm (0.85), so a shared place warns from the confirm band.

use theseus_judge::band::Band;

/// The question, by its id in the pack.
pub const QUESTION: &str = "personal";

/// The answer that asks for nothing.
pub const NONE: &str = "none";

/// Jev's answer: `none`, `elicit` or `expose`, how sure, and its band.
#[derive(Debug, Clone, PartialEq)]
pub struct PersonalAnswer {
    pub choice: String,
    pub confidence: f64,
    pub band: Band,
}

impl PersonalAnswer {
    /// A warning is owed: a reach for the operator's details at the act band,
    /// or in a shared place, where others can ask, at the confirm band too. In
    /// a place only the owner writes in it should almost never come (the
    /// owner, 2026-10-18), so only a sure answer warns there.
    pub fn warns(&self, shared: bool) -> bool {
        self.choice != NONE
            && match self.band {
                Band::Act => true,
                Band::Confirm => shared,
                Band::Escalate => false,
            }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sure reach warns anywhere; one at the confirm band only in a shared
    /// place; `none` and the escalate band never.
    #[test]
    fn a_reach_warns_by_its_band_and_the_place() {
        let a = |choice: &str, band| PersonalAnswer {
            choice: choice.into(),
            confidence: 0.85,
            band,
        };
        for shared in [false, true] {
            assert!(a("elicit", Band::Act).warns(shared));
            assert!(a("expose", Band::Act).warns(shared));
            assert!(!a("none", Band::Act).warns(shared));
            assert!(!a("elicit", Band::Escalate).warns(shared));
        }
        assert!(a("elicit", Band::Confirm).warns(true));
        assert!(!a("elicit", Band::Confirm).warns(false));
    }
}
