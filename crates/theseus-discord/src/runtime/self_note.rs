//! Drop, Halt and Resume on the self notes (theseus-o1h3f.7): the owner's DM notes `self_digest` and
//! `self_join` carry buttons, so a self-directed start can be answered where it is told.
//! A digest gets one **Drop** for each item it says it started (at most [`MAX_DROPS`], the
//! newest) and every note gets one **Halt**. A press is `self.drop` or `self.halt` as the
//! presser with their ids, and the core judges it: a halt is anyone's (as "halt self" is,
//! `halt.rs`), a drop is the owner's word (`rsi/backlog/word.rs`), from any place. The
//! notes' words are the core's and are not touched here: the items are read back out of the
//! text's `- started work on: … (item <id>)` lines. An item past the cap is reached with the
//! commands the note already names (`theseus self drop <item>`).
//!
//! A child of `runtime`, apart from it for the shape budget's file ceiling.

use serde_json::Value;
use theseus_protocol::rsi::{SelfDropParams, SelfDropResult, SelfHaltParams, SelfSwitchResult};
use twilight_model::application::interaction::Interaction;
use twilight_model::channel::message::component::{ActionRow, Button, ButtonStyle, Component};
use twilight_model::http::interaction::InteractionResponseType;

use super::{origin_of, Shared};

/// A button's custom id: `snote:drop:<item>`, `snote:halt` or `snote:resume`.
const PREFIX: &str = "snote";
/// The most Drop buttons a note carries: two rows of five at most, and Halt has a row of its own,
/// so the message never passes Discord's five rows of five buttons.
pub(crate) const MAX_DROPS: usize = 8;
const LINE: &str = "- started work on: ";

/// A press of a self note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Press {
    Drop(String),
    Halt,
    Resume,
}

