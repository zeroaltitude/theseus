//! What a card may honestly say about a question's wait (theseus-lrc3m). Since
//! theseus-v2dnc a call's question is held past its life, never expired: the
//! `expires_at_ms` of its live `confirm.requested` is the time of its first
//! notice, not a deadline (the Discord card is drawn from the same request).
//! So a card decides "held" by the question's kind, never by that field, and
//! says how long it has waited instead of a deadline that has not passed. The
//! cockpit's `heldquestion.ts` does the same.

use theseus_protocol::work::ask::held_tool;
use theseus_protocol::ConfirmRequest;

/// Whether the question is held until it is answered: a call's, not a budget
/// question (which says so itself) and not one that expires.
pub fn is_held(c: &ConfirmRequest) -> bool {
    c.budget.is_none() && held_tool(&c.tool)
}

/// The deadline a card may show, in epoch ms: none (0) for a held question,
/// whatever `expires_at_ms` says.
pub fn deadline_of(c: &ConfirmRequest) -> u64 {
    if is_held(c) {
        0
    } else {
        c.expires_at_ms
    }
}

/// How long a held question has waited, as the card words it: `waiting 30 s`,
/// `waiting 12 min`, `waiting 3 h`. None for a question that is not held,
/// which counts down to its deadline instead.
pub fn waited_label(c: &ConfirmRequest, now_ms: u64) -> Option<String> {
    is_held(c).then(|| {
        let s = now_ms.saturating_sub(c.requested_at_ms) / 1000;
        match s {
            0..=59 => format!("waiting {s} s"),
            60..=3599 => format!("waiting {} min", s / 60),
            _ => format!("waiting {} h", s / 3600),
        }
    })
}

/// The next time a held question's label changes, in epoch ms: each second
/// for its first minute, then each minute, then each hour. None for a
/// question that is not held.
pub fn next_change(c: &ConfirmRequest, now_ms: u64) -> Option<u64> {
    is_held(c).then(|| {
        let waited = now_ms.saturating_sub(c.requested_at_ms);
        let unit = match waited / 1000 {
            0..=59 => 1_000,
            60..=3599 => 60_000,
            _ => 3_600_000,
        };
        c.requested_at_ms + (waited / unit + 1) * unit
    })
}
