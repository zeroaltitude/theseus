//! `/remind` and a reminder's four buttons (theseus-8ux6x). A child of
//! `runtime`, apart from it for the shape budget's file ceiling.
//!
//! - **`/remind when: what:`** is `wake.create` with no session: the reminder
//!   lives in the owner's Home (the DM's conversation), whatever place it was
//!   typed in, and the core posts its note to the DM when it is due. `when`
//!   autocompletes ("in 1h", "tomorrow 9:00", "fri 9:00", …), and whatever the
//!   owner types goes through as typed: the core reads `in 10m`, `10m`,
//!   `tomorrow 9:00`, `fri 9:00`, `17:30`, a date, or an RFC 3339 time.
//! - **The reminder's post** (`reminder`, `Buttons::Reminder`) is one message
//!   in the place the core chose, with **Done**, **Snooze 10 min**, **Snooze
//!   1 h** and **Tomorrow 9:00** (`wake:<answer>:<wake id>`). A press is a
//!   direct call (`wake.done`, `wake.snooze { fired }`, as `reminder_answers`
//!   says) as the presser, answered in the message itself: its buttons go and a
//!   line says what happened. A later `work.answer` will route them.

use serde_json::Value;
use theseus_protocol::{
    method, reminder_answers, WakeCreateParams, WakeDoneResult, WakeEditResult, WakeSnoozeParams,
};
use twilight_model::application::command::{
    Command, CommandOptionChoice, CommandOptionChoiceValue, CommandType,
};
use twilight_model::application::interaction::application_command::CommandOptionValue;
use twilight_model::application::interaction::{Interaction, InteractionData, InteractionType};
use twilight_model::channel::message::component::{ActionRow, Button, ButtonStyle, Component};
use twilight_model::channel::message::{AllowedMentions, MessageFlags};
use twilight_model::http::interaction::{
    InteractionResponse, InteractionResponseData, InteractionResponseType,
};
use twilight_util::builder::command::{CommandBuilder, StringBuilder};

use super::Shared;

/// A button's custom id: `wake:<answer id>:<wake id>`.
const PREFIX: &str = "wake";

/// What `when` suggests, before the owner types: the core reads each.
const SUGGESTIONS: [&str; 10] = [
    "in 10m",
    "in 30m",
    "in 1h",
    "in 2h",
    "in 4h",
    "today 17:00",
    "tomorrow 9:00",
    "fri 9:00",
    "mon 9:00",
    "in 7d",
];

/// `/remind when: what:`.
pub(super) fn command() -> Command {
    CommandBuilder::new(
        "remind",
        "Remind you of something: it is posted here in your DM when it is due, with Done and Snooze",
        CommandType::ChatInput,
    )
    .option(
        StringBuilder::new("when", "When: in 1h, tomorrow 9:00, fri 9:00, 17:30, a date")
            .required(true)
            .autocomplete(true),
    )
    .option(StringBuilder::new("what", "What it says").required(true))
    .build()
}

/// The suggestions for what was typed: those that contain it, then what was
/// typed itself, so anything the core reads goes through; at most 25.
pub(super) fn suggest(typed: &str) -> Vec<String> {
    let t = typed.trim().to_ascii_lowercase();
    let mut out: Vec<String> = Vec::new();
    if !t.is_empty() {
        out.push(typed.trim().to_string());
    }
    out.extend(
        SUGGESTIONS
            .iter()
            .filter(|s| t.is_empty() || s.contains(&t))
            .map(|s| s.to_string())
            .filter(|s| !s.eq_ignore_ascii_case(typed.trim())),
    );
    out.truncate(25);
    out
}

/// A reminder's buttons: one row of the four answers, in order.
pub(crate) fn buttons(wake_id: &str) -> Vec<Component> {
    let row = reminder_answers()
        .into_iter()
        .map(|a| {
            Component::Button(Button {
                id: None,
                custom_id: Some(format!("{PREFIX}:{}:{wake_id}", a.id)),
                disabled: false,
                emoji: None,
                label: Some(a.label),
                style: if a.method == method::WAKE_DONE {
                    ButtonStyle::Success
                } else {
                    ButtonStyle::Secondary
                },
                url: None,
                sku_id: None,
            })
        })
        .collect();
    vec![Component::ActionRow(ActionRow {
        id: None,
        components: row,
    })]
}

/// A press: its answer's id and the wake's.
pub(crate) fn parse(custom_id: &str) -> Option<(String, String)> {
    let rest = custom_id.strip_prefix(PREFIX)?.strip_prefix(':')?;
    let (answer, wake) = rest.split_once(':')?;
    (reminder_answers().iter().any(|a| a.id == answer) && wake.starts_with("wak_"))
        .then(|| (answer.to_string(), wake.to_string()))
}

