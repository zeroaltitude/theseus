//! A scheduled item's card (issue 30): a reminder, a check-in Theseus set for
//! itself, or a routine, drawn as one Components V2 message of its own in the
//! task home, and as the first message of its post in the forum. Pure: a
//! `Face` in, components out. The card is edited for its own item's life
//! only: set, snoozed, fired, cancelled, a routine paused or removed.
//! `docs/design/board-place.md` has the rest.

use serde_json::Value;
use theseus_protocol::routines::RoutineInfo;

use super::board_v2::short;
use super::clip;
use super::horizon::{routine_line, routine_row, CANCEL_PREFIX, SNOOZE_PREFIX};
use super::v2::{button, container, link_button, row, text, Look};

/// What kind of item a card is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A reminder a person set.
    Reminder,
    /// A check-in Theseus set for itself.
    CheckIn,
    /// A standing schedule.
    Routine,
}

impl Kind {
    pub fn noun(self) -> &'static str {
        match self {
            Kind::Reminder => "reminder",
            Kind::CheckIn => "check-in",
            Kind::Routine => "routine",
        }
    }
}

/// Where an item is in its life.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Life {
    /// Set and waiting for its time; `snoozed` once it was moved later.
    Scheduled { snoozed: bool },
    /// It fired, at this time.
    Fired(u64),
    /// It was cancelled, at this time, by this person when known.
    Cancelled(u64, Option<String>),
    /// A routine that is gone, since this time.
    Removed(u64),
}

/// Everything a card says of one item.
#[derive(Debug, Clone)]
pub struct Face {
    pub id: String,
    pub kind: Kind,
    pub title: String,
    pub due_ms: Option<u64>,
    pub life: Life,
    /// The conversation that set it, by its name.
    pub from: Option<String>,
    /// A series' span (`1d`).
    pub every: Option<String>,
    pub paused: bool,
    /// A routine, as the board's line draws it.
    pub routine: Option<(RoutineInfo, Option<(u32, u32)>)>,
}

impl Face {
    pub fn is_over(&self) -> bool {
        !matches!(self.life, Life::Scheduled { .. })
    }
}

fn emoji(f: &Face) -> &'static str {
    match (&f.life, f.kind) {
        (Life::Fired(_), _) => "✅",
        (Life::Cancelled(..), _) => "⏹",
        (Life::Removed(_), _) => "🗑",
        (_, Kind::Reminder) => "⏰",
        (_, Kind::CheckIn) => "💬",
        (_, Kind::Routine) => "🔁",
    }
}

/// `Reminder · every 1d · from #pier`, or `Check-in · set by Theseus in #pier`.
fn kind_line(f: &Face) -> String {
    let mut bits = vec![match f.kind {
        Kind::Reminder => "Reminder".to_string(),
        Kind::CheckIn => "Check-in".to_string(),
        Kind::Routine => "Routine".to_string(),
    }];
    if let Some(e) = &f.every {
        // A routine's series is already words (`weekdays 08:00`).
        bits.push(if f.kind == Kind::Routine {
            e.clone()
        } else {
            format!("every {e}")
        });
    }
    match (f.kind, &f.from) {
        (Kind::CheckIn, Some(n)) => bits.push(format!("set by Theseus in {}", clip(n, 40))),
        (Kind::CheckIn, None) => bits.push("set by Theseus".into()),
        (Kind::Reminder, Some(n)) => bits.push(format!("from {}", clip(n, 40))),
        _ => {}
    }
    if f.kind != Kind::Routine {
        bits.push(format!("`…{}`", short(&f.id)));
    }
    bits.join(" · ")
}

/// The item's words: what it is, when, and where it came from.
fn words(f: &Face) -> Vec<Value> {
    if let (Some((r, rec)), Life::Scheduled { .. }) = (&f.routine, &f.life) {
        return vec![
            text(routine_line(r, *rec)),
            text(format!("-# {}", kind_line(f))),
        ];
    }
    let title = clip(f.title.trim(), 120);
    let stamp = |ms: u64| format!("<t:{}:f>", ms / 1000);
    let head = format!("{} **{title}**", emoji(f));
    let body = match &f.life {
        Life::Scheduled { snoozed } => {
            let due = f.due_ms.unwrap_or(0);
            let mut s = format!("{} · <t:{}:R>", stamp(due), due / 1000);
            if *snoozed {
                s.push_str("\n😴 Snoozed to this time");
            }
            if f.paused {
                s.push_str("\n⏸ Paused");
            }
            s
        }
        Life::Fired(at) => format!("Fired {}", stamp(*at)),
        Life::Cancelled(at, by) => match by {
            Some(who) => format!("Cancelled by {} · {}", clip(who, 40), stamp(*at)),
            None => format!("Cancelled · {}", stamp(*at)),
        },
        Life::Removed(at) => format!("Removed · {}", stamp(*at)),
    };
    vec![
        text(format!("{head}\n{body}")),
        text(format!("-# {}", kind_line(f))),
    ]
}

/// The card in the task home: its words and, while the item stands, its acts:
/// Cancel and Snooze 1 h for a reminder or check-in, Run now and Pause or
/// Resume for a routine.
pub fn card(f: &Face) -> Vec<Value> {
    let mut inner = words(f);
    if !f.is_over() {
        match (&f.routine, f.kind) {
            (Some((r, _)), _) => inner.push(routine_row(r)),
            (None, _) => inner.push(row(vec![
                button(format!("{CANCEL_PREFIX}{}", f.id), "⏹ Cancel", Look::Danger),
                button(
                    format!("{SNOOZE_PREFIX}{}", f.id),
                    "😴 Snooze 1 h",
                    Look::Secondary,
                ),
            ])),
        }
    }
    vec![container(Some(accent(f)), inner)]
}

