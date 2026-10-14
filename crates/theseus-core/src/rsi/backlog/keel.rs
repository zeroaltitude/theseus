//! The keel mark: an item whose likely fix touches a class the owner keeps
//! for their own yes is filed with the class named, and never starts. It
//! waits (`waiting_human`), and the rest of the queue goes on. The classes
//! (the owner's decision of 2026-10-09): tests, budgets or ceilings loosened;
//! sealed holdout sets; one-way store format bumps; security, policy,
//! secret, permission and IAM changes; the spend limits; the kill switch.
//!
//! The mark is read from the cause's key, a table of prefixes and words, by
//! code that guesses nothing about what a fix would be: a cause the table
//! does not name is unmarked, and the keel guard (the gate's first phase)
//! still fails any branch that loosens a test, budget or ceiling.

/// The class that holds a cause for the owner, if any.
pub fn class_of(key: &str) -> Option<&'static str> {
    const PREFIXES: &[(&str, &str)] = &[
        ("approval.refused", "security"),
        ("secrets.", "secrets"),
        ("turn.refused", "secrets"),
        ("loop.cap_reached", "spend limits"),
        ("budget.", "spend limits"),
        ("spend.", "spend limits"),
        ("graduate.ext.net:", "security"),
        ("store.", "store format"),
        ("ledger.", "store format"),
    ];
    const WORDS: &[(&str, &str)] = &[
        ("secret", "secrets"),
        ("credential", "secrets"),
        ("iam", "security"),
        ("permission", "security"),
        ("policy", "security"),
        ("egress", "security"),
        ("holdout", "sealed sets"),
        ("sealed", "sealed sets"),
        ("kill_switch", "kill switch"),
        ("format", "store format"),
        ("ceiling", "budgets and ceilings"),
    ];
    PREFIXES
        .iter()
        .find(|(p, _)| key.starts_with(p))
        .or_else(|| WORDS.iter().find(|(w, _)| key.contains(w)))
        .map(|(_, c)| *c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cause_in_a_keel_class_is_marked_and_another_is_not() {
        assert_eq!(class_of("approval.refused:self.resume"), Some("security"));
        assert_eq!(class_of("budget.over_limit:fable"), Some("spend limits"));
        assert_eq!(class_of("health:disk"), None);
        assert_eq!(class_of("turn.failed:timeout"), None);
        assert_eq!(class_of("health:mcp:iam_tool"), Some("security"));
        assert_eq!(class_of("graduate.ext.net:wc:3f2a1c"), Some("security"));
        assert_eq!(class_of("graduate.ext:wc:3f2a1c"), None);
    }
}
