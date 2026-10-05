//! What happened to a node, and the review it is (design M6 §2.7's table).
//!
//! The events come from the record: `memory.used` rows (what a recall admitted,
//! whether the reply used it, and how that went), the operator's labels, and
//! the gate's first sight of a node. Only use is a review: a node shown and not
//! used earns nothing, so exposure never raises retention (theseus-3nk).

use crate::fsrs::Grade;

/// One event in a node's history, at its wall-clock time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AccessEvent {
    /// When it happened, in ms since the epoch.
    pub at_ms: u64,
    pub access: Access,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    /// A recall admitted the node, and the reply did not use it.
    Shown,
    /// A recall admitted the node, and the reply used it.
    Used(Outcome),
    /// The operator labeled the node.
    Labeled(Label),
    /// The gate stored the node: its first sight, by its durability.
    FirstSight(Durability),
}

/// How a use went (§2.6's attribution).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The exchange went on, or the task succeeded.
    Ok,
    /// Neither went on nor was corrected.
    Unknown,
    /// The operator's next message corrected what it said.
    Corrected,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Label {
    Useful,
    /// The operator says recall should have offered it: the same vouching
    /// as `Useful`, for a node recall missed.
    ShouldHave,
    /// Wrong: recall also excludes it (the `labeled_wrong` filter).
    Wrong,
    /// Stale: recall also excludes it (the `labeled_wrong` filter).
    Stale,
}

/// A node's durability, the labeler's (§2.6), by the kinds that earn it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Durability {
    /// An operator's preference or decision.
    High,
    /// A fact or a procedure.
    Medium,
    /// An episode, such as a tool result.
    Low,
    /// A transient, such as a short acknowledgement.
    Floor,
}

impl Access {
    /// The review this event is, by §2.7's table; `None` when it is none.
    pub fn grade(self) -> Option<Grade> {
        match self {
            Access::Shown => None,
            Access::Used(Outcome::Ok) => Some(Grade::Good),
            Access::Used(Outcome::Unknown) => Some(Grade::Hard),
            Access::Used(Outcome::Corrected) | Access::Labeled(Label::Wrong | Label::Stale) => {
                Some(Grade::Again)
            }
            Access::Labeled(Label::Useful | Label::ShouldHave) => Some(Grade::Easy),
            Access::FirstSight(Durability::High) => Some(Grade::Easy),
            Access::FirstSight(Durability::Medium) => Some(Grade::Good),
            Access::FirstSight(Durability::Low) => Some(Grade::Hard),
            Access::FirstSight(Durability::Floor) => Some(Grade::Again),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// §2.7's table, row by row.
    #[test]
    fn grades_follow_the_design_table() {
        let table = [
            (Access::Shown, None),
            (Access::Used(Outcome::Ok), Some(Grade::Good)),
            (Access::Used(Outcome::Unknown), Some(Grade::Hard)),
            (Access::Used(Outcome::Corrected), Some(Grade::Again)),
            (Access::Labeled(Label::Wrong), Some(Grade::Again)),
            (Access::Labeled(Label::Stale), Some(Grade::Again)),
            (Access::Labeled(Label::Useful), Some(Grade::Easy)),
            (Access::Labeled(Label::ShouldHave), Some(Grade::Easy)),
            (Access::FirstSight(Durability::High), Some(Grade::Easy)),
            (Access::FirstSight(Durability::Medium), Some(Grade::Good)),
            (Access::FirstSight(Durability::Low), Some(Grade::Hard)),
            (Access::FirstSight(Durability::Floor), Some(Grade::Again)),
        ];
        for (access, grade) in table {
            assert_eq!(access.grade(), grade, "{access:?}");
        }
    }
}