/// The first message of the item's post in the forum: the same words, no
/// button that acts (a post is no place that answers), and a link to the card.
pub fn forum_card(f: &Face, link: Option<&str>) -> Vec<Value> {
    let mut inner = words(f);
    if let Some(url) = link {
        inner.push(row(vec![link_button(url, "Open the card ↗")]));
    }
    vec![container(Some(accent(f)), inner)]
}

fn accent(f: &Face) -> u32 {
    match f.life {
        Life::Scheduled { .. } => 0x3E_63_DD,
        Life::Fired(_) => 0x30_A4_6C,
        Life::Cancelled(..) | Life::Removed(_) => 0x8A_8F_98,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::v2::{custom_ids, texts, validate};

    fn reminder(life: Life) -> Face {
        Face {
            id: "wak_01J0aaaaaaaaaaaaaaaaaaaaab12cd".into(),
            kind: Kind::Reminder,
            title: "stretch".into(),
            due_ms: Some(1_790_000_300_000),
            life,
            from: Some("#pier".into()),
            every: None,
            paused: false,
            routine: None,
        }
    }

    #[test]
    fn a_reminder_card_says_what_when_and_from_where_and_has_two_acts() {
        let c = card(&reminder(Life::Scheduled { snoozed: false }));
        assert_eq!(validate(&c), Ok(()));
        assert_eq!(
            texts(&c),
            [
                "⏰ **stretch**\n<t:1790000300:f> · <t:1790000300:R>",
                "-# Reminder · from #pier · `…ab12cd`",
            ]
        );
        assert_eq!(
            custom_ids(&c),
            [
                "hq:cancel:wak_01J0aaaaaaaaaaaaaaaaaaaaab12cd",
                "hq:snooze:wak_01J0aaaaaaaaaaaaaaaaaaaaab12cd"
            ]
        );
    }

    #[test]
    fn a_check_in_theseus_set_for_itself_says_so_and_names_the_conversation() {
        let mut f = reminder(Life::Scheduled { snoozed: false });
        f.kind = Kind::CheckIn;
        let t = texts(&card(&f));
        assert!(t[0].starts_with("💬 **stretch**"), "{t:?}");
        assert!(t[1].contains("Check-in · set by Theseus in #pier"), "{t:?}");
        f.from = None;
        assert!(texts(&card(&f))[1].contains("set by Theseus ·"), "no name");
    }

    #[test]
    fn a_snoozed_card_says_so_and_keeps_its_acts() {
        let c = card(&reminder(Life::Scheduled { snoozed: true }));
        assert!(texts(&c)[0].contains("😴 Snoozed to this time"));
        assert_eq!(custom_ids(&c).len(), 2);
    }

    #[test]
    fn a_fired_or_cancelled_card_has_no_acts_and_says_how_it_ended() {
        let fired = card(&reminder(Life::Fired(1_790_000_301_000)));
        assert_eq!(texts(&fired)[0], "✅ **stretch**\nFired <t:1790000301:f>");
        assert!(custom_ids(&fired).is_empty());
        let by = card(&reminder(Life::Cancelled(
            1_790_000_100_000,
            Some("ana".into()),
        )));
        assert_eq!(
            texts(&by)[0],
            "⏹ **stretch**\nCancelled by ana · <t:1790000100:f>"
        );
        assert!(custom_ids(&by).is_empty());
        let unknown = card(&reminder(Life::Cancelled(1_790_000_100_000, None)));
        assert!(
            texts(&unknown)[0].contains("Cancelled · <t:"),
            "{:?}",
            texts(&unknown)
        );
    }

    #[test]
    fn a_series_says_how_often() {
        let mut f = reminder(Life::Scheduled { snoozed: false });
        f.every = Some("1d".into());
        assert!(texts(&card(&f))[1].contains("Reminder · every 1d · from #pier"));
    }

    fn routine_face(life: Life) -> Face {
        let r = crate::render::horizon::tests::routine("Tide digest", false, "complete");
        Face {
            id: r.id.clone(),
            kind: Kind::Routine,
            title: r.title.clone(),
            due_ms: r.next_ms,
            life,
            from: None,
            every: Some(r.series.clone()),
            paused: false,
            routine: Some((r, Some((28, 1)))),
        }
    }

    #[test]
    fn a_routines_card_has_its_board_lines_words_and_run_now_and_pause() {
        let c = card(&routine_face(Life::Scheduled { snoozed: false }));
        assert_eq!(validate(&c), Ok(()));
        let t = texts(&c);
        assert!(
            t[0].starts_with("🔁 **Tide digest** `…a41d04` · next <t:"),
            "{t:?}"
        );
        assert_eq!(t[1], "-# Routine · weekdays 08:00");
        assert_eq!(
            custom_ids(&c),
            [
                "hr:run:ses_01J0000000000000000000a41d04",
                "hr:pause:ses_01J0000000000000000000a41d04"
            ]
        );
    }

    #[test]
    fn a_removed_routines_card_says_so_and_has_no_acts() {
        let c = card(&routine_face(Life::Removed(1_790_000_100_000)));
        assert_eq!(
            texts(&c)[0],
            "🗑 **Tide digest**\nRemoved · <t:1790000100:f>"
        );
        assert!(custom_ids(&c).is_empty());
    }

    #[test]
    fn the_forum_card_has_the_words_and_a_link_and_no_act() {
        let f = reminder(Life::Scheduled { snoozed: false });
        let c = forum_card(&f, Some("https://discord.com/channels/1/2/3"));
        assert_eq!(validate(&c), Ok(()));
        assert!(custom_ids(&c).is_empty(), "no button that acts");
        assert!(c[0].to_string().contains("Open the card"));
        assert!(forum_card(&f, None)[0].to_string().contains("stretch"));
    }
}
