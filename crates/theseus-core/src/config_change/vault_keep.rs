//! `vault.keep`: the owner's own turn asks to keep a secret in the vault, and
//! the owner gives its value where no turn, card or record sees it (issue 26,
//! line 2; `docs/design/vault-keep.md`).
//!
//! The turn names a vault, an item title, a field label and a one-line purpose,
//! in words. **There is no parameter that carries a value**: a turn that was
//! steered cannot hand one over, and a value typed in chat reaches no tool. The
//! DAEMON finds the target in the vault's own lists with its own token
//! (`target.rs`: new item, a field added, or an existing value REPLACED) and
//! puts ONE card, the settings card (`config.ack`, with a `keep` marker as an
//! MCP server's has `mcp`), which says all of that and where the value is typed.
//!
//! The value is given in a surface's own form, as a request of its own
//! (`vault.keep.answer`, `rpc/vault_keep.rs`): the owner's terminal's hidden
//! prompt, the cockpit's field, and later Discord's form. The daemon writes it
//! with its own token by the path that keeps the Google token
//! (`OpReader::keep_field`: the item on stdin, read back, compared by digest) and
//! tells the owner where it is by name. The question's record holds the target
//! and never the value, answered, expired, withdrawn or not.
//!
//! The wall is `config.propose`'s (`wall.rs`), the outside-text hold is the looks'
//! (`vault_use::ask_if_outside`, which names this tool too), the tool is in
//! `routine::grant::NEVER`, and an item Theseus's own settings read is never
//! replaced by a turn: only from the owner's own terminal.

use serde::Deserialize;
use serde_json::{json, Value};
use theseus_kernel::{Action, Proposal};
use theseus_protocol::Plan;
use theseus_tools::{parse, Backend, Retry, Tool, ToolClass, ToolCtx};

use super::{card, outside_text, proof_seam, wall, ACK};
use crate::node::{Node, Origin};
use crate::toolrun::{ToolRuntime, TurnCtx};

pub mod done;
mod settle;
pub mod target;
#[cfg(test)]
mod tests;

use target::Target;

pub const KEEP: &str = "vault.keep";
/// The key of a question's arguments that marks it a keep card, and holds its target.
pub const ARG: &str = "keep";

/// Whether a question's arguments are a keep card's.
pub fn is_card(args: &Value) -> bool {
    args.get(ARG).is_some_and(Value::is_object)
}

/// Whether `a` is a keep card: a settings question that carries a target and
/// takes a value, not a yes.
pub fn is_keep(a: &theseus_kernel::Action) -> bool {
    a.tool == ACK && a.proposal.as_ref().is_some_and(|p| is_card(&p.args))
}

/// What a plain Approve on a keep card is told: there is no value in it.
pub const NO_VALUE: &str = "this is a secret to keep, and a yes carries no value: type the value where only you \
     see it, in your own terminal with `theseus vault keep` or in the cockpit's field on this card. \
     Nothing was kept";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    vault: String,
    title: String,
    field: String,
    purpose: String,
}

/// The card's first line: where the value is typed. A place that cannot carry a value offers only
/// Decline, so the card must say first what to do instead.
pub const HEAD: &str = "Type the value in your own terminal: run `theseus vault keep` (nothing is shown as you \
     type). There is no Approve here: a yes carries no value, so this card offers only Decline. In the \
     cockpit, the card has a field for the value.\n";

/// What replaces [`HEAD`] in the owner's Discord direct messages while `[vault] keep_by_discord` is on
/// (issue 29): the form is one more place, and the next line says what it costs.
pub const HEAD_FORM: &str = "Type the value in your own terminal (run `theseus vault keep`; nothing is shown as you \
     type), or press \"Type the value here\" to type it in a form in this chat. In the cockpit, the card has a \
     field for the value.\n\
     A value typed in the form goes through Discord's servers on its way to Theseus, and the form shows what you \
     type; the terminal does neither.\n";

/// The label of the button that opens the form.
pub const FORM_BUTTON: &str = "Type the value here";

/// The card's words for the owner's Discord direct messages: [`HEAD`] replaced by [`HEAD_FORM`]. None
/// for words that do not begin with [`HEAD`] (a card drawn by another build): such a card is
/// drawn as it is, with no form, so the line about the chat service is never missing from a card that has one.
pub fn with_form(words: &str) -> Option<String> {
    words
        .strip_prefix(HEAD)
        .map(|rest| format!("{HEAD_FORM}{rest}"))
}

