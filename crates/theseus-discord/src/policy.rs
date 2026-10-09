//! Which of the binding's messages notify (theseus-l1y1). Every create
//! notifies by default, as it always has: the owner wants a lively chat. The
//! kinds `[discord] silent` names post silent instead, with Discord's
//! `SUPPRESS_NOTIFICATIONS` flag: they post, and no device notifies, mentions
//! included. An edit never notifies, so only a create carries the flag.
//!
//! The table maps each write to the category `[discord] silent` names it by
//! (`theseus_core::config::discord::Category`). It is to be replaced by
//! `theseus_protocol::notices`, the shared notification policy (task
//! management's work-types row, theseus-753z), once both have joined: it is
//! data, so the replacement maps each `Event` to the shared policy's kinds.

use serde_json::Value;
use theseus_core::config::discord::Category;

/// Discord's `SUPPRESS_NOTIFICATIONS` message flag (1 << 12).
pub(crate) const SUPPRESS_NOTIFICATIONS: u64 = 1 << 12;

/// How a loop's thinking message's key ends: `<turn>:L<loop>:think`.
pub(crate) const THINK_SUFFIX: &str = ":think";

/// What a write is, as the table reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Event {
    /// A card: an approval, a budget's question, a layer-1 change.
    Card,
    /// The note beside a card sent to the owner's DM, or a card's only note.
    CardNote,
    /// A card whose question closed before it was written: written once,
    /// settled.
    CardClosed,
    /// A turn that failed (a `failed` post).
    TurnFailed,
    /// A task that failed (its report, any outcome but `complete` or
    /// `cancelled`).
    TaskFailed,
    /// A task that finished, or was cancelled (its report).
    TaskEnded,
    /// The first part of the reply to the owner's own message.
    Answer,
    /// A reply's later parts, its footer, and any part of a reply to someone
    /// else's message.
    ReplyPart,
    /// A turn a wake or a task's report started: its reply.
    Woken,
    /// A loop's tool line, and a notified call's embed.
    ToolLine,
    /// A loop's thinking message.
    Thinking,
    /// A notice: a bind, a publish, a budget's or the hours' line, a proposal.
    Note,
    /// The restart notice.
    Restarted,
    /// An MCP server's tools or prompt changed.
    Mcp,
    /// A Jev notice, its pause, and its label.
    Jev,
    /// A glide's post: another session's words.
    Glide,
    /// A hands group's line.
    Hands,
    /// Free space under the state dir fell below the floor.
    DiskCritical,
    /// Free space is low, or back.
    Disk,
    /// The task board: a task starts, moves, or finishes.
    Board,
}

/// The table: each write's category, the name `[discord] silent` gives it.
pub(crate) const TABLE: &[(Event, Category)] = &[
    (Event::Card, Category::Cards),
    (Event::CardNote, Category::Cards),
    (Event::CardClosed, Category::Cards),
    (Event::TurnFailed, Category::Failures),
    (Event::TaskFailed, Category::Failures),
    (Event::TaskEnded, Category::Tasks),
    (Event::Answer, Category::Answer),
    (Event::ReplyPart, Category::Replies),
    (Event::Woken, Category::Woken),
    (Event::ToolLine, Category::Tools),
    (Event::Thinking, Category::Thinking),
    (Event::Note, Category::Notes),
    (Event::Restarted, Category::Notes),
    (Event::Mcp, Category::Notes),
    (Event::Jev, Category::Notes),
    (Event::Glide, Category::Notes),
    (Event::Hands, Category::Notes),
    (Event::DiskCritical, Category::Disk),
    (Event::Disk, Category::Disk),
    (Event::Board, Category::Tasks),
];

/// Whether `e` pings, given the categories `[discord] silent` names: yes
/// unless its category is one of them. An event the table does not name
/// pings, as every write did before the table.
pub(crate) fn pings(e: Event, silent: &[Category]) -> bool {
    TABLE
        .iter()
        .find(|(t, _)| *t == e)
        .is_none_or(|(_, c)| !silent.contains(c))
}

/// A task's report, by its outcome.
pub(crate) fn of_report(body: &Value) -> Event {
    match body["outcome"].as_str() {
        Some("complete" | "cancelled") => Event::TaskEnded,
        _ => Event::TaskFailed,
    }
}

/// A disk crossing, by the state it crossed into.
pub(crate) fn of_disk(body: &Value) -> Event {
    match body["state"].as_str() {
        Some("below_floor") => Event::DiskCritical,
        _ => Event::Disk,
    }
}

/// A reply's post: the turn a wake or a report started, else a reply.
pub(crate) fn of_reply(body: &Value) -> Event {
    let woken = ["wakes", "reports"]
        .iter()
        .any(|k| body[*k].as_array().is_some_and(|a| !a.is_empty()));
    if woken {
        Event::Woken
    } else {
        Event::Answer
    }
}

