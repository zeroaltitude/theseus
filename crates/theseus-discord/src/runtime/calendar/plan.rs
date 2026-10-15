//! The calendar's rules, pure: which agenda lines become events, what an
//! event is named, and what a pass must do to bring Discord's events to the
//! agenda's next days (theseus-4zc4d). No clock, no network, no store.

use serde::{Deserialize, Serialize};
use theseus_protocol::work::horizon::{AgendaLine, AgendaWhat};

/// Discord's limit on an event's name, in characters.
pub(super) const NAME_MAX: usize = 100;
/// And on its description.
pub(super) const DESC_MAX: usize = 1000;
/// What an event's description ends with, so a restart finds its events
/// again even when the store lost the write that named them.
pub(super) const MARK: &str = "theseus-calendar:";

const MINUTE: u64 = 60_000;
const HOUR: u64 = 60 * MINUTE;

/// What an event stands for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Kind {
    Reminder,
    Run,
    Milestone,
}

impl Kind {
    /// The agenda line's kind, when it is one the calendar shows: a reminder
    /// (not a check-in, the model's own), a routine's run, a milestone.
    pub(super) fn of(l: &AgendaLine) -> Option<Self> {
        match l.what {
            AgendaWhat::Due if l.purpose.as_deref() == Some("remind") => Some(Self::Reminder),
            AgendaWhat::Run => Some(Self::Run),
            AgendaWhat::Milestone => Some(Self::Milestone),
            _ => None,
        }
    }

    fn noun(self) -> &'static str {
        match self {
            Self::Reminder => "reminder",
            Self::Run => "routine",
            Self::Milestone => "milestone",
        }
    }

    /// How long the event lasts (Discord wants an end for an external one):
    /// a milestone stands for its day.
    fn length_ms(self) -> u64 {
        match self {
            Self::Reminder => 15 * MINUTE,
            Self::Run => 30 * MINUTE,
            Self::Milestone => 24 * HOUR - MINUTE,
        }
    }
}

/// What the calendar may say of a line: whether its title may be shown, and
/// the place it speaks in, when that is a channel of the trusted guild.
#[derive(Clone, Debug, Default)]
pub(super) struct Shown {
    pub title: bool,
    pub place: Option<String>,
}

/// One event the agenda calls for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Item {
    pub key: String,
    pub kind: Kind,
    pub at_ms: u64,
    pub end_ms: u64,
    pub name: String,
    pub location: String,
}

/// An event the calendar made, as the store remembers it: found again by
/// `key`, the item's id and occurrence.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Entry {
    pub key: String,
    pub event_id: String,
    pub at_ms: u64,
    pub end_ms: u64,
    pub name: String,
    pub location: String,
    /// Discord's status: 1 scheduled, 2 active.
    pub status: u8,
}

/// The item's key: which work, and which occurrence of it.
pub(super) fn key_of(l: &AgendaLine) -> String {
    format!("{}#{}", l.id, l.occurrence.unwrap_or(0))
}

/// Whether a title speaks of money. Every figure stays out of a place everyone
/// in the guild reads, and a title is free text.
fn has_money(t: &str) -> bool {
    let low = t.to_lowercase();
    t.contains(['$', '€', '£']) || low.contains("usd") || low.contains("dollar")
}

/// An event's name: the title when it may be shown, else a neutral one.
pub(super) fn name_of(kind: Kind, title: &str, shown: bool) -> String {
    let t = title.split('\n').next().unwrap_or("").trim();
    if shown && !t.is_empty() && !has_money(t) {
        clip(t, NAME_MAX)
    } else {
        format!("Theseus · {}", kind.noun())
    }
}

/// `s` in at most `max` characters, an ellipsis where it was cut.
fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max - 1).collect();
    out.push('…');
    out
}

/// An event's description: what it is, and the marker that finds it.
pub(super) fn description(kind: Kind, key: &str) -> String {
    clip(
        &format!("A Theseus {}.\n\n{MARK}{key}", kind.noun()),
        DESC_MAX,
    )
}

/// The key a description carries.
pub(super) fn marker_of(description: &str) -> Option<&str> {
    let at = description.find(MARK)? + MARK.len();
    let key = description[at..].split_whitespace().next()?;
    (!key.is_empty()).then_some(key)
}

