//! Which of the binding's messages notify (theseus-l1y1): every create says
//! whether it pings, and only what needs the owner does. The rest go out
//! silent, with Discord's `SUPPRESS_NOTIFICATIONS` flag: they post, and no
//! device notifies, mentions included. An edit never notifies, so only a
//! create carries the flag.
//!
//! This table is to be replaced by `theseus_protocol::notify`, the shared
//! notification policy (task management's work-types row, theseus-753z), once
//! both have joined: its words are this table's (Interrupt for a question or
//! a failure, Inform for the rest), and it is data, so the replacement is
//! mechanical.
//!
//! On top of the table, a place pings at most once per `WINDOW`: a second
//! write that would ping inside it goes out silent (a card's buttons work as
//! always). The window is per channel, in memory, and bounded.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use serde_json::Value;
use tokio::time::Instant;

/// Discord's `SUPPRESS_NOTIFICATIONS` message flag (1 << 12).
pub(crate) const SUPPRESS_NOTIFICATIONS: u64 = 1 << 12;

/// At most one ping per place in this long (the owner's D3).
pub(crate) const WINDOW: Duration = Duration::from_secs(30);

/// How many places' last pings are kept: past it, the oldest is forgotten.
/// Only a ping inside `WINDOW` matters, so an older one is dropped first.
const PLACES_KEPT: usize = 256;

/// How much a message needs the owner: `theseus_protocol::notify`'s words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Urgency {
    /// It needs the owner: a question, or a failure. It pings.
    Interrupt,
    /// It tells: silent.
    Inform,
}

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
    /// The first part of the reply to the owner's own message (D1).
    Answer,
    /// A reply's later parts, its footer, and any part of a reply to someone
    /// else's message.
    ReplyPart,
    /// A turn a wake or a task's report started: its reply.
    Woken,
    /// A loop's tool line.
    ToolLine,
    /// A notice: a publish, a budget's or the hours' line, a proposal.
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
    /// The task board: a task starts, moves, or finishes (D2).
    Board,
}

/// The table: what pings.
pub(crate) const TABLE: &[(Event, Urgency)] = &[
    (Event::Card, Urgency::Interrupt),
    (Event::CardNote, Urgency::Inform),
    (Event::CardClosed, Urgency::Inform),
    (Event::TurnFailed, Urgency::Interrupt),
    (Event::TaskFailed, Urgency::Interrupt),
    (Event::TaskEnded, Urgency::Inform),
    (Event::Answer, Urgency::Interrupt),
    (Event::ReplyPart, Urgency::Inform),
    (Event::Woken, Urgency::Inform),
    (Event::ToolLine, Urgency::Inform),
    (Event::Note, Urgency::Inform),
    (Event::Restarted, Urgency::Inform),
    (Event::Mcp, Urgency::Inform),
    (Event::Jev, Urgency::Inform),
    (Event::Glide, Urgency::Inform),
    (Event::Hands, Urgency::Inform),
    (Event::DiskCritical, Urgency::Interrupt),
    (Event::Disk, Urgency::Inform),
    (Event::Board, Urgency::Inform),
];

