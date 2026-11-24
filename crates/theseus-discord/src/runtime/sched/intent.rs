//! A card made at most once per item, across a kill at any point. A child of
//! `sched`; the same order as a forum post's (`forum/intent.rs`).
//!
//! The card's create carries the item's nonce, so a retry in the same process
//! or soon after returns the first message; Discord's own nonce window closes
//! (the stand-in's never does), so a restart after it, with the card made and
//! its entry lost to a kill, made the card twice. The intent comes first:
//! `discord.sched_intents` (META) holds `(item, channel, when)` before the
//! create and is settled once the entry is saved. The next time the item needs a
//! card, an intent still on the list is reconciled (`Worker::settle_intent`):
//! the channel's newest messages are read for one of the bot's Components V2
//! cards that carries the item's short id (`…ab12cd`, in every card's words),
//! and that message is the item's card, edited to the truth; none is a card
//! never made, and the first is made. A read that fails is no answer: no card is
//! made until it goes through. An intent goes when its item has its entry, or is
//! over (among none of a pass's items), never by age: a card is a message and
//! never archives. Not covered: a card made, unrecorded, and more
//! than 100 messages up the task home's channel by the restart.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use twilight_http::request::Request;
use twilight_http::routing::Route as HttpRoute;

use super::{Home, Rec, Worker};
use crate::render::board_v2::short;
use crate::render::horizon::{CANCEL_PREFIX, SNOOZE_PREFIX};
use crate::render::sched::Face;

/// META key of the cards being made.
pub(super) const KEY: &str = "discord.sched_intents";

/// How many of the channel's newest messages are read to find a card.
const READ: u16 = 100;

/// Components V2: the flag of a card.
const V2: u64 = 1 << 15;

/// A card about to be made, or made and not yet recorded.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(super) struct Intent {
    pub id: String,
    pub channel: u64,
    pub at_ms: u64,
}

/// The intents worth keeping: not one whose item has its entry (the kill came
/// after the entry was saved). Age takes none off: a card is a message and
/// never archives, so one made before a long stop is found after it; only the
/// item's end does (`standing`).
pub(super) fn live(intents: Vec<Intent>, recs: &[Rec]) -> Vec<Intent> {
    intents
        .into_iter()
        .filter(|i| !recs.iter().any(|r| r.id == i.id))
        .collect()
}

/// The intents whose item still stands (is among the pass's items): one whose
/// item is over (fired or cancelled while the daemon was down, or never asked
/// for again) has no card left to find, and leaves its intent behind otherwise.
pub(super) fn standing(intents: Vec<Intent>, faces: &[Face]) -> Vec<Intent> {
    intents
        .into_iter()
        .filter(|i| faces.iter().any(|f| f.id == i.id))
        .collect()
}

/// What every card's words carry of its item.
pub(super) fn marker(id: &str) -> String {
    format!("`…{}`", short(id))
}

impl Worker {
    pub(super) fn save_intents(&self) {
        if let Err(e) = self.shared.core.store.put_meta(KEY, &self.intents) {
            tracing::warn!(error = %format!("{e:#}"), "the cards being made were not saved");
        }
    }

    /// The card of `id` is about to be made.
    pub(super) fn intend(&mut self, id: &str, channel: u64) {
        self.intents.retain(|i| i.id != id);
        self.intents.push(Intent {
            id: id.to_string(),
            channel,
            at_ms: theseus_protocol::now_unix_ms(),
        });
        self.save_intents();
    }

    /// The card is recorded, or was never made.
    pub(super) fn settle(&mut self, id: &str) {
        let before = self.intents.len();
        self.intents.retain(|i| i.id != id);
        if self.intents.len() != before {
            self.save_intents();
        }
    }

    /// An intent left by a card a kill may have cut off, resolved before any
    /// card is made for the item. True when the item has its card (found again
    /// and entered in the list: the caller makes none); an error is a read that
    /// did not go through.
    pub(super) async fn settle_intent(
        &mut self,
        f: &Face,
        home: &Home,
        now: u64,
    ) -> anyhow::Result<bool> {
        let Some(intent) = self.intents.iter().find(|i| i.id == f.id).cloned() else {
            return Ok(false);
        };
        let found = self.find_card(&intent).await?;
        let Some(message) = found else {
            self.settle(&f.id);
            tracing::info!(place = %home.label, "a card meant before a stop was never made: the first is made now");
            return Ok(false);
        };
        tracing::info!(place = %home.label, message, "a card made before a stop was recorded, found again: no second one");
        // Its digest is nothing, so the pass edits it to the item's truth.
        self.recs.push(Rec {
            id: f.id.clone(),
            kind: f.kind.noun().to_string(),
            title: f.title.clone(),
            from: f.from.clone(),
            every: f.every.clone(),
            channel: intent.channel,
            guild: home.guild.clone(),
            card: message,
            created_ms: now,
            due_ms: f.due_ms,
            snoozed: false,
            ended_ms: None,
            by: None,
            digest: 0,
        });
        if let Err(e) = self.shared.core.store.put_meta(super::KEY, &self.recs) {
            self.recs.pop();
            return Err(e);
        }
        self.settle(&f.id);
        Ok(true)
    }