/// The events the next lines call for: the soonest `max` of them that start
/// at least `lead_ms` from `now`, one to a key.
pub(super) fn items(
    lines: &[AgendaLine],
    now: u64,
    lead_ms: u64,
    max: usize,
    shown: impl Fn(&AgendaLine) -> Shown,
) -> Vec<Item> {
    let mut out: Vec<Item> = Vec::new();
    for l in lines {
        let Some(kind) = Kind::of(l) else { continue };
        if l.at_ms < now.saturating_add(lead_ms) {
            continue;
        }
        let key = key_of(l);
        if out.iter().any(|i| i.key == key) {
            continue;
        }
        let s = shown(l);
        out.push(Item {
            name: name_of(kind, &l.title, s.title),
            location: clip(s.place.as_deref().unwrap_or("Theseus"), NAME_MAX),
            end_ms: l.at_ms + kind.length_ms(),
            at_ms: l.at_ms,
            key,
            kind,
        });
        if out.len() == max {
            break;
        }
    }
    out
}

/// One write a pass makes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Op {
    /// The event's time has come and gone: complete it.
    Settle(Entry),
    /// The item is gone from the agenda's next days: delete its event.
    Delete(Entry),
    /// The item moved, or was renamed: edit its event.
    Edit(Entry, Item),
    /// A new item: make its event.
    Create(Item),
}