/// Whether `e` pings, by the table. An event it does not name is silent.
pub(crate) fn pings(e: Event) -> bool {
    TABLE
        .iter()
        .find(|(t, _)| *t == e)
        .is_some_and(|(_, u)| *u == Urgency::Interrupt)
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

/// Each place's last ping, by channel: a place pings at most once per
/// `WINDOW`. Shared by every lane, since a card from one place's lane lands
/// in the owner's DM, whose own lane writes there too.
#[derive(Default)]
pub(crate) struct Pings {
    last: Mutex<HashMap<u64, Instant>>,
}

impl Pings {
    /// May `channel` ping now? No when it pinged within `WINDOW`.
    pub(crate) fn open(&self, channel: u64, now: Instant) -> bool {
        self.last
            .lock()
            .unwrap()
            .get(&channel)
            .is_none_or(|at| now.saturating_duration_since(*at) >= WINDOW)
    }

    /// `channel` pinged now. Kept bounded: the pings older than `WINDOW` go
    /// first, then the oldest, past `PLACES_KEPT`.
    pub(crate) fn mark(&self, channel: u64, now: Instant) {
        let mut last = self.last.lock().unwrap();
        last.insert(channel, now);
        if last.len() <= PLACES_KEPT {
            return;
        }
        last.retain(|_, at| now.saturating_duration_since(*at) < WINDOW);
        while last.len() > PLACES_KEPT {
            let Some(oldest) = last.iter().min_by_key(|(_, at)| **at).map(|(c, _)| *c) else {
                break;
            };
            last.remove(&oldest);
        }
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.last.lock().unwrap().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The owner's table (theseus-l1y1): a card, a failure, the answer to
    /// the owner's own message, and disk critical ping; nothing else does.
    #[test]
    fn only_what_needs_the_owner_pings() {
        let loud: Vec<Event> = TABLE
            .iter()
            .filter(|(e, _)| pings(*e))
            .map(|(e, _)| *e)
            .collect();
        assert_eq!(
            loud,
            [
                Event::Card,
                Event::TurnFailed,
                Event::TaskFailed,
                Event::Answer,
                Event::DiskCritical
            ]
        );
        for e in [
            Event::CardNote,
            Event::CardClosed,
            Event::TaskEnded,
            Event::ReplyPart,
            Event::Woken,
            Event::ToolLine,
            Event::Note,
            Event::Restarted,
            Event::Mcp,
            Event::Jev,
            Event::Glide,
            Event::Hands,
            Event::Disk,
            Event::Board,
        ] {
            assert!(!pings(e), "{e:?} is silent");
        }
    }

    /// Each kind's reading: a report by its outcome, a disk crossing by its
    /// state, a reply by whether a wake or a report started its turn.
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
    }

    #[test]
    fn a_text_part_is_told_from_a_tool_line_and_a_footer() {
        assert!(is_text_part("turn_a:L0:p0"));
        assert!(is_text_part("turn_a:L12:p3"));
        assert!(!is_text_part("turn_a:L0:tools"));
        assert!(!is_text_part("turn_a:footer"));
        assert!(!is_text_part("turn_a:notice:tu_1"));
        assert!(!is_text_part("turn_a:L:p0"));
        assert!(!is_text_part("p0"));
        assert!(!is_text_part("confirm:act_1"));
    }

    /// One ping per place per `WINDOW`: a second inside it is held back, one
    /// at its end goes, and another place is its own.
    #[test]
    fn a_place_pings_once_per_window() {
        let p = Pings::default();
        let t0 = Instant::now();
        assert!(p.open(1, t0));
        p.mark(1, t0);
        assert!(!p.open(1, t0 + Duration::from_secs(1)));
        assert!(!p.open(1, t0 + WINDOW - Duration::from_millis(1)));
        assert!(p.open(1, t0 + WINDOW));
        assert!(p.open(2, t0), "another place is its own");
    }

    /// The window's map is bounded: past `PLACES_KEPT`, the pings older than
    /// the window go, then the oldest.
    #[test]
    fn the_windows_map_is_bounded() {
        let p = Pings::default();
        let t0 = Instant::now();
        for c in 0..PLACES_KEPT as u64 {
            p.mark(c, t0);
        }
        assert_eq!(p.len(), PLACES_KEPT);
        let later = t0 + WINDOW;
        p.mark(10_000, later);
        assert_eq!(p.len(), 1, "every ping older than the window went");
        for c in 0..(PLACES_KEPT as u64 * 2) {
            p.mark(20_000 + c, later + Duration::from_millis(c));
        }
        assert_eq!(p.len(), PLACES_KEPT);
        assert!(
            !p.open(20_000 + PLACES_KEPT as u64 * 2 - 1, later),
            "the newest is kept"
        );
    }
}