    /// The newest of the channel's messages that is one of the bot's cards
    /// carrying the item's marker.
    async fn find_card(&self, intent: &Intent) -> anyhow::Result<Option<u64>> {
        let req = Request::builder(&HttpRoute::GetMessages {
            after: None,
            around: None,
            before: None,
            channel_id: intent.channel,
            limit: Some(READ),
        })
        .build()?;
        let messages: Value = self
            .shared
            .http
            .request::<Value>(req)
            .await?
            .model()
            .await?;
        // Until the bot knows its own id any V2 message could be taken for its
        // own: no adoption, and the pass tries again.
        let bot = self.shared.bot_id();
        anyhow::ensure!(bot != 0, "the bot's own id is not known yet");
        Ok(messages
            .as_array()
            .into_iter()
            .flatten()
            .filter(|m| m["flags"].as_u64().unwrap_or(0) & V2 != 0)
            .filter(|m| {
                m["author"]["id"]
                    .as_str()
                    .and_then(|a| a.parse::<u64>().ok())
                    == Some(bot)
            })
            .filter(|m| is_card_of(m, &intent.id))
            .filter_map(|m| m["id"].as_str()?.parse::<u64>().ok())
            .max())
    }
}

/// The custom ids and the texts of a message's components, however deep
/// (a V2 card is a container of texts and a row of buttons).
fn parts(components: &Value) -> (Vec<String>, Vec<String>) {
    fn walk(v: &Value, ids: &mut Vec<String>, texts: &mut Vec<String>) {
        match v {
            Value::Array(all) => all.iter().for_each(|c| walk(c, ids, texts)),
            Value::Object(o) => {
                if let Some(id) = o.get("custom_id").and_then(Value::as_str) {
                    ids.push(id.to_string());
                }
                // A text display (type 10) says its words in `content`.
                if o.get("type").and_then(Value::as_u64) == Some(10) {
                    if let Some(t) = o.get("content").and_then(Value::as_str) {
                        texts.push(t.to_string());
                    }
                }
                for key in ["components", "accessory"] {
                    if let Some(c) = o.get(key) {
                        walk(c, ids, texts);
                    }
                }
            }
            _ => {}
        }
    }
    let (mut ids, mut texts) = (Vec::new(), Vec::new());
    walk(components, &mut ids, &mut texts);
    (ids, texts)
}