/// What brings `saved` to `want` at `now`, settling first, then deleting, then
/// editing, then making at most `room` events (the guild's room, after its own
/// events and the ones kept). A saved event whose time has passed is settled
/// whatever the agenda says now: a snoozed reminder is a new event.
pub(super) fn diff(saved: &[Entry], want: &[Item], now: u64, room: usize) -> Vec<Op> {
    let mut ops = Vec::new();
    let (past, kept): (Vec<&Entry>, Vec<&Entry>) = saved.iter().partition(|e| e.at_ms <= now);
    ops.extend(past.into_iter().cloned().map(Op::Settle));
    let mut edits = Vec::new();
    for e in &kept {
        match want.iter().find(|i| i.key == e.key) {
            None => ops.push(Op::Delete((*e).clone())),
            Some(i) => {
                let same = (e.at_ms, e.end_ms, &e.name, &e.location)
                    == (i.at_ms, i.end_ms, &i.name, &i.location);
                if !same {
                    edits.push(Op::Edit((*e).clone(), i.clone()));
                }
            }
        }
    }
    ops.extend(edits);
    let made = want
        .iter()
        .filter(|i| !kept.iter().any(|e| e.key == i.key))
        .take(room);
    ops.extend(made.cloned().map(Op::Create));
    ops
}

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_protocol::work::WorkKind;

    pub(in crate::runtime) fn line(
        id: &str,
        what: AgendaWhat,
        at_ms: u64,
        title: &str,
    ) -> AgendaLine {
        AgendaLine {
            at_ms,
            id: id.into(),
            kind: WorkKind::Wake,
            what,
            title: title.into(),
            session_id: Some("ses_a".into()),
            purpose: Some(
                if what == AgendaWhat::Run {
                    "routine"
                } else {
                    "remind"
                }
                .into(),
            ),
            occurrence: None,
        }
    }

    fn private(_: &AgendaLine) -> Shown {
        Shown {
            title: true,
            place: None,
        }
    }

    fn entry(i: &Item, id: &str) -> Entry {
        Entry {
            key: i.key.clone(),
            event_id: id.into(),
            at_ms: i.at_ms,
            end_ms: i.end_ms,
            name: i.name.clone(),
            location: i.location.clone(),
            status: 1,
        }
    }

    #[test]
    fn reminders_runs_and_milestones_are_events_and_the_rest_are_not() {
        let mut check_in = line("w3", AgendaWhat::Due, 9_000_000, "look again");
        check_in.purpose = Some("check".into());
        let lines = [
            line("w1", AgendaWhat::Due, 1_000_000, "stretch"),
            line("r1", AgendaWhat::Run, 2_000_000, "weekly digest"),
            line("p1", AgendaWhat::Milestone, 3_000_000, "ship it"),
            check_in,
            line("w4", AgendaWhat::Starts, 4_000_000, "a deferred task"),
            line("w5", AgendaWhat::Expires, 5_000_000, "a watch"),
        ];
        let got = items(&lines, 0, 0, 10, private);
        let kinds: Vec<(Kind, &str)> = got.iter().map(|i| (i.kind, i.name.as_str())).collect();
        assert_eq!(
            kinds,
            [
                (Kind::Reminder, "stretch"),
                (Kind::Run, "weekly digest"),
                (Kind::Milestone, "ship it")
            ]
        );
        // A milestone stands for its day; a reminder for a quarter hour.
        assert_eq!(got[2].end_ms - got[2].at_ms, 24 * HOUR - MINUTE);
        assert_eq!(got[0].end_ms - got[0].at_ms, 15 * MINUTE);
    }

    #[test]
    fn the_soonest_ten_stand_and_the_eleventh_waits() {
        let lines: Vec<AgendaLine> = (0..11)
            .map(|n| line(&format!("w{n}"), AgendaWhat::Due, 1_000_000 + n * 1000, "x"))
            .collect();
        let got = items(&lines, 0, 0, 10, private);
        assert_eq!(got.len(), 10);
        assert!(got.iter().all(|i| i.key != "w10#0"));
        // Past the lead, a start too close to now is left alone.
        let near = [line("w1", AgendaWhat::Due, 20_000, "x")];
        assert!(items(&near, 0, 30_000, 10, private).is_empty());
    }

    #[test]
    fn a_title_is_shown_only_where_it_may_be_and_never_a_figure() {
        let shown = |t: &str| name_of(Kind::Reminder, t, true);
        assert_eq!(shown("call the plumber"), "call the plumber");
        assert_eq!(
            name_of(Kind::Reminder, "call the plumber", false),
            "Theseus · reminder"
        );
        assert_eq!(
            name_of(Kind::Run, "weekly digest", false),
            "Theseus · routine"
        );
        assert_eq!(
            name_of(Kind::Milestone, "ship it", false),
            "Theseus · milestone"
        );
        for money in ["pay $50 rent", "refund 20 USD", "tip: five dollars", "€5"] {
            assert_eq!(shown(money), "Theseus · reminder", "{money}");
        }
        assert_eq!(shown("   "), "Theseus · reminder");
        // Only the first line, and no more than 100 characters.
        assert_eq!(shown("first\nsecond"), "first");
        let long = "é".repeat(150);
        let n = shown(&long);
        assert_eq!(n.chars().count(), NAME_MAX);
        assert!(n.ends_with('…'));
        assert!(description(Kind::Run, &"k".repeat(2000)).chars().count() <= DESC_MAX);
    }

    #[test]
    fn a_description_carries_the_key_back() {
        let d = description(Kind::Reminder, "wake_ab12#0");
        assert_eq!(marker_of(&d), Some("wake_ab12#0"));
        assert_eq!(marker_of("a person's own event"), None);
        assert_eq!(marker_of("theseus-calendar:"), None);
    }

    #[test]
    fn a_pass_settles_then_deletes_then_edits_then_makes() {
        let it = |id: &str, at: u64| Item {
            key: format!("{id}#0"),
            kind: Kind::Reminder,
            at_ms: at,
            end_ms: at + 1,
            name: id.into(),
            location: "Theseus".into(),
        };
        let (a, b, c, d, e) = (
            it("a", 100),
            it("b", 5_000),
            it("c", 6_000),
            it("d", 7_000),
            it("e", 8_000),
        );
        let saved = vec![entry(&a, "1"), entry(&b, "2"), entry(&c, "3")];
        // a passed; b is gone; c moved; d and e are new.
        let c2 = Item {
            at_ms: 6_500,
            end_ms: 6_501,
            ..c.clone()
        };
        let ops = diff(&saved, &[c2.clone(), d.clone(), e.clone()], 1_000, 10);
        assert_eq!(
            ops,
            [
                Op::Settle(entry(&a, "1")),
                Op::Delete(entry(&b, "2")),
                Op::Edit(entry(&c, "3"), c2),
                Op::Create(d.clone()),
                Op::Create(e),
            ]
        );
        // The guild's room bounds the makes.
        assert_eq!(diff(&[], std::slice::from_ref(&d), 1_000, 0), []);
        // Nothing to do when nothing changed.
        let same = vec![entry(&d, "9")];
        assert_eq!(diff(&same, &[d], 1_000, 10), []);
    }

    #[test]
    fn a_snoozed_reminder_settles_its_event_and_makes_another() {
        let old = Item {
            key: "w#0".into(),
            kind: Kind::Reminder,
            at_ms: 500,
            end_ms: 900,
            name: "n".into(),
            location: "Theseus".into(),
        };
        let new = Item {
            at_ms: 5_000,
            end_ms: 5_400,
            ..old.clone()
        };
        let ops = diff(&[entry(&old, "1")], std::slice::from_ref(&new), 1_000, 10);
        assert_eq!(ops, [Op::Settle(entry(&old, "1")), Op::Create(new)]);
    }
}
