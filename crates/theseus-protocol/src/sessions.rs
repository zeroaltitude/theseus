//! A session's state (theseus-emqx): live, quiet, or retired, derived when
//! read from what its record holds, never stored as a state. The rule is
//! here, pure, so the core and every client derive the same state from the
//! same fields: the core for `session.list`, and the cockpit's time machine
//! (`cockpit/src/lib/sessionState.ts` mirrors `derive`) for a past moment,
//! with the window `session.list` names.
//!
//! - **Live**: a turn within the window (`live_window_ms`, 24 hours by
//!   default), or busy: its execution runs or is queued, or it waits on the
//!   owner (a question, or attention that needs you), whatever its age, so
//!   nothing that needs the owner is ever filtered out of the default view.
//! - **Quiet**: not retired, and no turn within the window.
//! - **Retired**: superseded (a place's binding moved to a newer session),
//!   retired by hand (`session.retire`), or empty (no turn, past the grace
//!   after it opened). Nothing is deleted: a retired session keeps its
//!   nodes, memory and index entries, and `session.reopen` clears it.

use serde::{Deserialize, Serialize};

/// What a session is now (theseus-emqx).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    Live,
    Quiet,
    Retired,
}

impl SessionState {
    pub fn as_str(self) -> &'static str {
        match self {
            SessionState::Live => "live",
            SessionState::Quiet => "quiet",
            SessionState::Retired => "retired",
        }
    }
}

/// Why a session is retired.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum RetiredReason {
    /// Its place's binding moved to a newer session (`superseded_by`).
    Superseded,
    /// It never took a turn, and its grace after opening has passed.
    Empty,
    /// The owner retired it (`session.retire`).
    ByHand,
}

impl RetiredReason {
    /// The one wording every surface shows.
    pub fn words(self) -> &'static str {
        match self {
            RetiredReason::Superseded => "superseded",
            RetiredReason::Empty => "empty",
            RetiredReason::ByHand => "by hand",
        }
    }
}

/// A retirement: why, and when. Stored for `superseded` and `by_hand`;
/// derived for `empty` (its time the end of the grace).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SessionRetired {
    pub reason: RetiredReason,
    pub at_ms: u64,
}

/// One end of a supersession: the other session, and when the place moved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SessionLink {
    pub session_id: String,
    pub at_ms: u64,
    /// The place whose binding moved (`dm:<user>`, `channel:<id>`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub place: Option<String>,
}

/// The windows the state is derived with, as `session.list` names them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateRule {
    /// A turn within this long reads live.
    pub live_window_ms: u64,
    /// A session with no turn reads retired (empty) this long after it opened.
    pub empty_grace_ms: u64,
}

impl Default for StateRule {
    fn default() -> Self {
        Self {
            live_window_ms: 24 * 3_600_000,
            empty_grace_ms: 3_600_000,
        }
    }
}

/// What the rule reads of a session.
#[derive(Debug, Clone, Copy)]
pub struct StateOf<'a> {
    /// A stored retirement (superseded or by hand), if any.
    pub retired: Option<&'a SessionRetired>,
    pub turns: u64,
    pub created_ms: u64,
    pub last_active_ms: u64,
    /// When the owner last reopened it: activity, and no empty retirement.
    pub reopened_ms: Option<u64>,
    /// Its execution runs or is queued, or it waits on the owner.
    pub busy: bool,
}

/// The state at `now_ms`, and the retirement that makes it retired (or, for
/// a busy session, the stored one it still carries).
pub fn derive(
    s: StateOf<'_>,
    rule: StateRule,
    now_ms: u64,
) -> (SessionState, Option<SessionRetired>) {
    let retired = s.retired.cloned().or_else(|| {
        let empty_at = s.created_ms.saturating_add(rule.empty_grace_ms);
        (s.turns == 0 && s.reopened_ms.is_none() && now_ms >= empty_at).then_some(SessionRetired {
            reason: RetiredReason::Empty,
            at_ms: empty_at,
        })
    });
    if s.busy {
        return (SessionState::Live, s.retired.cloned());
    }
    if retired.is_some() {
        return (SessionState::Retired, retired);
    }
    let active = s
        .last_active_ms
        .max(s.created_ms)
        .max(s.reopened_ms.unwrap_or(0));
    if now_ms.saturating_sub(active) < rule.live_window_ms {
        (SessionState::Live, None)
    } else {
        (SessionState::Quiet, None)
    }
}

/// `session.retire`: the owner retires a session by hand. Nothing is
/// deleted, and `session.reopen` undoes it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SessionRetireParams {
    pub session_id: String,
}

/// `session.reopen`: clears a retirement (by hand, superseded, or empty).
/// A superseded session keeps both links as history, and its place stays
/// on its successor.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SessionReopenParams {
    pub session_id: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: u64 = 3_600_000;

    fn of(turns: u64, created: u64, active: u64) -> StateOf<'static> {
        StateOf {
            retired: None,
            turns,
            created_ms: created,
            last_active_ms: active,
            reopened_ms: None,
            busy: false,
        }
    }

    #[test]
    fn the_window_has_two_edges() {
        let rule = StateRule::default();
        let now = 100 * H;
        let just_inside = of(3, 0, now - 24 * H + 1);
        let just_outside = of(3, 0, now - 24 * H);
        assert_eq!(derive(just_inside, rule, now).0, SessionState::Live);
        assert_eq!(derive(just_outside, rule, now).0, SessionState::Quiet);
    }

    #[test]
    fn an_empty_session_has_its_grace_then_reads_retired() {
        let rule = StateRule::default();
        let created = 10 * H;
        let s = of(0, created, created);
        assert_eq!(derive(s, rule, created + H - 1).0, SessionState::Live);
        let (state, why) = derive(s, rule, created + H);
        assert_eq!(state, SessionState::Retired);
        assert_eq!(
            why,
            Some(SessionRetired {
                reason: RetiredReason::Empty,
                at_ms: created + H
            })
        );
        // Reopened, it is no longer empty-retired.
        let reopened = StateOf {
            reopened_ms: Some(created + 2 * H),
            ..s
        };
        assert_eq!(
            derive(reopened, rule, created + 3 * H).0,
            SessionState::Live
        );
    }

    #[test]
    fn a_busy_session_reads_live_whatever_its_age_or_retirement() {
        let rule = StateRule::default();
        let by_hand = SessionRetired {
            reason: RetiredReason::ByHand,
            at_ms: 5,
        };
        let s = StateOf {
            retired: Some(&by_hand),
            busy: true,
            ..of(4, 0, 0)
        };
        let (state, why) = derive(s, rule, 1000 * H);
        assert_eq!(state, SessionState::Live);
        assert_eq!(
            why,
            Some(by_hand.clone()),
            "it still carries why it was retired"
        );
        let old_and_waiting = StateOf {
            busy: true,
            ..of(0, 0, 0)
        };
        assert_eq!(
            derive(old_and_waiting, rule, 1000 * H),
            (SessionState::Live, None)
        );
        let stored = StateOf {
            retired: Some(&by_hand),
            ..of(4, 0, 1000 * H)
        };
        assert_eq!(derive(stored, rule, 1000 * H).0, SessionState::Retired);
    }
}
