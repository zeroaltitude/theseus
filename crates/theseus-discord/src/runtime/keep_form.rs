//! The form for a secret's value, in the owner's direct messages (issue 29). A child of `runtime`.
//!
//! A keep card (`vault.keep`) is answered by a value typed where only the owner sees it. The owner
//! said yes to one more place: **a form on the card, in their direct messages with Theseus only**.
//! The card there says, before the press, that a value typed in the form goes through Discord's
//! servers and that the form does not hide it (a modal's text field has no hidden style); the
//! terminal and the cockpit stay as they were and are still named on the card. Anywhere else the
//! card is what it was: a Decline and where the value is typed.
//!
//! - **The press** (`kf:<question>`, or the V2 card's `keep_form` option) lists the cards as the
//!   presser and opens a modal for the card it finds: one field for the value, labelled with the
//!   item's title and field (all a person sees while typing), and a field of its own for the second
//!   proof's code when the daemon says one is needed. A code and a value are never one field.
//! - **The submit** is `vault.keep.answer`: the question's id, the value in a `SecretText`, the code,
//!   and the presser, which the daemon judges as every press is. The daemon writes to the target it
//!   recorded when it asked; nothing here names a vault, an item or a field.
//! - **The setting** `[vault] keep_by_discord` turns the place off: the card is not drawn with the
//!   form, a press says so, and the daemon refuses a submit.
//!
//! **The value goes from the form to the daemon and nowhere else.** Here it is read off the modal,
//! put in a `SecretText` (which prints as nothing) and handed to the daemon's request; it is in no
//! message, no log line, no row (`binding_ledger` is given the outcome and never a field) and no
//! reply (the reply is the daemon's words by name, or its refusal). The submit's reply is private to
//! the presser, and the card itself is edited to its outcome by the daemon's own settle.

use std::time::Duration;

use serde_json::{json, Value};
use theseus_core::config_change::vault_keep::FORM_BUTTON;
use theseus_protocol::answer::ConfirmListParams;
use theseus_protocol::owner_proof::ProofCode;
use theseus_protocol::vault_keep::{SecretText, VaultKeepAnswerParams, VaultKept};
use theseus_protocol::work::{AnswerOption, Style};
use theseus_protocol::{method, ConfirmListResult, ConfirmRequest, DiscordOrigin, LedgerKind};
use twilight_model::application::interaction::modal::ModalInteractionData;
use twilight_model::application::interaction::{Interaction, InteractionData};
use twilight_model::channel::message::component::{ActionRow, Button, ButtonStyle, Component};

use super::persona_soul::field_of;
use super::proof_form::keeps;
use super::{origin_of, Shared};
use crate::render::clip;
use crate::rpc_client::CallError;

/// The V2 card's option for the form, beside Decline.
pub(crate) const OPTION: &str = "keep_form";
/// The plain card's button, and the modal, as `kf:<question>`.
const PREFIX: &str = "kf:";
/// The modal's fields.
const VALUE_FIELD: &str = "value";
const CODE_FIELD: &str = "code";

const TEXT_INPUT: u64 = 4;
const LABEL: u64 = 18;

/// What Discord allows: a label's characters, a description's, and a field's text. The daemon's own
/// limit (4 KiB) still judges what is sent.
const LABEL_LIMIT: usize = 45;
const DESCRIPTION: usize = 100;
const VALUE_MAX: usize = 4000;

/// How long a submit waits for the daemon's answer before it says so. The daemon has no deadline of its own for
/// a whole keep, but each call it makes to the vault's program has `OP_TIMEOUT` (10 s), and one keep makes at most
/// seven in a row (the vault list, the items, the permission, the item's fields, then the item read, the write and
/// the read back), so its own answer, or its refusal, comes within 70 s. Eight of them is that and a little more,
/// well under the 15 minutes Discord leaves to complete a deferred response.
const KEEP_WAIT: Duration = Duration::from_secs(8 * theseus_core::secrets::OP_TIMEOUT.as_secs());

#[cfg(test)]
thread_local! {
    /// A shorter wait, for the test of a daemon that does not answer (the thread is the test's own).
    static SHORTER: std::cell::Cell<Option<Duration>> = const { std::cell::Cell::new(None) };
}

