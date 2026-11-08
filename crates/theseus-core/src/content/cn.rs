//! The restricted-origin predicate: which content labels keep a session off a
//! model of restricted origin (`cn` or `unknown`). The owner's data rule
//! (2026-10-17) says such a model must not be sent certain content; this file
//! is the sentence that decides which content that is.
//!
//! **It is a file of its own, and in `scripts/keel-guard.py`'s `KEEL_FILES`,
//! because this is where the rule is decided, not merely read.** The provider
//! chokepoint (`data_gate.rs`) and the turn's floor (`turn/data_step.rs`) are
//! both keel-held, and both spend their whole decision here:
//! `data_gate.rs`'s refusal is `ContentFlags::disqualifies_cn`'s answer and
//! nothing else — a `None` from [`first`] returns the chokepoint early, before
//! any refusal is built. So a one-word edit here switches both chokepoints off
//! while leaving every guarded file untouched, which is the asymmetry the two
//! files were guarded to close.
//!
//! `tests_content_gate.rs`'s `the_chokepoints_refusal_is_the_cn_predicates_own_set`
//! holds that: it reads the set the chokepoint actually refuses on back out of
//! the chokepoint and compares it with [`disqualifies`]'s own, counting the
//! refusals first so a permissive predicate reds it rather than satisfying it
//! with an empty set.

use super::{ContentFlags, Flag};

/// Whether a label of `flag` keeps the session off a model of restricted
/// origin.
///
/// Written out flag by flag rather than as "everything but `injection`": the
/// rule is the list, and a flag added to [`Flag`] should stop the compiler
/// here and make its author answer this question, not inherit an answer.
/// `injection` is the one that does not disqualify — it freezes the session's
/// calls that act, which is a different rule with a different remedy.
pub(super) fn disqualifies(flag: Flag) -> bool {
    match flag {
        Flag::Secret | Flag::Confidential | Flag::Personal => true,
        Flag::Injection => false,
    }
}

/// The first of `flags`'s labels that keeps the session off such a model, in
/// [`Flag::ALL`]'s order so the word a refusal names is stable. `None` means
/// nothing found in the session says otherwise, and the request goes out.
pub(super) fn first(flags: &ContentFlags) -> Option<Flag> {
    Flag::ALL
        .into_iter()
        .find(|f| disqualifies(*f) && flags.has(*f))
}
