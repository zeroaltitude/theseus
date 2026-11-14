//! How a session's previous turn ran (theseus-8euum): the effort it ran at
//! and whether it failed, read from the session's own ledger rows. Nothing is
//! stored for it: the turn's `route.decided` row says its level, and a
//! `turn.failed` row with the same turn says it ended in an error. A turn the
//! person stopped wrote no `turn.failed`, so it is not one that failed.
//!
//! The effort question is told both (`RouteFacts::previous_turn`, which `effort.v1`'s state carries): the
//! owner's ruling is to tell the small model and let it choose (2026-11-14 13:54), so nothing here changes
//! its answer.

use theseus_judge::builders::PreviousTurn;
use theseus_store::pages::ledger_kind_session;
use theseus_store::{kinds, Page, Store as _};

use super::{self as effort};
use crate::config::Effort;
use crate::ledger::LedgerRow;

/// How the session's previous turn ran.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Previous {
    /// The level it ran at.
    pub level: Effort,
    /// It ended in an error (`turn.failed`) that is a failed attempt: see [`is_attempt`].
    pub failed: bool,
}

/// Whether a `turn.failed` row of `class` says the attempt failed, the thing a higher effort might
/// address. A rate limit, an overload, a timeout and a cancel count too: the class tells them
/// apart, but the owner ruled only on the limits and the window, so they are kept. These do not
/// count: a refusal by a money limit or the day's ceiling, by the context window, a secret that did
/// not resolve, a credential the provider refused, a turn the stop ended or one that stopped for a
/// fault of Theseus's own: more effort fixes none of them, and costs more. Any other class counts,
/// and so does a row from before the row carried its class (it says nothing, so it is not told apart).
pub fn is_attempt(class: Option<&str>) -> bool {
    const NOT: &[&str] = &[
        "over_limit",
        "spend_hard_stop",
        "daily_ceiling",
        "daily_runaway",
        "self_budget",
        "unpriced",
        "context_window",
        "context_overage",
        "context_unadmitted",
        "secret_failed",
        "secret_resolving",
        "auth",
        "stopped",
        "stopping",
        "kernel",
        "internal",
        "data_gate",
    ];
    !class.is_some_and(|c| NOT.contains(&c))
}

/// The newest row of `kind` in `session`, none while the index is being built.
fn newest(store: &crate::store::Store, kind: &str, session: &str) -> Option<LedgerRow> {
    let page = Page {
        kind: kinds::LEDGER,
        tags: vec![ledger_kind_session(kind, session)],
        limit: 1,
        ..Page::default()
    };
    let got = store.inner().page(&page).ok()??;
    got.records.last()?.decode().ok()
}

/// The previous turn of `session`: the level its newest `route.decided` row
/// says it ran at (the level sent when the sticky rule held one, else the
/// level it ran), and whether that turn wrote `turn.failed`. None when the
/// session has no such row, or the level is not one.
pub fn previous_turn(store: &crate::store::Store, session: &str) -> Option<Previous> {
    let decided = newest(store, "route.decided", session)?;
    let level = ["effort_sent", "effort_ran"]
        .iter()
        .find_map(|k| decided.data.get(*k)?.as_str())
        .and_then(effort::parse)?;
    let failed = newest(store, "turn.failed", session).is_some_and(|f| {
        f.turn_id.is_some()
            && f.turn_id == decided.turn_id
            && is_attempt(f.data.get("class").and_then(|c| c.as_str()))
    });
    Some(Previous { level, failed })
}

/// What the route is told of it.
pub fn told(p: Previous) -> PreviousTurn {
    PreviousTurn {
        effort: effort::name(p.level).to_string(),
        failed: p.failed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A failure counts unless one of the named classes: the money limits, the window,
    /// secrets, a refused credential, a stop and Theseus's own faults do not; a provider's error and
    /// a row from before the row had a class do.
    #[test]
    fn only_a_failed_attempt_counts() {
        for c in [
            "over_limit",
            "spend_hard_stop",
            "daily_ceiling",
            "context_window",
            "secret_failed",
            "auth",
            "stopped",
            "internal",
        ] {
            assert!(!is_attempt(Some(c)), "{c}");
        }
        for c in ["invalid_request", "server", "timeout", "refusal"] {
            assert!(is_attempt(Some(c)), "{c}");
        }
        assert!(is_attempt(None), "a row with no class says nothing");
    }
}