fn keep_wait() -> Duration {
    #[cfg(test)]
    if let Some(d) = SHORTER.with(std::cell::Cell::get) {
        return d;
    }
    KEEP_WAIT
}

/// What the presser is told when the daemon has not answered: not that it was kept, and not that it was not,
/// since the write is the daemon's and goes on. The card is what the daemon edits when the write ends. A value
/// given again is safe in every case: a second answer while the first is being kept is refused at once, and a
/// card that has settled takes no second value.
const NOT_YET: &str = "⏳ Theseus has not answered yet, so it cannot say whether the value was kept. \
     The card will change to show the outcome when Theseus finishes: look there before typing the value again. \
     If it still asks for the value, giving it again is safe.";

/// What a press is told where no form opens, to the presser alone.
const NOT_HERE: &str =
    "A secret's value is typed only in your direct messages with Theseus, or in \
     your own terminal (`theseus vault keep`) or the cockpit. It is not typed in a channel.";
const OFF: &str = "Typing a secret's value in Discord is turned off (`[vault] keep_by_discord`). Give it in your own \
     terminal with `theseus vault keep`, or in the card's field in the cockpit.";
const GONE: &str =
    "That card is not waiting for a value any more. Ask Theseus to keep the secret again.";

/// The card as the owner's direct messages draw it while the setting is on: its first words swapped
/// for the ones that say the form exists and what it costs, and the form's button beside Decline.
/// False (and nothing changed) for any card that is not a keep card in a direct message with the
/// setting on, or whose words are not the ones this build wrote: such a card is drawn as it was.
pub(crate) fn offer(req: &mut ConfirmRequest, in_dm: bool, on: bool) -> bool {
    if !(in_dm && on && keeps(req)) {
        return false;
    }
    let Some(plain) = req.plain.as_mut() else {
        return false;
    };
    let Some(detail) = theseus_core::config_change::vault_keep::with_form(&plain.detail) else {
        return false;
    };
    plain.detail = detail;
    req.options.push(AnswerOption {
        id: OPTION.into(),
        label: FORM_BUTTON.into(),
        style: Style::Primary,
        takes_note: false,
        effect: "opens a form for the value".into(),
    });
    true
}

/// The needs-you body of a keep card (the post always goes to the owner's direct messages) drawn with
/// the form: its words swapped as `offer` swaps them, and marked so that its last line names the
/// button. None for anything else, and for words that are not this build's.
pub(crate) fn needs_you_body(body: &Value) -> Option<Value> {
    if body["settings"]["keep"] != true {
        return None;
    }
    let words =
        theseus_core::config_change::vault_keep::with_form(body["settings"]["detail"].as_str()?)?;
    let mut drawn = body.clone();
    drawn["settings"]["detail"] = json!(words);
    drawn["settings"]["form"] = json!(true);
    Some(drawn)
}

/// Whether the card's options carry the form's button.
pub(crate) fn has_form(options: &[AnswerOption]) -> bool {
    options.iter().any(|o| o.id == OPTION)
}

/// The plain card's buttons: Decline, and the form's.
pub(crate) fn buttons(corr: &str) -> Vec<Component> {
    let button = |id: String, label: &str, style| {
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
    };
    vec![Component::ActionRow(ActionRow {
        id: None,
        components: vec![
            button(format!("{PREFIX}{corr}"), FORM_BUTTON, ButtonStyle::Primary),
            button(
                format!("confirm:decline:{corr}"),
                "Decline",
                ButtonStyle::Danger,
            ),
        ],
    })]
}

