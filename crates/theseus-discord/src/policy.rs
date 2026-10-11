//! Which of the binding's messages ping (theseus-l1y1): today's pings by
//! default, and silence is per category, per place. Every create says
//! whether it pings; one that does not carries Discord's
//! `SUPPRESS_NOTIFICATIONS` flag: it posts, and no device notifies, mentions
//! included. An edit never notifies, so only a create carries the flag.
//!
//! The table maps each write to its chat category, the name a `silent` list
//! gives it (`theseus_core::config::discord::Category`): a write pings unless
//! its place's list (its binding's, else `[discord] silent`) names its
//! category. Three writes have none and never ping: a card whose question
//! had already closed (nothing to answer: a buzz would be a false alarm),
//! the task board (status), and a loop's process message that holds its
//! thinking (`render/process.rs`), with its tool lines or without: a
//! thinking turn buzzes for its answer alone (the owner's call on the fold,
//! 2026-10-10), and a loop that does not think pings for its tool line as
//! before. The shared notification policy (`theseus_protocol::notices`)
//! decides what the terminal surfaces (the TUI, herdr) show and when they
//! ping; the chat does not read it, and whether a chat message buzzes is
//! this table's alone.
//!
//! On top of the table, a place may ping at most once per window
//! (`ping_window_secs`, its binding's, else `[discord]`'s; 0, the default, is
//! off): a second write that would ping inside it goes out silent, and its
//! row says `held` (a card's buttons work as always). The window is per
//! channel, in memory, and bounded.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use serde_json::Value;
use theseus_core::config::discord::Category;
use tokio::time::Instant;

/// Discord's `SUPPRESS_NOTIFICATIONS` message flag (1 << 12).
pub(crate) const SUPPRESS_NOTIFICATIONS: u64 = 1 << 12;

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
    /// A loop's process message that holds its thinking, streaming or
    /// folded, with its tool lines or before them (`render/process.rs`):
    /// never a ping; the turn's answer is.
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

/// The table: each write's chat category, the name a `silent` list gives
/// it; `None` for a write that never pings.
pub(crate) const TABLE: &[(Event, Option<Category>)] = &[
    (Event::Card, Some(Category::Cards)),
    (Event::CardNote, Some(Category::Cards)),
    (Event::CardClosed, None),
    (Event::TurnFailed, Some(Category::Failures)),
    (Event::TaskFailed, Some(Category::Failures)),
    (Event::DiskCritical, Some(Category::Failures)),
    (Event::Answer, Some(Category::Answer)),
    (Event::ReplyPart, Some(Category::LaterParts)),
    (Event::Woken, Some(Category::Woken)),
    (Event::ToolLine, Some(Category::ToolLines)),
    (Event::TaskEnded, Some(Category::Reports)),
    (Event::Hands, Some(Category::Reports)),
    (Event::Note, Some(Category::Notices)),
    (Event::Jev, Some(Category::Notices)),
    (Event::Glide, Some(Category::Notices)),
    (Event::Restarted, Some(Category::Ops)),
    (Event::Mcp, Some(Category::Ops)),
    (Event::Disk, Some(Category::Ops)),
    (Event::Thinking, None),
    (Event::Board, None),
];

/// Whether `e` pings, given the categories its place's `silent` list names:
/// yes unless the list names its category, and never for a write with none.
pub(crate) fn pings(e: Event, silent: &[Category]) -> bool {
    match TABLE.iter().find(|(t, _)| *t == e) {
        Some((_, Some(c))) => !silent.contains(c),
        Some((_, None)) => false,
        None => true,
    }
}

/// How many channels' last pings are kept: past it, the oldest is
/// forgotten. Only a ping inside its window matters, so an older one goes
/// first.
const PLACES_KEPT: usize = 256;

/// A place's own words on its pings, from its binding (`silent`,
/// `ping_window_secs` on a `[[channel]]` or a `[[dm]]`): each, when unset,
/// is `[discord]`'s.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct PlacePings {
    pub silent: Option<Vec<Category>>,
    pub window_secs: Option<u64>,
}

