//! Why a turn no message opened began (issue 44): a background job ended, a result the conversation was
//! waiting for arrived, or the turn was resumed after an interruption. The view assembles such a turn as any
//! other, with an empty "now" message, and says the occasion in one harness line at the top of the block
//! beside the message. The occasion is told by the turn's catch-up (`turn::catch_up`), never read from text.

use crate::compiler::view::Frozen;
use crate::node::{Body, Node};
use crate::toolrun::LateCall;

/// How every occasion's line begins: the harness's words, never the person's (`task_graph::view::is_head`
/// lists it, so a block that leads with the line still reads as the harness's).
pub const HEAD: &str = "No new message opened this turn:";

/// What woke a turn no person's message, wake or report opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::turn) enum Occasion {
    /// A background job's result was taken as a late result.
    JobEnded,
    /// A call the conversation was waiting on was answered (a confirm, a spend question, a resumed call).
    Answered,
    /// Nothing new was written: the turn finishes work an earlier turn left unanswered (a restart, a
    /// failed attempt).
    Resumed,
}

impl Occasion {
    /// The occasion of a turn, from what its catch-up took: none when a message, a wake or a report of its
    /// own opened it.
    pub(in crate::turn) fn of(opened: bool, late: &[LateCall], answered: bool) -> Option<Self> {
        match () {
            _ if opened => None,
            _ if late.iter().any(|l| !l.delivered) => Some(Self::JobEnded),
            _ if answered => Some(Self::Answered),
            _ => Some(Self::Resumed),
        }
    }

    /// The one line a person's model reads about it.
    pub(in crate::turn) fn line(self) -> &'static str {
        match self {
            Self::JobEnded => {
                "No new message opened this turn: a background job this conversation started has ended, \
                 and its result follows."
            }
            Self::Answered => {
                "No new message opened this turn: a call this conversation was waiting on has been \
                 answered, and its result follows."
            }
            Self::Resumed => {
                "No new message opened this turn: it resumes work an earlier turn left unfinished \
                 (Theseus restarted, or an attempt failed). Carry on from where it stopped."
            }
        }
    }
}

/// The block beside the message with the occasion's line first; the line alone when nothing else is beside.
pub(super) fn beside_with(
    beside: Option<Node>,
    occasion: Occasion,
    (sid, turn): (&str, &str),
) -> Option<Node> {
    let line = occasion.line();
    Some(match beside {
        Some(mut n) => {
            if let Body::UserMessage { text, .. } = &mut n.body {
                *text = format!("{line}\n\n{text}");
            }
            n
        }
        None => Frozen::block(sid, turn, "", line),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn late(delivered: bool) -> LateCall {
        LateCall {
            tool_use_id: "t1".into(),
            wire: "proc_run".into(),
            status: crate::node::ResultStatus::Ok,
            run_ms: None,
            delivered,
        }
    }

    #[test]
    fn every_line_begins_with_the_head_the_harness_readers_know() {
        for o in [Occasion::JobEnded, Occasion::Answered, Occasion::Resumed] {
            assert!(o.line().starts_with(HEAD), "{o:?}");
            assert!(crate::task_graph::view::is_head(o.line()), "{o:?}");
        }
    }

    #[test]
    fn a_turn_a_message_opened_has_no_occasion() {
        assert_eq!(Occasion::of(true, &[late(false)], true), None);
    }

    #[test]
    fn a_jobs_end_comes_before_an_answer_and_nothing_new_is_a_resume() {
        assert_eq!(
            Occasion::of(false, &[late(false)], true),
            Some(Occasion::JobEnded)
        );
        assert_eq!(
            Occasion::of(false, &[late(true)], true),
            Some(Occasion::Answered)
        );
        assert_eq!(Occasion::of(false, &[], false), Some(Occasion::Resumed));
    }

    #[test]
    fn the_line_leads_the_block_beside_the_message() {
        let n = beside_with(None, Occasion::Resumed, ("s", "t")).unwrap();
        assert_eq!(n.turn_id.as_deref(), Some("t"));
        let b = Frozen::block("s", "t", "beside", "in play: x");
        let n = beside_with(Some(b), Occasion::JobEnded, ("s", "t")).unwrap();
        let Body::UserMessage { text, .. } = &n.body else {
            panic!("a user message")
        };
        assert!(text.starts_with(Occasion::JobEnded.line()), "{text}");
        assert!(text.ends_with("in play: x"), "{text}");
    }
}