/// The question a `kf:` id is for.
fn question_of(custom_id: &str) -> Option<&str> {
    custom_id
        .strip_prefix(PREFIX)
        .filter(|q| !q.is_empty() && q.len() <= 90)
        .filter(|q| q.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
}

/// The form: a field for the value, labelled with the item's title and field, and, when the daemon
/// says a code is needed, a field of its own for it. `keep` is the target the question recorded.
fn modal_leaving(
    exits: &theseus_core::exits::Exits,
    question: &str,
    keep: &Value,
    code: bool,
) -> Value {
    let mut display = keep.clone();
    crate::render::leaving::strings(exits, &mut display);
    modal(question, &display, code)
}

pub(crate) fn modal(question: &str, keep: &Value, code_needed: bool) -> Value {
    let name = |k: &str| keep[k].as_str().unwrap_or("");
    let (vault, title, field) = (name("vault"), name("title"), name("field"));
    let mut rows = Vec::new();
    if code_needed {
        rows.push(json!({
            "type": LABEL,
            "label": "Code from your authenticator app",
            "description": "The six digits from the app. Not the secret.",
            "component": {"type": TEXT_INPUT, "custom_id": CODE_FIELD, "style": 1,
                          "required": true, "min_length": 6, "max_length": 7,
                          "placeholder": "123456"},
        }));
    }
    rows.push(json!({
        "type": LABEL,
        "label": clip(&format!("Value for {title}, field {field}"), LABEL_LIMIT),
        "description": description(vault, name("state") == "replaces"),
        "component": {"type": TEXT_INPUT, "custom_id": VALUE_FIELD, "style": 1,
                      "required": true, "min_length": 1, "max_length": VALUE_MAX},
    }));
    json!({"type": 9, "data": {
        "custom_id": format!("{PREFIX}{question}"),
        "title": clip("Keep a secret", LABEL_LIMIT),
        "components": rows,
    }})
}

/// The form's description, in Discord's 100 characters: the vault, that the text goes through Discord and is
/// shown as typed and, for a card that replaces a stored value, that word first. The vault's name is what is
/// clipped to fit, never the words that matter.
fn description(vault: &str, replaces: bool) -> String {
    const TAIL: &str = ". Goes through Discord, and is shown as you type.";
    let head = match replaces {
        true => "Replaces the value in vault ",
        false => "In the vault ",
    };
    let room = DESCRIPTION - head.chars().count() - TAIL.chars().count();
    format!("{head}{}{TAIL}", clip(vault, room))
}

impl Shared {
    /// A press or a submit that is this module's: true when it was, and answered.
    pub(super) async fn keep_form_interaction(&self, i: &Interaction, who: &str) -> bool {
        let discord = origin_of(
            i.author().map(|u| u.id.get()),
            i.channel.as_ref().map(|c| c.id.get()),
            i.guild_id,
        );
        match &i.data {
            Some(InteractionData::MessageComponent(c)) => {
                let Some(question) = question_of(&c.custom_id) else {
                    return false;
                };
                self.keep_form_open(i, question, discord).await;
                true
            }
            Some(InteractionData::ModalSubmit(m)) if m.custom_id.starts_with(PREFIX) => {
                self.keep_form_submit(i, m, who, discord).await;
                true
            }
            _ => false,
        }
    }

    /// A press on the form's button: the form opens for the card, or the presser alone is told why
    /// not. `question` is a card's id, or the end of one (a V2 card's id carries its last six
    /// characters); it must name exactly one keep card the daemon lists for the presser.
    pub(super) async fn keep_form_open(
        &self,
        i: &Interaction,
        question: &str,
        discord: Option<DiscordOrigin>,
    ) {
        let say = |text: &'static str| self.tell(i, format!("🔐 {text}"));
        if !self.core.cfg.vault.keep_by_discord {
            return say(OFF).await;
        }
        if i.guild_id.is_some() {
            return say(NOT_HERE).await;
        }
        let listed: Result<ConfirmListResult, _> = self
            .rpc
            .call(method::CONFIRM_LIST, ConfirmListParams { discord })
            .await;
        let Ok(listed) = listed else {
            return say(GONE).await;
        };
        let mut found = listed
            .confirms
            .into_iter()
            .filter(|c| c.correlation_id == question || c.correlation_id.ends_with(question));
        let (Some(req), None) = (found.next(), found.next()) else {
            return say(GONE).await;
        };
        if !keeps(&req) {
            return say(GONE).await;
        }
        if req.may_answer == Some(false) {
            return say(NOT_HERE).await;
        }
        // No authenticator is enrolled: a form could not be answered, so the daemon's words are said
        // whole, as the proof's own form says them.
        if let Some(p) = req.proof.as_ref().filter(|p| p.not_enrolled) {
            return self.tell(i, format!("🔐 {}", p.line)).await;
        }
        let code = req.needs_proof
            && (req.needs_fresh_proof || req.proof.as_ref().is_some_and(|p| p.code_needed));
        self.respond_modal(
            i,
            &modal_leaving(
                &self.core.exits,
                &req.correlation_id,
                &req.input["keep"],
                code,
            ),
        )
        .await;
    }

    /// A submitted form: `vault.keep.answer` as the presser. The daemon judges the place, the setting,
    /// the owner, the proof and the value, writes to the target it recorded, and settles the card; the
    /// reply is its words by name, or its refusal, to the presser alone.
    async fn keep_form_submit(
        &self,
        i: &Interaction,
        m: &ModalInteractionData,
        who: &str,
        discord: Option<DiscordOrigin>,
    ) {
        let Some(question) = question_of(&m.custom_id) else {
            return self.tell(i, format!("🔐 {GONE}")).await;
        };
        // The daemon writes to the vault, which can take over Discord's three seconds for a first response: the
        // presser is answered at once, privately, and the words complete that answer below.
        self.ack_ephemeral(i).await;
        let code = field_of(m, CODE_FIELD);
        let typed = field_of(m, VALUE_FIELD);
        // What the daemon would refuse is found here, since the form counts characters and the daemon bytes,
        // and no value is sent for it: the request says only why, and the daemon records the attempt (a
        // `vault.keep` row by class) and answers in its own words.
        let refused = theseus_core::config_change::vault_keep::done::refusal_of(&typed);
        let value = match refused {
            Some(_) => SecretText::default(),
            None => SecretText::new(typed),
        };
        let params = VaultKeepAnswerParams {
            question_id: question.to_string(),
            value,
            refused,
            code: Some(code.trim())
                .filter(|c| !c.is_empty())
                .map(ProofCode::new),
            author: Some(who.to_string()),
            discord,
            ..Default::default()
        };
        // The wait is bounded here, for this call alone: abandoning it leaves the daemon's write going (the request
        // is sent), and a late answer finds no one waiting and is dropped by the client.
        let wait = keep_wait();
        let answer = tokio::time::timeout(
            wait,
            self.rpc
                .call::<_, VaultKept>(method::VAULT_KEEP_ANSWER, params),
        )
        .await;
        let timed_out = answer.is_err();
        let r = answer.unwrap_or_else(|_| {
            Err(CallError {
                code: 0,
                message: format!("no answer came in {} s", wait.as_secs()),
                data: Value::Null,
            })
        });
        // The outcome, and never the field.
        self.core.binding_ledger(
            LedgerKind::DiscordConfirm,
            None,
            json!({"correlation_id": question, "option": OPTION, "by": who,
                   "ok": r.is_ok(), "modal": true, "keep": true,
                   "error": r.as_ref().err().map(|e| e.message.clone())}),
        );
        let told = match r {
            Ok(kept) => format!("✅ {}", kept.words),
            Err(e) if e.code == theseus_protocol::error_code::REFUSED => {
                let why = e.data.get("why").and_then(Value::as_str);
                format!("🔐 Nothing was kept: {}", why.unwrap_or(&e.message))
            }
            Err(_) if timed_out => NOT_YET.to_string(),
            Err(e) => format!("⚠️ Nothing was kept: {}", e.message),
        };
        self.finish_command(i, &told, "keep form reply").await;
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod modal_exit_tests_r4 {
    use super::*;
    use crate::runtime::tests_exits_r3::*;
    #[tokio::test]
    async fn keep_modal_masks_target_labels_before_cut() {
        let (_dir, core, _fake, _shared) = rig();
        let keep = json!({"title":format!("{}{}", "x".repeat(30), VALUE),"field":"password","vault":VALUE});
        let data = modal_leaving(&core.exits, "act_keep", &keep, false);
        let label = data["data"]["components"][0]["label"].as_str().unwrap();
        assert!(!label.contains("vau"), "{label}");
        assert_eq!(data["data"]["custom_id"], "kf:act_keep");
        assert_eq!(keep["vault"], VALUE);
    }
}