impl PlacePings {
    pub(crate) fn of_channel(c: &crate::bindings::ChannelBinding) -> Self {
        Self {
            silent: c.silent.clone(),
            window_secs: c.ping_window_secs,
        }
    }

    pub(crate) fn of_dm(d: &crate::bindings::DmBinding) -> Self {
        Self {
            silent: d.silent.clone(),
            window_secs: d.ping_window_secs,
        }
    }
}

/// Each place's words, by its lane's target, and each channel's last ping
/// with the window it was held to.
#[derive(Default)]
pub(crate) struct Pings {
    places: Mutex<HashMap<String, PlacePings>>,
    last: Mutex<HashMap<u64, (Instant, Duration)>>,
}

impl Pings {
    /// The place `target`'s words, set at its start and when its binding
    /// changes (theseus-ocwt).
    pub(crate) fn set(&self, target: &str, p: PlacePings) {
        self.places.lock().unwrap().insert(target.to_string(), p);
    }

    /// The place `target`'s words: none of its own when it is not a place
    /// (the operator's lane).
    pub(crate) fn of(&self, target: &str) -> PlacePings {
        self.places
            .lock()
            .unwrap()
            .get(target)
            .cloned()
            .unwrap_or_default()
    }

    /// May `channel` ping now, under `window`? No when it pinged within it;
    /// always when the window is zero.
    pub(crate) fn open(&self, channel: u64, now: Instant, window: Duration) -> bool {
        window.is_zero()
            || self
                .last
                .lock()
                .unwrap()
                .get(&channel)
                .is_none_or(|(at, _)| now.saturating_duration_since(*at) >= window)
    }