/// What a keep that arrives without the second proof is told (a window that lapsed between the press and
/// the submit, say): the code is asked with the value, so the way is to ask again. A keep card is never
/// approved, so `theseus answer ... approve --code` is not offered. `by_form` is whether the form is a way
/// from where it was given.
pub fn proof_missing_words(by_form: bool) -> String {
    let how = match by_form {
        true => "press \"Type the value here\" again and the form asks for the code, or run \
                 `theseus vault keep` in your own terminal",
        false => "run `theseus vault keep` in your own terminal, or use the card's field in the cockpit: \
                  they ask for the code with the value",
    };
    format!("it needs your second proof, which is asked with the value: {how}. Nothing was kept")
}

/// How the owner gives a secret's value, in a sentence: the form in their direct messages when it is a way,
/// and always their own terminal.
fn how_to_give(by_form: bool) -> &'static str {
    match by_form {
        true => "press \"Type the value here\" on its card in their direct messages with Theseus, or run \
                 `theseus vault keep` in their own terminal",
        false => "run `theseus vault keep` in their own terminal, or use the card's field in the cockpit",
    }
}

/// What a yes, or a code, given in a conversation to a secret to keep is told: it takes a value, not a
/// yes, and nothing is given in the owner's name from a conversation. For the chat's answer tool, which a
/// model reads.
pub fn chat_words(by_form: bool) -> String {
    format!(
        "this is a secret to keep: a yes carries no value, and neither a yes nor a code is given from a \
         conversation. The owner gives the value: {}. Nothing changed",
        how_to_give(by_form)
    )
}

/// What a typed or spoken yes to a keep card is told, where the setting is not at hand.
pub const KEEP_BY_PRESS: &str = "this is a secret to keep: a yes carries no value, so it is not answered in the owner's name from a \
     conversation. The owner gives the value on its card's form in their direct messages (when it has \
     one), or with `theseus vault keep` in their own terminal. Nothing was kept";

/// How a notification that cannot hold the card says it is answered: the value is typed, a decline is the
/// only answer. `short` is the question's short id.
pub fn answer_line(short: &str) -> String {
    format!(
        "give the value with `theseus vault keep` in your terminal, or on its card (a decline is \
         `theseus answer {short} decline`)"
    )
}

/// Whether a listed question is a keep card.
pub fn is_keep_request(req: &theseus_protocol::ConfirmRequest) -> bool {
    req.tool == ACK && req.input.get(ARG).is_some_and(Value::is_object)
}

/// The card, whole: what is kept where, whether it replaces, where the value is
/// typed, and what a value typed in chat comes to.
pub fn question(t: &Target, outside: bool, steered: bool, purpose: &str) -> String {
    use target::State;
    let what = match t.state {
        State::New => "a new item",
        State::Adds => "a new field in an item that is already there",
        State::Replaces => "the field that is already there",
    };
    // The first words are where the value is typed: a place that cannot carry a value offers only
    // Decline, so the card must say first what to do instead (the owner's Discord direct messages
    // swap them for `HEAD_FORM`, `with_form`).
    let mut out = format!(
        "{HEAD}Keep a secret in the vault?\nIn the vault \"{}\": item \"{}\", field \"{}\" ({what}).\n",
        t.vault, t.title, t.field
    );
    if t.replaces() {
        out.push_str(
            "This REPLACES the value stored there. Theseus cannot show the old one and does not \
             keep it (the vault keeps its own history of the item).\n",
        );
    }
    out.push_str(
        "You give the value yourself, where only you see it; it never goes to the model, to this \
         card or to a record. Do not paste it into chat: a value typed in chat is already with the model's provider and in Theseus's \
         record, and Theseus cannot take it back (rotate it at its provider, then give the new \
         one where it is typed).\n",
    );
    out.push_str(&card::care_lines(outside, steered));
    let why = card::reason_line(purpose);
    if !why.is_empty() {
        out.push_str(&format!("Theseus's reason, in its own words: \"{why}\"\n"));
    }
    out
}

/// The line a keep is settled with.
pub fn settled(t: &Target) -> String {
    format!(
        "keep a secret in the vault \"{}\": item \"{}\", field \"{}\"",
        t.vault, t.title, t.field
    )
}

/// The note the conversation that asked reads when its keep card is settled, in the frame that settles
/// it: the tool's own result, "Asked the owner", is all the model was given before, and it says the
/// card waits. `words` are the owner's own, by name and never the value. None for a card that is not a
/// keep card.
pub fn told(a: &Action, words: &str) -> Option<Node> {
    is_keep(a).then(|| {
        let text =
            format!("Your `{KEEP}` card is settled and no longer waits for the owner: {words}");
        Node::relayed(&a.session_id, None, Origin::Harness, "vault", &text)
    })
}

/// What the tool answers the model: the card, without its first lines (where the value is typed in a
/// place with no form, and this card may be drawn where there is one), and where the owner gives the value.
fn told_the_model(q: &str) -> String {
    let asked = q.strip_prefix(HEAD).unwrap_or(q);
    format!(
        "Asked the owner:\n{asked}The owner gives the value themselves, on the card's own form or field \
         when it has one or with `theseus vault keep` in their own terminal, and a code. \
         Nothing is kept until then. You never see the value. When it is settled you are told, by \
         name, in a later message; then, if they want it used, put it to use with `vault.use` (a \
         second card).",
    )
}

