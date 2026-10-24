//! The new-persona card (theseus-utfnl): a persona added without a soul draws
//! one `persona_soul` post to the owner's DM, which asks for the soul. Two
//! buttons: **Write its soul** opens a modal with one paragraph field, whose
//! submit is `persona.soul` as the presser; **Decline** is `persona.retire` as
//! the presser (the ledger and `ontology undo` keep it). Both are the core's to
//! judge: only the owner, from a private place, writes a soul, so a copy of the
//! card pressed anywhere else is refused and writes nothing.
//!
//! A child of `runtime`, apart from it for the shape budget's file ceiling.
//! It is not the locked DM task flow and touches none of it.

use serde_json::{json, Value};
use theseus_protocol::{method, PersonaKeyParams, PersonaSoulParams, PersonaWriteResult};
use twilight_model::application::interaction::modal::{
    ModalInteractionComponent, ModalInteractionData,
};
use twilight_model::application::interaction::Interaction;
use twilight_model::channel::message::component::{ActionRow, Button, ButtonStyle, Component};
use twilight_model::http::interaction::InteractionResponseType;

use super::{origin_of, Shared};

/// A button's custom id: `psoul:write:<key>` or `psoul:decline:<key>`.
const PREFIX: &str = "psoul";
/// The modal's: `psoul:m:<key>`.
pub(crate) const MODAL: &str = "psoul:m:";
/// The modal's one field.
const FIELD: &str = "soul";
/// The field's longest text (Discord's own cap on a paragraph input).
const MAX: usize = 4000;

/// A press of the card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Press {
    Write(String),
    Decline(String),
}