impl Shared {
    /// `/remind`'s command and its autocomplete: whether it was one, and
    /// answered.
    pub(super) async fn remind_interaction(&self, i: &Interaction, who: &str) -> bool {
        let Some(InteractionData::ApplicationCommand(c)) = &i.data else {
            return false;
        };
        if c.name != "remind" {
            return false;
        }
        let text = |name: &str| {
            c.options.iter().find_map(|o| match (&*o.name, &o.value) {
                (n, CommandOptionValue::String(s) | CommandOptionValue::Focused(s, _))
                    if n == name =>
                {
                    Some(s.clone())
                }
                _ => None,
            })
        };
        if i.kind == InteractionType::ApplicationCommandAutocomplete {
            let choices = suggest(&text("when").unwrap_or_default())
                .into_iter()
                .map(|s| CommandOptionChoice {
                    name: s.clone(),
                    name_localizations: None,
                    value: CommandOptionChoiceValue::String(s),
                })
                .collect();
            let data = InteractionResponseData {
                choices: Some(choices),
                ..Default::default()
            };
            self.remind_answer(
                i,
                InteractionResponseType::ApplicationCommandAutocompleteResult,
                data,
            )
            .await;
            return true;
        }
        let (when, what) = (
            text("when").unwrap_or_default(),
            text("what").unwrap_or_default(),
        );
        let r = self
            .rpc
            .call::<_, WakeEditResult>(
                method::WAKE_CREATE,
                WakeCreateParams {
                    when,
                    note: what,
                    author: Some(who.to_string()),
                    ..Default::default()
                },
            )
            .await;
        let words = match r {
            Ok(r) => format!(
                "⏰ Reminder `{}` is set for {}. I will post it here when it is due, with Done and \
                 Snooze.",
                r.wake.short, r.wake.due_local
            ),
            Err(e) => format!("⚠️ Not set: {}", e.message),
        };
        self.remind_answer(
            i,
            InteractionResponseType::ChannelMessageWithSource,
            InteractionResponseData {
                content: Some(words),
                flags: Some(MessageFlags::EPHEMERAL),
                allowed_mentions: Some(AllowedMentions::default()),
                ..Default::default()
            },
        )
        .await;
        true
    }

    /// A reminder's button: Done, or a snooze of the wake that fired, as the
    /// presser. The message itself answers: its buttons go, a line says what
    /// happened. Whether it was a reminder's button.
    pub(super) async fn remind_press(
        &self,
        i: &Interaction,
        (answer, wake): (String, String),
        who: &str,
    ) {
        let Some(a) = reminder_answers().into_iter().find(|a| a.id == answer) else {
            return;
        };
        let said = if a.method == method::WAKE_DONE {
            let r = self
                .rpc
                .call::<_, WakeDoneResult>(
                    method::WAKE_DONE,
                    theseus_protocol::WakeNameParams {
                        wake: wake.clone(),
                        author: Some(who.to_string()),
                    },
                )
                .await;
            r.map(|_| "✅ Done".to_string()).map_err(|e| e.message)
        } else {
            let r = self
                .rpc
                .call::<_, WakeEditResult>(
                    method::WAKE_SNOOZE,
                    WakeSnoozeParams {
                        wake: wake.clone(),
                        when: a.when.clone(),
                        fired: true,
                        author: Some(who.to_string()),
                    },
                )
                .await;
            r.map(|r| format!("😴 {}: again at {}", a.label, r.wake.due_local))
                .map_err(|e| e.message)
        };
        let was = i
            .message
            .as_ref()
            .map(|m| m.content.clone())
            .unwrap_or_default();
        let data = match said {
            Ok(line) => InteractionResponseData {
                content: Some(format!("{was}\n-# {line}")),
                components: Some(vec![]),
                allowed_mentions: Some(AllowedMentions::default()),
                ..Default::default()
            },
            // The buttons stay, so the owner may try again; the failure is theirs alone.
            Err(why) => {
                let data = InteractionResponseData {
                    content: Some(format!("⚠️ That did not count: {why}")),
                    flags: Some(MessageFlags::EPHEMERAL),
                    allowed_mentions: Some(AllowedMentions::default()),
                    ..Default::default()
                };
                self.remind_answer(i, InteractionResponseType::ChannelMessageWithSource, data)
                    .await;
                return;
            }
        };
        self.remind_answer(i, InteractionResponseType::UpdateMessage, data)
            .await;
    }