/// A live message, by its key: a turn's text part (`<turn>:L<loop>:p<part>`,
/// the stream's and the reply's, the answer's when `owed`), its thinking
/// (`…:think`), or its tool line and anything else live (a notice embed).
pub(crate) fn of_live(key: &str, owed: bool) -> Event {
    if is_text_part(key) {
        if owed {
            Event::Answer
        } else {
            Event::ReplyPart
        }
    } else if key.ends_with(THINK_SUFFIX) {
        Event::Thinking
    } else {
        Event::ToolLine
    }
}

/// Is `key` a turn's text part (`<turn>:L<loop>:p<part>`), the stream's and
/// the reply's, as against its tool line, footer, or notice card?
pub(crate) fn is_text_part(key: &str) -> bool {
    let mut it = key.rsplit(':');
    let (Some(part), Some(lp)) = (it.next(), it.next()) else {
        return false;
    };
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    part.strip_prefix('p').is_some_and(digits) && lp.strip_prefix('L').is_some_and(digits)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const EVERY: [Event; 20] = [
        Event::Card,
        Event::CardNote,
        Event::CardClosed,
        Event::TurnFailed,
        Event::TaskFailed,
        Event::TaskEnded,
        Event::Answer,
        Event::ReplyPart,
        Event::Woken,
        Event::ToolLine,
        Event::Thinking,
        Event::Note,
        Event::Restarted,
        Event::Mcp,
        Event::Jev,
        Event::Glide,
        Event::Hands,
        Event::DiskCritical,
        Event::Disk,
        Event::Board,
    ];

    /// With nothing named silent, every write pings, as before the table;
    /// the table names every event once, and every category has a write.
    #[test]
    fn with_nothing_silent_every_write_pings() {
        for e in EVERY {
            assert!(pings(e, &[]), "{e:?} pings by default");
            assert_eq!(TABLE.iter().filter(|(t, _)| *t == e).count(), 1, "{e:?}");
        }
        assert_eq!(TABLE.len(), EVERY.len());
        for c in Category::ALL {
            assert!(TABLE.iter().any(|(_, t)| *t == c), "{c:?} names a write");
        }
    }

    /// Each category silences its own writes and nothing else.
    #[test]
    fn each_category_silences_only_its_own() {
        for c in Category::ALL {
            for (e, of) in TABLE {
                assert_eq!(pings(*e, &[c]), *of != c, "{e:?} with {c:?} silent");
            }
        }
        assert!(!pings(Event::ToolLine, &[Category::Tools]));
        assert!(pings(Event::Answer, &[Category::Tools, Category::Replies]));
        assert!(!pings(Event::Board, &[Category::Tasks]));
        assert!(!pings(Event::Restarted, &[Category::Notes]));
    }

    /// Each kind's reading: a report by its outcome, a disk crossing by its
    /// state, a reply by whether a wake or a report started its turn, a live
    /// message by its key.
    #[test]
    fn a_posts_body_says_which_event_it_is() {
        assert_eq!(of_report(&json!({"outcome": "failed"})), Event::TaskFailed);
        assert_eq!(of_report(&json!({"outcome": "complete"})), Event::TaskEnded);
        assert_eq!(
            of_report(&json!({"outcome": "cancelled"})),
            Event::TaskEnded
        );
        assert_eq!(
            of_disk(&json!({"state": "below_floor"})),
            Event::DiskCritical
        );
        assert_eq!(of_disk(&json!({"state": "low"})), Event::Disk);
        assert_eq!(of_disk(&json!({"state": "ok"})), Event::Disk);
        assert_eq!(of_reply(&json!({"kind": "reply"})), Event::Answer);
        assert_eq!(of_reply(&json!({"wakes": []})), Event::Answer);
        assert_eq!(
            of_reply(&json!({"wakes": [{"text": "⏰ wake"}]})),
            Event::Woken
        );
        assert_eq!(
            of_reply(&json!({"reports": [{"text": "📋 task a1b2c3 reported"}]})),
            Event::Woken
        );
        assert_eq!(of_live("turn_a:L0:p0", true), Event::Answer);
        assert_eq!(of_live("turn_a:L0:p0", false), Event::ReplyPart);
        assert_eq!(of_live("turn_a:L0:think", true), Event::Thinking);
        assert_eq!(of_live("turn_a:L0:tools", true), Event::ToolLine);
        assert_eq!(of_live("turn_a:notice:tu_1", false), Event::ToolLine);
    }

    #[test]
    fn a_text_part_is_told_from_a_tool_line_and_a_footer() {
        assert!(is_text_part("turn_a:L0:p0"));
        assert!(is_text_part("turn_a:L12:p3"));
        assert!(!is_text_part("turn_a:L0:tools"));
        assert!(!is_text_part("turn_a:L0:think"));
        assert!(!is_text_part("turn_a:footer"));
        assert!(!is_text_part("turn_a:notice:tu_1"));
        assert!(!is_text_part("turn_a:L:p0"));
        assert!(!is_text_part("p0"));
        assert!(!is_text_part("confirm:act_1"));
    }
}