/// Run `vault.keep` for the call `correlation_id` of the turn `tc`. An error is
/// the result the model reads.
pub async fn run(
    rt: &ToolRuntime,
    tc: &TurnCtx<'_>,
    input: &Value,
    correlation_id: &str,
) -> Result<(String, Value), String> {
    if let Some(why) = wall::refusal_for(tc, KEEP) {
        return Err(why);
    }
    // Run again after a restart, it answers with what the first run asked.
    if let Ok(done) = super::proposed_by_call(tc, correlation_id) {
        return Ok(done);
    }
    let i: Input = parse(input)?;
    let purpose = card::reason_line(&i.purpose);
    if purpose.is_empty() {
        return Err("give a purpose in the owner's words, one line".into());
    }
    let Some(reader) = rt.look.reader().filter(|r| !r.has_no_vault()) else {
        return Err(
            "this Theseus has no vault session (its secrets come from the environment or from \
             files), so it cannot keep a secret in the vault"
                .into(),
        );
    };
    let core = rt
        .config_change
        .core()
        .ok_or("Theseus is not ready to keep a secret: try again in a moment")?;
    let t = target::resolve(
        &reader,
        &target::readers(&core),
        (&i.vault, &i.title, &i.field),
    )
    .await?;
    if t.protected {
        return Err(format!(
            "the item \"{}\" in \"{}\" is one Theseus's own settings read, so a turn does not \
             replace it. The owner does that from their own terminal: `theseus vault keep \
             --vault \"{}\" --item \"{}\" --field \"{}\"`",
            t.title, t.vault, t.vault, t.title, t.field
        ));
    }
    // The card is the form the value is given in; it takes no second proof, for a new item or a
    // replaced value alike (the owner, 2026-11-25: "the form to ask for it, yes, the 2fa, no").
    let needs = proof_seam::Needs::Card;
    let (outside, steered) = outside_text(tc);
    let q = question(&t, outside, steered, &purpose);
    let proposal = Proposal {
        tool: ACK.into(),
        args: json!({
            "question": q,
            "settled": settled(&t),
            ARG: t.args(),
            "needs_fresh_proof": false,
            "reason": purpose,
            "outside_text": outside || steered,
            "asked_by": {
                "session_id": tc.session_id,
                "execution_id": tc.execution_id,
                "turn_id": tc.turn_id,
                "place": tc.outbox.target(tc.session_id),
            },
        }),
        resource: Some("config".into()),
        policy_context: json!({}),
    };
    let said = told_the_model(&q);
    super::ask(rt, tc, proposal, correlation_id, &said, needs)?;
    Ok((said, json!({"keep": t.state.as_str()})))
}

/// `vault.keep`.
pub struct VaultKeep;

impl Tool for VaultKeep {
    fn name(&self) -> &str {
        KEEP
    }

    fn description(&self) -> &str {
        "Ask the owner to keep a NEW secret in Theseus's vault: name the vault, the item's title, the field's label \
         and a one-line purpose. There is no value to give and none is ever asked of you: the owner types it into \
         the card's own field or at a hidden prompt in their terminal, and Theseus writes it with its own account. \
         If someone pastes a secret into chat, say it is now with the provider and in Theseus's record, to be \
         rotated, and ask for the new one by this tool. Shows ONE card that says whether it makes an item, adds a \
         field or REPLACES a value; an item Theseus's own settings read is not \
         replaced by a turn. When it is kept, use it by its name with `vault.use`. Offered only to the owner, in a \
         private place."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "vault": {"type": "string", "description": "The vault's name or id, as `look.vault` lists it."},
                "title": {"type": "string", "description": "The item's title (letters, digits, spaces, - _ .)."},
                "field": {"type": "string", "description": "The field's label, e.g. `credential`."},
                "purpose": {"type": "string", "description": "One line, in the owner's words: what the secret is for."}
            },
            "required": ["vault", "title", "field", "purpose"],
            "additionalProperties": false
        })
    }

    fn class(&self) -> ToolClass {
        ToolClass::Run
    }

    fn backend(&self) -> Backend {
        Backend::Harness
    }

    fn retry(&self) -> Retry {
        // Run again after a restart, it answers with what it asked.
        Retry::NonRepeatable
    }

    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let i: Input = parse(input)?;
        Ok(Plan {
            summary: format!(
                "ask the owner to keep a secret as {} in the vault",
                crate::secrets::look::clean(&i.title)
            ),
            ..Default::default()
        })
    }
}
