//! Who may propose a change: the owner, in a private place only they write
//! in, in their own conversation (issue 22). Never a job, a task, the self
//! loop, or a session an MCP client opened.
//!
//! The walls are the ones `self.file` and `card.observed` keep, and one more:
//! the execution's principal must be the operator's. An MCP client's session
//! has the principal `mcp`, the self loop's `self`, and a task's session is
//! refused by its own record; none is the owner at their own keyboard.

use crate::places::PlaceClass;
use crate::toolrun::TurnCtx;

pub const NOT_PRIVATE: &str =
    "config.propose changes Theseus's own settings, so it is only for the owner's own private \
     place: it is not offered in a shared one";
pub const NOT_ALONE: &str =
    "config.propose is for a place only the owner writes in: this one is shared with others";
pub const NOT_A_TASK: &str =
    "config.propose is not for a task's session: a task cannot change Theseus's settings; report \
     what is needed and the owner asks for it";
pub const NOT_THE_OWNER: &str =
    "config.propose is only for the owner's own conversation: this session belongs to a job, the \
     self-improvement loop or a connected client, none of which changes Theseus's settings";

/// Why this call may not propose, or `None` when it may. The facts are read
/// from the turn and judged by [`judged`], so a test reaches every clause.
pub fn refusal(tc: &TurnCtx<'_>) -> Option<&'static str> {
    // A session record that cannot be read is read as a task's.
    let task = tc.task.is_some()
        || match tc
            .store
            .get_session::<crate::session::SessionRecord>(tc.session_id)
        {
            Ok(Some(s)) => s.task.is_some(),
            _ => true,
        };
    let place = tc.outbox.try_target(tc.session_id).ok().flatten();
    let principal = tc
        .kernel
        .execution(tc.execution_id)
        .ok()
        .flatten()
        .map(|e| e.authority.principal);
    judged(Facts {
        class: tc.class,
        task,
        place: place.as_deref(),
        principal: principal.as_deref(),
    })
}

/// What the wall reads.
pub struct Facts<'a> {
    pub class: PlaceClass,
    pub task: bool,
    pub place: Option<&'a str>,
    pub principal: Option<&'a str>,
}

/// The wall, clause by clause.
pub fn judged(f: Facts<'_>) -> Option<&'static str> {
    if f.class != PlaceClass::Private {
        return Some(NOT_PRIVATE);
    }
    if f.task {
        return Some(NOT_A_TASK);
    }
    if !crate::capture::owner_alone(f.class, f.place) {
        return Some(NOT_ALONE);
    }
    match f.principal == Some(crate::turn::OPERATOR) {
        true => None,
        false => Some(NOT_THE_OWNER),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts<'a>() -> Facts<'a> {
        Facts {
            class: PlaceClass::Private,
            task: false,
            place: Some("discord:dm:42"),
            principal: Some("operator"),
        }
    }

    #[test]
    fn only_the_owner_alone_in_their_own_session_passes() {
        assert_eq!(judged(facts()), None);
        assert_eq!(
            judged(Facts {
                place: None,
                ..facts()
            }),
            None,
            "the CLI or the web UI"
        );
    }

    #[test]
    fn every_other_caller_is_refused_in_its_own_words() {
        let shared = Facts {
            class: PlaceClass::Shared,
            ..facts()
        };
        assert_eq!(judged(shared), Some(NOT_PRIVATE));
        let task = Facts {
            task: true,
            ..facts()
        };
        assert_eq!(judged(task), Some(NOT_A_TASK));
        let guild = Facts {
            place: Some("discord:channel:7"),
            ..facts()
        };
        assert_eq!(judged(guild), Some(NOT_ALONE));
        for who in ["mcp", "self", "routine", "job", ""] {
            let f = Facts {
                principal: Some(who),
                ..facts()
            };
            assert_eq!(judged(f), Some(NOT_THE_OWNER), "{who}");
        }
        assert_eq!(
            judged(Facts {
                principal: None,
                ..facts()
            }),
            Some(NOT_THE_OWNER)
        );
    }
}