    /// `channel` pinged now, under `window` (nothing is kept for none).
    /// Kept bounded: the pings older than their window go first, then the
    /// oldest, past `PLACES_KEPT`.
    pub(crate) fn mark(&self, channel: u64, now: Instant, window: Duration) {
        if window.is_zero() {
            return;
        }
        let mut last = self.last.lock().unwrap();
        last.insert(channel, (now, window));
        if last.len() <= PLACES_KEPT {
            return;
        }
        last.retain(|_, (at, w)| now.saturating_duration_since(*at) < *w);
        while last.len() > PLACES_KEPT {
            let Some(oldest) = last.iter().min_by_key(|(_, at)| at.0).map(|(c, _)| *c) else {
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
/// the stream's and the reply's, the answer's when `owed`), a loop's process
/// message that holds its thinking (`content`), or its tool line and
/// anything else live (a notice embed).
pub(crate) fn of_live(key: &str, owed: bool, content: &str) -> Event {
    if is_text_part(key) {
        if owed {
            Event::Answer
        } else {
            Event::ReplyPart
        }
    } else if key.ends_with(":tools") && crate::render::process::holds_thinking(content) {
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

    /// With nothing named silent, today's pings: every write pings but a
    /// closed card, the board, and a process message made by its thinking
    /// alone: these 17, the redesign's; the table names every event once,
    /// and every category has a write.
    #[test]
    fn with_nothing_silent_every_chat_message_pings() {
        let loud: Vec<Event> = EVERY.into_iter().filter(|e| pings(*e, &[])).collect();
        assert_eq!(
            loud,
            [
                Event::Card,
                Event::CardNote,
                Event::TurnFailed,
                Event::TaskFailed,
                Event::TaskEnded,
                Event::Answer,
                Event::ReplyPart,
                Event::Woken,
                Event::ToolLine,
                Event::Note,
                Event::Restarted,
                Event::Mcp,
                Event::Jev,
                Event::Glide,
                Event::Hands,
                Event::DiskCritical,
                Event::Disk,
            ]
        );
        for e in EVERY {
            assert_eq!(TABLE.iter().filter(|(t, _)| *t == e).count(), 1, "{e:?}");
        }
        assert_eq!(TABLE.len(), EVERY.len());
        for c in Category::ALL {
            assert!(
                TABLE.iter().any(|(_, t)| *t == Some(c)),
                "{c:?} names a write"
            );
        }
    }

    /// Each category silences its own writes and nothing else; a closed card
    /// and the board are silent whatever the list.
    #[test]
    fn each_category_silences_only_its_own() {
        for c in Category::ALL {
            for (e, of) in TABLE {
                assert_eq!(
                    pings(*e, &[c]),
                    *of != Some(c) && of.is_some(),
                    "{e:?} with {c:?} silent"
                );
            }
        }
        assert!(!pings(Event::ToolLine, &[Category::ToolLines]));
        assert!(pings(
            Event::Answer,
            &[Category::ToolLines, Category::LaterParts]
        ));
        assert!(!pings(Event::DiskCritical, &[Category::Failures]));
        assert!(pings(Event::Disk, &[Category::Failures]));
        assert!(!pings(Event::Restarted, &[Category::Ops]));
        assert!(!pings(Event::Hands, &[Category::Reports]));
    }

    /// One ping per channel per window, when a window is set: a second
    /// inside it is held back, one at its end goes, another channel is its
    /// own, and no window holds nothing.
    #[test]
    fn a_place_pings_once_per_window_when_it_has_one() {
        let p = Pings::default();
        let w = Duration::from_secs(30);
        let t0 = Instant::now();
        assert!(p.open(1, t0, w));
        p.mark(1, t0, w);
        assert!(!p.open(1, t0 + Duration::from_secs(1), w));
        assert!(!p.open(1, t0 + w - Duration::from_millis(1), w));
        assert!(p.open(1, t0 + w, w));
        assert!(p.open(2, t0, w), "another channel is its own");
        let off = Duration::ZERO;
        p.mark(3, t0, off);
        assert!(p.open(3, t0, off), "no window holds nothing");
        assert_eq!(p.len(), 1, "and keeps nothing");
    }

    /// The window's map is bounded: past `PLACES_KEPT`, the pings older than
    /// their window go, then the oldest.
    #[test]
    fn the_windows_map_is_bounded() {
        let p = Pings::default();
        let w = Duration::from_secs(30);
        let t0 = Instant::now();
        for c in 0..PLACES_KEPT as u64 {
            p.mark(c, t0, w);
        }
        assert_eq!(p.len(), PLACES_KEPT);
        let later = t0 + w;
        p.mark(10_000, later, w);
        assert_eq!(p.len(), 1, "every ping older than its window went");
        for c in 0..(PLACES_KEPT as u64 * 2) {
            p.mark(20_000 + c, later + Duration::from_millis(c), w);
        }
        assert_eq!(p.len(), PLACES_KEPT);
        assert!(
            !p.open(20_000 + PLACES_KEPT as u64 * 2 - 1, later, w),
            "the newest is kept"
        );
    }

    /// A place's words are its own, and a lane that is no place's has none.
    #[test]
    fn a_places_words_are_kept_by_its_target() {
        let p = Pings::default();
        let words = PlacePings {
            silent: Some(vec![Category::ToolLines]),
            window_secs: Some(30),
        };
        p.set("discord:channel:1", words.clone());
        assert_eq!(p.of("discord:channel:1"), words);
        assert_eq!(p.of("discord:operator"), PlacePings::default());
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
        let line = "▫️ `fs.read` a";
        assert_eq!(of_live("turn_a:L0:p0", true, "x"), Event::Answer);
        assert_eq!(of_live("turn_a:L0:p0", false, "x"), Event::ReplyPart);
        let thought = "-# 💭 thinking\n-# Tides.";
        assert_eq!(of_live("turn_a:L0:tools", true, thought), Event::Thinking);
        // A thinking loop's tool line is its process message's, which never
        // pings (the owner's call, 2026-10-10): streaming or folded.
        let both = format!("{thought}\n{line}");
        assert_eq!(of_live("turn_a:L0:tools", true, &both), Event::Thinking);
        let folded = format!("-# 💭 thought for 2 s\n{line}");
        assert_eq!(of_live("turn_a:L0:tools", true, &folded), Event::Thinking);
        assert_eq!(of_live("turn_a:L0:tools", true, line), Event::ToolLine);
        let earlier = format!("-# … 3 earlier call(s)\n{line}");
        assert_eq!(of_live("turn_a:L0:tools", true, &earlier), Event::ToolLine);
        assert_eq!(of_live("turn_a:notice:tu_1", false, "x"), Event::ToolLine);
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
}