    async fn remind_answer(
        &self,
        i: &Interaction,
        kind: InteractionResponseType,
        data: InteractionResponseData,
    ) {
        let resp = InteractionResponse {
            kind,
            data: Some(data),
        };
        if let Err(e) = self
            .http
            .interaction(i.application_id)
            .create_response(i.id, &i.token, &resp)
            .await
        {
            self.board.error("interaction response", None, e);
        }
    }
}

/// The reminder's text in a place: its note as it is, the title of its head
/// (`⏰ Reminder · <note>`), then a line of how it came (late, a series'
/// occurrence); a note of more lines keeps the rest below.
pub(crate) fn text(body: &Value) -> String {
    let note = body["text"].as_str().unwrap_or("").trim_end();
    let (title, rest) = note.split_once('\n').unwrap_or((note, ""));
    let title = if title.trim().is_empty() {
        "a reminder"
    } else {
        title
    };
    let mut how = vec!["a reminder you set".to_string()];
    if let Some(s) = body["series"].as_str() {
        match body["occurrence"].as_u64() {
            Some(n) => how.push(format!("{s}, #{n}")),
            None => how.push(s.to_string()),
        }
    }
    let late = body["late_ms"].as_u64().unwrap_or(0);
    if late > 5_000 {
        let down = if body["while_down"].as_bool() == Some(true) {
            ", the daemon was down"
        } else {
            ""
        };
        how.push(format!("{} late{down}", span(late)));
    }
    // The note is the owner's own words, whole: they are the title, the head's
    // only words (the owner, 2026-10-22; theseus-x8b9k).
    theseus_core::owner_note::note(
        theseus_core::owner_note::Kind::Reminder,
        title,
        &how.join(", "),
        rest,
    )
}

fn span(ms: u64) -> String {
    match ms / 1000 {
        s @ 0..=59 => format!("{s} s"),
        s @ 60..=3599 => format!("{} min", s / 60),
        s => format!("{} h", s / 3600),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The four buttons, in order, whose ids parse back to their answer and
    /// the wake; a wake that is not one and an answer that is not offered
    /// parse to none.
    #[test]
    fn a_reminder_has_four_buttons_and_each_press_parses() {
        let rows = buttons("wak_0199aaaa3f9a1c");
        assert_eq!(rows.len(), 1);
        let Component::ActionRow(r) = &rows[0] else {
            panic!("{rows:?}")
        };
        let got: Vec<(String, String)> = r
            .components
            .iter()
            .map(|b| match b {
                Component::Button(b) => (b.label.clone().unwrap(), b.custom_id.clone().unwrap()),
                other => panic!("{other:?}"),
            })
            .collect();
        let labels: Vec<&str> = got.iter().map(|(l, _)| l.as_str()).collect();
        assert_eq!(
            labels,
            ["Done", "Snooze 10 min", "Snooze 1 h", "Tomorrow 9:00"]
        );
        for ((_, id), want) in got
            .iter()
            .zip(["done", "snooze_10m", "snooze_1h", "tomorrow_9"])
        {
            assert_eq!(
                parse(id),
                Some((want.to_string(), "wak_0199aaaa3f9a1c".to_string()))
            );
        }
        assert_eq!(parse("wake:maybe:wak_1"), None);
        assert_eq!(parse("wake:done:ses_1"), None);
        assert_eq!(parse("other:yes:x"), None);
    }

    /// What `when` suggests: all ten before anything is typed; what is typed
    /// first (it goes through as typed), then those that contain it.
    #[test]
    fn when_suggests_and_lets_what_was_typed_through() {
        assert_eq!(suggest("").len(), 10);
        assert_eq!(suggest("")[0], "in 10m");
        let s = suggest("fri");
        assert_eq!(s, ["fri", "fri 9:00"]);
        let s = suggest("in 1h");
        assert_eq!(s, ["in 1h"], "an exact suggestion is not listed twice");
        assert_eq!(suggest("17:45"), ["17:45"]);
    }

    /// The post's text: the note as it is, and how it came when that matters.
    #[test]
    fn the_post_says_the_note_and_how_it_came() {
        assert_eq!(
            text(&json!({"text": "stretch"})),
            "⏰ Reminder · stretch\na reminder you set"
        );
        assert_eq!(
            text(
                &json!({"text": "stretch", "series": "every 1d", "occurrence": 4,
                         "late_ms": 125_000, "while_down": true})
            ),
            "⏰ Reminder · stretch\na reminder you set, every 1d, #4, 2 min late, the daemon was \
             down"
        );
        assert_eq!(
            text(&json!({"text": "call the vet\nbefore noon\n"})),
            "⏰ Reminder · call the vet\na reminder you set\nbefore noon"
        );
    }
}