/// An item id as a custom id may carry it.
fn id_ok(k: &str) -> bool {
    !k.is_empty()
        && k.len() <= 64
        && k.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

pub(crate) fn parse(custom_id: &str) -> Option<Press> {
    let rest = custom_id.strip_prefix(PREFIX)?.strip_prefix(':')?;
    match rest.split_once(':') {
        None if rest == "halt" => Some(Press::Halt),
        None if rest == "resume" => Some(Press::Resume),
        Some(("drop", id)) if id_ok(id) => Some(Press::Drop(id.into())),
        _ => None,
    }
}

/// The items a digest says it started, `(id, title)` oldest first, each once. Any other text
/// (a join note, a digest that started nothing) gives none.
pub(crate) fn started(text: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = vec![];
    for line in text.lines() {
        let Some(rest) = line.strip_prefix(LINE) else {
            continue;
        };
        let Some((head, tail)) = rest.rsplit_once("(item ") else {
            continue;
        };
        let id = tail.trim_end().trim_end_matches(')');
        if !id_ok(id) || out.iter().any(|(i, _)| i == id) {
            continue;
        }
        let title = head.split(": seen ").next().unwrap_or(head).trim();
        out.push((id.to_string(), title.to_string()));
    }
    out
}

/// The post's drops: a digest's started items, none for a join note.
pub(crate) fn items_of(kind: &str, body: &Value) -> Vec<(String, String)> {
    match kind {
        "self_digest" => started(body["text"].as_str().unwrap_or("")),
        _ => vec![],
    }
}

fn button(id: String, label: String, style: ButtonStyle) -> Component {
    Component::Button(Button {
        id: None,
        custom_id: Some(id),
        disabled: false,
        emoji: None,
        label: Some(label),
        style,
        url: None,
        sku_id: None,
    })
}

fn row(components: Vec<Component>) -> Component {
    Component::ActionRow(ActionRow {
        id: None,
        components,
    })
}

/// The note's buttons: a Drop for each of the newest [`MAX_DROPS`] items, five to a row, then
/// Halt on a row of its own. Halt is grey like the Drops, not red: the note is an FYI, and a
/// red button on every nightly note read as an alarm (owner, 2026-10-28; theseus-1ss4g).
pub(crate) fn buttons(items: &[(String, String)]) -> Vec<Component> {
    let from = items.len().saturating_sub(MAX_DROPS);
    let drops: Vec<Component> = items[from..]
        .iter()
        .filter(|(id, _)| id_ok(id))
        .map(|(id, title)| {
            button(
                format!("{PREFIX}:drop:{id}"),
                crate::render::clip(&format!("Drop: {title}"), 80),
                ButtonStyle::Secondary,
            )
        })
        .collect();
    let mut rows: Vec<Component> = drops.chunks(5).map(|c| row(c.to_vec())).collect();
    rows.push(row(vec![button(
        format!("{PREFIX}:halt"),
        "Halt self-improvement".into(),
        ButtonStyle::Secondary,
    )]));
    rows
}

/// The halt note's one button: Resume (a press is the typed `resume self`'s own call) (theseus-6s7ll).
pub(crate) fn resume_buttons() -> Vec<Component> {
    vec![row(vec![button(
        format!("{PREFIX}:resume"),
        "Resume self-improvement".into(),
        ButtonStyle::Success,
    )])]
}

/// What the presser is told of a refusal or a failure (the sibling cards' shape).
fn failed(what: &str, e: &crate::rpc_client::CallError) -> String {
    match e.code == theseus_protocol::error_code::REFUSED {
        true => format!(
            "🔐 Your {what} did not count: {}.",
            e.data
                .get("why")
                .and_then(Value::as_str)
                .unwrap_or(&e.message)
        ),
        false => format!("⚠️ {}", e.message),
    }
}

/// The answer to a drop.
pub(crate) fn dropped_words(r: &SelfDropResult) -> String {
    match r.changed {
        true => format!(
            "↩️ Dropped {} ({}). Its cause, {}, is never filed again.",
            r.item, r.title, r.cause
        ),
        false => format!("↩️ {} ({}) was already dropped.", r.item, r.title),
    }
}

/// The answer to a halt (the words `halt self` answers).
pub(crate) fn halted_words(r: &SelfSwitchResult) -> String {
    match r.changed {
        true => "🛑 Self-improvement halted: nothing self-directed runs until the owner \
                 resumes it (`theseus self resume`, or \"resume self\" from the owner's own \
                 account)."
            .into(),
        false => "🛑 Self-improvement was already halted.".into(),
    }
}

impl Shared {
    /// A press of a self note, answered to the presser alone.
    pub(super) async fn self_note_press(&self, i: &Interaction, press: Press, who: &str) {
        self.respond(
            i,
            InteractionResponseType::DeferredChannelMessageWithSource,
            None,
            true,
        )
        .await;
        let discord = origin_of(
            i.author().map(|u| u.id.get()),
            i.channel.as_ref().map(|c| c.id.get()),
            i.guild_id,
        );
        let text = match press {
            Press::Resume => self.resume_self_words(discord, who).await,
            Press::Drop(item) => {
                let r = self
                    .rpc
                    .call::<_, SelfDropResult>(
                        theseus_protocol::method::SELF_DROP,
                        SelfDropParams {
                            item,
                            author: Some(who.to_string()),
                            discord,
                            from_job: None,
                        },
                    )
                    .await;
                r.map_or_else(|e| failed("drop", &e), |r| dropped_words(&r))
            }
            Press::Halt => {
                let r = self
                    .rpc
                    .call::<_, SelfSwitchResult>(
                        theseus_protocol::method::SELF_HALT,
                        SelfHaltParams {
                            why: None,
                            author: Some(who.to_string()),
                            discord,
                        },
                    )
                    .await;
                r.map_or_else(
                    |e| format!("⚠️ Could not halt self-improvement: {e}."),
                    |r| halted_words(&r),
                )
            }
        };
        self.followup(i, &text).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn items_come_out_of_the_started_lines_and_ids_parse_back() {
        let text = "- started work on: Fix the gate: seen 3 times since Oct 3 (item tsk_a1)\n\
                    - new: Other (item tsk_b2)\n\
                    - started work on: Tidy docs (item tsk_c3)\n\
                    - started work on: Fix the gate (item tsk_a1)\n";
        let got = started(text);
        assert_eq!(
            got,
            [
                ("tsk_a1".to_string(), "Fix the gate".to_string()),
                ("tsk_c3".to_string(), "Tidy docs".to_string())
            ]
        );
        assert_eq!(
            parse("snote:drop:tsk_a1"),
            Some(Press::Drop("tsk_a1".into()))
        );
        assert_eq!(parse("snote:halt"), Some(Press::Halt));
        assert_eq!(parse("snote:resume"), Some(Press::Resume));
        for bad in [
            "snote:drop:",
            "snote:halt:x",
            "snote:resume:x",
            "snote:resume:",
            "snote:wipe:x",
            "other:yes:x",
        ] {
            assert!(parse(bad).is_none(), "{bad}");
        }
    }
}

#[cfg(test)]
mod tests_resume {
    use super::*;

    #[test]
    fn the_resume_note_has_one_button_on_the_id_that_parses_back() {
        let rows = resume_buttons();
        assert_eq!(rows.len(), 1);
        let Component::ActionRow(row) = &rows[0] else {
            panic!("a row")
        };
        let [Component::Button(b)] = row.components.as_slice() else {
            panic!("one button")
        };
        assert_eq!(b.label.as_deref(), Some("Resume self-improvement"));
        assert_eq!(parse(b.custom_id.as_deref().unwrap()), Some(Press::Resume));
    }
}