/// Whether the message is the card of the item `id`: known by what only a card
/// has, never by a substring of the whole message (the pinned board lists a
/// scheduled or running task with the same `…short` marker).
/// - **A card with its acts** carries a button whose custom id is an act on the
///   item's FULL id: Cancel or Snooze (`hq:cancel:<id>`, `hq:snooze:<id>`), or a
///   routine's Run, Pause or Resume (`hr:run|pause|resume:<id>`).
/// - **An ended card** has no button left, so it is the message with NO custom
///   id at all whose footer line (`-# Reminder · from … · `…short``) ENDS with
///   the marker: its last ` · ` field, never a marker inside the `from` name
///   (a thread's name is the owner's to choose). The board carries its own
///   controls, so it is never one. (A removed routine's footer has no marker:
///   such a card is not found again, and a second final card is the cost.)
fn is_card_of(message: &Value, id: &str) -> bool {
    let (ids, texts) = parts(&message["components"]);
    let acts = [
        CANCEL_PREFIX,
        SNOOZE_PREFIX,
        "hr:run:",
        "hr:pause:",
        "hr:resume:",
    ];
    if ids
        .iter()
        .any(|c| acts.iter().any(|p| c.strip_prefix(p) == Some(id)))
    {
        return true;
    }
    let marker = marker(id);
    ids.is_empty()
        && texts
            .iter()
            .any(|t| t.starts_with("-# ") && t.trim_end().rsplit(" · ").next() == Some(&marker))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_marker_is_what_every_cards_words_carry_of_its_item() {
        let id = "wak_01J0aaaaaaaaaaaaaaaaaaaaab12cd";
        assert_eq!(marker(id), "`…ab12cd`");
        use crate::render::sched::{card, Face, Kind, Life};
        let mut face = Face {
            id: id.into(),
            kind: Kind::Reminder,
            title: "stretch".into(),
            due_ms: Some(1_790_000_300_000),
            life: Life::Scheduled { snoozed: false },
            from: None,
            every: None,
            paused: false,
            routine: None,
        };
        for kind in [Kind::Reminder, Kind::CheckIn, Kind::Task] {
            face.kind = kind;
            let words = serde_json::to_string(&card(&face)).unwrap();
            assert!(words.contains(&marker(id)), "{kind:?}: {words}");
        }
    }

    fn message(components: Vec<Value>) -> Value {
        serde_json::json!({"components": components})
    }

    /// What only a card has: the pinned board, which lists a scheduled task with
    /// the very marker the card carries, is never a card; a card is, with its
    /// acts, ended, and (a routine's) with its own.
    #[test]
    fn a_card_is_known_by_its_acts_or_its_footer_and_the_board_never_is_one() {
        use crate::render::sched::{card, Face, Kind, Life};
        let at = 1_790_003_600_000;
        let task = crate::render::task_view::tests::deferred(at);
        let board = message(crate::render::board_v2::render(
            &crate::render::board_v2::Parts {
                needs: &[],
                working: std::slice::from_ref(&task),
                done: &[],
                quiet_until_ms: None,
                future: None,
                now_ms: 0,
                links: crate::render::board_v2::no_links(),
            },
        ));
        assert!(
            serde_json::to_string(&board)
                .unwrap()
                .contains(&marker(&task.id)),
            "the board does carry the marker"
        );
        assert!(!is_card_of(&board, &task.id), "and is no card");
        let face = |life| Face {
            id: task.id.clone(),
            kind: Kind::Task,
            title: "Draft".into(),
            due_ms: Some(at),
            life,
            from: Some("DM @ana".into()),
            every: None,
            paused: false,
            routine: None,
        };
        let standing = message(card(&face(Life::Scheduled { snoozed: false })));
        assert!(is_card_of(&standing, &task.id), "a card with its acts");
        assert!(
            !is_card_of(&standing, "exe_01J000000000000000other1"),
            "of that item only"
        );
        let ended = message(card(&face(Life::Started(at))));
        assert!(is_card_of(&ended, &task.id), "an ended card, by its footer");
        let routine = crate::render::horizon::tests::routine("Digest", false, "complete");
        let rc = Face {
            id: routine.id.clone(),
            kind: Kind::Routine,
            title: routine.title.clone(),
            due_ms: routine.next_ms,
            life: Life::Scheduled { snoozed: false },
            from: None,
            every: Some(routine.series.clone()),
            paused: false,
            routine: Some((routine.clone(), None)),
        };
        assert!(
            is_card_of(&message(card(&rc)), &routine.id),
            "a routine's card"
        );
    }

    /// An intent is kept whatever its age (a card never archives), and goes
    /// when its item has its entry, or is over.
    #[test]
    fn an_intent_is_dropped_at_load_with_its_entry_and_by_the_pass_when_its_item_is_over() {
        let now = 10 * 24 * 3_600_000;
        let intent = |id: &str, at_ms| Intent {
            id: id.into(),
            channel: 1,
            at_ms,
        };
        let rec = |id: &str| super::Rec {
            id: id.into(),
            kind: "reminder".into(),
            title: "t".into(),
            from: None,
            every: None,
            channel: 1,
            guild: "2".into(),
            card: 3,
            created_ms: 0,
            due_ms: None,
            snoozed: false,
            ended_ms: None,
            by: None,
            digest: 0,
        };
        let all = vec![
            intent("wak_a", now - 1_000),
            intent("wak_b", now - 1_000),
            intent("wak_c", now - 9 * 24 * 3_600_000),
        ];
        let kept = live(all, &[rec("wak_b")]);
        assert_eq!(
            kept,
            [
                intent("wak_a", now - 1_000),
                intent("wak_c", now - 9 * 24 * 3_600_000)
            ],
            "the old one stays, the one with its entry goes"
        );
        use crate::render::sched::{Kind, Life};
        let face = |id: &str| Face {
            id: id.into(),
            kind: Kind::Reminder,
            title: "t".into(),
            due_ms: None,
            life: Life::Scheduled { snoozed: false },
            from: None,
            every: None,
            paused: false,
            routine: None,
        };
        let stands = standing(kept, &[face("wak_c")]);
        assert_eq!(stands, [intent("wak_c", now - 9 * 24 * 3_600_000)]);
    }

    /// A footer's `from` name may carry another item's marker; the card is
    /// known by the footer's last field alone.
    #[test]
    fn an_ended_card_is_known_by_the_last_field_of_its_footer_not_by_a_name_in_it() {
        let ids = (
            "wak_01J0aaaaaaaaaaaaaaaaaaaaab12cd",
            "wak_01J0aaaaaaaaaaaaaaaaaaaaff00ee",
        );
        let footer = format!(
            "-# Reminder · from thread {} · {}",
            marker(ids.0),
            marker(ids.1)
        );
        let ended = message(vec![serde_json::json!({"type": 10, "content": footer})]);
        assert!(is_card_of(&ended, ids.1), "the card of its own marker");
        assert!(
            !is_card_of(&ended, ids.0),
            "never of a marker inside the from name"
        );
    }
}