/// A persona key as a custom id may carry it.
fn key_ok(k: &str) -> bool {
    !k.is_empty()
        && k.len() <= 64
        && k.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

pub(crate) fn parse(custom_id: &str) -> Option<Press> {
    let rest = custom_id.strip_prefix(PREFIX)?.strip_prefix(':')?;
    let (verb, key) = rest.split_once(':')?;
    if !key_ok(key) {
        return None;
    }
    match verb {
        "write" => Some(Press::Write(key.into())),
        "decline" => Some(Press::Decline(key.into())),
        _ => None,
    }
}

/// The persona the core's `persona_soul` post is about.
pub(crate) fn key_of(body: &Value) -> String {
    body["persona"].as_str().unwrap_or_default().to_string()
}

fn button(id: String, label: &str, style: ButtonStyle) -> Component {
    Component::Button(Button {
        id: None,
        custom_id: Some(id),
        disabled: false,
        emoji: None,
        label: Some(label.to_string()),
        style,
        url: None,
        sku_id: None,
    })
}

/// The card's two buttons.
pub(crate) fn buttons(key: &str) -> Vec<Component> {
    if !key_ok(key) {
        return vec![];
    }
    vec![Component::ActionRow(ActionRow {
        id: None,
        components: vec![
            button(
                format!("{PREFIX}:write:{key}"),
                "Write its soul",
                ButtonStyle::Primary,
            ),
            button(
                format!("{PREFIX}:decline:{key}"),
                "Decline",
                ButtonStyle::Secondary,
            ),
        ],
    })]
}

/// The modal "Write its soul" opens: one paragraph.
pub(crate) fn modal(key: &str) -> Value {
    json!({"type": 9, "data": {
        "custom_id": format!("{MODAL}{key}"),
        "title": crate::render::clip(&format!("Soul of {key}"), 45),
        "components": [{
            "type": 18,
            "label": "Its soul",
            "description": "Voice, temperament, what it cares about, what it is for, and an emoji",
            "component": {"type": 4, "custom_id": FIELD, "style": 2,
                          "required": true, "max_length": MAX},
        }],
    }})
}

/// The text of the modal's input named `field`, walked through its labels and
/// rows (`persona_soul` and `card_question` each read one).
pub(super) fn field_of(m: &ModalInteractionData, field: &str) -> String {
    fn walk(c: &ModalInteractionComponent, field: &str, out: &mut String) {
        match c {
            ModalInteractionComponent::Label(l) => walk(&l.component, field, out),
            ModalInteractionComponent::ActionRow(r) => {
                r.components.iter().for_each(|c| walk(c, field, out))
            }
            ModalInteractionComponent::TextInput(t) if t.custom_id == field => {
                out.push_str(&t.value)
            }
            _ => {}
        }
    }
    let mut text = String::new();
    m.components.iter().for_each(|c| walk(c, field, &mut text));
    text
}

/// The modal's text, and the persona it is for.
fn submitted(m: &ModalInteractionData) -> Option<(String, String)> {
    let key = m.custom_id.strip_prefix(MODAL).filter(|k| key_ok(k))?;
    Some((key.to_string(), field_of(m, FIELD)))
}

/// What the owner is told of a refusal or a failure.
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

impl Shared {
    /// A press of the card.
    pub(super) async fn persona_soul_press(&self, i: &Interaction, press: Press, who: &str) {
        match press {
            Press::Write(key) => self.respond_modal(i, &modal(&key)).await,
            Press::Decline(key) => {
                self.respond(
                    i,
                    InteractionResponseType::DeferredChannelMessageWithSource,
                    None,
                    true,
                )
                .await;
                let r = self
                    .rpc
                    .call::<_, PersonaWriteResult>(
                        method::PERSONA_RETIRE,
                        PersonaKeyParams {
                            key: Some(key.clone()),
                            author: Some(who.to_string()),
                            discord: origin_of(
                                i.author().map(|u| u.id.get()),
                                i.channel.as_ref().map(|c| c.id.get()),
                                i.guild_id,
                            ),
                        },
                    )
                    .await;
                let text = match r {
                    Ok(_) => format!(
                        "↩️ Declined: persona {key} is retired (`theseus ontology undo` or \
                         `theseus persona add {key}` brings it back)."
                    ),
                    Err(e) => failed("decline", &e),
                };
                self.followup(i, &text).await;
            }
        }
    }

    /// A submitted soul: `persona.soul` as the presser, and the answer tells
    /// them to the presser alone.
    pub(super) async fn persona_soul_modal(
        &self,
        i: &Interaction,
        m: &ModalInteractionData,
        who: &str,
    ) {
        let Some((key, text)) = submitted(m) else {
            return;
        };
        let r = self
            .rpc
            .call::<_, PersonaWriteResult>(
                method::PERSONA_SOUL,
                PersonaSoulParams {
                    key: key.clone(),
                    text,
                    author: Some(who.to_string()),
                    discord: origin_of(
                        i.author().map(|u| u.id.get()),
                        i.channel.as_ref().map(|c| c.id.get()),
                        i.guild_id,
                    ),
                },
            )
            .await;
        let line = match r {
            Ok(done) => format!(
                "✅ **{key}** has its soul ({} of {} tokens): it is live at the next compile.",
                done.tokens.unwrap_or(0),
                done.cap
            ),
            Err(e) => failed("soul", &e),
        };
        self.tell(i, line).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The buttons parse back to their persona, and a bad id parses to none.
    #[test]
    fn the_cards_buttons_parse() {
        let Component::ActionRow(row) = &buttons("pilot")[0] else {
            panic!("a row")
        };
        let ids: Vec<&str> = row
            .components
            .iter()
            .map(|c| match c {
                Component::Button(b) => b.custom_id.as_deref().unwrap(),
                _ => panic!("a button"),
            })
            .collect();
        assert_eq!(ids, ["psoul:write:pilot", "psoul:decline:pilot"]);
        assert_eq!(parse(ids[0]), Some(Press::Write("pilot".into())));
        assert_eq!(parse(ids[1]), Some(Press::Decline("pilot".into())));
        for bad in [
            "psoul:write:",
            "psoul:wipe:pilot",
            "psoul:write:a b",
            "onto:undo:x",
        ] {
            assert!(parse(bad).is_none(), "{bad}");
        }
        assert!(buttons("a b").is_empty());
        assert_eq!(key_of(&json!({"persona": "pilot"})), "pilot");
        assert_eq!(modal("pilot")["data"]["custom_id"], "psoul:m:pilot");
    }
}
